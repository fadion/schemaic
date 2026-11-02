//! `schemaic import`'s decisions that need no file and no server: the
//! `--map`/`--skip` overrides on top of the app's own mapping, and what is said
//! about a column left out or a problem found.
//!
//! **The mapping starts as the app's** (`core::import::auto_map` — by header
//! name, or by position without one) and the flags only correct it, so a file
//! the app would map on its own needs none of them. The reading, the check and
//! the load are core's and the database layer's, as the app's import modal
//! drives them; nothing here reads a field.

use schemaic_core::import::{
    CsvDialect, ImportFormat, Issue, Mapping, NullRule, ReadConfig, Target,
};
use schemaic_core::schema::TableInfo;

/// How the file is to be read, as the flags and the file say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    /// `--format`, or `None` to go by the file's extension.
    pub format: Option<String>,
    pub no_header: bool,
    pub delimiter: Option<u8>,
    pub sheet: Option<String>,
    pub nulls: Vec<String>,
    pub trim: bool,
}

/// The format and the read settings for `file_name`, from the flags and, for
/// a CSV, the head of the file (`head`) to guess the delimiter from — the
/// app's import modal's opening answer, with every flag given taking the place
/// of a guess.
///
/// **The header is the flag's, never the guess's.** The app's sniffer reads a
/// numeric first row as data, and the modal shows the user what it decided;
/// with nobody looking at a preview, a guess that took a header row of years
/// for data would import the header as a row. A header unless `--no-header`.
pub fn read_config(
    file_name: &str,
    reading: &Reading,
    head: Option<&[u8]>,
) -> Result<(ImportFormat, ReadConfig), String> {
    let format = match reading.format.as_deref() {
        Some(name) => match name.to_ascii_lowercase().as_str() {
            "csv" | "tsv" => ImportFormat::Csv,
            "json" | "jsonl" => ImportFormat::Json,
            "xlsx" | "excel" => ImportFormat::Xlsx,
            _ => return Err(format!("unknown format '{name}'; use csv, json or xlsx")),
        },
        None => schemaic_core::import::infer_format(file_name).ok_or_else(|| {
            format!("{file_name}'s extension names no format; add --format csv, json or xlsx")
        })?,
    };
    if format != ImportFormat::Csv && (reading.delimiter.is_some() || !reading.nulls.is_empty()) {
        return Err("--delimiter and --null are for CSV files".to_string());
    }
    if format != ImportFormat::Xlsx && reading.sheet.is_some() {
        return Err("--sheet is for Excel files".to_string());
    }
    // A JSON record's keys are its header; without one, `auto_map` pairs
    // them with the table's columns by position, in their sorted order.
    if format == ImportFormat::Json && reading.no_header {
        return Err("--no-header is for CSV and Excel files".to_string());
    }
    let mut dialect = match (format, head) {
        (ImportFormat::Csv, Some(head)) => {
            schemaic_core::import::sniff(&String::from_utf8_lossy(head))
        }
        _ => CsvDialect::default(),
    };
    if let Some(d) = reading.delimiter {
        dialect.delimiter = d;
    }
    dialect.has_header = !reading.no_header;
    let nulls = if reading.nulls.is_empty() {
        NullRule::default()
    } else {
        NullRule {
            tokens: reading.nulls.clone(),
        }
    };
    Ok((
        format,
        ReadConfig {
            dialect,
            nulls,
            trim: reading.trim,
            sheet: reading.sheet.clone(),
        },
    ))
}

/// One `--map`: `file_col=table_col`. The file column is everything before the
/// **last** `=`, since a header may hold one and a column name rarely does.
pub fn parse_map(s: &str) -> Result<(String, String), String> {
    match s.rsplit_once('=') {
        Some((file, table)) if !file.trim().is_empty() && !table.trim().is_empty() => {
            Ok((file.to_string(), table.to_string()))
        }
        _ => Err(format!(
            "'{s}' is not a mapping; write it as file_column=table_column"
        )),
    }
}

/// Where `name` is among `names`: exactly, else case-insensitively and
/// trimmed — `auto_map`'s own matching, so a flag is read the way the
/// automatic mapping reads a header.
fn position(names: &[String], name: &str) -> Option<usize> {
    names.iter().position(|n| n == name).or_else(|| {
        let want = name.trim().to_ascii_lowercase();
        names
            .iter()
            .position(|n| n.trim().to_ascii_lowercase() == want)
    })
}

/// Every place `name` is among `names`, matched as [`position`] matches it —
/// the exact spelling where there is one, else the loose one.
fn positions(names: &[String], name: &str) -> Vec<usize> {
    let exact: Vec<usize> = (0..names.len()).filter(|&i| names[i] == name).collect();
    if !exact.is_empty() {
        return exact;
    }
    let want = name.trim().to_ascii_lowercase();
    (0..names.len())
        .filter(|&i| names[i].trim().to_ascii_lowercase() == want)
        .collect()
}

/// Why a file could not be turned into columns. Nothing has been written.
#[derive(Debug)]
pub enum FileProblem {
    /// It could not be parsed — the file's problem, [`crate::run::Exit::Rejected`].
    Unreadable(schemaic_core::import::ImportError),
    /// It holds no columns at all: a zero-byte CSV, a JSON `[]`.
    Empty,
}

impl FileProblem {
    /// The line for stderr, about the file called `name`.
    pub fn message(&self, name: &str) -> String {
        match self {
            FileProblem::Unreadable(e) => format!("{name}: {e}"),
            FileProblem::Empty => format!("{name} is empty; there is nothing to import"),
        }
    }
}

/// The file's columns, **as the load will read them** — for JSON, every key of
/// every record ([`schemaic_core::import::json_columns`]), since a record's
/// fields are its keys and a key the first record lacks is still a column;
/// otherwise the header (or the first row's width), from a one-row sample.
pub fn file_columns<R: std::io::Read>(
    open: impl Fn() -> std::io::Result<R>,
    format: ImportFormat,
    cfg: &ReadConfig,
) -> Result<Vec<String>, FileProblem> {
    use schemaic_core::import::{ImportError, json_columns, probe_sample};
    let columns = match format {
        ImportFormat::Json => {
            let r =
                open().map_err(|e| FileProblem::Unreadable(ImportError::Read(e.to_string())))?;
            json_columns(r)
        }
        _ => probe_sample(&open, format, cfg, 1).map(|s| s.columns),
    }
    .map_err(FileProblem::Unreadable)?;
    if columns.is_empty() {
        return Err(FileProblem::Empty);
    }
    Ok(columns)
}

/// What `schemaic import` will load, and what it says about the rest.
#[derive(Debug)]
pub struct ImportPlan {
    pub mapping: Mapping,
    /// A line of stderr per file column left out that nobody `--skip`ped.
    pub notes: Vec<String>,
}

/// The mapping for `file_columns` into `table` — the app's automatic one,
/// corrected by the flags — or why there is none to load.
///
/// **Every decision between reading the file and checking it**, in one place
/// a test reaches: which `has_header` the automatic mapping is told (a JSON
/// file always has its keys), the overrides, and the refusal of a mapping
/// that inserts nothing.
pub fn plan_mapping(
    file_columns: &[String],
    format: ImportFormat,
    cfg: &ReadConfig,
    table: &TableInfo,
    maps: &[(String, String)],
    skips: &[String],
) -> Result<ImportPlan, String> {
    use schemaic_core::import::{auto_map, insert_columns};
    let has_header = cfg.dialect.has_header || format == ImportFormat::Json;
    let auto = auto_map(file_columns, table, has_header);
    let mapping = apply_overrides(auto.clone(), file_columns, table, maps, skips)?;
    if insert_columns(&mapping, table).is_empty() {
        return Err(format!(
            "no column of the file matches a column of {}; --map one, e.g. --map 'file_column={}'",
            table.name,
            table
                .columns
                .first()
                .map(|c| c.name.as_str())
                .unwrap_or("column")
        ));
    }
    let notes = skipped(&auto, &mapping, file_columns, table, skips);
    Ok(ImportPlan { mapping, notes })
}

/// `mapping` with the `--map` and `--skip` flags applied, or why one of them
/// cannot be.
///
/// **A flag naming something that is not there is refused, never ignored.**
/// A misspelt `--skip passwrod` that quietly did nothing would import the very
/// column it was written to keep out.
///
/// An explicit `--map` onto a table column some other file column was
/// matched to takes it: the flag is the user's word, the match a guess, and
/// the other file column is left out, which [`skipped`] then says.
pub fn apply_overrides(
    mut mapping: Mapping,
    file_columns: &[String],
    table: &TableInfo,
    maps: &[(String, String)],
    skips: &[String],
) -> Result<Mapping, String> {
    let table_names: Vec<String> = table.columns.iter().map(|c| c.name.clone()).collect();
    // **A name the file has twice is refused**: no flag can say which one it
    // means, and reaching the first — the matched one — left both out.
    let file_index = |name: &str| match positions(file_columns, name).as_slice() {
        [] => Err(format!(
            "the file has no column '{name}'; it has {}",
            file_columns.join(", ")
        )),
        [i] => Ok(*i),
        several => Err(format!(
            "the file has more than one column '{name}' (columns {}); a flag can't say \
             which it means — rename one in the file",
            several
                .iter()
                .map(|i| (i + 1).to_string())
                .collect::<Vec<_>>()
                .join(" and ")
        )),
    };
    // Resolved first, so "mapped and skipped" is asked of the column the two
    // flags reach — `--map Name=name --skip name` name one column, spelled two
    // ways, and comparing the spellings let the skip quietly undo the map.
    let mut mapped = Vec::with_capacity(maps.len());
    for (file, _) in maps {
        mapped.push(file_index(file)?);
    }
    for skip in skips {
        if mapped.contains(&file_index(skip)?) {
            return Err(format!("'{skip}' is both mapped and skipped"));
        }
    }
    for (file, column) in maps {
        let i = file_index(file)?;
        let t = position(&table_names, column).ok_or_else(|| {
            format!(
                "{} has no column '{column}'; it has {}",
                table.name,
                table_names.join(", ")
            )
        })?;
        if table.columns[t].is_server_assigned() {
            return Err(format!(
                "{} is filled in by the server and takes no value from a file",
                table_names[t]
            ));
        }
        for (j, target) in mapping.targets.iter_mut().enumerate() {
            if j != i && *target == Target::Column(t) {
                *target = Target::Skip;
            }
        }
        if let Some(target) = mapping.targets.get_mut(i) {
            *target = Target::Column(t);
        }
    }
    for skip in skips {
        let i = file_index(skip)?;
        if let Some(target) = mapping.targets.get_mut(i) {
            *target = Target::Skip;
        }
    }
    Ok(mapping)
}

/// The file columns `mapping` leaves out, each with why and how to bring it
/// in — said on stderr, since a column silently not imported is data lost
/// without anyone deciding to lose it. `auto` is the mapping before the
/// flags, which tells a column a `--map` displaced from one nothing matched;
/// `skips` are the ones the user left out on purpose, which need no sentence.
///
/// **Asked by position, not by name**: one `--skip name` used to silence
/// every column of that name. A name the file has twice gets no advice to
/// flag it, since [`apply_overrides`] refuses that flag.
pub fn skipped(
    auto: &Mapping,
    mapping: &Mapping,
    file_columns: &[String],
    table: &TableInfo,
    skips: &[String],
) -> Vec<String> {
    let skipped_on_purpose: Vec<usize> = skips
        .iter()
        .filter_map(|s| position(file_columns, s))
        .collect();
    mapping
        .targets
        .iter()
        .zip(file_columns)
        .enumerate()
        .filter(|(i, (t, _))| **t == Target::Skip && !skipped_on_purpose.contains(i))
        .map(|(i, (_, name))| {
            if positions(file_columns, name).len() > 1 {
                return format!(
                    "the file's column '{name}' (column {}) is not imported: another column \
                     has the same name; rename one in the file to import both",
                    i + 1
                );
            }
            let taker = |t: usize| {
                let at = mapping
                    .targets
                    .iter()
                    .position(|m| *m == Target::Column(t))?;
                file_columns.get(at)
            };
            let why = match auto.targets.get(i) {
                Some(Target::Column(t)) => match taker(*t) {
                    Some(by) => format!(
                        "its match '{}' was given to '{by}' by --map",
                        table.columns[*t].name
                    ),
                    None => "a flag took its match".to_string(),
                },
                _ => "no table column matches it".to_string(),
            };
            format!(
                "the file's column '{name}' is not imported: {why}; \
                 --map '{name}=<column>' to import it, or --skip '{name}' to say so"
            )
        })
        .collect()
}

/// One problem the check found, as a line of stderr.
pub fn issue_line(issue: &Issue) -> String {
    format!(
        "line {}, column {}: {} ({:?})",
        issue.line,
        issue.column,
        issue.kind.message(),
        issue.text
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TableInfo {
        use schemaic_core::schema::ColumnInfo;
        // `id` is `GENERATED ALWAYS AS IDENTITY`: the server's, and refusing a
        // value — unlike a plain auto-increment, which takes one.
        let col = |name: &str, identity_always: bool| ColumnInfo {
            name: name.to_string(),
            type_name: if identity_always { "int" } else { "text" }.to_string(),
            nullable: !identity_always,
            identity_always,
            ..Default::default()
        };
        TableInfo {
            name: "people".to_string(),
            columns: vec![col("id", true), col("name", false), col("email", false)],
            ..Default::default()
        }
    }

    fn cols(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn map(s: &str) -> (String, String) {
        parse_map(s).unwrap()
    }

    #[test]
    fn the_format_comes_from_the_flag_else_the_extension() {
        let r = Reading::default();
        assert_eq!(read_config("a.csv", &r, None).unwrap().0, ImportFormat::Csv);
        assert_eq!(read_config("a.TSV", &r, None).unwrap().0, ImportFormat::Csv);
        assert_eq!(
            read_config("a.json", &r, None).unwrap().0,
            ImportFormat::Json
        );
        assert_eq!(
            read_config("a.xlsx", &r, None).unwrap().0,
            ImportFormat::Xlsx
        );
        assert!(read_config("a.dat", &r, None).is_err());
        let json = Reading {
            format: Some("JSON".into()),
            ..Default::default()
        };
        assert_eq!(
            read_config("a.dat", &json, None).unwrap().0,
            ImportFormat::Json
        );
        let bad = Reading {
            format: Some("parquet".into()),
            ..Default::default()
        };
        assert!(read_config("a.csv", &bad, None).is_err());
    }

    /// The delimiter is guessed from the head, as the app does, unless given;
    /// the header is the flag's alone — see `read_config`.
    #[test]
    fn a_csv_guesses_its_delimiter_but_never_its_header() {
        let head = b"2019;2020\n1;2\n3;4\n";
        let (_, cfg) = read_config("y.csv", &Reading::default(), Some(head)).unwrap();
        assert_eq!(cfg.dialect.delimiter, b';');
        assert!(
            cfg.dialect.has_header,
            "the sniffer's numeric-row guess is not taken"
        );
        let given = Reading {
            delimiter: Some(b'|'),
            no_header: true,
            ..Default::default()
        };
        let (_, cfg) = read_config("y.csv", &given, Some(head)).unwrap();
        assert_eq!(cfg.dialect.delimiter, b'|');
        assert!(!cfg.dialect.has_header);
    }

    #[test]
    fn nulls_default_to_the_empty_field_and_a_flag_replaces_them() {
        let (_, cfg) = read_config("a.csv", &Reading::default(), None).unwrap();
        assert_eq!(cfg.nulls, NullRule::default());
        let given = Reading {
            nulls: vec!["NULL".into(), "\\N".into()],
            ..Default::default()
        };
        let (_, cfg) = read_config("a.csv", &given, None).unwrap();
        assert_eq!(
            cfg.nulls.tokens,
            vec!["NULL".to_string(), "\\N".to_string()]
        );
    }

    /// A flag for another format is refused, not ignored.
    #[test]
    fn a_flag_for_another_format_is_refused() {
        let csv_flag = Reading {
            delimiter: Some(b';'),
            ..Default::default()
        };
        assert!(read_config("a.json", &csv_flag, None).is_err());
        let sheet = Reading {
            sheet: Some("S".into()),
            ..Default::default()
        };
        assert!(read_config("a.csv", &sheet, None).is_err());
        assert_eq!(
            read_config("a.xlsx", &sheet, None)
                .unwrap()
                .1
                .sheet
                .as_deref(),
            Some("S")
        );
    }

    #[test]
    fn a_map_splits_at_its_last_equals() {
        assert_eq!(map("Full Name=name"), ("Full Name".into(), "name".into()));
        assert_eq!(map("a=b=c"), ("a=b".into(), "c".into()));
        for bad in ["name", "=name", "name=", " = "] {
            assert!(parse_map(bad).is_err(), "{bad}");
        }
    }

    /// The automatic mapping stands where no flag speaks, and a `--map`
    /// corrects the one it could not make.
    #[test]
    fn a_map_corrects_the_automatic_mapping() {
        let t = table();
        let file = cols(&["Full Name", "EMAIL"]);
        let auto = schemaic_core::import::auto_map(&file, &t, true);
        assert_eq!(auto.targets, vec![Target::Skip, Target::Column(2)]);
        let m = apply_overrides(auto, &file, &t, &[map("full name=Name")], &[]).unwrap();
        assert_eq!(m.targets, vec![Target::Column(1), Target::Column(2)]);
    }

    /// The flag is the user's word and the automatic match a guess: a `--map`
    /// onto a column already claimed takes it, and the other is left out.
    #[test]
    fn a_map_onto_a_matched_column_takes_it_from_the_match() {
        let t = table();
        let file = cols(&["name", "nick"]);
        let auto = schemaic_core::import::auto_map(&file, &t, true);
        let m = apply_overrides(auto.clone(), &file, &t, &[map("nick=name")], &[]).unwrap();
        assert_eq!(m.targets, vec![Target::Skip, Target::Column(1)]);
        assert_eq!(skipped(&auto, &m, &file, &t, &[]).len(), 1);
    }

    #[test]
    fn a_skip_leaves_a_matched_column_out_and_is_not_reported() {
        let t = table();
        let file = cols(&["name", "email"]);
        let auto = schemaic_core::import::auto_map(&file, &t, true);
        let m = apply_overrides(auto.clone(), &file, &t, &[], &cols(&["email"])).unwrap();
        assert_eq!(m.targets, vec![Target::Column(1), Target::Skip]);
        assert!(skipped(&auto, &m, &file, &t, &cols(&["email"])).is_empty());
    }

    /// **A flag naming nothing is refused, never ignored** — a misspelt
    /// `--skip` would import the very column it was written to keep out.
    #[test]
    fn a_flag_naming_nothing_is_refused() {
        let t = table();
        let file = cols(&["name", "email"]);
        let auto = || schemaic_core::import::auto_map(&file, &t, true);
        assert!(apply_overrides(auto(), &file, &t, &[], &cols(&["emial"])).is_err());
        assert!(apply_overrides(auto(), &file, &t, &[map("nope=name")], &[]).is_err());
        assert!(apply_overrides(auto(), &file, &t, &[map("name=nope")], &[]).is_err());
        let both = apply_overrides(auto(), &file, &t, &[map("name=name")], &cols(&["name"]));
        assert!(both.is_err());
        // The same column spelled two ways is still both.
        let cased = apply_overrides(auto(), &file, &t, &[map("Name=name")], &cols(&["name"]));
        assert!(cased.is_err(), "{cased:?}");
    }

    #[test]
    fn a_server_assigned_column_takes_no_mapping() {
        let t = table();
        let file = cols(&["key"]);
        let auto = schemaic_core::import::auto_map(&file, &t, true);
        let why = apply_overrides(auto, &file, &t, &[map("key=id")], &[]).unwrap_err();
        assert!(why.contains("server"), "{why}");
    }

    /// A column the automatic mapping could not place is said, with the
    /// flags that would settle it.
    #[test]
    fn an_unmatched_column_is_reported_with_the_flags_that_settle_it() {
        let t = table();
        let file = cols(&["name", "age"]);
        let m = schemaic_core::import::auto_map(&file, &t, true);
        let notes = skipped(&m, &m, &file, &t, &[]);
        assert_eq!(notes.len(), 1);
        assert!(
            notes[0].contains("'age'") && notes[0].contains("--map") && notes[0].contains("--skip")
        );
    }

    /// The plan as `run_import` composes it — the file's columns, then the
    /// mapping — over in-memory bytes.
    fn plan(
        bytes: &'static str,
        file: &str,
        reading: &Reading,
        maps: &[(String, String)],
        skips: &[String],
    ) -> Result<(Vec<String>, ImportPlan), String> {
        let (format, cfg) = read_config(file, reading, Some(bytes.as_bytes()))?;
        let open = || Ok::<_, std::io::Error>(bytes.as_bytes());
        let columns = file_columns(open, format, &cfg).map_err(|e| e.message(file))?;
        let p = plan_mapping(&columns, format, &cfg, &table(), maps, skips)?;
        Ok((columns, p))
    }

    /// **Every key of a JSON file is a column**, not only the first record's —
    /// a key the first record omits (a null an export left out) was dropped
    /// from every row, silently.
    #[test]
    fn a_json_key_first_seen_after_record_one_is_imported() {
        let json = r#"[{"id":"1","name":"a"},{"id":"2","name":"b","email":"b@x"}]"#;
        let (columns, p) = plan(json, "p.json", &Reading::default(), &[], &[]).unwrap();
        assert!(columns.contains(&"email".to_string()), "{columns:?}");
        let at = columns.iter().position(|c| c == "email").unwrap();
        assert_eq!(p.mapping.targets[at], Target::Column(2));
    }

    /// **A JSON file has no header to leave out**: `--no-header` there paired
    /// its sorted keys with the table's columns by position.
    #[test]
    fn no_header_is_refused_on_a_json_file() {
        let no_header = Reading {
            no_header: true,
            ..Default::default()
        };
        assert!(read_config("p.json", &no_header, None).is_err());
        assert!(read_config("p.csv", &no_header, None).is_ok());
        assert!(read_config("p.xlsx", &no_header, None).is_ok());
    }

    /// **A flag cannot say which of two same-named columns it means**, so it is
    /// refused — `--skip name` reached the matched one and left both out.
    #[test]
    fn a_flag_naming_a_duplicated_column_is_refused() {
        let csv = "name,email,name\nann,a@x,Ann\n";
        let why = plan(csv, "p.csv", &Reading::default(), &[], &cols(&["name"])).unwrap_err();
        assert!(why.contains("more than one"), "{why}");
        let why = plan(csv, "p.csv", &Reading::default(), &[map("name=name")], &[]).unwrap_err();
        assert!(why.contains("more than one"), "{why}");
        // Unflagged, the second one is said, and not with advice that would
        // be refused.
        let (_, p) = plan(csv, "p.csv", &Reading::default(), &[], &[]).unwrap();
        assert_eq!(p.notes.len(), 1, "{:?}", p.notes);
        assert!(p.notes[0].contains("same name") && !p.notes[0].contains("--skip"));
    }

    /// **A column a `--map` displaced is said as displaced**, not as one no
    /// table column matches.
    #[test]
    fn a_displaced_column_is_said_to_be_displaced() {
        let csv = "name,nick\nann,a\n";
        let (_, p) = plan(csv, "p.csv", &Reading::default(), &[map("nick=name")], &[]).unwrap();
        assert_eq!(p.notes.len(), 1);
        assert!(
            p.notes[0].contains("'name'")
                && p.notes[0].contains("'nick'")
                && !p.notes[0].contains("no table column"),
            "{}",
            p.notes[0]
        );
    }

    /// **An empty file is said to be empty**, not answered with a `--map` the
    /// next run refuses.
    #[test]
    fn an_empty_file_is_said_to_be_empty() {
        for (bytes, file) in [("", "p.csv"), ("[]", "p.json")] {
            let why = plan(bytes, file, &Reading::default(), &[], &[]).unwrap_err();
            assert!(why.contains("empty"), "{file}: {why}");
        }
    }

    /// **A file that can't be parsed is the file's problem** (exit 7), not a
    /// usage error.
    #[test]
    fn an_unparseable_file_is_a_file_problem() {
        let (format, cfg) = read_config("p.json", &Reading::default(), None).unwrap();
        let open = || Ok::<_, std::io::Error>(&b"[1, 2]"[..]);
        let e = file_columns(open, format, &cfg).unwrap_err();
        assert!(matches!(e, FileProblem::Unreadable(_)), "{e:?}");
    }

    #[test]
    fn an_issue_names_its_line_column_and_text() {
        let line = issue_line(&Issue {
            line: 4,
            column: "age".into(),
            text: "abc".into(),
            kind: schemaic_core::import::IssueKind::NotAnInteger,
        });
        assert_eq!(line, "line 4, column age: not a whole number (\"abc\")");
    }
}
