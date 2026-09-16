//! File → table import: format inference, CSV dialect sniffing, and column
//! mapping. Pure over a *sample* of the file plus the target table's schema — no
//! IO, no DB (the app streams the real file past this).
//!
//! The design rule throughout is the one `intel` uses: **only decide what can be
//! decided.** A sniffed delimiter is a proposal the user sees and can override,
//! an auto-mapping is a starting point, and validation (see the coercion half of
//! this module) checks only what a wrong answer would definitely break. The
//! server remains the authority on whether a value is acceptable — it parses more
//! date and numeric formats than we could enumerate, and rejecting valid data is
//! a worse failure than passing it through.

use crate::intel::SqlDialect;
use crate::model::Value;
use crate::schema::TableInfo;
// A UTF-8 BOM lands inside the first header name, where it silently breaks
// name-matching on the very first column. The same three bytes broke the script
// splitter and the connection-file parsers, so the strip is shared.
use crate::text::strip_bom;

/// The file formats import accepts.
///
/// Deliberately not the mirror of [`crate::export::ExportFormat`]: a `.sql` file
/// belongs in the editor, and Markdown/HTML tables aren't a data interchange
/// anyone imports from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportFormat {
    /// Delimiter-separated text. The delimiter itself lives in [`CsvDialect`], so
    /// TSV is this too.
    Csv,
    /// Objects keyed by column name — either one array of them, or one per line
    /// (JSON Lines). Both read the same way; see [`ArrayUnwrap`].
    Json,
    /// An Excel workbook. One **sheet** of it — which one is
    /// [`ReadConfig::sheet`], because a workbook is a file with several tables in
    /// it and no other import format has to choose.
    Xlsx,
}

impl ImportFormat {
    pub fn label(self) -> &'static str {
        match self {
            ImportFormat::Csv => "CSV / TSV",
            ImportFormat::Json => "JSON",
            ImportFormat::Xlsx => "Excel",
        }
    }

    /// Does this format carry its own nulls?
    ///
    /// **A capability, because two unrelated decisions turn on it.** CSV cannot
    /// tell an empty field from a missing value, which is the entire reason
    /// [`NullRule`] exists; JSON has a real `null` and Excel has a genuinely
    /// empty cell, so applying the NULL-token rule to either would turn every
    /// empty *string* they hold into a NULL. `validate` and `row_iter` each ask
    /// this, and they must not answer it differently — the preview would then
    /// show one thing and the import do another.
    ///
    /// An exhaustive `match`, for the reason
    /// [`crate::export::ExportFormat::is_text`] gives: a `!matches!` lets a
    /// fourth format default to "carries its own nulls" without its author ever
    /// being asked, and the two readers would then agree on the wrong answer
    /// rather than disagreeing loudly.
    pub fn has_own_nulls(self) -> bool {
        match self {
            ImportFormat::Json | ImportFormat::Xlsx => true,
            ImportFormat::Csv => false,
        }
    }

    /// Every format, for the override dropdown.
    pub const ALL: [ImportFormat; 3] = [ImportFormat::Csv, ImportFormat::Json, ImportFormat::Xlsx];
}

/// Guess the format from a file name's extension. `None` when the extension says
/// nothing useful — the UI then leaves the dropdown on its default rather than
/// pretending to know.
pub fn infer_format(file_name: &str) -> Option<ImportFormat> {
    let ext = file_name.rsplit_once('.')?.1.to_ascii_lowercase();
    match ext.as_str() {
        "csv" | "tsv" | "tab" | "txt" => Some(ImportFormat::Csv),
        "json" => Some(ImportFormat::Json),
        // `.xlsm` is the same OOXML container with macros in it, which the
        // reader neither runs nor looks at. `.xls` and `.xlsb` are different
        // formats and are deliberately absent — guessing `Xlsx` for one would
        // fail at open time with a confusing error instead of leaving the
        // dropdown alone.
        "xlsx" | "xlsm" => Some(ImportFormat::Xlsx),
        _ => None,
    }
}

/// How to read a delimited file. Sniffed from a sample, then shown to the user as
/// editable settings — a wrong delimiter is the single most common import
/// failure, and it's obvious the moment the preview renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CsvDialect {
    pub delimiter: u8,
    pub quote: u8,
    /// Whether the first record names the columns.
    pub has_header: bool,
}

impl Default for CsvDialect {
    fn default() -> Self {
        Self {
            delimiter: b',',
            quote: b'"',
            has_header: true,
        }
    }
}

/// The delimiters worth guessing between, in preference order. Comma first: a
/// tie (a file with equal counts, e.g. one column and no delimiter at all) should
/// land on the overwhelmingly common case.
const CANDIDATE_DELIMITERS: &[u8] = b",\t;|";

/// Guess a file's delimiter and whether it has a header, from the first few
/// lines.
///
/// The delimiter is chosen by *consistency*, not raw frequency: the right
/// delimiter splits every line into the same number of fields, while a character
/// that merely appears often (a comma inside prose, say) splits them unevenly.
/// Counting outside quoted regions matters for the same reason — a quoted
/// `"Smith, John"` would otherwise vote for the comma in a semicolon file.
///
/// The header guess is deliberately weak, and negative: a numeric field anywhere
/// in the first row means it's data. Files where every column is text are
/// genuinely ambiguous, so it defaults to "yes, header" — the common case, and
/// visibly wrong in the preview if it isn't.
pub fn sniff(sample: &str) -> CsvDialect {
    let lines: Vec<&str> = sample
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(20)
        .collect();
    if lines.is_empty() {
        return CsvDialect::default();
    }

    let mut best = (b',', 0usize); // (delimiter, score)
    for &d in CANDIDATE_DELIMITERS {
        let counts: Vec<usize> = lines.iter().map(|l| count_unquoted(l, d, b'"')).collect();
        let first = counts[0];
        // A delimiter that never appears isn't a delimiter.
        if first == 0 {
            continue;
        }
        // Consistent across every sampled line ⇒ score by how many fields it
        // yields, so a file that's consistent under both `,` and `;` picks the
        // one actually structuring it.
        if counts.iter().all(|&c| c == first) && first > best.1 {
            best = (d, first);
        }
    }
    let delimiter = best.0;

    CsvDialect {
        delimiter,
        quote: b'"',
        has_header: guess_header(&lines, delimiter),
    }
}

/// Occurrences of `d` in `line` that are outside a quoted field.
fn count_unquoted(line: &str, d: u8, quote: u8) -> usize {
    let mut n = 0;
    let mut in_quotes = false;
    for &b in line.as_bytes() {
        if b == quote {
            in_quotes = !in_quotes;
        } else if b == d && !in_quotes {
            n += 1;
        }
    }
    n
}

/// Split on `d` outside quotes, dropping the quote characters themselves.
fn split_unquoted(line: &str, d: u8) -> Vec<String> {
    let (d, quote) = (d as char, '"');
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for ch in line.chars() {
        if ch == quote {
            in_quotes = !in_quotes;
        } else if ch == d && !in_quotes {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    out.push(cur);
    out
}

fn looks_numeric(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty() && s.parse::<f64>().is_ok()
}

/// Does the first line name the columns?
///
/// There's only one reliable signal, and it's negative: a numeric field in the
/// first row means it's data. Everything else is ambiguous — an all-text file
/// genuinely could go either way — so this answers "yes" unless it has that
/// evidence to the contrary. That's the common case, and the preview makes a
/// wrong guess obvious immediately (the header row shows up as data, or the
/// first data row goes missing).
/// Is `t` a plain decimal numeral — an optional sign, digits, and at most one
/// decimal point?
///
/// **The shape an exact column takes**, as distinct from the range an `f64`
/// covers. `NUMERIC` holds 131,072 digits before the point, so "no `f64` can
/// hold it" says nothing about whether the server can; what it *does* separate
/// is a numeral from `NaN`, `inf`, `Infinity` and `1e400`, none of which is one.
fn is_decimal_numeral(t: &str) -> bool {
    let body = t.strip_prefix(['+', '-']).unwrap_or(t);
    let mut digits = 0usize;
    let mut points = 0usize;
    for b in body.bytes() {
        match b {
            b'0'..=b'9' => digits += 1,
            b'.' => points += 1,
            _ => return false,
        }
    }
    digits > 0 && points <= 1
}

fn guess_header(lines: &[&str], d: u8) -> bool {
    !split_unquoted(lines[0], d).iter().any(|f| looks_numeric(f))
}

/// Where one of the file's columns lands in the target table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Into the table column at this index.
    Column(usize),
    /// Not imported. Also what an unmatched column starts as — importing a
    /// column nobody asked for is worse than leaving it out visibly.
    Skip,
}

/// One entry per *file* column, in file order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub targets: Vec<Target>,
}

impl Mapping {
    /// Table columns nothing maps to. These are left out of the `INSERT`
    /// entirely, so the server applies their default — which is how an
    /// auto-increment key stays out of the way without import having to know it's
    /// auto-increment.
    pub fn unmapped_columns(&self, table: &TableInfo) -> Vec<usize> {
        (0..table.columns.len())
            .filter(|i| !self.targets.contains(&Target::Column(*i)))
            .collect()
    }

    /// Unmapped NOT NULL columns the server won't fill in — the ones likely to
    /// fail on insert. Still "likely", not "certainly": a trigger can supply a
    /// value and nothing here can see one, so this stays a warning to weigh
    /// rather than a blocking error.
    ///
    /// The rule is what the *server* will supply, read off the model rather than
    /// guessed from the type text: a column needs warning about when it is
    /// unmapped and NOT NULL and nothing will fill it — no `DEFAULT`, not
    /// auto-increment/identity, not generated.
    ///
    /// This used to approximate auto-increment as "integer primary key", which is
    /// neither necessary nor sufficient. It stayed **silent** on a natural `INT`
    /// key (`year INT PRIMARY KEY`), where the import then fails on the second
    /// row with a duplicate key or on the first with a NOT NULL violation; and it
    /// **warned** about `status VARCHAR(10) NOT NULL DEFAULT 'new'` left unmapped,
    /// which is the ordinary, correct thing to do — training the user to ignore
    /// the warning, the exact outcome the heuristic existed to avoid.
    pub fn missing_required(&self, table: &TableInfo) -> Vec<String> {
        self.unmapped_columns(table)
            .into_iter()
            .filter(|&i| {
                let c = &table.columns[i];
                !c.nullable && !c.auto_increment && c.default.is_none() && c.generated.is_none()
            })
            .map(|i| table.columns[i].name.clone())
            .collect()
    }
}

/// What a probe's answer is worth by the time it lands.
///
/// A probe reads the file off the UI thread, so several can be in flight at
/// once — typing `\t` into the Delimiter box is three edits and therefore three
/// probes — and they report in *completion* order, not the order they were
/// asked. Only the newest may write, because everything a probe sets is a
/// statement about the settings the controls now show: the sample, the file
/// size, and above all `auto_map`, which matches by **name** with a header and
/// strictly by **position** without one. A loser landing last left the mapping
/// built from a config the user could no longer see, and the load then ran with
/// the live config against that stale mapping — for a header `name,email` over
/// `(id, email, name)` that writes every name into `email`, committed.
///
/// Two counters because they answer two questions: `open` is bumped per opening
/// of the modal (the answer is about a different *table*), `seq` per request
/// (the answer is about different *settings*).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeVerdict {
    /// Fold this answer into the modal — and, since it is the newest, clear the
    /// busy flag with it.
    Apply,
    /// Drop it whole. Not even the busy flag: a request still in flight is still
    /// a reason the modal must not let the user move on.
    Discard,
}

/// Whether a probe that has just finished may write. See [`ProbeVerdict`].
pub fn probe_verdict(mine: (u64, u64), current: (u64, u64)) -> ProbeVerdict {
    if mine == current {
        ProbeVerdict::Apply
    } else {
        ProbeVerdict::Discard
    }
}

/// What a schema refresh means for a modal editing `target`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetVerdict {
    /// Leave the modal alone.
    Keep,
    /// The table really is gone; close.
    Close,
    /// The table is gone *and* a load is running — cancel it (rolling the
    /// transaction back) rather than closing over a bulk write whose outcome
    /// would then have no reader.
    Cancel,
}

/// Whether a modal open on a table survives a schema change.
///
/// Closing needs **positive evidence** that the table is gone, which is the
/// whole content of this function: the schema list is *emptied* before a refetch
/// begins, so "I looked and it wasn't there" was true of every refresh and of
/// every connection switch, and a hand-built twelve-column mapping was discarded
/// by a background reload the user didn't ask for.
/// `loading` is a **bulk load**, not "anything is busy": the Cancel arm cancels
/// a token only the load holds, so handing it a probe's flag made a genuinely
/// vanished table cancel nothing and leave the modal open on it — the one
/// outcome `Close` exists to produce.
pub fn target_survives(no_evidence: bool, found: bool, loading: bool) -> TargetVerdict {
    if no_evidence || found {
        TargetVerdict::Keep
    } else if loading {
        TargetVerdict::Cancel
    } else {
        TargetVerdict::Close
    }
}

/// One database row of the schema tree, reduced to what a modal open on a table
/// needs to know.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DbNodeView<'a> {
    pub database: &'a str,
    /// Whether this database contains the table — `None` while its schema is
    /// loading or has failed.
    ///
    /// Three states, not two, and that is the whole reason this type exists.
    /// "I looked and it wasn't there" and "I haven't looked" are the same
    /// `false` to a bool, and a refresh *empties* what there is to look at, so
    /// the second was answering as the first and discarding hand-built mappings.
    pub has_table: Option<bool>,
}

/// What the schema tree currently says about the table a modal is open on.
///
/// The evidence half of [`target_survives`], moved out of the view because two
/// of its three inputs were bugs while the decision it fed was fully tested: the
/// probe/load conflation above, and this function's `same_connection` — which
/// the caller simply did not check. `db_nodes` holds only the **active**
/// connection's databases, so switching connections replaces the list wholesale;
/// with no connection check, "the table is not in this list" was true of another
/// server's list, and Ctrl+Shift+P → Switch Connection discarded a twelve-column
/// mapping the user had built by hand.
///
/// The two "no evidence" cases are deliberately different from the one "gone"
/// case: no node for this connection at all means the list is about somewhere
/// else (or is mid-reload), while a connection whose databases *are* listed and
/// which no longer has this one means the database was dropped.
pub fn target_verdict(
    nodes: &[DbNodeView<'_>],
    same_connection: bool,
    database: &str,
    loading: bool,
) -> TargetVerdict {
    if !same_connection || nodes.is_empty() {
        return TargetVerdict::Keep;
    }
    let found = nodes
        .iter()
        .any(|n| n.database == database && n.has_table != Some(false));
    target_survives(false, found, loading)
}

/// Propose a mapping from the file's columns onto the table's.
///
/// With a header, match on name, case-insensitively and ignoring surrounding
/// whitespace — that's what people actually expect, and it survives a file whose
/// columns are in a different order. Without one, fall back to position, which is
/// the only signal there is. Anything unmatched starts as [`Target::Skip`] so the
/// mapping step shows the gap instead of quietly inventing a pairing.
pub fn auto_map(file_columns: &[String], table: &TableInfo, has_header: bool) -> Mapping {
    // A column the server assigns and refuses an explicit value for is never a
    // candidate. `insert_columns` filters it out regardless, but leaving it
    // mapped here would show the user a plan that isn't the one that runs.
    let writable = |i: usize| {
        table
            .columns
            .get(i)
            .is_some_and(|c| !c.is_server_assigned())
    };
    if !has_header {
        return Mapping {
            targets: (0..file_columns.len())
                .map(|i| {
                    if writable(i) {
                        Target::Column(i)
                    } else {
                        Target::Skip
                    }
                })
                .collect(),
        };
    }
    let norm = |s: &str| s.trim().to_ascii_lowercase();
    // `used` keeps two same-named file columns from both claiming one target —
    // the second is left Skip for the user to resolve.
    let mut used = vec![false; table.columns.len()];
    let targets = file_columns
        .iter()
        .map(|fc| {
            let want = norm(fc);
            let found = table
                .columns
                .iter()
                .enumerate()
                .find(|(i, tc)| !used[*i] && writable(*i) && norm(&tc.name) == want)
                .map(|(i, _)| i);
            match found {
                Some(i) => {
                    used[i] = true;
                    Target::Column(i)
                }
                None => Target::Skip,
            }
        })
        .collect();
    Mapping { targets }
}

/// Synthesized names for a headerless file's columns (`Column 1`, `Column 2`, …),
/// so the mapping UI has something to label its rows with.
pub fn placeholder_columns(n: usize) -> Vec<String> {
    (1..=n).map(|i| format!("Column {i}")).collect()
}

/// Which field texts mean SQL `NULL`.
///
/// This is the setting that quietly corrupts data when it's wrong — an empty
/// field is `NULL` in one export and the empty string in the next, and nothing
/// about the file says which. So it's explicit, and it's shown in the first step
/// rather than buried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NullRule {
    /// Compared against the trimmed field, case-insensitively.
    pub tokens: Vec<String>,
}

impl Default for NullRule {
    /// Empty field ⇒ NULL. The most common convention, and the one every tool
    /// that wrote the file was probably using.
    fn default() -> Self {
        Self {
            tokens: vec![String::new()],
        }
    }
}

impl NullRule {
    /// No text means NULL. What JSON uses: the format carries nullness itself, so
    /// re-interpreting `""` as NULL would contradict the file.
    pub fn none() -> Self {
        Self { tokens: Vec::new() }
    }

    fn matches(&self, field: &str) -> bool {
        let trimmed = field.trim();
        self.tokens.iter().any(|t| {
            if t.is_empty() {
                // The empty token means an *empty* field, not a blank one. A
                // quoted `"   "` is a deliberate three spaces, and nulling it
                // would be this module rewriting data it was told to carry —
                // exactly what the trim setting exists to ask about first.
                field.is_empty()
            } else {
                // A written token is matched against the trimmed field, so
                // `NULL` still matches ` NULL ` in a padded file.
                t.eq_ignore_ascii_case(trimmed)
            }
        })
    }
}

/// One field of a source record.
///
/// `None` is a value the *format itself* says is absent — a JSON `null`, or a key
/// the object simply doesn't have. CSV never produces it: a missing CSV field is
/// empty text, and whether that means NULL is [`NullRule`]'s call. Keeping the
/// two distinct is what lets a JSON `""` stay an empty string while a CSV `` is
/// a NULL.
pub type Field = Option<String>;

/// One source record as the traversal hands it over: its fields, plus what the
/// *format* knows about them that the text no longer can say.
///
/// `sheet_errors` holds the indices of fields whose worksheet cell was a
/// `calamine::Data::Error` — a cell the sheet itself could not evaluate. It
/// exists because [`cell_text`] renders such a cell with Excel's own spelling
/// (`#N/A`, `#REF!`), which is the right thing to show and also the point at
/// which a formula error and a string cell holding those five characters become
/// byte-identical. Re-deriving it downstream from the spelling refused an
/// ordinary file: `#N/A` is pandas' first default `na_values` entry and what
/// people type by hand for "not applicable". Empty for CSV and JSON, which have
/// no such cell.
///
/// The indices are into the record as read. Nothing trims them alongside a
/// trimmed field list ([`trim_to_mapping`]) because nothing needs to: an index
/// past the end simply never matches a field that is still there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Record {
    fields: Vec<Field>,
    sheet_errors: Vec<usize>,
}

impl Record {
    /// A record from a format that has no notion of a broken cell.
    fn plain(fields: Vec<Field>) -> Self {
        Record {
            fields,
            sheet_errors: Vec::new(),
        }
    }
}

/// The column families import validates. Everything outside them is
/// [`ColKind::Other`] and passes through untouched — see the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColKind {
    Int,
    Uint,
    /// Binary floating point (`FLOAT`/`DOUBLE`/`REAL`).
    Float,
    /// Exact numeric (`DECIMAL`/`NUMERIC`) — validated, but kept as text so the
    /// precision that made someone choose the type in the first place survives.
    Exact,
    Bool,
    Other,
}

/// Classify a column by its declared type.
///
/// Matches the *base* type name, not a substring: `interval` and `point` both
/// contain "int", and treating them as integers would reject every valid value
/// they hold.
///
/// The split is [`crate::typename`]'s, which is where all three readings of a
/// declared type now live. This one wants the **leading word** — `timestamp`
/// out of `timestamp without time zone`, `double` out of `double precision` —
/// because it matches a fixed list of scalar keywords, where `ddl` wants the
/// whole base to decide whether two types are the same.
pub fn classify(type_name: &str) -> ColKind {
    let unsigned = crate::typename::is_unsigned(type_name);
    let base = crate::typename::leading_word(type_name);
    match base.as_str() {
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" | "int2" | "int4"
        | "int8" | "serial" | "bigserial" | "smallserial" => {
            if unsigned {
                ColKind::Uint
            } else {
                ColKind::Int
            }
        }
        "float" | "double" | "real" | "float4" | "float8" => ColKind::Float,
        "decimal" | "numeric" => ColKind::Exact,
        "bool" | "boolean" => ColKind::Bool,
        _ => ColKind::Other,
    }
}

/// Why a field couldn't be imported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueKind {
    NotAnInteger,
    NotANumber,
    NotABoolean,
    /// The worksheet itself could not evaluate this cell — `#N/A`, `#REF!`,
    /// `#DIV/0!`.
    ///
    /// **Raised whatever the column's type is**, which is the whole reason it
    /// exists. `cell_text` keeps Excel's own spelling of a formula error on the
    /// stated ground that "passing it on surfaces as a coercion `Issue` naming
    /// the row" — but that surfacing was `coerce`'s type dispatch, and
    /// `ColKind::Other` (every text, date, JSON, blob and enum column) has none.
    /// So a `VLOOKUP` column's `#N/A` went into a `VARCHAR` with no issue, no
    /// warning and a report saying the rows imported — the case where the value
    /// is least recoverable.
    CellError,
    /// The field is NULL (or empty) but the column doesn't allow it.
    NullInNotNull,
    /// The record has a different number of fields than the header did.
    FieldCount {
        expected: usize,
        found: usize,
    },
}

impl IssueKind {
    /// A short, user-facing explanation for the preview's error list.
    pub fn message(self) -> String {
        match self {
            IssueKind::NotAnInteger => "not a whole number".into(),
            IssueKind::NotANumber => "not a number".into(),
            IssueKind::NotABoolean => "not a true/false value".into(),
            IssueKind::CellError => "the sheet could not evaluate this cell".into(),
            IssueKind::NullInNotNull => "empty, but the column can't be NULL".into(),
            IssueKind::FieldCount { expected, found } => {
                format!("has {found} fields, expected {expected}")
            }
        }
    }
}

/// One problem, located, so the preview can say *where* rather than just *that*.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    /// 1-based record number within the file (the header, when present, is
    /// record 0 — so this matches what a text editor shows).
    pub line: u64,
    /// The target column's name, or the file column's when it maps to nothing.
    pub column: String,
    /// The offending text, for the message.
    pub text: String,
    pub kind: IssueKind,
}

/// One preview cell, as the mapping step must draw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewCell<'a> {
    Text(&'a str),
    /// Drawn the way the grid draws a NULL — faint italic — so it cannot be
    /// mistaken for the text that spells it.
    Null,
}

/// Would this field land as NULL? The preview's question, answered the way the
/// **load** answers it.
///
/// **The preview could not show a CSV NULL at all**, which made both NULL
/// controls invisible in the one step whose stated job is verification by
/// looking. The view decided nullness from the [`Field`] alone, and `Field` is
/// `None` only for a *format-level* null — `read_csv_sample` builds every field
/// as `Some(..)` and never reads `cfg.nulls`, because for CSV whether empty
/// text means NULL is [`NullRule`]'s call and that call is made at coercion,
/// which the preview does not run. So `people.csv` = `id,note\n1,NULL\n2,\n`
/// with the defaults previewed row 1's `note` as the plain text `NULL` and row
/// 2's as an empty cell, both in the ordinary colour and both indistinguishable
/// from the string values `'NULL'` and `''` — while the import stored NULL in
/// both. Toggling "Empty field is NULL" **off** left the preview byte-identical
/// while row 2 now stored `''`: the control whose entire reason to exist is
/// that distinction changed nothing on screen, at any setting, for any value.
///
/// The one arm that *did* render the faint italic was reachable only for a
/// record shorter than the header — which for CSV is an
/// [`IssueKind::FieldCount`], not a null, so the single case that drew it drew
/// it for the wrong reason.
///
/// **`format` is asked, not assumed**, and through the same
/// [`ImportFormat::has_own_nulls`] that `validate` and `row_iter` ask: JSON and
/// Excel carry their own nulls, so applying the token rule to them would paint
/// every empty JSON string and empty Excel cell as NULL — a preview that
/// contradicts the file. Three places must not answer this differently, and now
/// none of them re-derives it.
pub fn preview_field<'a>(
    field: &'a Field,
    nulls: &NullRule,
    format: ImportFormat,
) -> PreviewCell<'a> {
    match field {
        None => PreviewCell::Null,
        Some(text) if !format.has_own_nulls() && nulls.matches(text) => PreviewCell::Null,
        Some(text) => PreviewCell::Text(text),
    }
}

/// Turn one field's text into a [`Value`] for `kind`, or say why it can't be.
///
/// `dialect` is needed only for booleans, and only because the engines genuinely
/// disagree there — see [`ColKind::Bool`].
pub fn coerce(
    text: &str,
    kind: ColKind,
    nullable: bool,
    nulls: &NullRule,
    dialect: SqlDialect,
) -> Result<Value, IssueKind> {
    if nulls.matches(text) {
        return if nullable {
            Ok(Value::Null)
        } else {
            Err(IssueKind::NullInNotNull)
        };
    }
    let t = text.trim();
    match kind {
        ColKind::Int => t
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|_| IssueKind::NotAnInteger),
        ColKind::Uint => t
            .parse::<u64>()
            .map(Value::UInt)
            .map_err(|_| IssueKind::NotAnInteger),
        ColKind::Float => match t.parse::<f64>() {
            // A non-finite has no SQL literal — `sql_literal` renders it NULL,
            // which would silently drop the value rather than report it.
            Ok(f) if f.is_finite() => Ok(Value::Float(f)),
            _ => Err(IssueKind::NotANumber),
        },
        // Shape-checked only; the text is what gets inserted, so no precision is
        // lost on the way through.
        //
        // **`is_finite`, the same term the `Float` arm above carries**, and this
        // arm did not: `NaN`, `inf`, `Infinity` and `1e400` all parse as `f64`,
        // so they passed the shape check and went into the `INSERT` verbatim
        // with `validate` reporting zero issues — the pass whose whole purpose
        // is "a single list of everything wrong, before anything is written".
        // One file, three answers: MariaDB 10.11.14 answers `ERROR 1366:
        // Incorrect decimal value: 'NaN'` and rolls back whichever batch it
        // landed in; PostgreSQL 16.15 **accepts** `'NaN'::numeric` and
        // `'Infinity'::numeric`, so the file imports clean and leaves a NaN in
        // an exact column, where it changes every `SUM`, `AVG` and comparison
        // over it afterwards.
        //
        // The parse is still only a *shape* check — the text is what is
        // inserted, so a value no `f64` can hold precisely still goes through
        // unrounded, which is the whole reason this arm is not `Float`.
        // **`is_finite` alone was a *range* test where a *shape* one was
        // wanted.** An unconstrained PostgreSQL `NUMERIC` holds 131,072 digits
        // before the point, so a 401-digit integer is an ordinary value the
        // server stores exactly — and `parse::<f64>` answers `inf` for it. The
        // arm then reported "not a number" and `RowCtx::row` turned the first
        // such issue into an `Err` that aborts the whole import, on a file
        // `v0.24.0` imported correctly. A plain decimal numeral is admitted
        // however long it is; `NaN`, `inf`, `Infinity` and `1e400` are not
        // numerals and stay refused.
        ColKind::Exact => match t.parse::<f64>() {
            Ok(f) if f.is_finite() || is_decimal_numeral(t) => Ok(Value::Str(t.to_string())),
            _ => Err(IssueKind::NotANumber),
        },
        ColKind::Bool => {
            let b = match t.to_ascii_lowercase().as_str() {
                "1" | "t" | "true" | "y" | "yes" | "on" => true,
                "0" | "f" | "false" | "n" | "no" | "off" => false,
                _ => return Err(IssueKind::NotABoolean),
            };
            Ok(if bool_literal_is_integer(dialect) {
                Value::Int(b as i64)
            } else {
                Value::Str(if b { "true".into() } else { "false".into() })
            })
        }
        ColKind::Other => Ok(Value::Str(text.to_string())),
    }
}

/// Does a boolean go into this engine as the **integer** `1`/`0` rather than the
/// quoted literal `'true'`/`'false'`?
///
/// **A capability, because the engine test got a third engine wrong.** It was
/// `MySql => integer, _ => quoted`, written when there were two engines and the
/// default arm meant PostgreSQL:
///
/// - **MySQL**'s `BOOLEAN` is a `TINYINT`. `'true'` stores as 0, silently.
/// - **SQLite**'s is a declared type with NUMERIC affinity, so `'true'` is kept
///   as **TEXT** — and a TEXT value in a boolean context converts to 0. Every row
///   imported as true became invisible to `WHERE flag` and was returned by
///   `WHERE NOT flag`, on the spelling SQLAlchemy, Django, Rails and EF Core all
///   emit. The integer is what SQLite's own `TRUE`/`FALSE` keywords produce.
/// - **PostgreSQL** has a real boolean type and *rejects* the integer 1 for it,
///   but takes the quoted literal. It is the exception, not the default.
fn bool_literal_is_integer(dialect: SqlDialect) -> bool {
    !matches!(dialect, SqlDialect::Postgres)
}

/// Everything needed to turn a file's bytes into fields — the dialect plus what
/// counts as NULL. Bundled because the preview, the validation pass and the
/// import itself must all read the file identically; passing them separately is
/// how a preview ends up showing something the import doesn't do.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ReadConfig {
    pub dialect: CsvDialect,
    pub nulls: NullRule,
    /// Strip surrounding whitespace from every field and header.
    ///
    /// Off by default, and deliberately so: trimming silently rewrites the data,
    /// and `" x "` may be exactly what the column holds. The preview shows the
    /// padding either way, which is what lets the user decide rather than guess.
    ///
    /// Note it trims **quoted** fields too — `"  padded  "` becomes `padded`.
    /// That's the `csv` reader's behaviour, not a choice made here, and it's the
    /// sharpest reason to leave this off unless a file actually needs it.
    pub trim: bool,
    /// Which worksheet to read, for [`ImportFormat::Xlsx`]. `None` means the
    /// **first** one, which is what a single-sheet workbook wants and what the
    /// probe fills in for every other.
    ///
    /// It lives on the shared config for the same reason everything else here
    /// does: the preview, the validation pass and the load must read the same
    /// bytes. A sheet chosen only at load time would import a table the user
    /// never saw.
    ///
    /// A name that no longer matches any sheet is an error rather than a
    /// silent fall back to the first — see [`xlsx_records`]. Importing a
    /// different table than the one on screen is the failure worth being loud
    /// about.
    pub sheet: Option<String>,
}

/// Why a file couldn't be read or planned at all — as opposed to an [`Issue`],
/// which is one bad cell in an otherwise workable file.
#[derive(Debug)]
pub enum ImportError {
    /// The file couldn't be parsed as delimited text at all.
    Read(String),
    /// Not one file column maps to a table column, so there's nothing to insert.
    /// Caught here rather than emitting `INSERT INTO t () VALUES ()`.
    NoColumnsMapped,
    /// A JSON file whose **first record alone** is larger than the preview
    /// reads, carrying the cap in bytes.
    ///
    /// **A variant rather than a `Read(String)`, because it is not a read
    /// failure and the caller has to be able to tell.** The file is valid and
    /// the whole-file walk reads it end to end; only the preview cannot be
    /// built from a prefix. Spelled as a message, it reached the modal through
    /// the same channel as "this file is corrupt", the sample stayed `None`, and
    /// Next is gated on the sample — so a valid file could not be imported at
    /// all. [`read_json_columns`] is the answer, and a caller can only reach for
    /// it if it can recognise the case.
    PreviewRecordTooLarge {
        /// The preview's byte cap, as [`SAMPLE_MAX_BYTES`].
        cap: u64,
    },
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Read(e) => write!(f, "Couldn't read the file: {e}"),
            ImportError::NoColumnsMapped => {
                write!(f, "No file columns are mapped to table columns")
            }
            ImportError::PreviewRecordTooLarge { cap } => write!(
                f,
                "its first JSON record is larger than the {} MiB the preview \
                 reads, so there is nothing to show",
                cap / (1024 * 1024)
            ),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<csv::Error> for ImportError {
    fn from(e: csv::Error) -> Self {
        ImportError::Read(e.to_string())
    }
}

/// The first `limit` records of a file, for the mapping step's preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    /// The header's names, or `Column N` placeholders when there isn't one. For
    /// JSON, the union of the sampled objects' keys, first-seen order.
    pub columns: Vec<String>,
    /// Raw field text in `columns` order — coercion happens later, so the preview
    /// can show what's in the file beside what it would become. `None` is a
    /// format-level null (see [`Field`]).
    pub rows: Vec<Vec<Field>>,
    /// More records exist beyond the sample.
    pub more: bool,
    /// The columns are real and the **values were not read** — see
    /// [`read_json_columns`].
    ///
    /// A flag rather than letting the view infer it from `rows.is_empty()`: a
    /// file with a header and no records has exactly that shape, and the note
    /// the two deserve is not the same one. `more && rows.is_empty()` happens to
    /// discriminate today, which is precisely the kind of accident that stops
    /// being true without anyone noticing.
    pub values_withheld: bool,
}

fn reader_for<R: std::io::Read>(r: R, cfg: &ReadConfig) -> csv::Reader<R> {
    let dialect = cfg.dialect;
    csv::ReaderBuilder::new()
        .delimiter(dialect.delimiter)
        .quote(dialect.quote)
        // `Trim::All` covers headers too — a padded header would otherwise fail
        // to name-match the column it obviously means.
        .trim(if cfg.trim {
            csv::Trim::All
        } else {
            csv::Trim::None
        })
        // Headers are taken by hand below so the header row can be treated as
        // data when the file doesn't have one.
        .has_headers(false)
        // Ragged records are reported as an `Issue` with a line number, not a
        // hard read error that says nothing about where the problem is.
        .flexible(true)
        .from_reader(r)
}

/// How many bytes the **preview** may read, whatever the file's shape.
///
/// A record-count limit is not a byte limit. `reader_for` sets no field- or
/// record-size bound, so a single stray `"` in a 1.5 GB CSV makes the whole
/// remainder one unterminated field: the sample "of 200 records" reads to EOF,
/// materialising the file as a `String` and again as a `StringRecord` — from a
/// file the user only meant to *look at*, on a modal that (until the `reading`
/// flag was split out) could not be dismissed while it happened.
///
/// The JSON side was already bounded, so the bound had been thought about for
/// one format and not the other. Generous enough that no real preview is
/// affected: 200 records of anything a person would import fits many times over.
pub const SAMPLE_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Read the first `limit` records for the preview, in whichever format.
///
/// Bounded at [`SAMPLE_MAX_BYTES`] — see there for the unterminated-quote case
/// that makes a record count no bound at all. A truncated read can only make the
/// preview *shorter*; the load itself is a separate pass over the whole file.
pub fn read_sample<R: std::io::Read>(
    r: R,
    format: ImportFormat,
    cfg: &ReadConfig,
    limit: usize,
) -> Result<Sample, ImportError> {
    // Not bounded at `SAMPLE_MAX_BYTES` for Excel: an `.xlsx` is a ZIP whose
    // directory is at the end of the file, so a truncated prefix does not open
    // at all — that cap would turn "preview a 9 MB workbook" into "this file is
    // corrupt". It is bounded at `XLSX_MAX_BYTES` instead, inside
    // [`open_xlsx`]; the disclosure for reading it whole is
    // [`xlsx_memory_warning`], and the *rows* are streamed either way.
    if format == ImportFormat::Xlsx {
        return read_xlsx_sample(r, cfg, limit);
    }
    let r = r.take(SAMPLE_MAX_BYTES);
    match format {
        ImportFormat::Csv => read_csv_sample(r, cfg, limit),
        ImportFormat::Json => read_json_sample(r, limit),
        ImportFormat::Xlsx => unreachable!("returned above"),
    }
}

/// Walk a file's records, calling `on_record` with each one's fields and its
/// location, until `on_record` returns `false`.
///
/// The single traversal both the preview and the validation pass go through, so
/// the rows the user reviewed are literally the rows that get checked.
fn for_each_record<R: std::io::Read>(
    r: R,
    format: ImportFormat,
    cfg: &ReadConfig,
    mut on_record: impl FnMut(Record, u64) -> bool,
) -> Result<(), ImportError> {
    match format {
        ImportFormat::Csv => {
            let mut rdr = reader_for(r, cfg);
            let mut records = rdr.records();
            if cfg.dialect.has_header {
                records.next().transpose()?;
            }
            for rec in records {
                let rec = rec?;
                // The record's real line, from the parser — counting by hand goes
                // wrong the moment a quoted field contains a newline, and a wrong
                // line number in an error list is worse than none.
                let line = rec.position().map(|p| p.line()).unwrap_or(0);
                let fields = rec.iter().map(|f| Some(f.to_string())).collect();
                if !on_record(Record::plain(fields), line) {
                    break;
                }
            }
        }
        ImportFormat::Json => {
            let mut keys: Vec<String> = Vec::new();
            json_records(r, &mut keys, usize::MAX, None, |fields, line| {
                on_record(Record::plain(fields), line)
            })?;
        }
        ImportFormat::Xlsx => {
            let mut names: Vec<String> = Vec::new();
            xlsx_records(r, cfg, &mut names, usize::MAX, on_record)?;
        }
    }
    Ok(())
}

fn read_csv_sample<R: std::io::Read>(
    r: R,
    cfg: &ReadConfig,
    limit: usize,
) -> Result<Sample, ImportError> {
    let mut rdr = reader_for(r, cfg);
    let mut records = rdr.records();

    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<Field>> = Vec::new();

    if cfg.dialect.has_header {
        match records.next() {
            Some(rec) => {
                let rec = rec?;
                columns = rec
                    .iter()
                    .enumerate()
                    .map(|(i, f)| if i == 0 { strip_bom(f) } else { f }.to_string())
                    .collect();
            }
            None => {
                return Ok(Sample {
                    columns,
                    rows,
                    more: false,
                    values_withheld: false,
                });
            }
        }
    }

    let mut more = false;
    for rec in records {
        let rec = rec?;
        if rows.len() >= limit {
            more = true;
            break;
        }
        let fields: Vec<Field> = rec
            .iter()
            .enumerate()
            .map(|(i, f)| {
                Some(
                    if i == 0 && !cfg.dialect.has_header && rows.is_empty() {
                        strip_bom(f)
                    } else {
                        f
                    }
                    .to_string(),
                )
            })
            .collect();
        if columns.is_empty() {
            columns = placeholder_columns(fields.len());
        }
        rows.push(fields);
    }
    Ok(Sample {
        columns,
        rows,
        more,
        values_withheld: false,
    })
}

/// Presents a JSON *array* of values as the whitespace-separated stream of those
/// values, so one reader handles both shapes people actually have: `[{…}, {…}]`
/// and newline-delimited `{…}\n{…}` (JSON Lines).
///
/// Without this, an array has to be deserialized whole before its first record is
/// available — so *previewing* a multi-gigabyte export costs as much as importing
/// it. Blanking the wrapping brackets and the commas between top-level elements
/// turns the array into exactly what [`serde_json::StreamDeserializer`] already
/// reads a value at a time, which is what makes the sample's record limit real
/// rather than nominal.
///
/// It rewrites bytes in place (every replacement is one byte wide, so nothing is
/// buffered) and only ever *blanks* structure it has accounted for: anything
/// malformed passes through to serde, which reports it properly. A file that
/// doesn't open with `[` is passed through untouched, so JSON Lines is unaffected.
struct ArrayUnwrap<R> {
    inner: R,
    /// `None` until the first non-whitespace byte says whether this is an array.
    array: Option<bool>,
    /// Nesting depth *within* the outer array. Commas and the closing bracket
    /// matter only at 0; deeper ones belong to a record and are left alone.
    depth: u32,
    in_string: bool,
    escaped: bool,
    /// How many bytes of a leading UTF-8 BOM have been blanked, capped at 3 —
    /// which also means "stop looking". See [`ArrayUnwrap::rewrite`].
    bom: usize,
}

impl<R: std::io::Read> ArrayUnwrap<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            array: None,
            depth: 0,
            in_string: false,
            escaped: false,
            bom: 0,
        }
    }

    fn rewrite(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            // Decide what kind of file this is on the first byte that isn't
            // whitespace, then never revisit it.
            if self.array.is_none() {
                // **A leading UTF-8 BOM first**, blanked to a space. Windows
                // PowerShell's UTF-8 encoders emit one, so this is what
                // `Set-Content -Encoding utf8` writes — and `serde_json` skips
                // only space/tab/CR/LF, so it stopped on `0xEF` and the user was
                // told *"expected value at line 1 column 1"* about a file that
                // is perfectly well formed. It also defeated the array
                // detection below, so a BOM'd `[{…}]` was not even unwrapped.
                //
                // `strip_bom` is the rule for a string; this is the same rule
                // for a byte stream that must not change length. Blanking
                // rather than removing keeps every later offset — and the caps
                // that count bytes — exactly where they were.
                if self.bom < 3 {
                    const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
                    if *b == BOM[self.bom] {
                        self.bom += 1;
                        *b = b' ';
                        continue;
                    }
                    // Not a BOM after all. A `0xEF` that opens a JSON document
                    // is not a value on any reading, so nothing legible was
                    // blanked; stop looking either way.
                    self.bom = 3;
                }
                if b.is_ascii_whitespace() {
                    continue;
                }
                self.array = Some(*b == b'[');
                if *b == b'[' {
                    *b = b' ';
                    continue;
                }
            }
            if self.array != Some(true) {
                return; // JSON Lines: nothing to rewrite, ever.
            }
            if self.in_string {
                if self.escaped {
                    self.escaped = false;
                } else if *b == b'\\' {
                    self.escaped = true;
                } else if *b == b'"' {
                    self.in_string = false;
                }
                continue;
            }
            match *b {
                b'"' => self.in_string = true,
                b'{' | b'[' => self.depth += 1,
                // At depth 0 this is the array's own `]`; anything after it
                // should be whitespace, and serde says so if it isn't.
                b']' | b'}' if self.depth == 0 => *b = b' ',
                b']' | b'}' => self.depth -= 1,
                b',' if self.depth == 0 => *b = b' ',
                _ => {}
            }
        }
    }
}

impl<R: std::io::Read> std::io::Read for ArrayUnwrap<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.rewrite(&mut buf[..n]);
        Ok(n)
    }
}

/// Counts the bytes handed out, through a shared cell the caller keeps.
///
/// **So [`json_records`] can tell two things apart that look identical from
/// inside the deserializer**: the byte cap arriving, and a file that really is
/// truncated. Both are `Error::is_eof`. `read_sample` builds the
/// `Read::take(SAMPLE_MAX_BYTES)` and discards the handle, and
/// `StreamDeserializer` never gives its reader back, so the count is taken on
/// the way past instead.
struct Counting<R> {
    inner: R,
    seen: std::rc::Rc<std::cell::Cell<u64>>,
}

impl<R: std::io::Read> std::io::Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.seen.set(self.seen.get() + n as u64);
        Ok(n)
    }
}

/// Walk a JSON source's records.
///
/// Both shapes stream a record at a time — an array via [`ArrayUnwrap`], JSON
/// Lines natively — so reading a sample really does stop at the sample.
///
/// `keys` accumulates the columns across every object, so a later record
/// carrying an extra key widens the set instead of being dropped. That
/// accumulation is why a *whole-file* walk (validate, import) still holds the
/// records it has seen: every one has to be emitted against the final key set.
/// Sampling doesn't, since `limit` bounds it.
///
/// Their *order* is alphabetical, not the document's: `serde_json::Map` is a
/// `BTreeMap` without the `preserve_order` feature, and turning that feature on
/// would reorder every other `serde_json::Map` in the workspace to fix a
/// cosmetic issue here. JSON records are matched to columns by name and inserted
/// in table order, so key order only affects how the preview lays its columns
/// out.
/// `bounded` says the reader is a **prefix** of the file — a
/// `Read::take(SAMPLE_MAX_BYTES)` — so running out of input mid-value is the cap
/// arriving, not a broken file.
///
/// **That distinction is the whole of B6.1-L1-02.** `read_sample` wraps both
/// non-Excel formats in the byte cap, and CSV degrades gracefully under
/// truncation (the reader simply yields fewer records) while `serde_json`'s
/// `StreamDeserializer` meets EOF *inside* a value and errors — which reached
/// the user as `Couldn't read the file: EOF while parsing a string at line 1
/// column 8388608`, a message that reads as file corruption, on a file
/// `validate` then walks end to end without complaint. The trigger is the first
/// `limit` records exceeding the cap, i.e. an average record over ~42 KiB,
/// which one text column reaches easily: a JSON export of a hundred support
/// tickets is inside it. `read_sample`'s own doc says "a truncated read can
/// only make the preview *shorter*", and it could not.
///
/// A whole-file walk passes `None`, so a genuinely truncated file still fails
/// there rather than importing a prefix in silence — which is why this is a
/// parameter and not an unconditional `is_eof` arm.
///
/// **It carries the cap rather than a flag because "did anything parse" was the
/// wrong question.** With a first record over the cap — one object with a large
/// text or base64 column — `objects` is still empty when EOF arrives, so the arm
/// did not fire and the corruption message came back on a valid file. The reader
/// knows the right fact and the deserializer does not, so [`Counting`] carries it
/// out: the cap arriving and a short file are then two different answers.
fn json_records<R: std::io::Read>(
    r: R,
    keys: &mut Vec<String>,
    limit: usize,
    bounded: Option<u64>,
    mut on_record: impl FnMut(Vec<Field>, u64) -> bool,
) -> Result<bool, ImportError> {
    // Collected first so every record can be emitted against the *final* key set
    // — otherwise the first row's fields wouldn't line up with a column list that
    // grew later.
    let mut objects: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
    let mut more = false;

    let seen = std::rc::Rc::new(std::cell::Cell::new(0u64));
    let stream = serde_json::Deserializer::from_reader(ArrayUnwrap::new(Counting {
        inner: r,
        seen: seen.clone(),
    }))
    .into_iter::<serde_json::Value>();
    for v in stream {
        // Past the limit nothing more needs reading — and for a large file that's
        // the whole point, so stop before deserializing another record.
        if objects.len() >= limit {
            more = true;
            break;
        }
        let v = match v {
            Ok(v) => v,
            Err(e) => {
                if let Some(cap) = bounded
                    && e.is_eof()
                {
                    // The cap, not a broken file — and something parsed, so the
                    // preview is simply shorter.
                    if !objects.is_empty() {
                        more = true;
                        break;
                    }
                    // Nothing parsed, and the reader delivered every byte the
                    // cap allows: the first record alone is bigger than the
                    // preview reads. Its own variant, rather than serde's
                    // `EOF while parsing` or a sentence — the file is fine, the
                    // whole-file walk reads it, and the caller has to be able to
                    // tell this from corruption so it can fall back to
                    // [`read_json_columns`].
                    if seen.get() >= cap {
                        return Err(ImportError::PreviewRecordTooLarge { cap });
                    }
                }
                return Err(ImportError::Read(e.to_string()));
            }
        };
        let serde_json::Value::Object(map) = v else {
            return Err(ImportError::Read(
                "expected JSON objects (an array of them, or one per line)".into(),
            ));
        };
        for k in map.keys() {
            if !keys.iter().any(|s| s == k) {
                keys.push(k.clone());
            }
        }
        objects.push(map);
    }

    // `drain`, not `iter`: the caller turns each record into its own `Vec<Field>`
    // and keeps it, so with a borrow both full materializations were alive at
    // once — measured at 7× the file size against 5× for this buffer alone.
    // Draining frees each parsed record as its fields are built, so the peak is
    // the larger of the two rather than their sum.
    for (i, map) in objects.drain(..).enumerate() {
        let fields = keys
            .iter()
            .map(|k| match map.get(k) {
                // A key that's absent, or explicitly null, is a real null — not
                // the empty string, and not subject to the NULL-token rule.
                None | Some(serde_json::Value::Null) => None,
                // A JSON string is used as-is; anything else (number, bool,
                // nested object/array) becomes its JSON text, which is both what
                // a numeric column wants and what a JSON column wants.
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                Some(other) => Some(other.to_string()),
            })
            .collect();
        // No line numbers in a JSON array, so records are numbered from 1 — see
        // `Issue::line`.
        if !on_record(fields, i as u64 + 1) {
            break;
        }
    }
    Ok(more)
}

/// The largest `.xlsx` the importer will open.
///
/// **A refusal, because this format cannot be previewed cheaply.** CSV and JSON
/// bound their preview at [`SAMPLE_MAX_BYTES`] and read no further, so a huge
/// file of either costs 8 MiB to look at and the memory warning arrives before
/// anything expensive happens. A ZIP has its directory at the end, so a workbook
/// must be read whole before its first row can be seen — which put the
/// unbounded read *before* the warning meant to precede it, and left the app
/// able to die at preview time on a file the user only meant to glance at.
///
/// Generous enough that no workbook a person would import is affected: Excel
/// itself is unhappy well below this.
pub const XLSX_MAX_BYTES: u64 = 512 * 1024 * 1024;

/// The largest an `.xlsx` may **unpack** to.
///
/// **[`XLSX_MAX_BYTES`] bounds the wrong quantity.** Both of its enforcement
/// points measure compressed bytes — the file's size on disk, and the bytes read
/// through [`open_xlsx`]'s `take` — and [`xlsx_memory_warning`] is derived from
/// the same figure. What is actually held in memory is the *inflated* archive:
/// calamine materialises the shared-strings table and the sheet XML, with no
/// ceiling of its own. A 4 MiB workbook whose `xl/sharedStrings.xml` is a few
/// hundred megabytes of one repeated byte passes every compressed-size check,
/// gets a "this is a small workbook" disclosure, and takes the process out — at
/// *preview* time, on a probe the import modal re-fires on every settings
/// change, on a file the user only meant to glance at.
///
/// Same shape as the declared sheet width: a figure the workbook supplies,
/// trusted to describe what the workbook costs. `sheet_width`'s refusal and the
/// JSON preview cap both bound the inflated quantity; this is the third.
///
/// **What this closes and what it does not.** The sum is read from the central
/// directory before a byte is inflated, so it costs nothing on an ordinary file
/// and refuses the way a bomb is actually built — an archive whose declared
/// sizes are honest, because a bomb has to stay a valid archive that ordinary
/// tools will unpack. An archive that *understates* its entries in the directory
/// is not caught here: the `zip` reader does not stop a decompressor at the
/// declared size, so closing that would mean inflating the whole archive once
/// ourselves and throwing it away, doubling the cost of every preview of every
/// legitimate workbook.
pub const XLSX_MAX_INFLATED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Why this file cannot be opened at all, or `None` — asked from the file's
/// *size on disk*, before a byte of it is read.
pub fn xlsx_size_refusal(format: ImportFormat, file_bytes: u64) -> Option<String> {
    if format != ImportFormat::Xlsx || file_bytes <= XLSX_MAX_BYTES {
        return None;
    }
    Some(oversize_workbook(Some(file_bytes)))
}

/// The Excel preview **and** the workbook's sheet names, from one read of the
/// file.
///
/// The two together because they come from the same parse: the probe used to
/// call a separate `xlsx_sheet_names`, which opened and inflated the whole
/// workbook a second time to populate a dropdown — doubling the cost of a probe
/// that fires on every settings change.
pub fn read_workbook_sample<R: std::io::Read>(
    r: R,
    cfg: &ReadConfig,
    limit: usize,
) -> Result<(Sample, Vec<String>), ImportError> {
    use calamine::Reader;
    let mut wb = open_xlsx(r)?;
    let sheets = wb.sheet_names().to_vec();
    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<Field>> = Vec::new();
    // The preview shows an error cell as the text Excel shows, which is what
    // `rec.fields` already holds — `sheet_errors` is the coercion's question,
    // not the rendering's.
    let more = xlsx_rows(&mut wb, &sheets, cfg, &mut columns, limit, |rec, _| {
        rows.push(rec.fields);
        true
    })?;
    Ok((
        Sample {
            columns,
            rows,
            more,
            values_withheld: false,
        },
        sheets,
    ))
}

/// Read the whole reader and open it as a workbook.
///
/// **The whole file, deliberately.** An `.xlsx` is a ZIP, so it cannot be read
/// as a prefix at all — its central directory is at the *end*, and a truncated
/// one does not open. That rules out [`SAMPLE_MAX_BYTES`]'s approach for this
/// format (see [`read_sample`]) and makes an Excel import memory-bound like
/// JSON's rather than streamed like CSV's; [`xlsx_memory_warning`] is the
/// disclosure of that.
/// **The ceiling is enforced here, not only where the file is picked.**
/// [`xlsx_size_refusal`] answers from the size on disk so the app can refuse
/// without opening anything, but that is a step a *launcher* has to remember —
/// and only the probe did. The two load-path opens went straight to
/// `read_to_end`. Reading through a `take` makes the bound a property of the
/// reader every path shares; the launcher's check survives because it can say
/// so before the file is touched, and because it names the size.
fn open_xlsx<R: std::io::Read>(
    r: R,
) -> Result<calamine::Xlsx<std::io::Cursor<Vec<u8>>>, ImportError> {
    open_xlsx_within(r, XLSX_MAX_INFLATED_BYTES)
}

/// [`open_xlsx`] with the inflated ceiling named.
///
/// The ceiling is a parameter for one reason: a test that builds a
/// two-gigabyte fixture is not a test anyone runs, and the decision worth
/// pinning is that opening a workbook consults the bound at all — the seam,
/// not the predicate.
fn open_xlsx_within<R: std::io::Read>(
    r: R,
    inflated_ceiling: u64,
) -> Result<calamine::Xlsx<std::io::Cursor<Vec<u8>>>, ImportError> {
    use calamine::Reader;
    let mut bytes = Vec::new();
    // One byte past the ceiling, so "exactly at it" and "over it" are
    // distinguishable without reading the rest.
    let mut r = std::io::Read::take(r, XLSX_MAX_BYTES + 1);
    std::io::Read::read_to_end(&mut r, &mut bytes).map_err(|e| ImportError::Read(e.to_string()))?;
    if bytes.len() as u64 > XLSX_MAX_BYTES {
        return Err(ImportError::Read(oversize_workbook(None)));
    }
    // The archive's own size said nothing about what opening it costs — see
    // [`XLSX_MAX_INFLATED_BYTES`]. Asked here, between the read and the inflate,
    // because this is the one place both are in scope.
    if let Some(msg) = inflated_refusal(&bytes, inflated_ceiling) {
        return Err(ImportError::Read(msg));
    }
    calamine::Xlsx::new(std::io::Cursor::new(bytes)).map_err(|e| ImportError::Read(e.to_string()))
}

/// Why this archive unpacks to more than `ceiling`, or `None`.
///
/// Reads the central directory only — `by_index_raw` decompresses nothing — so
/// an ordinary workbook pays one pass over its entry list.
///
/// **Anything that isn't a readable archive answers `None`**, deliberately:
/// calamine is about to open the same bytes and its own message says what is
/// wrong with them. A refusal here would replace a real diagnosis with a size
/// complaint about a file that has no size problem.
///
/// **An entry this cannot size is skipped, not an exit.** That sentence above
/// is about the *archive*, and the same `?` was once applied to a case it does
/// not cover: a readable archive holding one entry whose record the scan could
/// not follow. `by_index_raw` is not a directory read — it seeks to the entry's
/// declared `local_header_offset` and checks the local-file-header magic, which
/// `ZipArchive::new` never does — so a four-byte tamper to the central-directory
/// record of an entry no workbook reader opens turned the ceiling off for the
/// whole archive, while calamine (which reaches every part `by_name`) read it
/// happily. Skipping is safe in the direction that matters: the sum is a lower
/// bound and the message says so.
///
/// The ceiling is a parameter so the decision is testable without building a
/// two-gigabyte fixture; the one caller passes [`XLSX_MAX_INFLATED_BYTES`].
fn inflated_refusal(bytes: &[u8], ceiling: u64) -> Option<String> {
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let mut total: u64 = 0;
    for i in 0..zin.len() {
        let Ok(entry) = zin.by_index_raw(i) else {
            continue;
        };
        total = total.saturating_add(entry.size());
        if total > ceiling {
            // **What the sentence may promise is bounded by what the sum
            // measures.** It measures *declared uncompressed XML bytes*; what
            // calamine then holds is the parsed shared-strings table, a
            // `Vec<String>` filled eagerly before any sheet is touched. Measured
            // against calamine 0.36.1 with a counting allocator, a 64 MiB
            // `xl/sharedStrings.xml` of minimal `<si><t>a</t></si>` entries costs
            // 99 MB live — **1.48×** the quantity the bound counts, and that is a
            // floor: the allocator charges `Layout::size()`, so a one-character
            // `String` counts 1 byte where a real allocator's minimum chunk is
            // 16–32, and `<si><t/></si>` pushes it to ~1.85× with no rounding at
            // all. So "the most this can hold in memory" was a promise in the
            // wrong unit, understated by about half. The sentence now says what
            // the bound is actually about.
            return Some(format!(
                "This workbook is {} on disk but unpacks to at least {}, and an Excel file has \
                 to be read whole before any of it can be shown — {} of unpacked content is the \
                 most this will open, and it needs more memory than that to hold it. Export the \
                 sheet as CSV and import that instead; a CSV loads in constant memory.",
                crate::format::human_bytes(bytes.len() as i64),
                crate::format::human_bytes(total as i64),
                crate::format::human_bytes(ceiling as i64),
            ));
        }
    }
    None
}

/// The refusal for a workbook past [`XLSX_MAX_BYTES`], with the size named when
/// the caller knows it.
///
/// One sentence, two callers: [`xlsx_size_refusal`], which is asked before the
/// file is opened and can say how big it is, and [`open_xlsx`], which is reading
/// through a cap and only knows it was exceeded.
fn oversize_workbook(file_bytes: Option<u64>) -> String {
    let size = match file_bytes {
        Some(n) => format!("This workbook is {}", crate::format::human_bytes(n as i64)),
        None => format!(
            "This workbook is over {}",
            crate::format::human_bytes(XLSX_MAX_BYTES as i64)
        ),
    };
    format!(
        "{size}, and an Excel file has to be read whole before any of it can be \
         shown — {} is the most this can open. Export the sheet as CSV and \
         import that instead; a CSV loads in constant memory.",
        crate::format::human_bytes(XLSX_MAX_BYTES as i64)
    )
}

/// Walk one worksheet's rows as [`Field`]s, the way [`json_records`] walks a
/// JSON document — the single reader the preview, the validation pass and the
/// load all go through.
///
/// `columns` is filled with the header names (or `Column N` placeholders when
/// the sheet has no header row). Returns whether rows exist beyond `limit`.
///
/// The row number handed to `on_record` is the **worksheet row**, 1-based and
/// counted from the top of the *sheet* — the number in Excel's own margin, and
/// the equivalent of a CSV's line number.
///
/// **It is not the offset within the used range**, which is what it would be if
/// the range were simply enumerated. `worksheet_range` returns the *used* range,
/// whose origin is the first cell that holds anything: a sheet with a title
/// block above its header starts at row 5, and enumerating from zero would send
/// the user to look at row 2, which is blank. `range.start()` is the correction.
///
/// **A row with nothing in any cell is not a record.** The used range is a
/// rectangle, so a blank spacer row between two blocks of data — routine in a
/// hand-made spreadsheet — arrives as a row of empty cells, and emitting it
/// would insert a row of NULLs nobody typed (or fail the whole import on the
/// first NOT NULL column). CSV has no equivalent: its reader yields no record
/// for a blank line. Only a *wholly* empty row is skipped; one blank cell among
/// values is a real NULL and is kept.
fn xlsx_records<R: std::io::Read>(
    r: R,
    cfg: &ReadConfig,
    columns: &mut Vec<String>,
    limit: usize,
    on_record: impl FnMut(Record, u64) -> bool,
) -> Result<bool, ImportError> {
    use calamine::Reader;
    let mut wb = open_xlsx(r)?;
    let names = wb.sheet_names().to_vec();
    xlsx_rows(&mut wb, &names, cfg, columns, limit, on_record)
}

/// [`xlsx_records`] over an already-opened workbook, so the preview can take
/// the sheet names and the rows from one parse ([`read_workbook_sample`]).
fn xlsx_rows(
    wb: &mut calamine::Xlsx<std::io::Cursor<Vec<u8>>>,
    names: &[String],
    cfg: &ReadConfig,
    columns: &mut Vec<String>,
    limit: usize,
    mut on_record: impl FnMut(Record, u64) -> bool,
) -> Result<bool, ImportError> {
    let name = match &cfg.sheet {
        // Not a fall back to the first sheet: a workbook the user edited between
        // the preview and the load would then import a different table than the
        // one they mapped, silently.
        Some(s) => {
            if !names.iter().any(|n| n == s) {
                return Err(ImportError::Read(format!(
                    "this workbook has no sheet called \"{s}\" — it has {}",
                    names.join(", ")
                )));
            }
            s.clone()
        }
        None => names
            .first()
            .cloned()
            .ok_or_else(|| ImportError::Read("this workbook has no sheets".into()))?,
    };
    let mut cells = wb
        .worksheet_cells_reader(&name)
        .map_err(|e| ImportError::Read(e.to_string()))?;
    let dims = cells.dimensions();
    // The **declared** extent, which is a ceiling and a starting guess and not
    // the width. See `sheet_width`.
    let mut width = sheet_width(dims)?;
    // The used range's own left edge. A sheet with a title block starts partway
    // across, and its first data column is the row's column 0 — the same
    // correction `range.start()` used to make, and the reason a cell is placed
    // relative to this rather than at its absolute column.
    let mut left = dims.start.1;
    // **The declaration is advisory, so the first row gets to widen it.**
    // ECMA-376 makes `<dimension>` optional, and a writer that streams cannot
    // know the extent in advance — so `ref="A1"`, an omitted element and a stale
    // `ref="B1:C3"` all exist in the wild, and each of them made a three-column
    // sheet import as one, silently, in the preview and in the load. Open while
    // the first row is being assembled and shut the moment it is emitted:
    // every record has to carry the same field count, and the first row is the
    // header, which is the full column list by definition.
    let mut geometry_open = true;

    // Records emitted so far. Not the row's index: a skipped blank row advances
    // one and not the other, which is the whole reason the two are separate.
    let mut seen = 0usize;
    let mut took_header = !cfg.dialect.has_header;
    // One row at a time. Cells arrive in sheet order and an empty cell is not
    // emitted at all, so a row is complete when a cell for a later row shows up
    // — and a *wholly* empty row never appears, which is exactly the rule the
    // doc above states, arrived at for free instead of by a filter.
    let mut row: Vec<Option<String>> = vec![None; width];
    // Which of this row's slots came from a cell the sheet could not evaluate —
    // see [`Record`]. A sparse index list rather than a parallel `Vec<bool>`
    // precisely so it has no width to keep in step with `row`'s: the only two
    // things that touch it are the push at the placement site and the take at
    // the emit site, both below.
    let mut errs: Vec<usize> = Vec::new();
    let mut at: Option<u32> = None;
    loop {
        let cell = cells
            .next_cell()
            .map_err(|e| ImportError::Read(e.to_string()))?;
        let finished = match &cell {
            Some(c) => at.is_some_and(|r| r != c.get_position().0),
            None => at.is_some(),
        };
        if finished {
            // `at` is 0-based within the sheet, so the number Excel shows in its
            // margin — the equivalent of a CSV's line number — is one more.
            let line = u64::from(at.take().unwrap_or(0)) + 1;
            // The first row settles the geometry, and nothing may widen it after
            // this point: every record has to carry the same field count, or the
            // mismatch report becomes noise on every row after the widest one.
            geometry_open = false;
            let full = std::mem::replace(&mut row, vec![None; width]);
            // Taken on every emitted row *and* on the header row, which is why
            // it sits beside the replace above rather than in the data branch.
            let errs = std::mem::take(&mut errs);
            // A headerless sheet names its columns from that settled width,
            // here rather than before the loop — before the loop the width was
            // still only the file's claim.
            if !cfg.dialect.has_header && columns.is_empty() {
                columns.extend((1..=width).map(|i| format!("Column {i}")));
            }
            if !took_header {
                for (i, c) in full.into_iter().enumerate() {
                    let name = c.unwrap_or_default();
                    // The shared strip, not a fourth reader of its own. A BOM
                    // that survived a round-trip through a BOM'd CSV lands
                    // *inside* the first header name and silently breaks
                    // name-matching on the very first column — the failure this
                    // module's own header comment names, and which the CSV path
                    // was the only one taking the cure for.
                    let name = strip_bom(&name);
                    let name = if cfg.trim { name.trim() } else { name };
                    columns.push(if name.is_empty() {
                        format!("Column {}", i + 1)
                    } else {
                        name.to_string()
                    });
                }
                took_header = true;
            } else {
                // Asked *after* a whole row has been assembled, so a sheet
                // padded with empty rows past its data does not report there is
                // more to read.
                if seen >= limit {
                    return Ok(true);
                }
                seen += 1;
                let rec = Record {
                    fields: full,
                    sheet_errors: errs,
                };
                if !on_record(rec, line) {
                    return Ok(false);
                }
            }
        }
        let Some(c) = cell else { break };
        let (r, col) = c.get_position();
        // **Widen to the cell while the first row is still being assembled.**
        // Cells arrive in sheet order, so the first one seen is the leftmost of
        // the first non-empty row: it corrects a declared origin that starts too
        // far right (a stale `ref="B1:C3"` over data that really begins at A),
        // and only ever moves `left` *outwards*, so a genuine title block still
        // reports its own first data column as column 0.
        if geometry_open {
            // Moving `left` is safe only while nothing has been placed — the
            // slots are indexed relative to it — and by the ordering above that
            // is exactly the first cell of the first row.
            if col < left && row.iter().all(Option::is_none) {
                width += (left - col) as usize;
                left = col;
            }
            let need = col.saturating_sub(left) as usize + 1;
            // `XLSX_MAX_COLS` is still the ceiling `sheet_width` holds a sheet's
            // claim to; the cells cannot raise it either.
            if need > width && need as u64 <= XLSX_MAX_COLS {
                width = need;
            }
            if row.len() < width {
                row.resize(width, None);
            }
        }
        at = Some(r);
        let value: calamine::Data = c.get_value().clone().into();
        // **Asked here and nowhere else.** `cell_text` is about to render an
        // error cell with Excel's own spelling, which is the right thing to
        // show and the point at which it stops being distinguishable from a
        // string cell holding those same characters. See [`Record`].
        let broken = matches!(value, calamine::Data::Error(_));
        let Some(text) = cell_text(&value) else {
            continue;
        };
        let text = if cfg.trim {
            text.trim().to_string()
        } else {
            text
        };
        // A cell *left* of the settled origin is still dropped: the slots are
        // indexed relative to `left`, and moving it after the first row would
        // shift every value already placed onto the wrong column.
        let Some(i) = col.checked_sub(left).map(|i| i as usize) else {
            continue;
        };
        // **A cell right of the settled width widens this row, and only this
        // row.** It used to be dropped here, and the drop had no reporter:
        // `trim_to_mapping`'s doc promised that "a worksheet's columns are fixed
        // by its used range, so every row is already the same width and a count
        // mismatch is a real one worth reporting" — true of the arrival, and
        // that was the defect. Padding every row to `width` made
        // `coerce_record`'s mismatch branch unreachable *by construction* for
        // Excel, so a two-column header over three-column data (a streaming
        // writer with no `<dimension>`, or a stale one) previewed two columns,
        // said nothing in `missing_required` when the lost column was nullable,
        // and reported the right number of rows having written two-thirds of the
        // data. CSV refuses the identical file with `IssueKind::FieldCount`.
        //
        // Widening per row is what makes the count differ, which is what
        // `coerce_record` already reports — the same path, and the same message,
        // a ragged CSV row takes. `XLSX_MAX_COLS` still bounds it: the cells
        // could not raise the ceiling while the geometry was open and cannot
        // raise it now.
        if i >= row.len() {
            if i as u64 >= XLSX_MAX_COLS {
                continue;
            }
            row.resize(i + 1, None);
        }
        row[i] = Some(text);
        if broken {
            errs.push(i);
        }
    }
    // A sheet with no cells at all: no header to take, and no rows to emit.
    Ok(false)
}

/// The starting width and the **ceiling**, from the extent the sheet declares.
///
/// **Not the width.** `<dimension>` is optional and advisory in ECMA-376 — a
/// writer that streams cannot know the extent in advance — so `ref="A1"`, an
/// omitted element and a stale `ref="B1:C3"` all exist, and each of them made a
/// three-column sheet import as one column: the preview agreed with the load,
/// `missing_required` had nothing to say (the dropped columns are nullable), and
/// the import reported the right number of rows having written a third of the
/// data. The caller widens this to the first row's own cells, which is where the
/// truth is; the doc that stood here reasoned the absent case to the wrong
/// answer ("either way one column is the right answer, and the header row is
/// what names them"), which is true of a one-cell sheet and false of every sheet
/// anyone imports.
///
/// **What it still decides is the only unbounded thing in an Excel import.** The
/// rows are streamed, so a sheet's height costs nothing to skip past; a row
/// buffer is the one allocation whose size the file controls, and Excel's own
/// ceiling is 16,384 columns. A workbook claiming more is refused here rather
/// than believed, and the cells cannot raise that ceiling either.
///
/// This is what replaced materialising the sheet. `worksheet_range` builds the
/// **dense** bounding rectangle of every cell present, so a legal 5,461-byte
/// workbook holding two cells at opposite corners cost 262 MB — 48,099× the file
/// — and the worst legal sheet 550 GB, which is not an error but
/// `handle_alloc_error`, i.e. the process. Both size guards measured the file on
/// disk, which is the wrong quantity in the wrong place: the same call is made
/// by the preview and by both load-path opens, so a guard the *launcher*
/// remembers is a guard two of the three do not have.
fn sheet_width(dims: calamine::Dimensions) -> Result<usize, ImportError> {
    // A sheet with no `<dimension>` element reads as the degenerate (0,0)-(0,0),
    // which is also what a one-cell sheet reads as. One column is the right
    // *start* for both, and the first row's cells settle which it was.
    let width = u64::from(dims.end.1.saturating_sub(dims.start.1)) + 1;
    if width > XLSX_MAX_COLS {
        return Err(ImportError::Read(format!(
            "this sheet claims {width} columns; an Excel worksheet holds {XLSX_MAX_COLS}. \
             Export it as CSV and import that instead."
        )));
    }
    Ok(width as usize)
}

/// Columns in an Excel worksheet — the ceiling [`sheet_width`] holds a sheet's
/// claim to.
pub const XLSX_MAX_COLS: u64 = 16_384;

/// An Excel duration — `days` as the fraction of a day it stores — as
/// `[-]H:MM:SS`, the form a `TIME` column reads.
///
/// **Not a decimal number of hours**, which is what a duration's underlying
/// serial looks like and what this used to emit. MySQL parses a bare decimal in
/// a `TIME` context as *seconds*, so a timesheet's 8h30m went in as `8.500000`
/// and was stored as eight and a half **seconds** — wrong by a factor of 3600,
/// and silent, because the value coerces perfectly well. PostgreSQL's `INTERVAL`
/// rejects the bare number instead, so the same file failed on one engine and
/// corrupted on the other.
///
/// Hours are **not** wrapped at 24: an elapsed time of 36 hours is `36:00:00`,
/// which is what Excel's own `[h]:mm:ss` format means and what MySQL `TIME`
/// accepts (its range is ±838:59:59).
fn duration_hms(days: f64) -> String {
    let sign = if days < 0.0 { "-" } else { "" };
    // Rounded to the second before splitting, so 0.9999999 of a day is 24:00:00
    // rather than 23:59:59 with a discarded remainder.
    let total = (days.abs() * 86_400.0).round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    format!("{sign}{h}:{m:02}:{s:02}")
}

/// One worksheet cell as the text an import coerces, or `None` for a cell that
/// holds nothing.
///
/// **An empty cell is a real null**, like JSON's — not the empty string, and not
/// subject to [`NullRule`]. That is what [`ImportFormat::has_own_nulls`] says
/// about this format.
///
/// The conversions are chosen so a value that came *out* of a database and back
/// in again survives the trip:
///
/// - A **number** is written with `f64`'s own shortest round-trip form, so a
///   whole number is `7` rather than `7.0` — an integer column would reject the
///   latter, and Excel has no separate integer type to distinguish them.
/// - A **date or time** becomes ISO 8601, which every engine's date parser
///   accepts. Excel stores these as a serial number with a display format, so
///   the alternative is importing `45292` into a `DATE` column.
/// - A **duration** becomes `H:MM:SS` — see [`duration_hms`], which is where
///   the 3600× trap lives.
/// - A **formula error** becomes Excel's own spelling of it (`#REF!`,
///   `#DIV/0!`) rather than a null: it is a cell the sheet itself could not
///   evaluate, and [`Record::sheet_errors`] — recorded by the caller, which
///   still has the `Data` variant this rendering flattens — is what turns that
///   into an [`IssueKind::CellError`] naming the row, where a silent null
///   would not.
fn cell_text(c: &calamine::Data) -> Field {
    use calamine::Data;
    match c {
        Data::Empty => None,
        Data::String(s) => Some(s.clone()),
        Data::Int(i) => Some(i.to_string()),
        Data::Float(f) => Some(f.to_string()),
        // `1`/`0`, not `true`/`false`. Both spellings are in `ColKind::Bool`'s
        // accepted list, so this is not a trade between engines — it is the one
        // that also works where the column is *not* classified `Bool`. MySQL and
        // MariaDB report a `BOOLEAN` column as `tinyint(1)`, so `classify` gives
        // `ColKind::Int` and `coerce("true", Int)` is `NotAnInteger`: every row
        // of an ordinary spreadsheet's TRUE/FALSE column was refused, and the
        // same workbook imported fine on PostgreSQL and SQLite. The asymmetry
        // is already written down in `bool_literal_is_integer`; this was the one
        // place in the file that forgot it.
        Data::Bool(b) => Some(if *b { "1".into() } else { "0".into() }),
        Data::DateTimeIso(s) | Data::DurationIso(s) => Some(s.clone()),
        // A duration is an elapsed time and not a clock time, so it is the one
        // kind `to_ymd_hms_milli` says nothing useful about — hence the split
        // before that call rather than after it.
        Data::DateTime(d) if d.is_duration() => Some(duration_hms(d.as_f64())),
        Data::DateTime(d) => {
            let (y, mo, da, h, mi, s, ms) = d.to_ymd_hms_milli();
            Some(match (h, mi, s, ms) {
                // A pure date: no clock part to write, and a `DATE` column takes
                // the short form directly.
                (0, 0, 0, 0) => format!("{y:04}-{mo:02}-{da:02}"),
                (_, _, _, 0) => format!("{y:04}-{mo:02}-{da:02} {h:02}:{mi:02}:{s:02}"),
                _ => format!("{y:04}-{mo:02}-{da:02} {h:02}:{mi:02}:{s:02}.{ms:03}"),
            })
        }
        // `Display`, not `Debug`: calamine's `Display` is Excel's own spelling
        // (`#DIV/0!`), and `Debug` is the Rust variant name (`Div0`) — a token
        // that appears nowhere in Excel, so a user could not connect the issue
        // to the cell it names.
        Data::Error(e) => Some(e.to_string()),
    }
}

fn read_xlsx_sample<R: std::io::Read>(
    r: R,
    cfg: &ReadConfig,
    limit: usize,
) -> Result<Sample, ImportError> {
    read_workbook_sample(r, cfg, limit).map(|(s, _)| s)
}

fn read_json_sample<R: std::io::Read>(r: R, limit: usize) -> Result<Sample, ImportError> {
    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<Field>> = Vec::new();
    let more = json_records(
        r,
        &mut columns,
        limit,
        Some(SAMPLE_MAX_BYTES),
        |fields, _| {
            rows.push(fields);
            true
        },
    )?;
    Ok(Sample {
        columns,
        rows,
        more,
        values_withheld: false,
    })
}

/// The columns of a JSON file whose first record the preview cannot hold —
/// **keys only, values skipped**.
///
/// The answer to [`ImportError::PreviewRecordTooLarge`]. The mapping step needs
/// a column list and nothing else to be usable; the preview's rows are a
/// convenience. So when one record is larger than the prefix the preview reads —
/// one long text or base64 column does it — this reads the *first record alone*,
/// unbounded in bytes and bounded in memory, and hands back that record's keys
/// with no rows. The user maps the columns and imports; the whole-file walk was
/// always going to read every byte anyway.
///
/// **Unbounded bytes is not unbounded memory.** Every value is deserialized as
/// `serde::de::IgnoredAny`, which parses and discards without allocating, so a
/// two-gigabyte record costs its key names. That is the whole reason this can
/// drop the cap the preview cannot.
///
/// A second reader, because the first was consumed: the caller reopens the file.
/// `more` is `true` unconditionally — one record was read, and there is no
/// claim here about whether it was the only one.
pub fn read_json_columns<R: std::io::Read>(r: R) -> Result<Sample, ImportError> {
    let mut columns: Vec<String> = Vec::new();
    // `BTreeMap`, not `serde_json::Map`, which only deserializes with `Value` as
    // its value type — and `Value` is the allocation this exists to avoid. The
    // key order is the same either way: `serde_json::Map` *is* a `BTreeMap`
    // without the `preserve_order` feature, which is what `json_records`' own
    // doc says about the column order it produces.
    let stream = serde_json::Deserializer::from_reader(ArrayUnwrap::new(r))
        .into_iter::<std::collections::BTreeMap<String, serde::de::IgnoredAny>>();
    // The first record only — its keys are the column list, and reading a second
    // buys nothing this path can use.
    let mut stream = stream;
    if let Some(v) = stream.next() {
        let map = v.map_err(|e| ImportError::Read(e.to_string()))?;
        columns.extend(map.into_keys());
    }
    if columns.is_empty() {
        return Err(ImportError::Read(
            "expected JSON objects (an array of them, or one per line)".into(),
        ));
    }
    Ok(Sample {
        columns,
        rows: Vec::new(),
        more: true,
        values_withheld: true,
    })
}

/// The mapping step's sample for a file, **falling back to its column list when
/// the preview cannot be built from a prefix**.
///
/// `open` returns a fresh reader on the file; it is called again only on the
/// fallback, which needs to start over — the first reader is spent by then and
/// the failure is not knowable until it has been.
///
/// **The decision lives here rather than in the probe's closure** because it is
/// the composition that was wrong, not either half: `read_sample` correctly
/// refuses to invent a preview, and the modal correctly gates Next on having a
/// sample — and between them a valid JSON file whose first record is one long
/// text or base64 column could not be imported at all, under a sentence saying
/// the *preview* was the problem. A decision in a view closure is a decision no
/// test can reach, which is how it stayed that way.
///
/// Only [`ImportError::PreviewRecordTooLarge`] falls back. A truncated or
/// malformed file still fails, on the same bytes, through a different reader.
pub fn probe_sample<R: std::io::Read>(
    open: impl Fn() -> std::io::Result<R>,
    format: ImportFormat,
    cfg: &ReadConfig,
    limit: usize,
) -> Result<Sample, ImportError> {
    let first = open().map_err(|e| ImportError::Read(e.to_string()))?;
    match read_sample(first, format, cfg, limit) {
        Err(ImportError::PreviewRecordTooLarge { .. }) => {
            let again = open().map_err(|e| ImportError::Read(e.to_string()))?;
            read_json_columns(again)
        }
        other => other,
    }
}

/// How much memory a JSON load needs, as a multiple of the file's size.
///
/// **Measured**, not guessed (`review/importmem`, a counting allocator driving
/// the real `validate`/`row_iter`): 5.0× at 20 k rows, 4.9× at 100 k, 4.8× at
/// 400 k — linear and stable. CSV, by contrast, is flat at 0.1 MB however large
/// the file is.
pub const JSON_MEMORY_FACTOR: u64 = 5;

/// Past this file size, [`json_memory_warning`] speaks up. Chosen so the warning
/// stays rare enough to mean something: at 200 MB the estimate is ~1 GB, which is
/// where it stops being something a machine absorbs without noticing.
pub const JSON_WARN_BYTES: u64 = 200 * 1024 * 1024;

/// Roughly the peak memory a JSON import of `file_bytes` will need.
pub fn json_load_estimate(file_bytes: u64) -> u64 {
    file_bytes.saturating_mul(JSON_MEMORY_FACTOR)
}

/// What to tell the user before a large JSON load starts, or `None`.
///
/// A JSON import can't stream: the columns are the *union* of every object's
/// keys, so no record can be emitted until the last one has been read (see
/// [`json_records`]). That is a real constraint, but it used to be an unbounded
/// and undisclosed one — the modal presented CSV and JSON as interchangeable,
/// and a large JSON file was discovered to be too big by the app dying. Saying
/// the number up front, next to the other pre-load warnings, is the honest
/// minimum; converting the same data to CSV is the way out, so the message says
/// so.
pub fn json_memory_warning(format: ImportFormat, file_bytes: u64) -> Option<String> {
    if format != ImportFormat::Json || file_bytes <= JSON_WARN_BYTES {
        return None;
    }
    let est = json_load_estimate(file_bytes);
    Some(format!(
        "This JSON file is {}, and a JSON import is held in memory while it \
         loads — expect it to need about {}. A CSV of the same data loads in \
         constant memory.",
        crate::format::human_bytes(file_bytes as i64),
        crate::format::human_bytes(est as i64)
    ))
}

/// How much memory an Excel load needs, as a multiple of the file's size.
///
/// **Larger than JSON's, and the multiplier is against the *compressed* size.**
/// **Measured, on the same counting allocator [`JSON_MEMORY_FACTOR`] came off**,
/// after the reader stopped materialising the sheet. It used to be a guess of
/// 25× against the compressed size, on the reasoning that an `.xlsx` is a ZIP of
/// XML whose markup deflates well — which was right about the file and wrong
/// about what was actually held: the cost was the **dense** `Range` the old
/// reader built, not the file.
///
/// Two shapes, both `read_sample`, `--release`:
///
/// | Workbook | File | Peak | Ratio | Preview |
/// | --- | --- | --- | --- | --- |
/// | 200 k × 50, mixed number/text (the project's own target) | 32 MB | 64 MB | 2.0× | 21 ms |
/// | 120 k × 50, every cell a distinct 60-char string | 21 MB | 32 MB | 1.5× | 18 ms |
///
/// The ratio is stable because what is held is the file's own bytes plus the
/// strings of the rows actually read — not the sheet.
///
/// **But the warning stands in front of the *load*, and `read_sample` is the
/// preview.** Those two paths do not cost the same thing: `row_iter`'s `Xlsx`
/// arm materialises every row of the sheet into a `Vec<(Record, u64)>` before it
/// hands out the first one, so its cost is linear in *cells* where the preview's
/// is linear in rows read. Quoting the preview's 4× for the load was an
/// under-estimate on both paths, which is the direction a warning may not be
/// wrong in. Re-measured with a counting global allocator over
/// `schemaic_core::import` itself, `--release`, on a `rust_xlsxwriter` workbook
/// of the shape this doc's table uses (alternating numbers and short distinct
/// strings):
///
/// | Workbook | File | `read_sample` peak | `row_iter` peak | Held after build |
/// | --- | --- | --- | --- | --- |
/// | 100 k × 50 | 23.3 MB | 128.0 MB (5.49×) | 309.5 MB (**13.27×**) | 181.6 MB (7.79×) |
/// | 20 k × 50 | 4.7 MB | — | 13.53× | 36.5 bytes/cell |
///
/// The ratios are stable across scale because the buffer is linear in cells. 14×
/// is the larger measurement rounded up, which is the over-estimate the warning
/// needs. It is *not* the streaming rewrite — `xlsx_records` already streams
/// through a callback, and the buffer exists only because `RowIter` is an
/// `Iterator` rather than an internal-iteration walk, as the CSV arm is.
pub const XLSX_MEMORY_FACTOR: u64 = 14;

/// The estimate above which [`xlsx_memory_warning`] must already have spoken.
///
/// Not a limit — nothing refuses at it. It is the number that gives
/// [`XLSX_WARN_BYTES`] its meaning: "the point at which the estimate stops being
/// something a machine absorbs without noticing", which was prose and is now a
/// figure the two constants are pinned against, so raising either one without
/// the other fails a test rather than silently moving the disclosure past the
/// cost.
pub const XLSX_DISCLOSURE_BUDGET: u64 = 1024 * 1024 * 1024;

/// Past this file size, [`xlsx_memory_warning`] speaks up.
///
/// **Derived from [`XLSX_MEMORY_FACTOR`], not set beside it.** It was
/// [`JSON_WARN_BYTES`] on the reasoning that the two factors were near enough
/// (4× against JSON's 5×) for one threshold to mean the same thing for both —
/// which stopped being true when the Excel factor was re-measured against the
/// load path it actually guards. At 14× a 200 MiB workbook costs ~2.8 GB and
/// the sentence in front of it had not been said yet.
///
/// So the relationship is the definition: the warning fires before the estimate
/// reaches [`XLSX_DISCLOSURE_BUDGET`], and
/// `a_large_workbook_is_disclosed_before_it_costs` is what holds the two
/// together.
pub const XLSX_WARN_BYTES: u64 = XLSX_DISCLOSURE_BUDGET / XLSX_MEMORY_FACTOR;

/// Roughly the peak memory an Excel import of `file_bytes` will need.
pub fn xlsx_load_estimate(file_bytes: u64) -> u64 {
    file_bytes.saturating_mul(XLSX_MEMORY_FACTOR)
}

/// What to tell the user before a large Excel load starts, or `None`.
///
/// The same honesty [`json_memory_warning`] exists for, for a sharper version of
/// the same constraint: a ZIP's directory is at the end of the file, so an
/// `.xlsx` cannot be read as a stream even in principle — where JSON's buffering
/// is a consequence of its key union, this one is the container format. Saying
/// the number before the load is the only alternative to discovering it by the
/// app dying.
pub fn xlsx_memory_warning(format: ImportFormat, file_bytes: u64) -> Option<String> {
    if format != ImportFormat::Xlsx || file_bytes <= XLSX_WARN_BYTES {
        return None;
    }
    let est = xlsx_load_estimate(file_bytes);
    Some(format!(
        "This workbook is {}, and an Excel import is held in memory while it \
         loads — expect it to need roughly {}, since the file is compressed. \
         A CSV of the same sheet loads in constant memory.",
        crate::format::human_bytes(file_bytes as i64),
        crate::format::human_bytes(est as i64)
    ))
}

/// The pre-load memory warning for whichever format, or `None`.
///
/// **One call site, so a format that needs a warning cannot be given one nobody
/// asks for.** The JSON warning was reached directly by the modal; adding a
/// second direct call is how the two drift, and the third format would then have
/// to be remembered in a third place.
pub fn memory_warning(format: ImportFormat, file_bytes: u64) -> Option<String> {
    json_memory_warning(format, file_bytes).or_else(|| xlsx_memory_warning(format, file_bytes))
}

/// The table columns an import writes, as indices in **table order**.
///
/// Table order rather than file order so the generated `INSERT` reads naturally
/// and every batch lists its columns identically.
///
/// A **server-assigned** column is excluded however it got mapped
/// ([`crate::schema::ColumnInfo::is_server_assigned`]). This is the single authority `validate`,
/// `row_iter` and `build_insert` all funnel through, so filtering here is what
/// makes it impossible to write one: a generated column matched by name from a
/// file Schemaic itself exported used to sail through validation and then fail
/// the entire transaction on the first batch.
pub fn insert_columns(mapping: &Mapping, table: &TableInfo) -> Vec<usize> {
    let mut cols: Vec<usize> = mapping
        .targets
        .iter()
        .filter_map(|t| match t {
            Target::Column(i)
                if *i < table.columns.len() && !table.columns[*i].is_server_assigned() =>
            {
                Some(*i)
            }
            _ => None,
        })
        .collect();
    cols.sort_unstable();
    cols.dedup();
    cols
}

/// Coerce one file record into the values for an `INSERT`, in
/// [`insert_columns`] order, collecting anything wrong with it.
///
/// `line` is the record's 1-based line in the file, so an issue can say where.
/// `sheet_errors` holds the indices of fields the *format itself* reported as
/// broken — a worksheet's formula errors ([`IssueKind::CellError`]), which are
/// wrong for every column type and which no amount of reading the text can
/// establish; see [`Record`]. Empty for CSV and JSON, which have nothing of the
/// kind, so no format argument is needed to gate it.
pub fn coerce_record(
    fields: &[Field],
    sheet_errors: &[usize],
    mapping: &Mapping,
    table: &TableInfo,
    nulls: &NullRule,
    dialect: SqlDialect,
    line: u64,
) -> (Vec<Value>, Vec<Issue>) {
    let cols = insert_columns(mapping, table);
    let mut issues = Vec::new();

    // A record whose field count doesn't match the header is reported once, then
    // read as far as it goes — the alternative is discarding a row that may be
    // only trailing-comma wrong.
    if fields.len() != mapping.targets.len() {
        issues.push(Issue {
            line,
            column: String::new(),
            text: String::new(),
            kind: IssueKind::FieldCount {
                expected: mapping.targets.len(),
                found: fields.len(),
            },
        });
    }

    // Which file field feeds each table column, resolved in one pass. The
    // obvious `targets.iter().position(..)` inside the per-column loop is
    // quadratic per row, which at 50 columns × 100k rows is hundreds of millions
    // of comparisons for a lookup that never changes.
    let mut field_of = vec![None; table.columns.len()];
    for (fi, t) in mapping.targets.iter().enumerate() {
        if let Target::Column(ci) = t
            && *ci < field_of.len()
            && field_of[*ci].is_none()
        {
            field_of[*ci] = Some(fi);
        }
    }

    let values = cols
        .iter()
        .map(|&ci| {
            let col = &table.columns[ci];
            // Three cases, and they're genuinely different: a field the format
            // says is null (`Some(None)`), a field the record simply doesn't
            // reach (`None` — a short CSV record), and text to interpret.
            let from = field_of[ci];
            let field = match from.and_then(|fi| fields.get(fi)) {
                Some(Some(text)) => text.as_str(),
                Some(None) | None => {
                    return if col.nullable {
                        Value::Null
                    } else {
                        issues.push(Issue {
                            line,
                            column: col.name.clone(),
                            text: String::new(),
                            kind: IssueKind::NullInNotNull,
                        });
                        Value::Null
                    };
                }
            };
            // Asked before the type dispatch, and independently of it: a cell
            // the sheet could not evaluate is wrong for *every* column type,
            // and `ColKind::Other` — text, date, JSON, blob, enum — has no
            // dispatch to catch it with.
            //
            // **Asked of the reader, not of the text.** This used to be
            // `format == Xlsx && is_worksheet_error(field)`, matching the ten
            // spellings — which cannot tell a formula error from a string cell
            // whose whole content is one of them, so an ordinary file with
            // `#N/A` typed into a `status` column was refused *whole*: the
            // first issue makes `row_iter` return `Err` and aborts the
            // transaction. `sheet_errors` is what calamine knew and
            // `cell_text` had to discard; see [`Record`]. No format gate is
            // needed any more, because only a worksheet ever fills it.
            if from.is_some_and(|fi| sheet_errors.contains(&fi)) {
                issues.push(Issue {
                    line,
                    column: col.name.clone(),
                    text: field.to_string(),
                    kind: IssueKind::CellError,
                });
                return Value::Null;
            }
            match coerce(
                field,
                classify(&col.type_name),
                col.nullable,
                nulls,
                dialect,
            ) {
                Ok(v) => v,
                Err(kind) => {
                    issues.push(Issue {
                        line,
                        column: col.name.clone(),
                        text: field.to_string(),
                        kind,
                    });
                    // Keep the row shaped correctly so a later issue still lines
                    // up with its column; nothing is inserted anyway.
                    Value::Null
                }
            }
        })
        .collect();
    (values, issues)
}

/// What a whole-file check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validation {
    /// Records read (excluding the header).
    pub rows: u64,
    pub issues: Vec<Issue>,
    /// The issue list was capped — there are more.
    pub more_issues: bool,
}

/// The line under a problem list that says what it is not showing, or `None`
/// when it is showing everything.
///
/// **There are two caps and they are not the same one.** `more_issues` is set
/// when [`validate`] stops *collecting* at its `max_issues`; a view has its own,
/// much smaller, cap on how many it *renders*. The disclosure used to be wired
/// only to the first, so a file with 57 problems rendered 20 lines under a
/// heading reading "57 problems in the file" and said nothing — the user fixed
/// twenty, re-imported, was told "37 problems", and repeated, at one whole file
/// pass each.
///
/// `shown` is what the caller is about to render, `total` is
/// `Validation::issues.len()`, and `capped` is `Validation::more_issues`. Both
/// caps can be in force at once, and the sentence has to name both: at exactly
/// `max_issues` the old wording said "…and more." over 20 of 200 lines, which
/// is the same gap one notch worse.
pub fn issue_tail(shown: usize, total: usize, capped: bool) -> Option<String> {
    match (total > shown, capped) {
        (false, false) => None,
        (true, false) => Some(format!("Showing {shown} of {total}.")),
        (false, true) => Some(format!(
            "Showing all {total} — the check stopped counting there, so the file holds more."
        )),
        (true, true) => Some(format!(
            "Showing {shown} of the first {total} — the check stopped counting there, so the file holds more."
        )),
    }
}

/// Check every record without inserting anything.
///
/// This is what makes the all-or-nothing import bearable: the transaction would
/// roll back on the first bad row anyway, one error at a time, across however
/// many attempts it takes. Reading the file through once first turns that into a
/// single list of everything wrong, before anything is written.
///
/// Counting continues past the issue cap so the row total stays truthful.
pub fn validate<R: std::io::Read>(
    r: R,
    format: ImportFormat,
    cfg: &ReadConfig,
    table: &TableInfo,
    mapping: &Mapping,
    dialect: SqlDialect,
    max_issues: usize,
) -> Result<Validation, ImportError> {
    if insert_columns(mapping, table).is_empty() {
        return Err(ImportError::NoColumnsMapped);
    }
    // JSON and Excel carry their own nulls, so the NULL-token rule (which exists
    // because CSV can't tell an empty field from a missing value) must not also
    // apply — it would turn every empty JSON string, and every empty Excel
    // string cell, into a NULL. Asked as a capability so this and `row_iter`
    // cannot answer it differently; see [`ImportFormat::has_own_nulls`].
    let nulls = if format.has_own_nulls() {
        NullRule::none()
    } else {
        cfg.nulls.clone()
    };

    let mut out = Validation {
        rows: 0,
        issues: Vec::new(),
        more_issues: false,
    };
    for_each_record(r, format, cfg, |mut rec, line| {
        out.rows += 1;
        // See `trim_to_mapping` — the check must see exactly what the import will.
        rec.fields
            .truncate(trim_to_mapping(&rec.fields, format, mapping));
        let (_, issues) = coerce_record(
            &rec.fields,
            &rec.sheet_errors,
            mapping,
            table,
            &nulls,
            dialect,
            line,
        );
        for i in issues {
            if out.issues.len() >= max_issues {
                out.more_issues = true;
                break;
            }
            out.issues.push(i);
        }
        true
    })?;
    Ok(out)
}

/// Narrow a JSON record to the columns the mapping was built from.
///
/// JSON columns are the *union* of every object's keys, and the mapping the user
/// approved was built from a sample of the first records. A key that first
/// appears past that sample widens every record — so without this, each row would
/// carry more fields than the mapping has targets and be reported as a
/// field-count mismatch, failing the whole import on a file that's perfectly
/// fine. Keys accumulate in first-seen order, so the sampled ones are always a
/// prefix and the tail dropped here is exactly the columns nothing maps to.
///
/// CSV is untouched: its columns are fixed by the header, so a count mismatch
/// there is a real stray-delimiter problem worth reporting.
fn trim_to_mapping(fields: &[Field], format: ImportFormat, mapping: &Mapping) -> usize {
    match format {
        ImportFormat::Json => fields.len().min(mapping.targets.len()),
        // Excel sides with CSV, not JSON: a worksheet's columns are settled by
        // its header row, so a count mismatch is a real one worth reporting —
        // there is no key union here that could widen a later row. The reader
        // has to let a wider row *arrive* wider for that to mean anything: it
        // used to pad and truncate every row to the settled width, which made
        // this branch unreachable by construction and the loss it reports
        // invisible. See the placement site in `xlsx_rows`.
        ImportFormat::Csv | ImportFormat::Xlsx => fields.len(),
    }
}

/// Everything a row needs to become values, owned so the iterator can outlive
/// the call that built it.
struct RowCtx {
    table: TableInfo,
    mapping: Mapping,
    nulls: NullRule,
    dialect: SqlDialect,
    format: ImportFormat,
}

impl RowCtx {
    fn row(
        &self,
        fields: &[Field],
        sheet_errors: &[usize],
        line: u64,
    ) -> Result<Vec<Value>, String> {
        let fields = &fields[..trim_to_mapping(fields, self.format, &self.mapping)];
        let (values, issues) = coerce_record(
            fields,
            sheet_errors,
            &self.mapping,
            &self.table,
            &self.nulls,
            self.dialect,
            line,
        );
        match issues.first() {
            // Only the first issue is reported here: the import is all-or-nothing,
            // and `validate` has already shown the user the whole list. This is
            // the backstop for a file that changed underneath them.
            Some(i) => Err(format!(
                "line {}, column {}: {} ({})",
                i.line,
                i.column,
                i.kind.message(),
                if i.text.is_empty() {
                    "empty".to_string()
                } else {
                    i.text.clone()
                }
            )),
            None => Ok(values),
        }
    }
}

/// Streams a file's rows, already coerced into the values an `INSERT` takes.
///
/// This is what the database layer pulls batches from, so a CSV is never held in
/// memory. JSON is the caveat, and it's the key union rather than the bracket
/// syntax: every record has to be emitted against the columns of *all* of them
/// (see [`json_records`]), so a whole-file walk buffers whichever shape it's in.
/// Sampling doesn't — that's bounded by its limit — so previewing a large JSON
/// file is cheap even though importing it isn't.
pub struct RowIter<R: std::io::Read> {
    ctx: RowCtx,
    source: RowSourceIter<R>,
}

enum RowSourceIter<R: std::io::Read> {
    Csv(csv::StringRecordsIntoIter<R>),
    /// Buffered, for the same reason in both cases and a different cause: JSON
    /// cannot know its columns before EOF, and an `.xlsx` cannot be read as a
    /// prefix at all. Either way the rows exist before the first one is handed
    /// out, so one variant carries both.
    Buffered(std::vec::IntoIter<(Record, u64)>),
    /// Buffered like the above, but **without the padding**.
    ///
    /// `xlsx_records` hands every row out already widened to the sheet's used
    /// width, and `sheet_width` admits `XLSX_MAX_COLS` = 16,384 — so a buffered
    /// row costs 16,384 slots whether or not it holds a cell. Keeping N of them
    /// re-materialised the dense rectangle `sheet_width`'s own doc says it
    /// replaced: a ~40 KB workbook declaring `A1:XFD10000` and holding one cell
    /// per row allocated ~3.9 GB before the first row was handed out, and the
    /// worst legal sheet ~412 GB, with no warning in front of it because
    /// `xlsx_memory_warning` estimates from the file's size **on disk**.
    ///
    /// So the trailing `None`s — which carry no information; a `Field` is an
    /// `Option` and an absent cell *is* `None` — are dropped on the way in and
    /// restored on the way out. The row the consumer sees is byte-identical,
    /// which matters: `trim_to_mapping` reads `fields.len()` for Excel and a
    /// short row would report a count mismatch that is not there.
    Xlsx {
        rows: std::vec::IntoIter<(Record, u64)>,
        width: usize,
    },
}

/// Build the row stream for an import. `mapping` must have at least one target,
/// which [`validate`] checks first.
pub fn row_iter<R: std::io::Read>(
    r: R,
    format: ImportFormat,
    cfg: &ReadConfig,
    table: &TableInfo,
    mapping: &Mapping,
    dialect: SqlDialect,
) -> Result<RowIter<R>, ImportError> {
    if insert_columns(mapping, table).is_empty() {
        return Err(ImportError::NoColumnsMapped);
    }
    let ctx = RowCtx {
        table: table.clone(),
        mapping: mapping.clone(),
        // JSON and Excel carry their own nulls — see `validate`.
        nulls: if format.has_own_nulls() {
            NullRule::none()
        } else {
            cfg.nulls.clone()
        },
        dialect,
        format,
    };
    let source = match format {
        ImportFormat::Csv => {
            let mut records = reader_for(r, cfg).into_records();
            if cfg.dialect.has_header {
                records.next().transpose()?;
            }
            RowSourceIter::Csv(records)
        }
        ImportFormat::Json => {
            let mut keys = Vec::new();
            let mut rows = Vec::new();
            json_records(r, &mut keys, usize::MAX, None, |fields, n| {
                rows.push((Record::plain(fields), n));
                true
            })?;
            RowSourceIter::Buffered(rows.into_iter())
        }
        ImportFormat::Xlsx => {
            let mut names = Vec::new();
            let mut rows = Vec::new();
            let mut width = 0usize;
            xlsx_records(r, cfg, &mut names, usize::MAX, |mut rec, n| {
                // The sheet's used width, taken from the row as it arrives and
                // not from a second reading of the dimension — every row
                // `xlsx_records` emits is already padded to it.
                width = width.max(rec.fields.len());
                while rec.fields.last().is_some_and(Option::is_none) {
                    rec.fields.pop();
                }
                rec.fields.shrink_to_fit();
                rows.push((rec, n));
                true
            })?;
            RowSourceIter::Xlsx {
                rows: rows.into_iter(),
                width,
            }
        }
    };
    Ok(RowIter { ctx, source })
}

impl<R: std::io::Read> Iterator for RowIter<R> {
    type Item = Result<Vec<Value>, String>;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.source {
            RowSourceIter::Csv(records) => {
                let rec = records.next()?;
                Some(match rec {
                    Ok(rec) => {
                        let line = rec.position().map(|p| p.line()).unwrap_or(0);
                        let fields: Vec<Field> = rec.iter().map(|f| Some(f.to_string())).collect();
                        self.ctx.row(&fields, &[], line)
                    }
                    Err(e) => Err(e.to_string()),
                })
            }
            RowSourceIter::Buffered(rows) => {
                let (rec, line) = rows.next()?;
                Some(self.ctx.row(&rec.fields, &rec.sheet_errors, line))
            }
            RowSourceIter::Xlsx { rows, width } => {
                let (mut rec, line) = rows.next()?;
                // Re-padded to the sheet's width, so the consumer sees exactly
                // the row `xlsx_records` produced — see the variant's doc.
                rec.fields.resize(*width, None);
                Some(self.ctx.row(&rec.fields, &rec.sheet_errors, line))
            }
        }
    }
}

/// Rows per `INSERT`. Bulk import is one transaction of batched statements
/// rather than the grid write-back's statement-per-row: at 100k rows that's 100k
/// server round-trips, which is minutes on a remote host.
pub const INSERT_BATCH_ROWS: usize = 500;

/// And a **byte** ceiling, which a row count alone cannot stand in for.
///
/// 500 rows of a 40 KiB text column — routine for an article, a log line, a JSON
/// payload, a base64 blob — is a 20 MB statement. Measured against MariaDB
/// 10.11.14 at its ship default `max_allowed_packet` of 16 MiB, the server does
/// not refuse it: it answers `ERROR 2006 (HY000) Server has gone away` and
/// **closes the connection**. `import_on`'s error arm then tries to `ROLLBACK`
/// down a socket that is already gone, so the result is `Rollback::Incomplete` —
/// the "the rows may still be there" wording, produced rather than merely
/// disclosed — and on a non-transactional MySQL table the batches already sent
/// really are durable. All of it lands *after* `validate` has reported the file
/// clean, because row size is not something it could have checked.
///
/// It is engine-divergent, which is why the row bound survived: the same 20 MB
/// batch is accepted by MySQL 8.4.11, whose default is 64 MiB. The identical
/// file imports on MySQL 8 and kills the connection on MariaDB 10.11.
///
/// Same value and same reasoning as the inverse path's
/// [`crate::export::INSERT_BATCH_BYTES`], which had this argument written down
/// and this constant while import had neither: 512 KiB is the smallest bound
/// that makes the round-trip cost disappear and stays well inside every engine's
/// limit. A batch is closed **before** the row that would cross it, so one
/// enormous row still gets a statement of its own rather than being split into
/// something that would not parse.
pub const INSERT_BATCH_BYTES: usize = crate::export::INSERT_BATCH_BYTES;

/// A row's contribution to [`INSERT_BATCH_BYTES`], estimated from the values
/// rather than from the rendered SQL.
///
/// The batch is assembled before anything is rendered, so this is what there is
/// to measure. It counts a string's own bytes plus a fixed allowance per value
/// for the quotes, the comma and the worst case of escaping — an estimate that
/// errs *high*, which is the direction that keeps the statement under the
/// server's limit rather than the one that discovers it.
pub fn row_bytes(row: &[crate::model::Value]) -> usize {
    row.iter()
        .map(|v| match v {
            crate::model::Value::Str(s) => s.len() * 2 + 8,
            _ => 24,
        })
        .sum()
}

/// Is this batch full — by either bound?
///
/// `bytes` is what the rows already in it come to, and `next` the one about to
/// be added; the batch closes before a row that would cross the byte ceiling, so
/// a single row larger than the whole ceiling still goes on its own.
pub fn batch_is_full(rows: usize, bytes: usize, next: usize) -> bool {
    rows >= INSERT_BATCH_ROWS || (rows > 0 && bytes + next > INSERT_BATCH_BYTES)
}

/// One multi-row `INSERT` for `rows`, in the connection's dialect. `None` when
/// there's nothing to insert.
///
/// Identifier and literal quoting come from [`crate::export`] — import is the
/// inverse of the SQL export, so the escaping that's already tested there (the
/// MySQL-only backslash doubling in particular) is the escaping used here.
pub fn build_insert(
    database: &str,
    schema: Option<&str>,
    table: &str,
    columns: &[&str],
    rows: &[Vec<Value>],
    dialect: SqlDialect,
) -> Option<String> {
    if rows.is_empty() || columns.is_empty() {
        return None;
    }
    let q = |s: &str| crate::export::ident_sql(s, dialect);
    // How a table is addressed per engine is `export::qualified_table`'s rule —
    // this was a second copy of it, which is what let SQLite's bare-name case
    // reach one and not the other.
    let target = crate::export::qualified_table(database, schema, table, dialect);
    let cols = columns.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ");
    let values = rows
        .iter()
        .map(|r| {
            let cells = r
                .iter()
                .map(|v| crate::export::sql_literal(v, dialect))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({cells})")
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!("INSERT INTO {target} ({cols}) VALUES {values}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ColumnInfo;

    #[test]
    fn only_the_newest_probe_may_write() {
        assert_eq!(probe_verdict((1, 3), (1, 3)), ProbeVerdict::Apply);
    }

    /// **500 rows is not a bound on a statement.** A 40 KiB text column — an
    /// article, a log line, a JSON payload — makes 500 of them a 20 MB
    /// `INSERT`, and MariaDB 10.11.14 at its ship default `max_allowed_packet`
    /// answers `ERROR 2006` by closing the connection, which is the one channel
    /// the rollback needs. Measured; MySQL 8.4.11 accepts the same statement, so
    /// the identical file imports on one engine and kills the other.
    #[test]
    fn a_batch_of_long_rows_closes_on_bytes_rather_than_on_the_row_count() {
        let long = crate::model::Value::Str("x".repeat(40 * 1024));
        let row = [long];
        // The shape of the failure: at 500 rows this is far past every engine's
        // conservative limit, and the row bound alone would have taken all 500.
        assert!(row_bytes(&row) * INSERT_BATCH_ROWS > 16 * 1024 * 1024);

        let mut rows = 0usize;
        let mut bytes = 0usize;
        let each = row_bytes(&row);
        while !batch_is_full(rows, bytes, each) {
            rows += 1;
            bytes += each;
        }
        assert!(rows < INSERT_BATCH_ROWS, "closed on bytes, at {rows} rows");
        assert!(bytes <= INSERT_BATCH_BYTES, "and inside the ceiling");
    }

    /// Narrow rows must still fill a batch to the row bound, or the round trips
    /// the batching exists to remove come back.
    #[test]
    fn a_batch_of_narrow_rows_still_closes_on_the_row_count() {
        let row = [crate::model::Value::Int(1), crate::model::Value::Null];
        let each = row_bytes(&row);
        let mut rows = 0usize;
        let mut bytes = 0usize;
        while !batch_is_full(rows, bytes, each) {
            rows += 1;
            bytes += each;
        }
        assert_eq!(rows, INSERT_BATCH_ROWS);
    }

    /// One row larger than the whole ceiling gets a statement of its own rather
    /// than an empty batch or a split that would not parse — the same rule the
    /// export side states.
    #[test]
    fn a_single_oversized_row_is_not_split_and_does_not_stall() {
        let huge = row_bytes(&[crate::model::Value::Str("x".repeat(INSERT_BATCH_BYTES * 4))]);
        assert!(
            !batch_is_full(0, 0, huge),
            "an empty batch always takes one"
        );
        assert!(batch_is_full(1, huge, huge), "and closes right after it");
    }

    /// Typing `\t` into the Delimiter box is three edits and so three probes,
    /// reporting in completion order. An older one landing last rebuilt the
    /// mapping — name-matched or positional depending on a `has_header` the
    /// controls no longer showed — and the load then ran the live config against
    /// it.
    #[test]
    fn an_overtaken_probe_is_discarded_whole() {
        assert_eq!(probe_verdict((1, 2), (1, 3)), ProbeVerdict::Discard);
    }

    /// The other counter: the modal was closed and reopened on a different
    /// table while this one was reading.
    #[test]
    fn a_probe_from_a_previous_opening_is_discarded() {
        assert_eq!(probe_verdict((1, 3), (2, 3)), ProbeVerdict::Discard);
    }

    /// The schema list is *emptied* before a refetch begins, so "I looked and it
    /// wasn't there" is true of every refresh — and used to discard the file and
    /// a hand-built mapping over a reload nobody asked for.
    #[test]
    fn an_unloaded_schema_is_not_evidence_the_table_is_gone() {
        assert_eq!(target_survives(true, false, false), TargetVerdict::Keep);
        assert_eq!(target_survives(true, false, true), TargetVerdict::Keep);
    }

    #[test]
    fn a_table_still_listed_keeps_the_modal_open() {
        assert_eq!(target_survives(false, true, false), TargetVerdict::Keep);
        assert_eq!(target_survives(false, true, true), TargetVerdict::Keep);
    }

    #[test]
    fn a_table_really_gone_closes_the_modal() {
        assert_eq!(target_survives(false, false, false), TargetVerdict::Close);
    }

    /// Closing over a running load abandons a bulk write with nobody left to
    /// read its outcome — on a non-transactional engine the user then cannot
    /// tell whether rows landed, and a re-run duplicates whatever did.
    #[test]
    fn a_running_load_is_cancelled_rather_than_abandoned() {
        assert_eq!(target_survives(false, false, true), TargetVerdict::Cancel);
    }

    fn node(database: &str, has_table: Option<bool>) -> DbNodeView<'_> {
        DbNodeView {
            database,
            has_table,
        }
    }

    /// The half that stayed in the view and was therefore never asserted:
    /// `db_nodes` holds only the **active** connection's databases, so after a
    /// connection switch the list is about a different server and says nothing
    /// about this table. It used to say "not found" and discard the mapping.
    #[test]
    fn another_connections_database_list_is_not_evidence() {
        let nodes = [node("other", Some(false))];
        assert_eq!(
            target_verdict(&nodes, false, "world", false),
            TargetVerdict::Keep
        );
    }

    #[test]
    fn an_empty_list_mid_reload_is_not_evidence() {
        assert_eq!(
            target_verdict(&[], true, "world", false),
            TargetVerdict::Keep
        );
    }

    /// A database still loading has looked at nothing, so it is not a report
    /// that the table has gone.
    #[test]
    fn a_database_whose_schema_has_not_loaded_is_not_evidence() {
        let nodes = [node("world", None)];
        assert_eq!(
            target_verdict(&nodes, true, "world", false),
            TargetVerdict::Keep
        );
    }

    #[test]
    fn a_loaded_database_that_still_has_the_table_keeps_the_modal() {
        let nodes = [node("other", Some(false)), node("world", Some(true))];
        assert_eq!(
            target_verdict(&nodes, true, "world", false),
            TargetVerdict::Keep
        );
    }

    #[test]
    fn a_loaded_database_that_lost_the_table_closes_the_modal() {
        let nodes = [node("world", Some(false))];
        assert_eq!(
            target_verdict(&nodes, true, "world", false),
            TargetVerdict::Close
        );
    }

    /// The database itself was dropped: this connection's list *is* loaded and
    /// simply doesn't have it any more. Distinct from an empty list, which is
    /// what a reload looks like.
    #[test]
    fn a_dropped_database_closes_the_modal() {
        let nodes = [node("other", Some(false))];
        assert_eq!(
            target_verdict(&nodes, true, "world", false),
            TargetVerdict::Close
        );
    }

    #[test]
    fn a_running_load_still_cancels_rather_than_closing() {
        let nodes = [node("world", Some(false))];
        assert_eq!(
            target_verdict(&nodes, true, "world", true),
            TargetVerdict::Cancel
        );
    }

    fn tbl(cols: &[(&str, &str, bool)]) -> TableInfo {
        TableInfo {
            name: "t".into(),
            schema: None,
            columns: cols
                .iter()
                .map(|(n, ty, nullable)| ColumnInfo {
                    name: (*n).into(),
                    type_name: (*ty).into(),
                    nullable: *nullable,
                    primary_key: *n == "id",
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn format_is_inferred_from_the_extension() {
        assert_eq!(infer_format("rows.csv"), Some(ImportFormat::Csv));
        assert_eq!(infer_format("rows.TSV"), Some(ImportFormat::Csv));
        assert_eq!(infer_format("export.json"), Some(ImportFormat::Json));
        assert_eq!(infer_format("data.xlsx"), Some(ImportFormat::Xlsx));
        assert_eq!(infer_format("book.XLSM"), Some(ImportFormat::Xlsx));
        // An extension that says nothing shouldn't be guessed at — and the two
        // *other* Excel formats say something this reader can't act on, so they
        // are as good as nothing. Guessing `Xlsx` for a `.xls` would trade a
        // dropdown left alone for an "is not a zip archive" at open time.
        assert_eq!(infer_format("old.xls"), None);
        assert_eq!(infer_format("binary.xlsb"), None);
        assert_eq!(infer_format("noextension"), None);
    }

    #[test]
    fn sniff_finds_the_delimiter_by_consistency() {
        let csv = "a,b,c\n1,2,3\n4,5,6\n";
        assert_eq!(sniff(csv).delimiter, b',');
        let tsv = "a\tb\tc\n1\t2\t3\n";
        assert_eq!(sniff(tsv).delimiter, b'\t');
        let semi = "a;b;c\n1;2;3\n";
        assert_eq!(sniff(semi).delimiter, b';');
        let pipe = "a|b|c\n1|2|3\n";
        assert_eq!(sniff(pipe).delimiter, b'|');
    }

    /// A comma inside prose shouldn't outvote the real delimiter just by being
    /// more frequent — this is the case raw frequency counting gets wrong.
    #[test]
    fn sniff_prefers_the_consistent_delimiter_over_the_frequent_one() {
        let s = "name;note\nSmith;a, b, c, d\nJones;e, f, g, h\n";
        assert_eq!(sniff(s).delimiter, b';');
    }

    /// A delimiter inside a quoted field isn't a delimiter.
    #[test]
    fn sniff_ignores_delimiters_inside_quotes() {
        let s = "name;city\n\"Smith, John\";Berlin\n\"Doe, Jane\";Paris\n";
        assert_eq!(sniff(s).delimiter, b';');
    }

    #[test]
    fn sniff_of_a_single_column_file_defaults_to_comma() {
        let s = "name\nSmith\nJones\n";
        let d = sniff(s);
        assert_eq!(d.delimiter, b',');
    }

    #[test]
    fn sniff_of_empty_input_is_the_default_dialect() {
        assert_eq!(sniff(""), CsvDialect::default());
        assert_eq!(sniff("\n\n  \n"), CsvDialect::default());
    }

    #[test]
    fn header_is_detected_when_the_first_row_is_text_over_numeric_data() {
        let s = "id,name\n1,Smith\n2,Jones\n";
        assert!(sniff(s).has_header);
    }

    /// A numeric field in the first row means it's data, not names.
    #[test]
    fn a_numeric_first_row_is_not_a_header() {
        let s = "1,Smith\n2,Jones\n";
        assert!(!sniff(s).has_header);
    }

    #[test]
    fn an_all_text_file_assumes_a_header() {
        // Genuinely ambiguous — take the common case, which the preview shows.
        let s = "name,city\nSmith,Berlin\nJones,Paris\n";
        assert!(sniff(s).has_header);
    }

    #[test]
    fn auto_map_matches_on_name_ignoring_case_and_space() {
        let t = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        let m = auto_map(&[" NAME ".into(), "id".into()], &t, true);
        // Order doesn't matter — a name match survives a reordered file.
        assert_eq!(m.targets, vec![Target::Column(1), Target::Column(0)]);
    }

    #[test]
    fn auto_map_skips_a_file_column_with_no_match() {
        let t = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        let m = auto_map(&["name".into(), "nonsense".into()], &t, true);
        assert_eq!(m.targets, vec![Target::Column(1), Target::Skip]);
    }

    #[test]
    fn auto_map_never_maps_two_file_columns_onto_one_target() {
        let t = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        let m = auto_map(&["name".into(), "NAME".into()], &t, true);
        assert_eq!(m.targets, vec![Target::Column(1), Target::Skip]);
    }

    #[test]
    fn auto_map_without_a_header_falls_back_to_position() {
        let t = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        let m = auto_map(&placeholder_columns(3), &t, false);
        // The third file column has nowhere to go.
        assert_eq!(
            m.targets,
            vec![Target::Column(0), Target::Column(1), Target::Skip]
        );
    }

    #[test]
    fn unmapped_columns_are_left_to_the_server_default() {
        let t = tbl(&[
            ("id", "int", false),
            ("name", "varchar", true),
            ("note", "text", true),
        ]);
        let m = auto_map(&["name".into()], &t, true);
        assert_eq!(m.unmapped_columns(&t), vec![0, 2]);
    }

    /// An unmapped NOT NULL column is worth warning about — unless the server
    /// fills it in. That is read off the model, not guessed from the type.
    #[test]
    fn missing_required_warns_about_not_null_but_not_an_auto_key() {
        let mut t = tbl(&[
            ("id", "int", false),
            ("name", "varchar", false),
            ("note", "text", true),
        ]);
        t.columns[0].auto_increment = true;
        let m = auto_map(&["note".into()], &t, true);
        assert_eq!(m.missing_required(&t), vec!["name".to_string()]);
    }

    /// The false negative the old "integer primary key ⇒ auto-increment"
    /// approximation produced: a natural key isn't assigned by anyone. MySQL
    /// inserts `year = 0` and fails the second row on a duplicate key;
    /// PostgreSQL fails the first on NOT NULL. Validation said the file was clean.
    #[test]
    fn a_natural_integer_key_is_still_required() {
        let mut t = tbl(&[("year", "int", false), ("value", "int", false)]);
        t.columns[0].primary_key = true; // …but not auto_increment
        let m = auto_map(&["value".into()], &t, true);
        assert_eq!(m.missing_required(&t), vec!["year".to_string()]);
    }

    /// The false positive: leaving a defaulted column out is the ordinary,
    /// correct thing to do, and warning about it teaches the user to ignore the
    /// warning — the outcome the heuristic existed to avoid.
    #[test]
    fn a_not_null_column_with_a_default_is_not_required() {
        let mut t = tbl(&[("id", "int", false), ("status", "varchar(10)", false)]);
        t.columns[0].auto_increment = true;
        t.columns[1].default = Some("'new'".into());
        let m = auto_map(&["id".into()], &t, true);
        assert!(
            m.missing_required(&t).is_empty(),
            "{:?}",
            m.missing_required(&t)
        );
    }

    /// A generated column is never "missing" — it is also never insertable, so
    /// skipping it must not then warn that it wasn't supplied.
    #[test]
    fn a_generated_column_is_not_reported_as_missing() {
        let mut t = tbl(&[("id", "int", false), ("full_name", "varchar", false)]);
        t.columns[0].auto_increment = true;
        t.columns[1].generated = Some("concat(a,b)".into());
        let m = auto_map(&["id".into(), "full_name".into()], &t, true);
        assert!(
            m.missing_required(&t).is_empty(),
            "{:?}",
            m.missing_required(&t)
        );
    }

    /// A non-key `AUTO_INCREMENT` column — which the old predicate missed in the
    /// other direction, since it required `primary_key`.
    #[test]
    fn a_non_key_auto_increment_column_is_not_required() {
        let mut t = tbl(&[("id", "int", false), ("seq", "bigint", false)]);
        t.columns[0].auto_increment = true;
        t.columns[1].auto_increment = true;
        let m = auto_map(&["id".into()], &t, true);
        assert!(m.missing_required(&t).is_empty());
    }

    /// A `varchar` primary key is never auto-assigned — `classicmodels.offices`
    /// has exactly this shape — so leaving it unmapped fails every time and has
    /// to be warned about.
    #[test]
    fn missing_required_warns_about_a_non_integer_primary_key() {
        let t = tbl(&[("id", "varchar(10)", false), ("city", "varchar(50)", false)]);
        let m = auto_map(&["city".into()], &t, true);
        assert_eq!(m.missing_required(&t), vec!["id".to_string()]);
    }

    #[test]
    fn placeholder_columns_are_one_based() {
        assert_eq!(placeholder_columns(2), vec!["Column 1", "Column 2"]);
        assert!(placeholder_columns(0).is_empty());
    }

    // ── coercion ────────────────────────────────────────────────────────────

    use crate::intel::SqlDialect::{MySql, Postgres, Sqlite};

    #[test]
    fn classify_recognizes_the_families_we_validate() {
        assert_eq!(classify("int(11)"), ColKind::Int);
        assert_eq!(classify("BIGINT"), ColKind::Int);
        assert_eq!(classify("int4"), ColKind::Int);
        assert_eq!(classify("int(10) unsigned"), ColKind::Uint);
        assert_eq!(classify("double"), ColKind::Float);
        assert_eq!(classify("real"), ColKind::Float);
        assert_eq!(classify("decimal(10,2)"), ColKind::Exact);
        assert_eq!(classify("numeric"), ColKind::Exact);
        assert_eq!(classify("boolean"), ColKind::Bool);
    }

    /// `interval` and `point` contain "int" — a substring match would classify
    /// them as integers and then reject every valid value in them.
    #[test]
    fn classify_does_not_match_int_as_a_substring() {
        assert_eq!(classify("interval"), ColKind::Other);
        assert_eq!(classify("point"), ColKind::Other);
        assert_eq!(classify("varchar(45)"), ColKind::Other);
        assert_eq!(classify("timestamptz"), ColKind::Other);
        assert_eq!(classify("jsonb"), ColKind::Other);
        assert_eq!(classify("uuid"), ColKind::Other);
    }

    #[test]
    fn coerce_parses_integers_and_rejects_text() {
        let n = NullRule::default();
        assert_eq!(
            coerce("42", ColKind::Int, true, &n, MySql),
            Ok(Value::Int(42))
        );
        assert_eq!(
            coerce(" -7 ", ColKind::Int, true, &n, MySql),
            Ok(Value::Int(-7))
        );
        assert_eq!(
            coerce("N/A", ColKind::Int, true, &n, MySql),
            Err(IssueKind::NotAnInteger)
        );
        // The classic: a float where an integer belongs.
        assert_eq!(
            coerce("1.5", ColKind::Int, true, &n, MySql),
            Err(IssueKind::NotAnInteger)
        );
    }

    /// DECIMAL/NUMERIC must never round-trip through f64 — that's the exact
    /// lossiness the read path goes out of its way to avoid.
    #[test]
    fn coerce_keeps_exact_numerics_as_text() {
        let n = NullRule::default();
        let big = "1234567890123456789012.345";
        assert_eq!(
            coerce(big, ColKind::Exact, true, &n, MySql),
            Ok(Value::Str(big.to_string()))
        );
        assert_eq!(
            coerce("oops", ColKind::Exact, true, &n, MySql),
            Err(IssueKind::NotANumber)
        );
    }

    /// **And an exact numeric refuses them too**, which it did not — three
    /// lines from the `Float` arm that says why.
    ///
    /// `Exact` shape-checks with `parse::<f64>()` and keeps the *text*, which is
    /// right (a `DECIMAL` must never round-trip through `f64`). But `"NaN"`,
    /// `"inf"`, `"Infinity"` and `"1e400"` all parse as `f64`, so they passed
    /// the check and went into the `INSERT` verbatim — with `validate`
    /// reporting **zero issues**, on the pass whose whole purpose is "a single
    /// list of everything wrong, before anything is written".
    ///
    /// Then, one file, three answers: MariaDB 10.11.14 answers
    /// `ERROR 1366: Incorrect decimal value: 'NaN'` and rolls back whichever
    /// batch it landed in, showing the raw server error for a file validation
    /// had just certified; PostgreSQL 16.15 **accepts** `'NaN'::numeric` and
    /// `'Infinity'::numeric`, so the same file imports clean and puts a NaN in
    /// an exact-numeric column, where it then changes every `SUM`, `AVG` and
    /// comparison over it.
    #[test]
    fn coerce_rejects_non_finite_exact_numerics() {
        let n = NullRule::default();
        for bad in [
            "NaN",
            "nan",
            "inf",
            "-inf",
            "Infinity",
            "-Infinity",
            "1e400",
            "-1e400",
        ] {
            assert_eq!(
                coerce(bad, ColKind::Exact, true, &n, MySql),
                Err(IssueKind::NotANumber),
                "{bad:?}"
            );
        }
        // And the values an exact column is *for* still go through as text,
        // including ones no `f64` could hold precisely.
        for good in [
            "1234567890123456789012.345",
            "-0.00000000000000000001",
            "0",
            "1e30",
        ] {
            assert_eq!(
                coerce(good, ColKind::Exact, true, &n, MySql),
                Ok(Value::Str(good.to_string())),
                "{good:?}"
            );
        }
    }

    /// **A `NUMERIC` is not an `f64`, and the shape check was measuring range.**
    ///
    /// An unconstrained PostgreSQL `NUMERIC` holds 131,072 digits before the
    /// point, so a 401-digit integer is an ordinary value the server stores
    /// exactly — and `parse::<f64>` answers `inf` for it, so the `is_finite`
    /// arm reported *"not a number"* and `RowCtx::row` turned the first such
    /// issue into an `Err` that aborts the whole import. `v0.24.0` imported the
    /// same file correctly, because the *text* is what is inserted.
    ///
    /// The arm's own doc already draws the distinction it was not making —
    /// "a value no `f64` can hold precisely still goes through unrounded, which
    /// is the whole reason this arm is not `Float`" — and the same sentence
    /// applies to one it cannot hold at all.
    #[test]
    fn an_exact_column_takes_a_numeral_no_f64_can_hold() {
        let n = NullRule::default();
        let huge = format!("1{}", "0".repeat(400));
        assert_eq!(
            huge.parse::<f64>().map(f64::is_finite),
            Ok(false),
            "the premise: no `f64` holds it"
        );
        let negative = format!("-{huge}");
        let fractional = format!("{huge}.5");
        for good in [huge.as_str(), negative.as_str(), fractional.as_str(), "0.1"] {
            assert_eq!(
                coerce(good, ColKind::Exact, true, &n, MySql),
                Ok(Value::Str(good.to_string())),
                "{good:?}"
            );
        }
        // And the shapes the `is_finite` term was added for are still refused:
        // none of them is a decimal numeral, which is the real question.
        for bad in ["NaN", "inf", "Infinity", "-inf", "1e400", "1.2.3"] {
            assert_eq!(
                coerce(bad, ColKind::Exact, true, &n, MySql),
                Err(IssueKind::NotANumber),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn coerce_rejects_non_finite_floats() {
        let n = NullRule::default();
        assert_eq!(
            coerce("inf", ColKind::Float, true, &n, MySql),
            Err(IssueKind::NotANumber)
        );
        assert_eq!(
            coerce("NaN", ColKind::Float, true, &n, MySql),
            Err(IssueKind::NotANumber)
        );
    }

    /// Booleans are the one place the engines genuinely disagree: MySQL's BOOLEAN
    /// is a TINYINT that silently stores `'true'` as 0, while PostgreSQL rejects
    /// the integer 1. So the literal is normalized per dialect rather than passed
    /// through — passing through is what corrupts MySQL data.
    #[test]
    fn coerce_normalizes_booleans_per_dialect() {
        let n = NullRule::default();
        // **All three engines**, because the rule used to be "MySQL, or else",
        // and SQLite falling into the `or else` stored the *text* `'true'` in a
        // NUMERIC-affinity column — where every boolean context then reads it
        // as false.
        for t in ["true", "TRUE", "t", "yes", "1"] {
            assert_eq!(coerce(t, ColKind::Bool, true, &n, MySql), Ok(Value::Int(1)));
            assert_eq!(
                coerce(t, ColKind::Bool, true, &n, Sqlite),
                Ok(Value::Int(1))
            );
            assert_eq!(
                coerce(t, ColKind::Bool, true, &n, Postgres),
                Ok(Value::Str("true".into()))
            );
        }
        for f in ["false", "F", "no", "0"] {
            assert_eq!(coerce(f, ColKind::Bool, true, &n, MySql), Ok(Value::Int(0)));
            assert_eq!(
                coerce(f, ColKind::Bool, true, &n, Sqlite),
                Ok(Value::Int(0))
            );
            assert_eq!(
                coerce(f, ColKind::Bool, true, &n, Postgres),
                Ok(Value::Str("false".into()))
            );
        }
        for d in [MySql, Postgres, Sqlite] {
            assert_eq!(
                coerce("maybe", ColKind::Bool, true, &n, d),
                Err(IssueKind::NotABoolean),
                "{d:?}"
            );
        }
    }

    /// Anything we can't be certain about goes to the server verbatim — it parses
    /// more date and numeric formats than we could enumerate, and rejecting valid
    /// data is worse than passing it on.
    #[test]
    fn coerce_passes_unclassified_types_through_untouched() {
        let n = NullRule::default();
        assert_eq!(
            coerce("2026-02-30ish", ColKind::Other, true, &n, MySql),
            Ok(Value::Str("2026-02-30ish".into()))
        );
    }

    #[test]
    fn an_empty_field_is_null_by_default() {
        let n = NullRule::default();
        assert_eq!(coerce("", ColKind::Other, true, &n, MySql), Ok(Value::Null));
        assert_eq!(coerce("", ColKind::Int, true, &n, MySql), Ok(Value::Null));
    }

    /// A blank field is not an empty one. Quoted padding is how a file says the
    /// spaces are deliberate, and the `csv` reader hands them over identically
    /// either way — so nulling them would rewrite data on a guess. `trim` is the
    /// setting that says "treat blank as empty", and it applies before this.
    #[test]
    fn a_whitespace_only_field_is_not_null() {
        let n = NullRule::default();
        assert_eq!(
            coerce("   ", ColKind::Other, true, &n, MySql),
            Ok(Value::Str("   ".into()))
        );
        // With trim on, the reader has already emptied it, so it *is* NULL.
        assert_eq!(coerce("", ColKind::Other, true, &n, MySql), Ok(Value::Null));
    }

    /// A written token still matches a padded field — only the empty one is
    /// exact, since it's the only one whose meaning trimming would change.
    #[test]
    fn a_written_null_token_still_matches_a_padded_field() {
        let n = NullRule {
            tokens: vec!["NULL".into()],
        };
        assert_eq!(
            coerce("  null  ", ColKind::Other, true, &n, MySql),
            Ok(Value::Null)
        );
    }

    #[test]
    fn null_tokens_are_configurable_and_case_insensitive() {
        let n = NullRule {
            tokens: vec!["NULL".into(), r"\N".into()],
        };
        assert_eq!(
            coerce("null", ColKind::Other, true, &n, MySql),
            Ok(Value::Null)
        );
        assert_eq!(
            coerce(r"\N", ColKind::Other, true, &n, MySql),
            Ok(Value::Null)
        );
        // With "" no longer a token, an empty field is the empty string.
        assert_eq!(
            coerce("", ColKind::Other, true, &n, MySql),
            Ok(Value::Str(String::new()))
        );
    }

    #[test]
    fn a_null_in_a_not_null_column_is_an_issue() {
        let n = NullRule::default();
        assert_eq!(
            coerce("", ColKind::Other, false, &n, MySql),
            Err(IssueKind::NullInNotNull)
        );
    }

    // ── INSERT building ─────────────────────────────────────────────────────

    #[test]
    fn build_insert_emits_one_multi_row_statement() {
        let rows = vec![
            vec![Value::Int(1), Value::Str("a".into())],
            vec![Value::Int(2), Value::Null],
        ];
        let sql = build_insert("db", None, "t", &["id", "name"], &rows, MySql).unwrap();
        assert_eq!(
            sql,
            "INSERT INTO `db`.`t` (`id`, `name`) VALUES (1, 'a'), (2, NULL)"
        );
    }

    /// A PostgreSQL namespace qualifies the table *instead of* the database, and
    /// identifiers double-quote — same rule the export path follows.
    #[test]
    fn build_insert_qualifies_per_dialect() {
        let rows = vec![vec![Value::Int(1)]];
        let sql = build_insert("db", Some("sales"), "t", &["id"], &rows, Postgres).unwrap();
        assert_eq!(sql, r#"INSERT INTO "sales"."t" ("id") VALUES (1)"#);
    }

    #[test]
    fn row_iter_streams_coerced_rows_in_insert_order() {
        let t = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        let csv = "name,id\nSmith,1\nJones,2\n";
        let m = auto_map(&["name".into(), "id".into()], &t, true);
        let rows: Vec<_> = row_iter(csv.as_bytes(), ImportFormat::Csv, &cfg(true), &t, &m, MySql)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                vec![Value::Int(1), Value::Str("Smith".into())],
                vec![Value::Int(2), Value::Str("Jones".into())],
            ]
        );
    }

    /// The backstop for a file that changed since it was validated: the first bad
    /// row stops the stream, and the message says where.
    #[test]
    fn row_iter_reports_the_first_bad_row_and_says_where() {
        let t = tbl(&[("id", "int", false)]);
        let csv = "id\n1\nnope\n";
        let m = auto_map(&["id".into()], &t, true);
        let mut it =
            row_iter(csv.as_bytes(), ImportFormat::Csv, &cfg(true), &t, &m, MySql).unwrap();
        assert!(it.next().unwrap().is_ok());
        let err = it.next().unwrap().unwrap_err();
        assert!(err.contains("line 3"), "{err}");
        assert!(err.contains("id"), "{err}");
    }

    #[test]
    fn row_iter_streams_json_too() {
        let t = tbl(&[("id", "int", false)]);
        let json = "{\"id\": 1}\n{\"id\": 2}\n";
        let m = auto_map(&["id".into()], &t, true);
        let rows: Vec<_> = row_iter(
            json.as_bytes(),
            ImportFormat::Json,
            &cfg(true),
            &t,
            &m,
            MySql,
        )
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
        assert_eq!(rows, vec![vec![Value::Int(1)], vec![Value::Int(2)]]);
    }

    #[test]
    fn build_insert_of_no_rows_is_nothing_to_run() {
        assert_eq!(build_insert("db", None, "t", &["id"], &[], MySql), None);
    }

    /// Quoting is export's, so an apostrophe can't break out of the literal.
    #[test]
    fn build_insert_escapes_values() {
        let rows = vec![vec![Value::Str("x'; DROP TABLE t; --".into())]];
        let sql = build_insert("db", None, "t", &["c"], &rows, MySql).unwrap();
        assert!(sql.contains("'x''; DROP TABLE t; --'"), "{sql}");
    }

    // ── reading + validating ────────────────────────────────────────────────

    fn cfg(has_header: bool) -> ReadConfig {
        ReadConfig {
            dialect: CsvDialect {
                has_header,
                ..CsvDialect::default()
            },
            nulls: NullRule::default(),
            trim: false,
            sheet: None,
        }
    }

    // ── The preview's nullness ──────────────────────────────────────────────
    //
    // **Asserted end to end, from `read_sample` rather than from a hand-built
    // `Field`.** `NullRule::matches` passed its own tests from the day it was
    // written; the defect was that the preview path never called it, and a
    // sample built by hand with a `None` in it would have hidden exactly that.
    // What makes these red on the unfixed tree is that `read_sample`'s output
    // has no `None` to find.

    /// The file from the report, previewed as it will land: with the modal's
    /// defaults plus `NULL` in the tokens box, **both** `note` cells are NULL —
    /// the spelled one and the empty one — and neither `id` is.
    #[test]
    fn a_csv_preview_shows_a_token_null_and_an_empty_field_as_null() {
        let nulls = NullRule {
            tokens: vec![String::new(), "NULL".to_string()],
        };
        let c = ReadConfig {
            nulls: nulls.clone(),
            ..cfg(true)
        };
        let s = read_sample(
            b"id,note\n1,NULL\n2,\n".as_slice(),
            ImportFormat::Csv,
            &c,
            10,
        )
        .unwrap();
        let cell = |r: usize, f: usize| preview_field(&s.rows[r][f], &nulls, ImportFormat::Csv);
        assert_eq!(cell(0, 0), PreviewCell::Text("1"));
        assert_eq!(cell(0, 1), PreviewCell::Null, "the spelled NULL");
        assert_eq!(cell(1, 0), PreviewCell::Text("2"));
        assert_eq!(cell(1, 1), PreviewCell::Null, "the empty field");
    }

    /// And the control that decides it is visible on screen: turn "Empty field
    /// is NULL" off — drop the empty token — and row 2's cell becomes the empty
    /// *string* it will actually store. The preview used to be byte-identical
    /// at both settings.
    #[test]
    fn turning_off_empty_is_null_changes_what_the_preview_shows() {
        let nulls = NullRule {
            tokens: vec!["NULL".to_string()],
        };
        let c = ReadConfig {
            nulls: nulls.clone(),
            ..cfg(true)
        };
        let s = read_sample(
            b"id,note\n1,NULL\n2,\n".as_slice(),
            ImportFormat::Csv,
            &c,
            10,
        )
        .unwrap();
        let cell = |r: usize, f: usize| preview_field(&s.rows[r][f], &nulls, ImportFormat::Csv);
        assert_eq!(cell(0, 1), PreviewCell::Null, "the token still matches");
        assert_eq!(
            cell(1, 1),
            PreviewCell::Text(""),
            "an empty field is now an empty string, and must look like one"
        );
    }

    /// **A format that carries its own nulls is not swept up.** JSON says which
    /// values are absent, so an empty string in the file is an empty string in
    /// the preview whatever the token box holds — the same answer `validate`
    /// and `row_iter` give through `has_own_nulls`.
    #[test]
    fn a_json_empty_string_is_not_previewed_as_null() {
        let nulls = NullRule {
            tokens: vec![String::new(), "NULL".to_string()],
        };
        let json = r#"[{"id":1,"note":""},{"id":2,"note":null}]"#;
        let s = read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 10).unwrap();
        let at = |name: &str| s.columns.iter().position(|c| c == name).expect(name);
        let (id, note) = (at("id"), at("note"));
        assert_eq!(
            preview_field(&s.rows[0][note], &nulls, ImportFormat::Json),
            PreviewCell::Text(""),
            "the token rule must not reach a format that carries its own nulls"
        );
        assert_eq!(
            preview_field(&s.rows[1][note], &nulls, ImportFormat::Json),
            PreviewCell::Null,
            "a JSON null is a null whatever the tokens say"
        );
        assert_eq!(
            preview_field(&s.rows[0][id], &nulls, ImportFormat::Json),
            PreviewCell::Text("1")
        );
    }

    /// A written token still matches a padded field, and the empty token still
    /// does not match a quoted run of spaces — `matches`' two rules, asked
    /// through the preview so the screen and the load agree about them.
    #[test]
    fn the_preview_reads_padding_the_way_the_load_does() {
        let nulls = NullRule {
            tokens: vec![String::new(), "NULL".to_string()],
        };
        let c = ReadConfig {
            nulls: nulls.clone(),
            ..cfg(true)
        };
        let s = read_sample(
            b"a,b\n NULL ,\"   \"\n".as_slice(),
            ImportFormat::Csv,
            &c,
            10,
        )
        .unwrap();
        let cell = |f: usize| preview_field(&s.rows[0][f], &nulls, ImportFormat::Csv);
        assert_eq!(cell(0), PreviewCell::Null, "a padded token is still NULL");
        assert_eq!(
            cell(1),
            PreviewCell::Text("   "),
            "three deliberate spaces are data, not an empty field"
        );
    }

    // ── Excel ───────────────────────────────────────────────────────────────

    /// What a test writes into a worksheet cell. Deliberately not [`Field`]: the
    /// point of these tests is that Excel's *types* survive the trip, so a
    /// fixture has to be able to say "the number 7" as distinct from "the text
    /// 7".
    enum Cell {
        Blank,
        Text(&'static str),
        Num(f64),
        Bool(bool),
        Date(u16, u8, u8),
        DateTime(u16, u8, u8, u16, u8, u8),
        /// An elapsed time, as a fraction of a day — a number under an
        /// `[h]:mm:ss` format, which is what makes calamine read it back as a
        /// duration rather than a clock time.
        Duration(f64),
    }

    /// Build a real `.xlsx` in memory from `sheets`.
    ///
    /// **The export half writes the fixtures the import half reads**, which is
    /// what makes these tests worth more than a golden file: they fail if either
    /// side of the feature drifts, and neither side can be "corrected" into
    /// agreement with a stale blob.
    fn workbook(sheets: &[(&str, &[&[Cell]])]) -> Vec<u8> {
        workbook_at(0, 0, sheets)
    }

    /// [`workbook`] with the data written at `(top, left)` instead of `A1`.
    ///
    /// A separate entry point because the origin is the *point* of two tests and
    /// noise in every other: a used range that does not start at `A1` is what a
    /// title block above a header produces, and it is the arrangement in which
    /// enumerating the range and numbering the worksheet stop agreeing.
    fn workbook_at(top: u32, left: u16, sheets: &[(&str, &[&[Cell]])]) -> Vec<u8> {
        use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};
        let mut wb = Workbook::new();
        // calamine reads a cell as a date because of its *number format*, not
        // its value — a date is a serial number underneath — so the fixture has
        // to carry one, exactly as a real workbook does.
        let date = Format::new().set_num_format("yyyy\\-mm\\-dd");
        let stamp = Format::new().set_num_format("yyyy\\-mm\\-dd\\ hh:mm:ss");
        // The bracketed hour is what makes this an *elapsed* time rather than a
        // clock time — it is the format that tells calamine to hand back a
        // `TimeDelta`, so the fixture cannot express a duration without it.
        let elapsed = Format::new().set_num_format("[h]:mm:ss");
        for (name, rows) in sheets {
            let sheet = wb.add_worksheet();
            sheet.set_name(*name).unwrap();
            for (r, row) in rows.iter().enumerate() {
                for (c, cell) in row.iter().enumerate() {
                    let (r, c) = (top + r as u32, left + c as u16);
                    match cell {
                        Cell::Blank => continue,
                        Cell::Duration(d) => {
                            sheet.write_number_with_format(r, c, *d, &elapsed).unwrap()
                        }
                        Cell::Text(s) => sheet.write_string(r, c, *s).unwrap(),
                        Cell::Num(n) => sheet.write_number(r, c, *n).unwrap(),
                        Cell::Bool(b) => sheet.write_boolean(r, c, *b).unwrap(),
                        Cell::Date(y, m, d) => sheet
                            .write_datetime_with_format(
                                r,
                                c,
                                ExcelDateTime::from_ymd(*y, *m, *d).unwrap(),
                                &date,
                            )
                            .unwrap(),
                        Cell::DateTime(y, mo, d, h, mi, s) => sheet
                            .write_datetime_with_format(
                                r,
                                c,
                                ExcelDateTime::from_ymd(*y, *mo, *d)
                                    .unwrap()
                                    .and_hms(*h, *mi, *s as f64)
                                    .unwrap(),
                                &stamp,
                            )
                            .unwrap(),
                    };
                }
            }
        }
        let mut buf = Vec::new();
        wb.save_to_writer(&mut buf).unwrap();
        buf
    }

    fn xlsx_cfg(has_header: bool, sheet: Option<&str>) -> ReadConfig {
        ReadConfig {
            sheet: sheet.map(str::to_string),
            ..cfg(has_header)
        }
    }

    /// **A spreadsheet's TRUE/FALSE column, into the type MySQL actually
    /// reports.** Driven from the *declared type string* through `classify`,
    /// not from `ColKind::Bool`, because in isolation `"true"` looks right and
    /// that is exactly what let this ship: MySQL and MariaDB report a `BOOLEAN`
    /// column as `tinyint(1)`, so `classify` gives `Int`, and every row of an
    /// ordinary workbook was refused with "not a whole number (true)" while the
    /// same file imported fine on PostgreSQL and SQLite.
    #[test]
    fn a_boolean_cell_imports_into_a_mysql_boolean_column() {
        let bytes = workbook(&[(
            "S",
            &[
                &[Cell::Text("id"), Cell::Text("flag")],
                &[Cell::Num(1.0), Cell::Bool(true)],
                &[Cell::Num(2.0), Cell::Bool(false)],
            ],
        )]);
        // What MySQL/MariaDB put in `ColumnInfo::type_name` for a `BOOLEAN`.
        assert_eq!(classify("tinyint(1)"), ColKind::Int);
        let table = tbl(&[("id", "int", false), ("flag", "tinyint(1)", false)]);
        let mapping = auto_map(&["id".into(), "flag".into()], &table, true);
        let v = validate(
            &bytes[..],
            ImportFormat::Xlsx,
            &xlsx_cfg(true, None),
            &table,
            &mapping,
            MySql,
            100,
        )
        .unwrap();
        assert!(v.issues.is_empty(), "{:?}", v.issues);

        // And on the engines where the column really is boolean, both spellings
        // were always accepted — so this is not a trade between them.
        for dialect in [MySql, Postgres] {
            let kind = classify(if dialect == MySql {
                "tinyint(1)"
            } else {
                "bool"
            });
            assert!(coerce("1", kind, false, &NullRule::default(), dialect).is_ok());
            assert!(coerce("0", kind, false, &NullRule::default(), dialect).is_ok());
        }
    }

    /// **The same header, through the two readers, must map the same way.**
    /// A BOM that survived a round-trip through a BOM'd CSV lands inside the
    /// first header name; the CSV reader strips it and the worksheet reader did
    /// not, so the first column silently imported into nothing. Asserted
    /// against the CSV path rather than against a literal, because the claim is
    /// that the two agree.
    #[test]
    fn a_bom_on_the_first_worksheet_header_is_stripped_like_a_csvs() {
        let bytes = workbook(&[(
            "S",
            &[
                &[Cell::Text("\u{feff}id"), Cell::Text("name")],
                &[Cell::Num(1.0), Cell::Text("Ada")],
            ],
        )]);
        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 10).unwrap();
        let csv = read_sample(
            "\u{feff}id,name\n1,Ada\n".as_bytes(),
            ImportFormat::Csv,
            &cfg(true),
            10,
        )
        .unwrap();
        assert_eq!(s.columns, csv.columns);
        assert_eq!(s.columns, ["id", "name"]);

        let table = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        assert_eq!(
            auto_map(&s.columns, &table, true).targets,
            auto_map(&csv.columns, &table, true).targets
        );
    }

    /// **A cell the sheet could not evaluate is wrong for every column type.**
    /// `cell_text` keeps Excel's spelling on the stated ground that it "surfaces
    /// as a coercion Issue naming the row" — but that surfacing was `coerce`'s
    /// type dispatch, and `ColKind::Other` has none. So the test is written
    /// against a **varchar** column: against an `int` one it passes vacuously,
    /// which is the trap the house rule names.
    #[test]
    fn an_error_cell_is_reported_even_for_a_text_column() {
        use calamine::{CellErrorType, Data};
        let table = tbl(&[("id", "int", false), ("note", "varchar", true)]);
        let mapping = auto_map(&["id".into(), "note".into()], &table, true);

        // Still the seam, both ends — but the two ends no longer meet in the
        // *spelling*. What the reader renders for an error cell is what the
        // user reads; what the coercion is asked is the index the reader
        // recorded, so this half only has to stay a non-null rendering.
        for e in [CellErrorType::NA, CellErrorType::Div0, CellErrorType::Ref] {
            let text = cell_text(&Data::Error(e)).expect("an error cell is not a null");
            assert!(text.starts_with('#'), "{text}");
        }
        let na = cell_text(&Data::Error(CellErrorType::NA)).unwrap();

        let (_, issues) = coerce_record(
            &f(&["2", &na]),
            &[1],
            &mapping,
            &table,
            &NullRule::default(),
            MySql,
            3,
        );
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].kind, IssueKind::CellError);
        assert_eq!(issues[0].column, "note");
        assert_eq!(issues[0].line, 3);

        // …and the same text with nothing recorded against it is text. A CSV
        // reaches here that way by construction, and so does a worksheet's
        // *string* cell — which is the whole of [B6.1-L1-04].
        let (_, issues) = coerce_record(
            &f(&["2", &na]),
            &[],
            &mapping,
            &table,
            &NullRule::default(),
            MySql,
            3,
        );
        assert!(issues.is_empty(), "{issues:?}");

        // An index recorded against a *different* field does not leak onto this
        // one: `sheet_errors` is positional, and getting that wrong would flag
        // whichever column happened to sort first.
        let (_, issues) = coerce_record(
            &f(&["2", &na]),
            &[0],
            &mapping,
            &table,
            &NullRule::default(),
            MySql,
            3,
        );
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].column, "id");
    }

    #[test]
    fn an_xlsx_sample_takes_its_header_and_rows_from_the_first_sheet() {
        let bytes = workbook(&[(
            "People",
            &[
                &[Cell::Text("id"), Cell::Text("name")],
                &[Cell::Num(1.0), Cell::Text("Ada")],
                &[Cell::Num(2.0), Cell::Text("Grace")],
            ],
        )]);
        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 10)
            .expect("a workbook we just wrote");
        assert_eq!(s.columns, ["id", "name"]);
        assert_eq!(s.rows.len(), 2);
        assert!(!s.more);
        assert_eq!(s.rows[0], f(&["1", "Ada"]));
        assert_eq!(s.rows[1], f(&["2", "Grace"]));
    }

    /// **A whole number must not arrive as `1.0`.** Excel has one numeric type
    /// and stores every number as a float, so this is the very first thing an
    /// import of a real spreadsheet hits: an `INT` column rejects `1.0`, and it
    /// would reject it on every row.
    #[test]
    fn an_excel_number_comes_across_without_a_decimal_point_it_never_had() {
        let bytes = workbook(&[(
            "S",
            &[
                &[Cell::Text("n"), Cell::Text("f")],
                &[Cell::Num(42.0), Cell::Num(1.5)],
                &[Cell::Num(-7.0), Cell::Num(0.1)],
            ],
        )]);
        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 10).unwrap();
        assert_eq!(s.rows[0], f(&["42", "1.5"]));
        assert_eq!(s.rows[1], f(&["-7", "0.1"]));
    }

    /// **An empty cell is a null, and an empty string is not.** A worksheet is
    /// the one import format that can tell them apart, so the NULL-token rule
    /// CSV needs must not be applied to it — that rule would turn the empty
    /// string into a null too and lose the distinction the format carries.
    #[test]
    fn an_empty_excel_cell_is_a_null_and_an_empty_string_is_not() {
        assert!(ImportFormat::Xlsx.has_own_nulls());
        let bytes = workbook(&[(
            "S",
            &[
                &[Cell::Text("a"), Cell::Text("b")],
                &[Cell::Blank, Cell::Text("kept")],
            ],
        )]);
        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 10).unwrap();
        assert_eq!(s.rows[0][0], None, "a blank cell is a real null");
        assert_eq!(s.rows[0][1].as_deref(), Some("kept"));

        // The other half of the distinction is asserted on the cell reader
        // directly, because the fixture writer **cannot express it**:
        // `rust_xlsxwriter` emits nothing at all for an empty string, so it
        // round-trips as a blank cell. Files from other tools do carry a real
        // empty-string cell, and this is the code that meets one — so the case
        // is tested where it can be, rather than left to a fixture that would
        // quietly assert the wrong thing.
        assert_eq!(
            cell_text(&calamine::Data::String(String::new())),
            Some(String::new()),
            "an empty string cell is a value, not a null"
        );
        assert_eq!(cell_text(&calamine::Data::Empty), None);
        // And neither is subject to the NULL-token rule, which is what
        // `has_own_nulls` buys: with CSV's default rule an empty string would
        // become a NULL.
        assert!(NullRule::default().matches(""));
        assert!(!NullRule::none().matches(""));
    }

    /// Excel stores a date as a serial number with a display format. Handing the
    /// serial to a `DATE` column would import `45292`; ISO 8601 is what every
    /// engine's date parser takes.
    #[test]
    fn an_excel_date_comes_across_as_iso_8601_rather_than_its_serial_number() {
        let bytes = workbook(&[(
            "S",
            &[
                &[Cell::Text("d"), Cell::Text("t")],
                &[
                    Cell::Date(2024, 1, 1),
                    Cell::DateTime(2024, 3, 9, 14, 30, 5),
                ],
            ],
        )]);
        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 10).unwrap();
        // A pure date keeps the short form a `DATE` column wants — no midnight
        // clock part invented for it.
        assert_eq!(s.rows[0][0].as_deref(), Some("2024-01-01"));
        assert_eq!(s.rows[0][1].as_deref(), Some("2024-03-09 14:30:05"));
    }

    /// Booleans arrive as the spellings `coerce`'s boolean family already
    /// accepts — the seam between this reader and the coercion is exactly where
    /// a `TRUE` that no column would take could hide.
    #[test]
    fn an_excel_boolean_arrives_in_a_spelling_coerce_accepts() {
        let bytes = workbook(&[(
            "S",
            &[
                &[Cell::Text("flag")],
                &[Cell::Bool(true)],
                &[Cell::Bool(false)],
            ],
        )]);
        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 10).unwrap();
        for (row, want) in s.rows.iter().zip([Value::Int(1), Value::Int(0)]) {
            let text = row[0].clone().expect("a value");
            assert_eq!(
                coerce(&text, ColKind::Bool, false, &NullRule::none(), MySql),
                Ok(want)
            );
        }
    }

    /// A workbook is a file with several tables in it — the only import format
    /// that has to choose. A name that no longer matches is an error rather than
    /// a quiet fall back to the first sheet, because importing a *different*
    /// table than the one previewed is the failure worth being loud about.
    #[test]
    fn the_named_sheet_is_read_and_a_missing_one_is_refused() {
        let bytes = workbook(&[
            ("First", &[&[Cell::Text("a")], &[Cell::Text("from first")]]),
            (
                "Second",
                &[&[Cell::Text("a")], &[Cell::Text("from second")]],
            ),
        ]);
        // The names come off the *same* read as the preview — one parse of the
        // workbook, not one for the rows and another for the dropdown.
        let (sample, sheets) = read_workbook_sample(&bytes[..], &xlsx_cfg(true, None), 10).unwrap();
        assert_eq!(sheets, ["First".to_string(), "Second".to_string()]);
        assert_eq!(sample.rows[0][0].as_deref(), Some("from first"));
        // No sheet named: the first, which is what a one-sheet workbook wants.
        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 10).unwrap();
        assert_eq!(s.rows[0][0].as_deref(), Some("from first"));

        let s = read_sample(
            &bytes[..],
            ImportFormat::Xlsx,
            &xlsx_cfg(true, Some("Second")),
            10,
        )
        .unwrap();
        assert_eq!(s.rows[0][0].as_deref(), Some("from second"));

        let err = read_sample(
            &bytes[..],
            ImportFormat::Xlsx,
            &xlsx_cfg(true, Some("Gone")),
            10,
        )
        .expect_err("a sheet that isn't there");
        let msg = err.to_string();
        assert!(msg.contains("no sheet called \"Gone\""), "{msg}");
        // …and it names what the workbook does have, so the message is actionable.
        assert!(msg.contains("First, Second"), "{msg}");
    }

    /// Without a header row every row is data, and the columns get the same
    /// `Column N` placeholders CSV uses — so `auto_map`'s positional path works
    /// identically for both.
    #[test]
    fn a_sheet_read_without_a_header_row_names_its_columns_positionally() {
        let bytes = workbook(&[(
            "S",
            &[
                &[Cell::Text("a"), Cell::Text("b")],
                &[Cell::Num(1.0), Cell::Num(2.0)],
            ],
        )]);
        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &xlsx_cfg(false, None), 10).unwrap();
        assert_eq!(s.columns, ["Column 1", "Column 2"]);
        assert_eq!(s.rows.len(), 2, "the first row is data, not a header");
        assert_eq!(s.rows[0], f(&["a", "b"]));

        // A header row with a blank cell in it still names every column — the
        // width comes from the used range, so a nameless column would otherwise
        // shift every mapping after it.
        let gappy = workbook(&[(
            "S",
            &[
                &[Cell::Text("a"), Cell::Blank, Cell::Text("c")],
                &[Cell::Num(1.0), Cell::Num(2.0), Cell::Num(3.0)],
            ],
        )]);
        let s = read_sample(&gappy[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 10).unwrap();
        assert_eq!(s.columns, ["a", "Column 2", "c"]);
    }

    /// The sample bound applies to Excel too — `more` is what the modal shows,
    /// and a preview that claimed to be the whole sheet would be a lie about a
    /// file the user is about to import.
    #[test]
    fn an_xlsx_sample_stops_at_the_limit_and_says_there_is_more() {
        let rows: Vec<&[Cell]> = vec![
            &[Cell::Text("a")],
            &[Cell::Num(1.0)],
            &[Cell::Num(2.0)],
            &[Cell::Num(3.0)],
        ];
        let bytes = workbook(&[("S", &rows)]);
        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 2).unwrap();
        assert_eq!(s.rows.len(), 2);
        assert!(s.more);
    }

    /// **The load path buffers cells, not the dense rectangle.**
    ///
    /// `xlsx_records` pads every row to the sheet's used width, and
    /// `sheet_width` admits `XLSX_MAX_COLS` = 16,384 — so buffering N of those
    /// rows re-materialised exactly what `sheet_width`'s own doc says it
    /// replaced. A workbook declaring a wide used range and holding one cell per
    /// row costs 16,384 × 24 bytes a row: ~3.9 GB at 10,000 rows, ~412 GB at the
    /// worst legal sheet, and no warning in front of either, because
    /// `xlsx_memory_warning` estimates from the file's size **on disk** and such
    /// a workbook is tens of kilobytes. Preview and `validate` were never
    /// affected — both stream through `for_each_record`.
    ///
    /// Two assertions, and the second is the one that keeps the first honest:
    /// the buffer is proportional to the cells present, **and** the rows handed
    /// out are still the full-width rows the consumer expects (`trim_to_mapping`
    /// reads `fields.len()` for Excel, so a short row would report a count
    /// mismatch that is not there).
    #[test]
    fn a_wide_sparse_sheet_does_not_buffer_its_empty_cells() {
        // A header as wide as the sheet, then rows holding only the first and
        // the last cell — the shape that makes the padding the whole cost.
        const W: usize = 60;
        let mut header: Vec<Cell> = (0..W).map(|_| Cell::Blank).collect();
        header[0] = Cell::Text("id");
        header[1] = Cell::Text("tail");
        // The far cell is what makes the sheet's used width `W`; the body never
        // reaches it, which is the shape that made the padding the whole cost.
        header[W - 1] = Cell::Text("far");
        let mut body: Vec<Vec<Cell>> = Vec::new();
        for i in 0..40u32 {
            let mut r: Vec<Cell> = (0..2).map(|_| Cell::Blank).collect();
            r[0] = Cell::Num(f64::from(i) + 1.0);
            r[1] = Cell::Text("x");
            body.push(r);
        }
        let mut rows: Vec<&[Cell]> = vec![&header];
        rows.extend(body.iter().map(|r| r.as_slice()));
        let bytes = workbook(&[("S", &rows)]);

        let table = tbl(&[("id", "int", false), ("tail", "varchar", true)]);
        let cfg = xlsx_cfg(true, None);
        // Only the two real columns are mapped; the empties in between are not.
        let names: Vec<String> = (0..W)
            .map(|i| match i {
                0 => "id".to_string(),
                1 => "tail".to_string(),
                x => format!("c{x}"),
            })
            .collect();
        let mapping = auto_map(&names, &table, true);

        let it = row_iter(
            &bytes[..],
            ImportFormat::Xlsx,
            &cfg,
            &table,
            &mapping,
            MySql,
        )
        .expect("a workbook we just wrote");
        let RowSourceIter::Xlsx { rows, width } = &it.source else {
            panic!("the Excel arm must buffer without padding");
        };
        assert_eq!(*width, W, "the sheet's used width is still known");
        let buffered = rows.as_slice();
        let slots: usize = buffered.iter().map(|(r, _)| r.fields.len()).sum();
        assert_eq!(buffered.len(), 40);
        assert!(
            slots <= buffered.len() * W / 2,
            "the buffer is the dense rectangle again: {slots} slots for \
             {} rows of width {W}",
            buffered.len()
        );

        // And the rows still come out whole — the mapped columns are read, and
        // the Excel count check sees the width it expects.
        let out: Vec<_> = it.collect::<Result<Vec<_>, _>>().expect("every row");
        assert_eq!(out.len(), 40);
        assert_eq!(out[0], vec![Value::Int(1), Value::Str("x".into())]);
        assert_eq!(out[39], vec![Value::Int(40), Value::Str("x".into())]);
    }

    /// **The seam the two-pass import turns on.** `validate` and `row_iter` are
    /// separate walks of the same file, and the whole contract is that the
    /// second inserts exactly what the first approved — so a format wired into
    /// one and not the other, or given a different NULL rule by each, produces
    /// a clean validation followed by a failed transaction. Both passes are
    /// driven here over one workbook.
    #[test]
    fn an_xlsx_import_validates_and_then_streams_the_same_rows() {
        let bytes = workbook(&[(
            "S",
            &[
                &[Cell::Text("id"), Cell::Text("name"), Cell::Text("when")],
                &[Cell::Num(1.0), Cell::Text("Ada"), Cell::Date(2024, 1, 1)],
                &[Cell::Num(2.0), Cell::Blank, Cell::Date(2024, 6, 30)],
            ],
        )]);
        let table = tbl(&[
            ("id", "int", false),
            ("name", "varchar", true),
            ("when", "date", true),
        ]);
        let cfg = xlsx_cfg(true, None);
        let mapping = auto_map(&["id".into(), "name".into(), "when".into()], &table, true);

        let v = validate(
            &bytes[..],
            ImportFormat::Xlsx,
            &cfg,
            &table,
            &mapping,
            MySql,
            100,
        )
        .expect("a workbook we just wrote");
        assert_eq!(v.rows, 2);
        assert!(v.issues.is_empty(), "{:?}", v.issues);

        let rows: Vec<_> = row_iter(
            &bytes[..],
            ImportFormat::Xlsx,
            &cfg,
            &table,
            &mapping,
            MySql,
        )
        .expect("the same file the validation approved")
        .collect::<Result<Vec<_>, _>>()
        .expect("every row the validation approved");
        assert_eq!(rows.len(), 2);
        // The number reached an `int` column as an integer, not `1.0`, and the
        // date as text a `DATE` column parses.
        assert_eq!(
            rows[0],
            vec![
                Value::Int(1),
                Value::Str("Ada".into()),
                Value::Str("2024-01-01".into())
            ]
        );
        // The blank cell became a real NULL in a nullable column.
        assert_eq!(rows[1][1], Value::Null);
    }

    /// **`has_own_nulls` asked through its callers, not on its own.** Testing
    /// the predicate beside `NullRule`'s semantics would pass with the two
    /// wired together wrongly — the bug would sit in the composition, which is
    /// where these have historically hidden. So this drives a real NULL token
    /// through both passes: a workbook cell reading `N/A`, with `N/A`
    /// configured as a CSV null token.
    ///
    /// Excel carries its own nulls, so the token must **not** apply and the
    /// cell must arrive as the string it is. The same file read as CSV is the
    /// control: there the token does apply.
    #[test]
    fn a_csv_null_token_does_not_reach_an_excel_cell_that_merely_says_it() {
        let bytes = workbook(&[(
            "S",
            &[
                &[Cell::Text("id"), Cell::Text("name")],
                &[Cell::Num(1.0), Cell::Text("N/A")],
            ],
        )]);
        let table = tbl(&[("id", "int", false), ("name", "varchar", false)]);
        let mapping = auto_map(&["id".into(), "name".into()], &table, true);
        let cfg = ReadConfig {
            nulls: NullRule {
                tokens: vec!["N/A".into()],
            },
            ..xlsx_cfg(true, None)
        };

        let v = validate(
            &bytes[..],
            ImportFormat::Xlsx,
            &cfg,
            &table,
            &mapping,
            MySql,
            100,
        )
        .unwrap();
        assert!(
            v.issues.is_empty(),
            "the token must not turn an Excel cell into a NULL: {:?}",
            v.issues
        );
        let rows: Vec<_> = row_iter(
            &bytes[..],
            ImportFormat::Xlsx,
            &cfg,
            &table,
            &mapping,
            MySql,
        )
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
        assert_eq!(rows[0][1], Value::Str("N/A".into()));

        // The control: the same token, the same rule, a format that needs it.
        // `name` is NOT NULL, so applying the token is visible as an issue.
        let csv = "id,name\n1,N/A\n";
        let v = validate(
            csv.as_bytes(),
            ImportFormat::Csv,
            &cfg,
            &table,
            &mapping,
            MySql,
            100,
        )
        .unwrap();
        assert_eq!(v.issues.len(), 1, "CSV's token still applies");
        assert_eq!(v.issues[0].kind, IssueKind::NullInNotNull);
    }

    /// A blank cell in a `NOT NULL` column is a real problem, and the row it is
    /// on is the actionable part — the row number has to be the worksheet's own,
    /// so the user can go and look at it.
    #[test]
    fn a_blank_cell_in_a_not_null_column_is_reported_against_its_worksheet_row() {
        let bytes = workbook(&[(
            "S",
            &[
                &[Cell::Text("id"), Cell::Text("name")],
                &[Cell::Num(1.0), Cell::Text("Ada")],
                &[Cell::Num(2.0), Cell::Blank],
            ],
        )]);
        let table = tbl(&[("id", "int", false), ("name", "varchar", false)]);
        let mapping = auto_map(&["id".into(), "name".into()], &table, true);
        let v = validate(
            &bytes[..],
            ImportFormat::Xlsx,
            &xlsx_cfg(true, None),
            &table,
            &mapping,
            MySql,
            100,
        )
        .unwrap();
        assert_eq!(v.issues.len(), 1, "{:?}", v.issues);
        assert_eq!(v.issues[0].kind, IssueKind::NullInNotNull);
        // Row 3 of the worksheet: the header is row 1, so the second data row is
        // the third — the number Excel puts in its own margin.
        assert_eq!(v.issues[0].line, 3);
    }

    /// **A duration is an elapsed time, and a decimal is not one.** A `[h]:mm:ss`
    /// cell's underlying value is a fraction of a day; emitting it as decimal
    /// hours put a timesheet's 8h30m into a MySQL `TIME` column as `8.500000`,
    /// which MySQL reads as eight and a half *seconds* — wrong by 3600×, and
    /// silent, because the value coerces perfectly well.
    #[test]
    fn an_excel_duration_arrives_as_a_clock_span_not_a_decimal() {
        let bytes = workbook(&[(
            "S",
            &[
                &[Cell::Text("worked")],
                // 8h30m, the shape a timesheet holds.
                &[Cell::Duration(8.5 / 24.0)],
                // Past 24h, which is the whole point of the `[h]` bracket and
                // which MySQL `TIME` accepts (its range is ±838:59:59).
                &[Cell::Duration(36.0 / 24.0)],
            ],
        )]);
        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 10).unwrap();
        assert_eq!(s.rows[0][0].as_deref(), Some("8:30:00"));
        assert_eq!(
            s.rows[1][0].as_deref(),
            Some("36:00:00"),
            "an elapsed time is not wrapped at 24 hours"
        );

        // The unit itself, over the cases a workbook cannot easily produce.
        assert_eq!(duration_hms(0.0), "0:00:00");
        assert_eq!(duration_hms(-1.5 / 24.0), "-1:30:00");
        // Rounded to the second rather than truncated, so a hair under a whole
        // day is a whole day.
        assert_eq!(duration_hms(1.0 - f64::EPSILON), "24:00:00");
    }

    /// A formula error carries **Excel's** spelling. `Debug` would give `Div0`,
    /// a token that appears nowhere in Excel, so a user could not connect the
    /// issue to the cell it names.
    #[test]
    fn a_formula_error_keeps_the_spelling_excel_shows() {
        use calamine::{CellErrorType, Data};
        assert_eq!(
            cell_text(&Data::Error(CellErrorType::Div0)).as_deref(),
            Some("#DIV/0!")
        );
        assert_eq!(
            cell_text(&Data::Error(CellErrorType::Ref)).as_deref(),
            Some("#REF!")
        );
        // Not a null: a cell the sheet could not evaluate has to surface as an
        // issue naming its row, which a silent NULL would not do.
        assert!(cell_text(&Data::Error(CellErrorType::NA)).is_some());
    }

    /// **The row number has to be the one in Excel's own margin.** A sheet whose
    /// data sits below a title block starts its used range partway down, and
    /// enumerating that range numbers the first data row `2` while the user is
    /// looking at row `6` — an issue list pointing at a blank row.
    #[test]
    fn an_offset_sheet_reports_the_row_number_excel_shows() {
        // Four blank rows and two blank columns above and left of the data, the
        // shape a title block leaves behind.
        let bytes = workbook_at(
            4,
            2,
            &[(
                "S",
                &[
                    &[Cell::Text("id"), Cell::Text("name")],
                    &[Cell::Num(1.0), Cell::Text("Ada")],
                    &[Cell::Num(2.0), Cell::Blank],
                ],
            )],
        );
        // The columns are unaffected — they come off the used range's width, so
        // only the *numbering* depends on the origin.
        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 10).unwrap();
        assert_eq!(s.columns, ["id", "name"]);
        assert_eq!(s.rows.len(), 2);

        let table = tbl(&[("id", "int", false), ("name", "varchar", false)]);
        let mapping = auto_map(&["id".into(), "name".into()], &table, true);
        let v = validate(
            &bytes[..],
            ImportFormat::Xlsx,
            &xlsx_cfg(true, None),
            &table,
            &mapping,
            MySql,
            100,
        )
        .unwrap();
        assert_eq!(v.issues.len(), 1, "{:?}", v.issues);
        // The header is worksheet row 5, so the blank cell is on row 7 — not the
        // row 3 an offset-blind count would report.
        assert_eq!(v.issues[0].line, 7);
    }

    /// **A blank row is not a record.** The used range is a rectangle, so a
    /// spacer row between two blocks of data arrives as a row of empty cells;
    /// emitting it inserts a row of NULLs nobody typed, or fails the import on
    /// the first NOT NULL column.
    #[test]
    fn a_wholly_blank_row_is_skipped_and_does_not_shift_the_numbering() {
        let bytes = workbook(&[(
            "S",
            &[
                &[Cell::Text("id"), Cell::Text("name")],
                &[Cell::Num(1.0), Cell::Text("Ada")],
                &[Cell::Blank, Cell::Blank],
                &[Cell::Num(3.0), Cell::Text("Grace")],
            ],
        )]);
        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 10).unwrap();
        assert_eq!(
            s.rows.len(),
            2,
            "the spacer row is not a record: {:?}",
            s.rows
        );
        assert_eq!(s.rows[0], f(&["1", "Ada"]));
        assert_eq!(s.rows[1], f(&["3", "Grace"]));

        // …and the row that follows it keeps its real number, so skipping does
        // not quietly renumber what comes after.
        let mut lines = Vec::new();
        let mut names = Vec::new();
        xlsx_records(
            &bytes[..],
            &xlsx_cfg(true, None),
            &mut names,
            usize::MAX,
            |_, line| {
                lines.push(line);
                true
            },
        )
        .unwrap();
        assert_eq!(lines, [2, 4], "worksheet rows, with row 3 skipped");

        // A blank cell *among* values is still a real NULL — only a wholly empty
        // row is dropped, so the skip cannot swallow data.
        let partial = workbook(&[(
            "S",
            &[
                &[Cell::Text("id"), Cell::Text("name")],
                &[Cell::Num(1.0), Cell::Blank],
            ],
        )]);
        let s = read_sample(&partial[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 10).unwrap();
        assert_eq!(s.rows.len(), 1);
        assert_eq!(s.rows[0][1], None);
    }

    /// **The size refusal is asked of the file's stat, before it is read.** CSV
    /// and JSON bound their preview at `SAMPLE_MAX_BYTES` and stop there, so a
    /// huge file of either is cheap to look at; a workbook has to be read whole
    /// before its first row can be shown, which put the unbounded read *before*
    /// the memory warning that was supposed to precede it.
    #[test]
    fn an_oversized_workbook_is_refused_before_a_byte_is_read() {
        assert_eq!(xlsx_size_refusal(ImportFormat::Xlsx, 1024), None);
        assert_eq!(xlsx_size_refusal(ImportFormat::Xlsx, XLSX_MAX_BYTES), None);
        // The other two formats are never refused on size — they are bounded by
        // `SAMPLE_MAX_BYTES` instead, so there is nothing to protect them from.
        assert_eq!(
            xlsx_size_refusal(ImportFormat::Csv, XLSX_MAX_BYTES * 4),
            None
        );
        assert_eq!(
            xlsx_size_refusal(ImportFormat::Json, XLSX_MAX_BYTES * 4),
            None
        );
        let msg =
            xlsx_size_refusal(ImportFormat::Xlsx, XLSX_MAX_BYTES + 1).expect("past the ceiling");
        // The way out is named, as it is in every other message here.
        assert!(msg.contains("CSV"), "{msg}");

        // The refusal sits *above* the warning, so a file that is merely large
        // is warned about and still opens.
        assert!(memory_warning(ImportFormat::Xlsx, XLSX_WARN_BYTES + 1).is_some());
        assert!(xlsx_size_refusal(ImportFormat::Xlsx, XLSX_WARN_BYTES + 1).is_none());
    }

    /// **The ordering this test's neighbour is named for, asked of the thing
    /// that reads.** `xlsx_size_refusal` is a step the *launcher* takes, and
    /// only one of the three launchers took it — the two load-path opens went
    /// straight to `read_to_end`. Asserted by handing the reader more bytes than
    /// the ceiling and getting a refusal rather than an allocation, which is the
    /// property, rather than by re-checking the threshold function.
    #[test]
    fn the_reader_refuses_an_oversized_workbook_even_when_nobody_asked_first() {
        // Not a real workbook — it never gets that far, which is the point.
        let huge = std::io::Read::take(std::io::repeat(b'x'), XLSX_MAX_BYTES + 4096);
        let err = read_sample(huge, ImportFormat::Xlsx, &xlsx_cfg(true, None), 10)
            .expect_err("past the ceiling");
        assert!(err.to_string().contains("CSV"), "{err}");
        // …and a small non-workbook still fails as a *workbook* problem, so the
        // cap has not swallowed the ordinary error.
        let err = read_sample(
            &b"not a zip"[..],
            ImportFormat::Xlsx,
            &xlsx_cfg(true, None),
            10,
        )
        .expect_err("not a workbook");
        assert!(!err.to_string().contains("CSV"), "{err}");
    }

    /// **A workbook is streamed, not materialised.** `worksheet_range` builds
    /// the dense bounding rectangle of every cell present, so this file — two
    /// cells, opposite corners of a legal sheet, a few kilobytes on disk — asked
    /// for 17.2 billion `Data` values, about 550 GB. That is not an `Err` but
    /// `handle_alloc_error`, so the process goes and any unsaved editor text
    /// with it, at *probe* time: selecting the file is enough.
    ///
    /// **If this is ever regressed the failure is an abort, not a red test.**
    /// There is no assertion that can catch an allocation that kills the
    /// process, so the test is the reproduction itself: it passes in
    /// milliseconds against a streaming reader and takes the test binary with it
    /// against a materialising one.
    #[test]
    fn a_sheet_whose_corners_are_far_apart_costs_only_its_cells() {
        use rust_xlsxwriter::Workbook;
        let mut wb = Workbook::new();
        let sheet = wb.add_worksheet();
        sheet.write_string(0, 0, "id").unwrap();
        // The last cell of the largest legal worksheet.
        sheet.write_string(1_048_575, 16_383, "x").unwrap();
        let bytes = wb.save_to_buffer().unwrap();
        assert!(bytes.len() < 16 * 1024, "{} bytes", bytes.len());

        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &xlsx_cfg(true, None), 10).unwrap();
        // One header row and one data row, a million rows apart, and the width
        // the sheet declares.
        assert_eq!(s.columns.len(), 16_384);
        assert_eq!(s.columns[0], "id");
        assert_eq!(s.rows.len(), 1);
        assert_eq!(s.rows[0][16_383].as_deref(), Some("x"));
    }

    /// **The archive's own size says nothing about what opening it costs.**
    ///
    /// `XLSX_MAX_BYTES` bounds *compressed* bytes at both enforcement points —
    /// `xlsx_size_refusal` from the file's size on disk, `open_xlsx` from the
    /// bytes read through its `take` — and `xlsx_memory_warning` is derived from
    /// the same figure. None of them measures what is inflated, so a small
    /// workbook holding one hugely compressible part passed every check and
    /// handed calamine an archive that unpacks with no ceiling of its own, at
    /// *preview* time, on a probe that re-fires on every settings change in the
    /// import modal.
    ///
    /// Same shape as `b0374bc` ("take an Excel sheet's width from its cells, not
    /// from a declaration it may not have"): a figure the workbook supplies,
    /// trusted to describe what the workbook costs.
    ///
    /// The ceiling is a parameter here so the test can be fast; `open_xlsx`
    /// passes [`XLSX_MAX_INFLATED_BYTES`].
    #[test]
    fn a_workbook_that_unpacks_far_larger_than_it_looks_is_refused() {
        let good = workbook(&[("Sheet1", &[&[Cell::Text("id")], &[Cell::Num(1.0)]])]);
        let bomb = with_entry(&good, "xl/bomb.bin", &vec![b'0'; 4 * 1024 * 1024]);
        assert!(
            (bomb.len() as u64) < 64 * 1024,
            "the fixture must look small on disk: {} bytes",
            bomb.len()
        );
        // Every bound that reads the compressed size is happy with it.
        assert!(xlsx_size_refusal(ImportFormat::Xlsx, bomb.len() as u64).is_none());
        assert!(memory_warning(ImportFormat::Xlsx, bomb.len() as u64).is_none());

        let msg = inflated_refusal(&bomb, 1024 * 1024).expect("4 MiB is over a 1 MiB ceiling");
        assert!(msg.contains("unpacks"), "{msg}");
        // An ordinary workbook under the same ceiling is not refused, and the
        // bomb is not refused under a ceiling that fits it — the bound is the
        // inflated size, not the shape of the archive.
        assert!(inflated_refusal(&good, 1024 * 1024).is_none());
        assert!(inflated_refusal(&bomb, 64 * 1024 * 1024).is_none());
        // Something that is not an archive at all is left to calamine to
        // report, rather than being refused with the wrong reason.
        assert!(inflated_refusal(b"not a zip", 1).is_none());

        // The seam, not just the predicate: opening a workbook consults the
        // bound, and answers with the bound's own reason rather than whatever
        // calamine makes of the archive.
        let err = match open_xlsx_within(&bomb[..], 1024 * 1024) {
            Err(e) => e,
            Ok(_) => panic!("a workbook over the ceiling was opened"),
        };
        assert!(err.to_string().contains("unpacks"), "{err}");

        // And with the shipping ceiling both still open: the bound is generous.
        assert!(open_xlsx(&bomb[..]).is_ok());
        assert!(open_xlsx(&good[..]).is_ok());
    }

    /// **The disclosure has to arrive before the cost, and the two constants
    /// that decide that were set independently.**
    ///
    /// `XLSX_MEMORY_FACTOR` was measured on `read_sample` — the preview — and
    /// quoted by `xlsx_memory_warning`, which stands in front of the *load*;
    /// `XLSX_WARN_BYTES` was copied from JSON's on the reasoning that 4× and 5×
    /// were near enough. The load's measured ratio is 13.27×, so at the old
    /// pair a 200 MiB workbook cost ~2.8 GB and was disclosed as ~800 MB, and a
    /// 23 MB workbook cost ~310 MB and was disclosed as nothing at all.
    ///
    /// This is a pin on the relationship, not a measurement of it — a unit test
    /// cannot weigh a load. What it can do is fail when someone moves one of the
    /// two without the other, which is how they drifted apart.
    #[test]
    fn a_large_workbook_is_disclosed_before_it_costs() {
        // The warning fires strictly before the estimate reaches the budget.
        assert!(
            xlsx_load_estimate(XLSX_WARN_BYTES) <= XLSX_DISCLOSURE_BUDGET,
            "a workbook at the threshold already costs {} against a {} budget",
            crate::format::human_bytes(xlsx_load_estimate(XLSX_WARN_BYTES) as i64),
            crate::format::human_bytes(XLSX_DISCLOSURE_BUDGET as i64),
        );
        assert!(
            xlsx_memory_warning(ImportFormat::Xlsx, XLSX_WARN_BYTES + 1).is_some(),
            "nothing is said one byte past the threshold"
        );
        // The estimate is an over-estimate at the two shapes the factor was
        // measured on, `--release` with a counting global allocator over this
        // module: 23.3 MB / 100k × 50 peaked at 309.5 MB on `row_iter`, and
        // 4.7 MB / 20k × 50 at 13.53×. Stated over the estimate rather than over
        // the constant, because `assert!(CONST >= n)` is an assertion with a
        // constant value and cannot go red for the reason it is written for.
        assert!(
            xlsx_load_estimate(23_315_012) >= 309_468_534,
            "the estimate understates the shape it was measured on"
        );
        assert!(
            xlsx_load_estimate(4_700_000) >= (4_700_000f64 * 13.53) as u64,
            "the estimate understates the second measured shape"
        );
    }

    /// **The inflated-size refusal may not promise a memory ceiling**, because
    /// the quantity it counts is not memory.
    ///
    /// It sums the central directory's *declared uncompressed bytes*; what
    /// calamine then holds is the parsed shared-strings table, a `Vec<String>`.
    /// Measured against calamine 0.36.1, a 64 MiB `xl/sharedStrings.xml` of
    /// minimal entries costs 99 MB live — 1.48× the counted quantity, and that
    /// is a floor. So a workbook admitted at the ceiling costs half again as
    /// much as the sentence said was "the most this can hold in memory".
    #[test]
    fn the_inflated_refusal_does_not_promise_a_memory_ceiling() {
        let good = workbook(&[("Sheet1", &[&[Cell::Text("id")], &[Cell::Num(1.0)]])]);
        let bomb = with_entry(&good, "xl/bomb.bin", &vec![b'0'; 4 * 1024 * 1024]);
        let msg = inflated_refusal(&bomb, 1024 * 1024).expect("over the ceiling");
        assert!(
            !msg.contains("most this can hold in memory"),
            "the refusal promises a ceiling in the wrong unit: {msg}"
        );
        // It still names the ceiling and still says the file is the problem.
        assert!(msg.contains("unpacks to at least"), "{msg}");
        assert!(msg.contains("unpacked content"), "{msg}");
    }

    /// **One unreadable entry must not turn the bound off.**
    ///
    /// `by_index_raw` is not a directory read: it seeks to the entry's declared
    /// `local_header_offset` and checks the local-file-header magic. A `?` on
    /// that abandoned the whole scan and answered `None`, which `open_xlsx_within`
    /// reads as "no size problem" — so a four-byte tamper to the central-directory
    /// record of an entry **no workbook reader ever opens** turned the ceiling
    /// off for the archive. `ZipArchive::new` never visits a local header, and
    /// calamine reaches every part `by_name`, so the archive still opens, lists
    /// and reads fine; only the bound was skipped.
    ///
    /// The decoy is written *before* the bomb, which is the whole of the attack:
    /// the scan dies at index 1 and the bomb at index 2 is never summed.
    #[test]
    fn an_entry_that_cannot_be_sized_does_not_turn_the_bound_off() {
        let good = workbook(&[("Sheet1", &[&[Cell::Text("id")], &[Cell::Num(1.0)]])]);
        let decoy = with_entry(&good, "docProps/thumbnail.jpeg", b"not really a jpeg");
        let bomb = with_entry(&decoy, "xl/bomb.bin", &vec![b'0'; 4 * 1024 * 1024]);
        // The premise: intact, the bomb is refused over a 1 MiB ceiling.
        assert!(inflated_refusal(&bomb, 1024 * 1024).is_some());

        let tampered = break_local_header_offset(&bomb, "docProps/thumbnail.jpeg");
        // Still a readable archive by every route a workbook reader takes.
        let mut zin = zip::ZipArchive::new(std::io::Cursor::new(&tampered[..]))
            .expect("the central directory is intact");
        assert!(zin.by_name("xl/bomb.bin").is_ok(), "the bomb still reads");
        let decoy_at = zin
            .index_for_name("docProps/thumbnail.jpeg")
            .expect("the decoy is listed");
        let bomb_at = zin
            .index_for_name("xl/bomb.bin")
            .expect("the bomb is listed");
        assert!(decoy_at < bomb_at, "the decoy must be scanned first");
        assert!(
            zin.by_index_raw(decoy_at).is_err(),
            "the fixture must break the entry it claims to"
        );

        let msg = inflated_refusal(&tampered, 1024 * 1024)
            .expect("a broken decoy entry turned the ceiling off");
        assert!(msg.contains("unpacks"), "{msg}");
        // And the seam: opening it answers with the bound's own reason.
        let err = match open_xlsx_within(&tampered[..], 1024 * 1024) {
            Err(e) => e,
            Ok(_) => panic!("a workbook over the ceiling was opened"),
        };
        assert!(err.to_string().contains("unpacks"), "{err}");
        // An archive whose *directory* cannot be read is still calamine's to
        // diagnose — the doc's case, which this must not widen.
        assert!(inflated_refusal(b"not a zip", 1).is_none());
    }

    /// Point one central-directory record's `local_header_offset` at a byte that
    /// is not `PK\x03\x04`, leaving every other field — including the declared
    /// uncompressed size — exactly as it was.
    ///
    /// The record layout is fixed: signature, 38 bytes of header, then the
    /// four-byte offset at 42, then the name.
    fn break_local_header_offset(zip: &[u8], name: &str) -> Vec<u8> {
        let mut out = zip.to_vec();
        let sig = [b'P', b'K', 1, 2];
        let mut at = 0;
        while at + 46 <= out.len() {
            if out[at..at + 4] != sig {
                at += 1;
                continue;
            }
            let n = u16::from_le_bytes([out[at + 28], out[at + 29]]) as usize;
            if out.get(at + 46..at + 46 + n) == Some(name.as_bytes()) {
                // Byte 1 is inside the first local header, so it is a real
                // offset into the archive and not a truncation — the read gets
                // as far as the magic check and fails there.
                out[at + 42..at + 46].copy_from_slice(&1u32.to_le_bytes());
                return out;
            }
            at += 46 + n;
        }
        panic!("no central-directory record for {name}");
    }

    /// A copy of `xlsx` with one extra entry, for the fixture above — the same
    /// re-zip `restate_dimension` does, without touching what is there.
    fn with_entry(xlsx: &[u8], name: &str, payload: &[u8]) -> Vec<u8> {
        use std::io::{Cursor, Read, Write};
        let mut zin = zip::ZipArchive::new(Cursor::new(xlsx)).expect("a workbook is a zip");
        let mut out = Vec::new();
        let mut zout = zip::ZipWriter::new(Cursor::new(&mut out));
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).expect("an entry");
            let entry = f.name().to_string();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).expect("entry bytes");
            zout.start_file(entry, opts).expect("start");
            zout.write_all(&bytes).expect("write");
        }
        zout.start_file(name, opts).expect("start");
        zout.write_all(payload).expect("write");
        zout.finish().expect("finish");
        out
    }

    /// A real workbook with its `<dimension ref>` restated — the one thing
    /// `rust_xlsxwriter` will never write, because it always emits a correct
    /// one, and therefore the one case no fixture in this suite could produce.
    ///
    /// `ref=""` removes the element entirely, which is the other half of the
    /// same defect: ECMA-376 makes `<dimension>` optional and advisory, and a
    /// writer that streams cannot know the extent in advance.
    fn restate_dimension(xlsx: &[u8], reference: &str) -> Vec<u8> {
        use std::io::{Cursor, Read, Write};
        let mut zin = zip::ZipArchive::new(Cursor::new(xlsx)).expect("a workbook is a zip");
        let mut out = Vec::new();
        let mut zout = zip::ZipWriter::new(Cursor::new(&mut out));
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut patched = false;
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).expect("an entry");
            let name = f.name().to_string();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).expect("entry bytes");
            if name.starts_with("xl/worksheets/") && name.ends_with(".xml") {
                let xml = String::from_utf8(bytes).expect("sheet xml is utf-8");
                let at = xml.find("<dimension ").expect("rust_xlsxwriter writes one");
                let end = xml[at..].find("/>").expect("a self-closing element") + at + 2;
                let replacement = if reference.is_empty() {
                    String::new()
                } else {
                    format!("<dimension ref=\"{reference}\"/>")
                };
                bytes = format!("{}{replacement}{}", &xml[..at], &xml[end..]).into_bytes();
                patched = true;
            }
            zout.start_file(name, opts).expect("start");
            zout.write_all(&bytes).expect("write");
        }
        zout.finish().expect("finish");
        assert!(patched, "no worksheet XML in the workbook");
        out
    }

    /// Turn one cell of a real workbook into an **error cell** (`t="e"`) — the
    /// other thing `rust_xlsxwriter` will not write, since it has no API for a
    /// cached formula result, let alone a failed one.
    ///
    /// Without it the two halves of the error-cell question cannot be asked of
    /// the same file: a fixture can hold the *text* `#N/A` or it can hold what
    /// a `VLOOKUP` miss leaves behind, and until this existed only the first
    /// was reachable — which is exactly why the code could tell them apart by
    /// spelling and look right. `reference` is a cell the fixture already
    /// wrote, so there is a `<c>` element to replace.
    fn make_error_cell(xlsx: &[u8], reference: &str, spelling: &str) -> Vec<u8> {
        use std::io::{Cursor, Read, Write};
        let mut zin = zip::ZipArchive::new(Cursor::new(xlsx)).expect("a workbook is a zip");
        let mut out = Vec::new();
        let mut zout = zip::ZipWriter::new(Cursor::new(&mut out));
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut patched = false;
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).expect("an entry");
            let name = f.name().to_string();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).expect("entry bytes");
            if name.starts_with("xl/worksheets/") && name.ends_with(".xml") {
                let xml = String::from_utf8(bytes).expect("sheet xml is utf-8");
                let needle = format!("<c r=\"{reference}\"");
                let at = xml.find(&needle).expect("the fixture wrote that cell");
                let end = xml[at..].find("</c>").expect("a cell with a value") + at + 4;
                bytes = format!(
                    "{}<c r=\"{reference}\" t=\"e\"><v>{spelling}</v></c>{}",
                    &xml[..at],
                    &xml[end..]
                )
                .into_bytes();
                patched = true;
            }
            zout.start_file(name, opts).expect("start");
            zout.write_all(&bytes).expect("write");
        }
        zout.finish().expect("finish");
        assert!(patched, "no worksheet XML in the workbook");
        out
    }

    /// **A cell whose text *is* `#N/A` and a cell the sheet could not evaluate
    /// are different things, and only the second is a [`IssueKind::CellError`].**
    ///
    /// `cell_text` is handed a `Data::String` and a `Data::Error` — calamine
    /// keeps them apart — and flattened both to `Some(String)`, after which the
    /// only oracle left was the spelling. So a `status` column holding the
    /// literal `#N/A` was reported as *"the sheet could not evaluate this
    /// cell"*, which is untrue, and the file became unimportable: `row_iter`'s
    /// `Err` aborts the whole transaction, and `NullRule` is no escape because
    /// `has_own_nulls()` replaces the rule for a worksheet. `#N/A` is pandas'
    /// first default `na_values` entry and what people type for "not
    /// applicable", so this is an ordinary file, not an exotic one.
    ///
    /// Both cells are in the same column of the same workbook on purpose: a
    /// fixture with only one of them passes whichever way the code guesses.
    #[test]
    fn a_typed_na_is_text_and_only_the_sheet_s_own_error_is_refused() {
        let good = workbook(&[(
            "Sheet1",
            &[
                &[Cell::Text("id"), Cell::Text("status")],
                &[Cell::Num(1.0), Cell::Text("#N/A")],
                &[Cell::Num(2.0), Cell::Num(0.0)],
            ],
        )]);
        let bytes = make_error_cell(&good, "B3", "#N/A");

        let table = tbl(&[("id", "int", false), ("status", "varchar", true)]);
        let mapping = auto_map(&["id".into(), "status".into()], &table, true);
        let cfg = xlsx_cfg(true, None);

        let v = validate(
            &bytes[..],
            ImportFormat::Xlsx,
            &cfg,
            &table,
            &mapping,
            MySql,
            200,
        )
        .unwrap();
        assert_eq!(v.issues.len(), 1, "{:?}", v.issues);
        assert_eq!(v.issues[0].kind, IssueKind::CellError);
        assert_eq!(v.issues[0].line, 3);
        assert_eq!(v.issues[0].column, "status");

        let rows: Vec<_> = row_iter(
            &bytes[..],
            ImportFormat::Xlsx,
            &cfg,
            &table,
            &mapping,
            MySql,
        )
        .unwrap()
        .collect();
        assert_eq!(
            rows[0],
            Ok(vec![Value::Int(1), Value::Str("#N/A".into())]),
            "the typed one is text"
        );
        assert!(
            rows[1].is_err(),
            "the sheet's own one is not: {:?}",
            rows[1]
        );
    }

    /// **A worksheet's `<dimension>` is advisory, and believing it as the width
    /// threw away two columns of three — in the preview *and* in the load, with
    /// every surface reporting success.**
    ///
    /// `auto_map` yields one column, `missing_required` returns nothing (the
    /// dropped columns are nullable, so nothing warns), and the import reports
    /// the right number of *rows*. The doc that stood here reasoned the absent
    /// case to the wrong answer — "either way one column is the right answer,
    /// and the header row is what names them" — which is true of a one-cell
    /// sheet and false of every sheet anyone imports.
    ///
    /// The fixture is the point: every other Excel test in this module is built
    /// through `rust_xlsxwriter`, which always writes a correct `<dimension>`,
    /// so the under-declared direction had nothing that could produce it.
    #[test]
    fn a_sheet_that_under_declares_its_extent_still_imports_every_column() {
        let good = workbook(&[(
            "Sheet1",
            &[
                &[Cell::Text("id"), Cell::Text("name"), Cell::Text("email")],
                &[Cell::Num(1.0), Cell::Text("ann"), Cell::Text("a@x")],
                &[Cell::Num(2.0), Cell::Text("bob"), Cell::Text("b@x")],
            ],
        )]);
        let cfg = xlsx_cfg(true, None);
        let want_cols = ["id", "name", "email"];
        let want_rows = [["1", "ann", "a@x"], ["2", "bob", "b@x"]];

        for reference in ["A1:C3", "A1", "", "B1:C3", "A1:XFD3"] {
            let bytes = restate_dimension(&good, reference);
            let s = read_sample(&bytes[..], ImportFormat::Xlsx, &cfg, 10)
                .unwrap_or_else(|e| panic!("ref={reference:?}: {e}"));
            assert_eq!(
                s.columns.len(),
                // `A1:XFD3` declares the whole sheet; the cells decide the rest,
                // but the declared ceiling is still honoured as a *ceiling*.
                if reference == "A1:XFD3" { 16_384 } else { 3 },
                "ref={reference:?} columns={:?}",
                &s.columns[..s.columns.len().min(6)]
            );
            assert_eq!(&s.columns[..3], &want_cols, "ref={reference:?}");
            assert_eq!(s.rows.len(), 2, "ref={reference:?}");
            for (r, want) in want_rows.iter().enumerate() {
                let got: Vec<&str> = s.rows[r][..3]
                    .iter()
                    .map(|f| f.as_deref().unwrap_or(""))
                    .collect();
                assert_eq!(got, want, "ref={reference:?} row {r}");
            }
        }
    }

    /// **A row wider than the settled width is reported, not silently shortened
    /// — the same answer CSV gives the same file.**
    ///
    /// `trim_to_mapping`'s doc claims the guarantee this tests: *"Excel sides
    /// with CSV, not JSON: a worksheet's columns are fixed by its used range, so
    /// every row is already the same width and a count mismatch is a real one
    /// worth reporting"*. It was right that every row arrived the same width,
    /// and that was the defect — the reader padded and truncated every row to
    /// `width` before `coerce_record` saw it, so the branch the doc points at
    /// was unreachable by construction and the loss it was meant to report had
    /// no other reporter: the preview showed two columns, `missing_required`
    /// was silent (the lost column is nullable) and the import reported the
    /// right number of rows having written two-thirds of the data.
    ///
    /// The fixture needs a *narrow* declaration, because `rust_xlsxwriter`
    /// always writes a correct `<dimension>` and the first row would then settle
    /// the full width.
    #[test]
    fn an_excel_row_wider_than_the_header_is_reported_like_a_ragged_csv_row() {
        let wide = workbook(&[(
            "Sheet1",
            &[
                &[Cell::Text("id"), Cell::Text("name")],
                &[Cell::Num(1.0), Cell::Text("ann"), Cell::Text("a@x")],
            ],
        )]);
        let bytes = restate_dimension(&wide, "A1:B2");
        let cfg = xlsx_cfg(true, None);

        let s = read_sample(&bytes[..], ImportFormat::Xlsx, &cfg, 10).expect("preview");
        assert_eq!(s.columns, vec!["id".to_string(), "name".to_string()]);
        assert_eq!(
            s.rows[0].len(),
            3,
            "the third cell has to reach the preview to be reportable: {:?}",
            s.rows[0]
        );

        // …and the same file through the same CSV shape, for the comparison the
        // doc makes.
        let t = tbl(&[("id", "int", true), ("name", "varchar", true)]);
        let m = auto_map(&["id".into(), "name".into()], &t, true);
        let v = validate(&bytes[..], ImportFormat::Xlsx, &cfg, &t, &m, MySql, 100)
            .expect("validation runs");
        assert!(
            v.issues.iter().any(|i| matches!(
                i.kind,
                IssueKind::FieldCount {
                    expected: 2,
                    found: 3
                }
            )),
            "the widened row was dropped in silence: {:?}",
            v.issues
        );
    }

    /// And the arrangement the drop site exists for stays silent: a title block
    /// puts the used range's origin partway across, and those rows are *not*
    /// ragged.
    #[test]
    fn a_title_block_offset_sheet_reports_no_field_count_mismatch() {
        let bytes = workbook_at(
            4,
            2,
            &[(
                "Sheet1",
                &[
                    &[Cell::Text("id"), Cell::Text("name")],
                    &[Cell::Num(1.0), Cell::Text("ann")],
                    &[Cell::Num(2.0), Cell::Text("bob")],
                ],
            )],
        );
        let cfg = xlsx_cfg(true, None);
        let t = tbl(&[("id", "int", true), ("name", "varchar", true)]);
        let m = auto_map(&["id".into(), "name".into()], &t, true);
        let v = validate(&bytes[..], ImportFormat::Xlsx, &cfg, &t, &m, MySql, 100)
            .expect("validation runs");
        assert!(
            !v.issues
                .iter()
                .any(|i| matches!(i.kind, IssueKind::FieldCount { .. })),
            "{:?}",
            v.issues
        );
        assert_eq!(v.rows, 2);
    }

    /// The one thing a sheet's declared extent can still make unbounded: the row
    /// buffer. Excel's own ceiling is 16,384 columns, so a workbook claiming
    /// more is refused rather than believed.
    #[test]
    fn a_sheet_claiming_more_columns_than_excel_has_is_refused() {
        use calamine::Dimensions;
        assert_eq!(sheet_width(Dimensions::default()).unwrap(), 1);
        assert_eq!(
            sheet_width(Dimensions::new((0, 0), (9, 3))).unwrap(),
            4,
            "inclusive of both edges"
        );
        // An offset used range is as wide as it is, not as far right as it ends.
        assert_eq!(sheet_width(Dimensions::new((4, 2), (9, 5))).unwrap(), 4);
        assert_eq!(
            sheet_width(Dimensions::new((0, 0), (0, XLSX_MAX_COLS as u32 - 1))).unwrap(),
            XLSX_MAX_COLS as usize
        );
        let err = sheet_width(Dimensions::new((0, 0), (0, XLSX_MAX_COLS as u32)))
            .expect_err("past Excel's own ceiling");
        assert!(err.to_string().contains("CSV"), "{err}");
    }

    /// The memory disclosure fires for a big workbook and for nothing else. The
    /// shared entry point is the point: a format that needs a warning must not
    /// be able to have one nobody asks for.
    #[test]
    fn the_memory_warning_speaks_up_for_a_large_workbook_only() {
        assert_eq!(memory_warning(ImportFormat::Xlsx, 1024), None);
        assert_eq!(
            memory_warning(ImportFormat::Csv, XLSX_WARN_BYTES * 10),
            None
        );
        let msg = memory_warning(ImportFormat::Xlsx, XLSX_WARN_BYTES + 1)
            .expect("a workbook past the threshold");
        assert!(msg.contains("held in memory"), "{msg}");
        assert!(msg.contains("CSV"), "{msg}");
        // The JSON warning still reaches the same entry point — the whole reason
        // it exists is that a second direct caller is how the two drift.
        assert!(memory_warning(ImportFormat::Json, JSON_WARN_BYTES + 1).is_some());
    }

    /// Fields as CSV produces them — text, never a format-level null.
    fn f(v: &[&str]) -> Vec<Field> {
        v.iter().map(|s| Some((*s).to_string())).collect()
    }

    #[test]
    fn read_sample_takes_the_header_and_the_first_rows() {
        let csv = "id,name\n1,Smith\n2,Jones\n3,Ray\n";
        let s = read_sample(csv.as_bytes(), ImportFormat::Csv, &cfg(true), 2).unwrap();
        assert_eq!(s.columns, vec!["id", "name"]);
        assert_eq!(s.rows, vec![f(&["1", "Smith"]), f(&["2", "Jones"])]);
        assert!(s.more, "a fourth record exists beyond the sample");
    }

    #[test]
    fn read_sample_without_a_header_synthesizes_column_names() {
        let csv = "1,Smith\n2,Jones\n";
        let s = read_sample(csv.as_bytes(), ImportFormat::Csv, &cfg(false), 10).unwrap();
        assert_eq!(s.columns, vec!["Column 1", "Column 2"]);
        assert_eq!(s.rows.len(), 2);
        assert!(!s.more);
    }

    /// A UTF-8 BOM is invisible in an editor but becomes part of the first
    /// column's name, so name-matching silently fails on the one column most
    /// likely to be the key.
    #[test]
    fn read_sample_strips_a_utf8_bom_from_the_first_column() {
        let csv = "\u{feff}id,name\n1,Smith\n";
        let s = read_sample(csv.as_bytes(), ImportFormat::Csv, &cfg(true), 10).unwrap();
        assert_eq!(s.columns, vec!["id", "name"]);
    }

    /// `name, city` with a space after the comma is everywhere, and only numeric
    /// parsing trims — a text column would store the leading space verbatim.
    #[test]
    fn trim_strips_surrounding_whitespace_from_fields_and_headers() {
        let csv = " id , name \n 1 , Smith \n";
        let mut c = cfg(true);
        c.trim = true;
        let s = read_sample(csv.as_bytes(), ImportFormat::Csv, &c, 10).unwrap();
        assert_eq!(s.columns, vec!["id", "name"]);
        assert_eq!(s.rows[0], f(&["1", "Smith"]));
    }

    /// Off by default: trimming silently rewrites data, so it's the user's call —
    /// and the preview shows the spaces, which is what makes it their call.
    #[test]
    fn without_trim_the_whitespace_is_kept() {
        let csv = " id , name \n 1 , Smith \n";
        let s = read_sample(csv.as_bytes(), ImportFormat::Csv, &cfg(true), 10).unwrap();
        assert_eq!(s.columns, vec![" id ", " name "]);
        assert_eq!(s.rows[0], f(&[" 1 ", " Smith "]));
        assert!(!ReadConfig::default().trim, "trim defaults off");
    }

    /// Trimming reaches *inside* quotes too — arguably it shouldn't, since
    /// quoting padding is how a file says it's deliberate, but that's the `csv`
    /// reader's behaviour. Pinned so it's a known limitation rather than a
    /// surprise, and it's why the setting defaults off.
    #[test]
    fn trim_also_strips_padding_inside_quotes() {
        let csv = "name\n\"  padded  \"\n";
        let mut c = cfg(true);
        c.trim = true;
        let s = read_sample(csv.as_bytes(), ImportFormat::Csv, &c, 10).unwrap();
        assert_eq!(s.rows[0][0].as_deref(), Some("padded"));
        // Off (the default), the padding is kept.
        let s = read_sample(csv.as_bytes(), ImportFormat::Csv, &cfg(true), 10).unwrap();
        assert_eq!(s.rows[0][0].as_deref(), Some("  padded  "));
    }

    #[test]
    fn read_sample_keeps_quoted_delimiters_and_newlines_intact() {
        let csv = "name,note\n\"Smith, John\",\"line one\nline two\"\n";
        let s = read_sample(csv.as_bytes(), ImportFormat::Csv, &cfg(true), 10).unwrap();
        assert_eq!(s.rows[0][0].as_deref(), Some("Smith, John"));
        assert_eq!(s.rows[0][1].as_deref(), Some("line one\nline two"));
    }

    // ── JSON ────────────────────────────────────────────────────────────────

    #[test]
    fn read_sample_reads_a_json_array_of_objects() {
        let json = r#"[{"id": 1, "name": "Smith"}, {"id": 2, "name": "Jones"}]"#;
        let s = read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 10).unwrap();
        assert_eq!(s.columns, vec!["id", "name"]);
        assert_eq!(s.rows, vec![f(&["1", "Smith"]), f(&["2", "Jones"])]);
    }

    /// **A BOM'd JSON file is the ordinary output of this project's own shell.**
    ///
    /// Windows PowerShell 5.1's UTF-8 encoders (`Set-Content -Encoding utf8`,
    /// `Out-File`) emit `EF BB BF`, and `serde_json` skips only space, tab, CR
    /// and LF as whitespace — so it stopped on `0xEF` and the user was told
    /// *"Couldn't read the file: expected value at line 1 column 1"* about a
    /// perfectly well-formed file, at preview time, with nothing on screen to
    /// say what was wrong. The BOM also defeated `ArrayUnwrap`'s array
    /// detection, so a BOM'd `[{…}]` was not even unwrapped.
    ///
    /// `strip_bom` is the rule and it reached the CSV and Excel readers only —
    /// JSON was the third reader and took no cure. Both shapes here, because
    /// the array one needs the *unwrap* to see past it as well as the parser.
    #[test]
    fn a_bom_does_not_make_a_json_file_unreadable() {
        const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
        for body in [
            br#"[{"id": 1, "name": "Smith"}]"#.as_slice(),
            br#"{"id": 1, "name": "Smith"}"#.as_slice(),
        ] {
            let mut bytes = BOM.to_vec();
            bytes.extend_from_slice(body);
            let s = read_sample(&bytes[..], ImportFormat::Json, &cfg(true), 10)
                .unwrap_or_else(|e| panic!("{e:?} over {:?}", String::from_utf8_lossy(body)));
            assert_eq!(s.columns, vec!["id", "name"]);
            assert_eq!(s.rows, vec![f(&["1", "Smith"])]);
        }
    }

    /// Newline-delimited JSON is what most tools emit for anything large, and it
    /// streams where an array can't.
    #[test]
    fn read_sample_reads_newline_delimited_json() {
        let json = "{\"id\": 1}\n{\"id\": 2}\n";
        let s = read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 10).unwrap();
        assert_eq!(s.columns, vec!["id"]);
        assert_eq!(s.rows, vec![f(&["1"]), f(&["2"])]);
    }

    /// A later object carrying a key the first one lacked must widen the column
    /// set, not be silently dropped.
    #[test]
    fn json_columns_are_the_union_of_every_objects_keys() {
        let json = r#"[{"b": 1}, {"a": 2, "b": 3}]"#;
        let s = read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 10).unwrap();
        assert_eq!(s.columns, vec!["b", "a"]);
        // The first object has no `a`, so that field is a real null.
        assert_eq!(s.rows[0], vec![Some("1".to_string()), None]);
        assert_eq!(
            s.rows[1],
            vec![Some("3".to_string()), Some("2".to_string())]
        );
    }

    /// Within one object the keys arrive alphabetically, not in document order —
    /// `serde_json::Map` is a `BTreeMap`. Pinned so it's a known, deliberate
    /// limitation rather than a surprise; it only affects preview column order,
    /// since JSON maps to columns by name.
    #[test]
    fn json_keys_within_an_object_come_out_alphabetically() {
        let json = r#"[{"zebra": 1, "apple": 2}]"#;
        let s = read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 10).unwrap();
        assert_eq!(s.columns, vec!["apple", "zebra"]);
    }

    /// The distinction CSV can't make: JSON says outright which is which.
    #[test]
    fn json_null_and_empty_string_stay_different() {
        let json = r#"[{"a": null, "b": ""}]"#;
        let s = read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 10).unwrap();
        assert_eq!(s.rows[0], vec![None, Some(String::new())]);
    }

    /// A nested value becomes its JSON text — which is exactly what a JSON column
    /// wants, and readable in the preview either way.
    #[test]
    fn json_nested_values_become_their_json_text() {
        let json = r#"[{"meta": {"k": [1, 2]}, "flag": true, "n": 1.5}]"#;
        let s = read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 10).unwrap();
        // Columns are alphabetical: flag, meta, n.
        assert_eq!(s.columns, vec!["flag", "meta", "n"]);
        assert_eq!(s.rows[0][0].as_deref(), Some("true"));
        assert_eq!(s.rows[0][1].as_deref(), Some(r#"{"k":[1,2]}"#));
        assert_eq!(s.rows[0][2].as_deref(), Some("1.5"));
    }

    /// A comma inside a string is data, not a separator — blanking it would
    /// silently rewrite the value, which is the one way this reader could corrupt
    /// an import rather than just fail it.
    #[test]
    fn json_array_commas_inside_strings_and_records_survive() {
        let json = r#"[{"a": "x,y", "b": [1, 2], "c": {"d": 3}},
                       {"a": "esc\", still string, here", "b": [], "c": {}}]"#;
        let s = read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 10).unwrap();
        assert_eq!(s.columns, vec!["a", "b", "c"]);
        assert_eq!(s.rows[0][0].as_deref(), Some("x,y"));
        assert_eq!(s.rows[0][1].as_deref(), Some("[1,2]"));
        assert_eq!(s.rows[0][2].as_deref(), Some(r#"{"d":3}"#));
        // A `,` after an escaped quote is still inside the string.
        assert_eq!(s.rows[1][0].as_deref(), Some(r#"esc", still string, here"#));
    }

    /// The point of streaming the array: a sample must stop at its limit instead
    /// of deserializing the whole file first. Reading through a reader that
    /// refuses to go past the sample is the only way to assert it actually did.
    #[test]
    fn sampling_a_json_array_stops_reading_at_the_limit() {
        struct Fused<'a> {
            data: &'a [u8],
            pos: usize,
            cap: usize,
        }
        impl std::io::Read for Fused<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.pos >= self.cap {
                    panic!("read past byte {} — the whole array was parsed", self.cap);
                }
                let n = (self.data.len() - self.pos).min(buf.len()).min(1);
                buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
                self.pos += n;
                Ok(n)
            }
        }
        let mut json = String::from("[");
        for i in 0..2000 {
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!(r#"{{"id": {i}}}"#));
        }
        json.push(']');
        // Three records is a few dozen bytes; a materializing reader would run to
        // the end of a 20KB document and trip the panic above.
        let r = Fused {
            data: json.as_bytes(),
            pos: 0,
            cap: 400,
        };
        let s = read_sample(r, ImportFormat::Json, &cfg(true), 3).unwrap();
        assert_eq!(s.rows.len(), 3);
        assert!(s.more);
        assert_eq!(s.rows[0][0].as_deref(), Some("0"));
        assert_eq!(s.rows[2][0].as_deref(), Some("2"));
    }

    /// **A record count is not a byte bound.** `reader_for` sets no field- or
    /// record-size limit, so one stray `"` makes the whole remainder of a file a
    /// single unterminated field, and a sample "of 200 records" reads to EOF —
    /// materialising the file as a `String` and again as a `StringRecord`, from
    /// a file the user only meant to look at.
    ///
    /// The reader here **panics** past the cap, so a read that isn't bounded
    /// fails loudly rather than merely taking a while.
    #[test]
    fn an_unterminated_quote_cannot_read_past_the_sample_bound() {
        struct Fused<'a> {
            data: &'a [u8],
            pos: usize,
            cap: usize,
        }
        impl std::io::Read for Fused<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                assert!(self.pos <= self.cap, "read past the bound at {}", self.pos);
                let n = (self.data.len() - self.pos).min(buf.len());
                buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
                self.pos += n;
                Ok(n)
            }
        }
        // A header, then a record that opens a quote and never closes it — the
        // rest of the "file" is one field as far as the CSV reader is concerned.
        let mut csv = String::from("id,name\n1,\"");
        csv.push_str(&"x".repeat(SAMPLE_MAX_BYTES as usize * 2));
        let r = Fused {
            data: csv.as_bytes(),
            pos: 0,
            // A little slack for the reader's own buffering past the `take`.
            cap: SAMPLE_MAX_BYTES as usize + 64 * 1024,
        };
        // It may fail or return a short sample; what it must not do is read on.
        let _ = read_sample(r, ImportFormat::Csv, &cfg(true), 200);
    }

    /// **A big JSON file previews as a prefix of records, not as an error.**
    ///
    /// `read_sample` wraps both non-Excel formats in the `SAMPLE_MAX_BYTES`
    /// cap. CSV degrades gracefully under truncation — the reader just yields
    /// fewer records — while `serde_json`'s `StreamDeserializer` meets EOF
    /// *inside* a value and errors, which reached the user as `Couldn't read
    /// the file: EOF while parsing a string at line 1 column 8388608`: a
    /// message that reads as file corruption, on a file `validate` then walks
    /// end to end without complaint. `read_sample`'s own doc says "a truncated
    /// read can only make the preview *shorter*", and it could not.
    ///
    /// The trigger is the first `limit` records exceeding the cap — an average
    /// record over ~42 KiB, which one text column reaches easily.
    #[test]
    fn a_json_file_past_the_sample_cap_previews_what_it_read() {
        // Five records of ~3 MiB each: the cap lands inside the third string.
        let big = "x".repeat(3 * 1024 * 1024);
        let mut json = String::from("[");
        for i in 0..5 {
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!(r#"{{"id": {i}, "body": "{big}"}}"#));
        }
        json.push(']');
        assert!(
            json.len() as u64 > SAMPLE_MAX_BYTES,
            "the fixture must exceed the cap"
        );

        let s = read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 200)
            .expect("a large file previews rather than failing");
        assert!(!s.rows.is_empty(), "nothing was previewed at all");
        assert!(s.rows.len() < 5, "the cap did not bite: {}", s.rows.len());
        assert!(s.more, "a cut-short preview has to say there is more");
        assert_eq!(s.columns, vec!["body".to_string(), "id".to_string()]);
    }

    /// **And when the cap lands inside the *first* record, the reason is the
    /// cap — not corruption.**
    ///
    /// `531c756`'s arm asked "did anything parse", which conflates two facts
    /// the code can tell apart: *did the cap arrive* and *did anything parse*.
    /// One JSON object with a large text or base64 column left `objects` empty,
    /// so the arm did not fire and the user got `Couldn't read the file: EOF
    /// while parsing a string at line 1 column 8388608` — the exact message, on
    /// the exact cause, that commit was written to remove, on a file the
    /// unbounded walk reads end to end without complaint.
    #[test]
    fn a_json_first_record_over_the_cap_blames_the_cap_not_the_file() {
        let big = "x".repeat(SAMPLE_MAX_BYTES as usize + 1024);
        let json = format!(r#"[{{"id": 1, "body": "{big}"}}]"#);

        let err = read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 200)
            .expect_err("there is genuinely nothing to preview");
        let msg = err.to_string();
        assert!(msg.contains("preview"), "{msg}");
        assert!(!msg.contains("EOF while parsing"), "{msg}");
        // **A variant, not a sentence**, because the caller has to tell this
        // from corruption to know it can fall back to `read_json_columns`.
        assert!(
            matches!(err, ImportError::PreviewRecordTooLarge { .. }),
            "{err:?}"
        );

        // The file itself is fine, which is the whole point: the unbounded walk
        // reads it.
        let mut n = 0;
        for_each_record(json.as_bytes(), ImportFormat::Json, &cfg(true), |_, _| {
            n += 1;
            true
        })
        .expect("the file is not corrupt");
        assert_eq!(n, 1);
    }

    /// **…and the file is still importable.**
    ///
    /// The half the message left out. `read_sample` returning `Err` sets the
    /// modal's sample to `None`, and Next is gated on the sample being `Some` —
    /// so a valid JSON file whose first record is one long text or base64 column
    /// could not be imported at all, under a sentence explaining that the
    /// *preview* was the problem.
    ///
    /// The mapping step needs a column list and nothing else. `read_json_columns`
    /// reads the first record with every value as `IgnoredAny` — unbounded in
    /// bytes, bounded in memory — and hands back its keys with no rows.
    #[test]
    fn a_json_record_too_big_to_preview_still_yields_its_columns() {
        let big = "x".repeat(SAMPLE_MAX_BYTES as usize + 1024);
        let json = format!(r#"[{{"id": 1, "body": "{big}", "note": null}}]"#);

        let s = read_json_columns(json.as_bytes()).expect("the columns are readable");
        assert_eq!(
            s.columns,
            vec!["body".to_string(), "id".to_string(), "note".to_string()],
            "every key of the first record, including the null one"
        );
        assert!(s.rows.is_empty(), "there are deliberately no preview rows");
        assert!(s.more);

        // And the columns it names are the ones the whole-file walk emits, which
        // is the property that makes the mapping built from them correct.
        let mut walked: Vec<String> = Vec::new();
        json_records(json.as_bytes(), &mut walked, 1, None, |_, _| true)
            .expect("the file is not corrupt");
        assert_eq!(
            walked, s.columns,
            "the fallback names different columns from the walk that will import them"
        );
    }

    /// **The composition, which is where the defect was.**
    ///
    /// `read_sample` refusing to invent a preview is right, and the modal gating
    /// Next on having a sample is right; between them a valid JSON file whose
    /// first record is one long text or base64 column could not be imported at
    /// all. The decision that joins them used to live in the probe's closure,
    /// where no test could reach it.
    #[test]
    fn a_file_whose_first_record_cannot_be_previewed_is_still_probed() {
        let big = "x".repeat(SAMPLE_MAX_BYTES as usize + 1024);
        let json = format!(r#"[{{"id": 1, "body": "{big}"}}]"#);

        // The half that refuses, unchanged.
        assert!(matches!(
            read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 200),
            Err(ImportError::PreviewRecordTooLarge { .. })
        ));

        // …and the probe over the same file hands back something the mapping
        // step can use.
        let s = probe_sample(|| Ok(json.as_bytes()), ImportFormat::Json, &cfg(true), 200)
            .expect("a file the walk can read is a file the modal can offer");
        assert_eq!(s.columns, vec!["body".to_string(), "id".to_string()]);
        assert!(s.values_withheld, "the view has to be able to say why");
        assert!(s.rows.is_empty());

        // An ordinary file is untouched by any of this — same sample, one read.
        let plain = r#"[{"a": 1, "b": 2}]"#;
        let via_probe =
            probe_sample(|| Ok(plain.as_bytes()), ImportFormat::Json, &cfg(true), 200).unwrap();
        let direct = read_sample(plain.as_bytes(), ImportFormat::Json, &cfg(true), 200).unwrap();
        assert_eq!(via_probe, direct);
        assert!(!via_probe.values_withheld);
    }

    /// A genuinely broken file is still broken on the fallback path — it is a
    /// different reader over the same bytes, not a way past the parser.
    #[test]
    fn the_column_only_read_still_refuses_a_broken_file() {
        // …including through the probe, which must not turn a corrupt file into
        // a column list.
        assert!(
            probe_sample(
                || Ok(br#"[{"a": "unterminated"#.as_slice()),
                ImportFormat::Json,
                &cfg(true),
                200,
            )
            .is_err(),
            "the probe fell back on a file that is genuinely truncated"
        );
        assert!(read_json_columns(br#"[{"a": "unterminated"#.as_slice()).is_err());
        assert!(read_json_columns(b"not json at all".as_slice()).is_err());
        assert!(
            read_json_columns(b"[1, 2, 3]".as_slice()).is_err(),
            "an array of scalars has no columns"
        );
        assert!(read_json_columns(b"[]".as_slice()).is_err(), "no records");
    }

    /// The other half of the same question: a file that really is truncated
    /// before its first record is still reported as the broken file it is.
    #[test]
    fn a_json_file_truncated_before_its_first_record_is_still_an_error() {
        let err = read_sample(
            br#"[{"a": "unterminated"#.as_slice(),
            ImportFormat::Json,
            &cfg(true),
            200,
        )
        .expect_err("a truncated file is an error");
        let msg = err.to_string();
        assert!(
            !msg.contains("preview"),
            "blamed the cap for a short file: {msg}"
        );
    }

    /// And a file that really is broken still says so — the reason the bound is
    /// a parameter rather than an unconditional "EOF means stop".
    #[test]
    fn a_truncated_json_file_is_still_an_error_on_the_whole_file_walk() {
        let json = r#"[{"a": 1}, {"a": "unterminated"#;
        // The sample path is bounded, and something parsed, so it previews what
        // it got — the same rule as above.
        let s = read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 200).expect("preview");
        assert_eq!(s.rows.len(), 1);
        assert!(s.more);
        // The whole-file walk is not bounded, so the same bytes are a failure
        // there rather than a silent partial import.
        let mut n = 0;
        let err = for_each_record(json.as_bytes(), ImportFormat::Json, &cfg(true), |_, _| {
            n += 1;
            true
        });
        assert!(
            err.is_err(),
            "a truncated file imported {n} rows in silence"
        );
    }

    /// The rewrite is byte-for-byte in place, so it has to survive a record
    /// straddling any read boundary — including one that splits an escape pair.
    #[test]
    fn json_array_reads_the_same_however_the_bytes_arrive() {
        struct Trickle<'a>(&'a [u8], usize, usize);
        impl std::io::Read for Trickle<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = (self.0.len() - self.1).min(buf.len()).min(self.2);
                buf[..n].copy_from_slice(&self.0[self.1..self.1 + n]);
                self.1 += n;
                Ok(n)
            }
        }
        let json = r#"[{"a": "x,\"y", "b": 1}, {"a": "z", "b": 2}]"#;
        let whole = read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 10).unwrap();
        for chunk in [1usize, 2, 3, 7, 13] {
            let s = read_sample(
                Trickle(json.as_bytes(), 0, chunk),
                ImportFormat::Json,
                &cfg(true),
                10,
            )
            .unwrap();
            assert_eq!(s, whole, "chunk size {chunk}");
        }
    }

    /// JSON Lines must pass through the unwrapper untouched — a top-level comma
    /// inside an object would otherwise be blanked.
    #[test]
    fn json_lines_are_not_rewritten() {
        let json = "{\"a\": 1, \"b\": \"x,y\"}\n{\"a\": 2, \"b\": \"z\"}\n";
        let s = read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 10).unwrap();
        assert_eq!(s.rows.len(), 2);
        assert_eq!(s.rows[0][1].as_deref(), Some("x,y"));
    }

    #[test]
    fn an_empty_json_array_reads_as_no_rows() {
        let s = read_sample(b"[]".as_slice(), ImportFormat::Json, &cfg(true), 10).unwrap();
        assert!(s.columns.is_empty());
        assert!(s.rows.is_empty());
        assert!(!s.more);
    }

    #[test]
    fn json_that_is_not_objects_is_a_read_error() {
        let json = "[1, 2, 3]";
        assert!(matches!(
            read_sample(json.as_bytes(), ImportFormat::Json, &cfg(true), 10),
            Err(ImportError::Read(_))
        ));
    }

    /// JSON booleans and numbers must survive the same coercion path CSV uses —
    /// this is the check that the two formats really do share one validator.
    #[test]
    fn json_validates_through_the_same_path_as_csv() {
        let t = tbl(&[("id", "int", false), ("ok", "boolean", true)]);
        let json = r#"[{"id": 1, "ok": true}, {"id": "nope", "ok": false}]"#;
        let m = auto_map(&["id".into(), "ok".into()], &t, true);
        let v = validate(
            json.as_bytes(),
            ImportFormat::Json,
            &cfg(true),
            &t,
            &m,
            MySql,
            100,
        )
        .unwrap();
        assert_eq!(v.rows, 2);
        assert_eq!(v.issues.len(), 1);
        assert_eq!(v.issues[0].kind, IssueKind::NotAnInteger);
        // Records are numbered from 1 — a JSON array has no meaningful lines.
        assert_eq!(v.issues[0].line, 2);
    }

    /// The NULL-token rule is CSV's answer to a format that can't express null.
    /// Applying it to JSON would turn every empty string into a NULL.
    #[test]
    fn json_ignores_the_csv_null_token_rule() {
        let t = tbl(&[("name", "varchar", false)]);
        let json = r#"[{"name": ""}]"#;
        let m = auto_map(&["name".into()], &t, true);
        let v = validate(
            json.as_bytes(),
            ImportFormat::Json,
            &cfg(true),
            &t,
            &m,
            MySql,
            100,
        )
        .unwrap();
        // An empty string is a value, so a NOT NULL column is satisfied.
        assert!(v.issues.is_empty(), "{:?}", v.issues);
    }

    /// A JSON null in a NOT NULL column is the real error the above must not mask.
    #[test]
    fn a_json_null_in_a_not_null_column_is_reported() {
        let t = tbl(&[("name", "varchar", false)]);
        let json = r#"[{"name": null}]"#;
        let m = auto_map(&["name".into()], &t, true);
        let v = validate(
            json.as_bytes(),
            ImportFormat::Json,
            &cfg(true),
            &t,
            &m,
            MySql,
            100,
        )
        .unwrap();
        assert_eq!(v.issues.len(), 1);
        assert_eq!(v.issues[0].kind, IssueKind::NullInNotNull);
    }

    /// The mapping is built from a *sample*, but JSON columns are the union of
    /// every object's keys — so a key that first appears past the sample widens
    /// every record. Left alone, that reads as a field-count mismatch on all of
    /// them and refuses a file that's perfectly importable.
    #[test]
    fn a_json_key_appearing_past_the_sample_does_not_fail_every_row() {
        let t = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        // The mapping the user approved, from a sample that only saw `id`/`name`.
        let m = auto_map(&["id".into(), "name".into()], &t, true);
        // The third record introduces `note`, which nothing maps to.
        let json = r#"[{"id": 1, "name": "a"}, {"id": 2, "name": "b"},
                       {"id": 3, "name": "c", "note": "late"}]"#;
        let v = validate(
            json.as_bytes(),
            ImportFormat::Json,
            &cfg(true),
            &t,
            &m,
            MySql,
            100,
        )
        .unwrap();
        assert_eq!(v.rows, 3);
        assert!(v.issues.is_empty(), "{:?}", v.issues);

        // And the load agrees with the check — the unmapped key is just dropped.
        let rows: Vec<_> = row_iter(
            json.as_bytes(),
            ImportFormat::Json,
            &cfg(true),
            &t,
            &m,
            MySql,
        )
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2], vec![Value::Int(3), Value::Str("c".into())]);
    }

    /// The CSV half of the rule above: a stray delimiter really does mean the
    /// values may have shifted, so an over-long record stays an issue.
    #[test]
    fn a_csv_record_with_extra_fields_is_still_reported() {
        let t = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        let m = auto_map(&["id".into(), "name".into()], &t, true);
        let csv = "id,name\n1,a\n2,b,stray\n";
        let v = validate(
            csv.as_bytes(),
            ImportFormat::Csv,
            &cfg(true),
            &t,
            &m,
            MySql,
            100,
        )
        .unwrap();
        assert_eq!(
            v.issues.iter().map(|i| i.kind).collect::<Vec<_>>(),
            vec![IssueKind::FieldCount {
                expected: 2,
                found: 3
            }]
        );
    }

    // ── the JSON load's memory cost ──────────────────────────────────────────

    #[test]
    fn only_a_large_json_file_is_warned_about() {
        // CSV streams, so its size is never worth a warning.
        assert_eq!(json_memory_warning(ImportFormat::Csv, 10 << 30), None);
        // A small JSON file isn't either — the warning has to stay rare enough
        // to mean something.
        assert_eq!(json_memory_warning(ImportFormat::Json, 1 << 20), None);
        assert_eq!(
            json_memory_warning(ImportFormat::Json, JSON_WARN_BYTES),
            None
        );
    }

    #[test]
    fn a_large_json_file_says_what_it_will_cost() {
        let msg = json_memory_warning(ImportFormat::Json, 600 * 1024 * 1024)
            .expect("600 MB is past the threshold");
        // The estimate, in a unit a person reads, and why. (600 MB × 5 = 2.9 GiB.)
        assert!(msg.contains("600.0 MB") && msg.contains("2.9 GB"), "{msg}");
        assert!(msg.contains("CSV"), "{msg}");
    }

    #[test]
    fn the_estimate_is_the_measured_multiple() {
        assert_eq!(json_load_estimate(0), 0);
        assert_eq!(json_load_estimate(100), 500);
        // No overflow panic on an absurd size.
        assert_eq!(json_load_estimate(u64::MAX), u64::MAX);
    }

    // ── server-assigned columns are never written ────────────────────────────

    #[test]
    fn a_generated_column_is_never_written_even_when_the_file_has_it() {
        // Export a table with a generated column and import the file back: the
        // mapping matched `full_name` by name, validation reported it clean, and
        // the server rejected the whole transaction on the first batch.
        let mut t = tbl(&[
            ("id", "int", false),
            ("first", "varchar", false),
            ("full_name", "varchar", true),
        ]);
        t.columns[2].generated = Some("concat(first,' ',last)".into());
        let cols = [
            "id".to_string(),
            "first".to_string(),
            "full_name".to_string(),
        ];
        let m = auto_map(&cols, &t, true);
        assert_eq!(
            insert_columns(&m, &t),
            vec![0, 1],
            "a generated column must stay out of the INSERT"
        );
        // …and the mapping the user approves says so, rather than promising a
        // write that then gets filtered out behind their back.
        assert_eq!(m.targets[2], Target::Skip);
    }

    #[test]
    fn an_always_identity_is_skipped_but_a_by_default_one_is_written() {
        // PostgreSQL's two identity forms differ exactly here: ALWAYS rejects an
        // explicit value, BY DEFAULT accepts it — and someone re-importing rows
        // usually wants their keys. MySQL AUTO_INCREMENT and `serial` behave like
        // BY DEFAULT.
        let mut always = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        always.columns[0].auto_increment = true;
        always.columns[0].identity_always = true;
        let cols = ["id".to_string(), "name".to_string()];
        let m = auto_map(&cols, &always, true);
        assert_eq!(insert_columns(&m, &always), vec![1]);

        let mut by_default = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        by_default.columns[0].auto_increment = true;
        let m = auto_map(&cols, &by_default, true);
        assert_eq!(
            insert_columns(&m, &by_default),
            vec![0, 1],
            "an AUTO_INCREMENT / BY DEFAULT key accepts an explicit value"
        );
    }

    #[test]
    fn a_headerless_file_also_skips_a_server_assigned_column() {
        // Without a header the mapping is positional, so the generated column
        // would otherwise take whichever field lands on it.
        let mut t = tbl(&[
            ("id", "int", false),
            ("first", "varchar", false),
            ("full_name", "varchar", true),
        ]);
        t.columns[2].generated = Some("x".into());
        let m = auto_map(&["a".into(), "b".into(), "c".into()], &t, false);
        assert_eq!(insert_columns(&m, &t), vec![0, 1]);
    }

    #[test]
    fn a_deliberately_mapped_generated_column_is_still_not_written() {
        // `insert_columns` is the single authority every path funnels through,
        // so it has to hold even when the mapping says otherwise — the target
        // picker will let a user pick one until B7.2's half lands.
        let mut t = tbl(&[("id", "int", false), ("g", "varchar", true)]);
        t.columns[1].generated = Some("x".into());
        let m = Mapping {
            targets: vec![Target::Column(0), Target::Column(1)],
        };
        assert_eq!(insert_columns(&m, &t), vec![0]);
    }

    #[test]
    fn insert_columns_are_the_mapped_ones_in_table_order() {
        let t = tbl(&[
            ("id", "int", false),
            ("name", "varchar", true),
            ("note", "text", true),
        ]);
        // File order is reversed; the INSERT should still list table order.
        let m = auto_map(&["note".into(), "name".into()], &t, true);
        assert_eq!(insert_columns(&m, &t), vec![1, 2]);
    }

    #[test]
    fn coerce_record_orders_values_to_match_insert_columns() {
        let t = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        let m = auto_map(&["name".into(), "id".into()], &t, true);
        let (vals, issues) = coerce_record(
            &f(&["Smith", "7"]),
            &[],
            &m,
            &t,
            &NullRule::default(),
            MySql,
            2,
        );
        // insert_columns is [0 (id), 1 (name)] — values follow that, not the file.
        assert_eq!(vals, vec![Value::Int(7), Value::Str("Smith".into())]);
        assert!(issues.is_empty());
    }

    #[test]
    fn coerce_record_locates_a_bad_cell_by_line_and_column() {
        let t = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        let m = auto_map(&["id".into(), "name".into()], &t, true);
        let (_, issues) = coerce_record(
            &f(&["N/A", "Smith"]),
            &[],
            &m,
            &t,
            &NullRule::default(),
            MySql,
            42,
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].line, 42);
        assert_eq!(issues[0].column, "id");
        assert_eq!(issues[0].text, "N/A");
        assert_eq!(issues[0].kind, IssueKind::NotAnInteger);
    }

    /// A short record shouldn't panic or silently shift values into the wrong
    /// columns — it's reported, and the missing fields read as empty.
    #[test]
    fn coerce_record_reports_a_field_count_mismatch() {
        let t = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        let m = auto_map(&["id".into(), "name".into()], &t, true);
        let (vals, issues) = coerce_record(&f(&["7"]), &[], &m, &t, &NullRule::default(), MySql, 3);
        assert!(issues.iter().any(|i| i.kind
            == IssueKind::FieldCount {
                expected: 2,
                found: 1
            }));
        assert_eq!(vals, vec![Value::Int(7), Value::Null]);
    }

    #[test]
    fn validate_reports_every_bad_row_with_its_line() {
        let t = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        let csv = "id,name\n1,a\nzz,b\n3,c\nyy,d\n";
        let m = auto_map(&["id".into(), "name".into()], &t, true);
        let v = validate(
            csv.as_bytes(),
            ImportFormat::Csv,
            &cfg(true),
            &t,
            &m,
            MySql,
            100,
        )
        .unwrap();
        assert_eq!(v.rows, 4);
        assert_eq!(v.issues.len(), 2);
        // Line numbers match what a text editor shows: the header is line 1.
        assert_eq!(v.issues[0].line, 3);
        assert_eq!(v.issues[1].line, 5);
        assert!(!v.more_issues);
    }

    /// The two caps are independent, and the tail has to see both. The middle
    /// case is the one that shipped wrong: 57 issues, all of them collected, 20
    /// of them rendered, and nothing said.
    #[test]
    fn the_problem_list_tail_names_whichever_cap_is_in_force() {
        assert_eq!(issue_tail(20, 20, false), None);
        assert_eq!(issue_tail(50, 0, false), None);
        assert_eq!(
            issue_tail(20, 57, false).as_deref(),
            Some("Showing 20 of 57.")
        );
        // Core's cap alone: everything collected is on screen, but the file has
        // more than the check counted.
        let both = issue_tail(200, 200, true).expect("the file holds more");
        assert!(both.starts_with("Showing all 200"), "{both}");
        assert!(both.ends_with("the file holds more."), "{both}");
        // And both at once, which is the case the old wording got worst: it
        // said "…and more." over 20 of 200.
        let both = issue_tail(20, 200, true).expect("both caps");
        assert!(both.starts_with("Showing 20 of the first 200"), "{both}");
        assert!(both.ends_with("the file holds more."), "{both}");
    }

    /// A file that's wrong in a thousand places shouldn't produce a thousand-row
    /// error list — the first screenful is what tells you what's wrong.
    #[test]
    fn validate_caps_the_issue_list_but_says_it_did() {
        let t = tbl(&[("id", "int", false)]);
        let mut csv = String::from("id\n");
        for _ in 0..50 {
            csv.push_str("nope\n");
        }
        let m = auto_map(&["id".into()], &t, true);
        let v = validate(
            csv.as_bytes(),
            ImportFormat::Csv,
            &cfg(true),
            &t,
            &m,
            MySql,
            10,
        )
        .unwrap();
        assert_eq!(v.issues.len(), 10);
        assert!(v.more_issues);
        // Still counts every row, so the summary is honest.
        assert_eq!(v.rows, 50);
    }

    #[test]
    fn validate_of_a_clean_file_finds_nothing() {
        let t = tbl(&[("id", "int", false), ("name", "varchar", true)]);
        let csv = "id,name\n1,a\n2,b\n";
        let m = auto_map(&["id".into(), "name".into()], &t, true);
        let v = validate(
            csv.as_bytes(),
            ImportFormat::Csv,
            &cfg(true),
            &t,
            &m,
            MySql,
            100,
        )
        .unwrap();
        assert_eq!(v.rows, 2);
        assert!(v.issues.is_empty());
    }

    #[test]
    fn a_file_with_no_mapped_columns_is_an_error_not_an_empty_insert() {
        let t = tbl(&[("id", "int", false)]);
        let csv = "other\n1\n";
        let m = auto_map(&["other".into()], &t, true);
        assert!(matches!(
            validate(
                csv.as_bytes(),
                ImportFormat::Csv,
                &cfg(true),
                &t,
                &m,
                MySql,
                100
            ),
            Err(ImportError::NoColumnsMapped)
        ));
    }
}
