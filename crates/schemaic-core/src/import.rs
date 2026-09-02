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
    pub fn has_own_nulls(self) -> bool {
        !matches!(self, ImportFormat::Csv)
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
        ColKind::Exact => match t.parse::<f64>() {
            Ok(_) => Ok(Value::Str(t.to_string())),
            Err(_) => Err(IssueKind::NotANumber),
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
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Read(e) => write!(f, "Couldn't read the file: {e}"),
            ImportError::NoColumnsMapped => {
                write!(f, "No file columns are mapped to table columns")
            }
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
    mut on_record: impl FnMut(Vec<Field>, u64) -> bool,
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
                if !on_record(fields, line) {
                    break;
                }
            }
        }
        ImportFormat::Json => {
            let mut keys: Vec<String> = Vec::new();
            json_records(r, &mut keys, usize::MAX, on_record)?;
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
}

impl<R: std::io::Read> ArrayUnwrap<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            array: None,
            depth: 0,
            in_string: false,
            escaped: false,
        }
    }

    fn rewrite(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            // Decide what kind of file this is on the first byte that isn't
            // whitespace, then never revisit it.
            if self.array.is_none() {
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
fn json_records<R: std::io::Read>(
    r: R,
    keys: &mut Vec<String>,
    limit: usize,
    mut on_record: impl FnMut(Vec<Field>, u64) -> bool,
) -> Result<bool, ImportError> {
    // Collected first so every record can be emitted against the *final* key set
    // — otherwise the first row's fields wouldn't line up with a column list that
    // grew later.
    let mut objects: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
    let mut more = false;

    let stream =
        serde_json::Deserializer::from_reader(ArrayUnwrap::new(r)).into_iter::<serde_json::Value>();
    for v in stream {
        // Past the limit nothing more needs reading — and for a large file that's
        // the whole point, so stop before deserializing another record.
        if objects.len() >= limit {
            more = true;
            break;
        }
        let v = v.map_err(|e| ImportError::Read(e.to_string()))?;
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
    let more = xlsx_rows(&mut wb, &sheets, cfg, &mut columns, limit, |fields, _| {
        rows.push(fields);
        true
    })?;
    Ok((
        Sample {
            columns,
            rows,
            more,
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
    use calamine::Reader;
    let mut bytes = Vec::new();
    // One byte past the ceiling, so "exactly at it" and "over it" are
    // distinguishable without reading the rest.
    let mut r = std::io::Read::take(r, XLSX_MAX_BYTES + 1);
    std::io::Read::read_to_end(&mut r, &mut bytes).map_err(|e| ImportError::Read(e.to_string()))?;
    if bytes.len() as u64 > XLSX_MAX_BYTES {
        return Err(ImportError::Read(oversize_workbook(None)));
    }
    calamine::Xlsx::new(std::io::Cursor::new(bytes)).map_err(|e| ImportError::Read(e.to_string()))
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
    on_record: impl FnMut(Vec<Field>, u64) -> bool,
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
    mut on_record: impl FnMut(Vec<Field>, u64) -> bool,
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
    let width = sheet_width(dims)?;
    // The used range's own left edge. A sheet with a title block starts partway
    // across, and its first data column is the row's column 0 — the same
    // correction `range.start()` used to make, and the reason a cell is placed
    // relative to this rather than at its absolute column.
    let left = dims.start.1;

    // Records emitted so far. Not the row's index: a skipped blank row advances
    // one and not the other, which is the whole reason the two are separate.
    let mut seen = 0usize;
    let mut took_header = !cfg.dialect.has_header;
    if !cfg.dialect.has_header {
        columns.extend((1..=width).map(|i| format!("Column {i}")));
    }
    // One row at a time. Cells arrive in sheet order and an empty cell is not
    // emitted at all, so a row is complete when a cell for a later row shows up
    // — and a *wholly* empty row never appears, which is exactly the rule the
    // doc above states, arrived at for free instead of by a filter.
    let mut row: Vec<Option<String>> = vec![None; width];
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
            let full = std::mem::replace(&mut row, vec![None; width]);
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
                if !on_record(full, line) {
                    return Ok(false);
                }
            }
        }
        let Some(c) = cell else { break };
        let (r, col) = c.get_position();
        at = Some(r);
        let Some(text) = cell_text(&c.get_value().clone().into()) else {
            continue;
        };
        let text = if cfg.trim {
            text.trim().to_string()
        } else {
            text
        };
        // A cell outside the declared range is dropped rather than widening or
        // shifting the row: every record has to carry the same field count, or
        // the mismatch report becomes noise on every row after the widest one.
        if let Some(slot) = col.checked_sub(left).and_then(|i| row.get_mut(i as usize)) {
            *slot = Some(text);
        }
    }
    // A sheet with no cells at all: no header to take, and no rows to emit.
    Ok(false)
}

/// How wide a row of this sheet is, from the extent the sheet **declares**.
///
/// **The width is decided before a cell is read, and it is the only unbounded
/// thing in an Excel import.** The rows are streamed, so a sheet's height costs
/// nothing to skip past; a row buffer is the one allocation whose size the file
/// controls, and Excel's own ceiling is 16,384 columns. A workbook claiming more
/// is refused here rather than believed.
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
    // which is also what a one-cell sheet reads as — either way one column is
    // the right answer, and the header row is what names them.
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
///   evaluate, and passing it on surfaces as a coercion [`Issue`] naming the
///   row, where a silent null would not.
/// Is this field one of Excel's own formula-error spellings?
///
/// The closed set the format defines, matched exactly — `#N/A` and its siblings
/// are not values a sheet can otherwise produce, and `cell_text` writes them
/// with `Display`, which is Excel's spelling rather than calamine's variant
/// name. Case-sensitive and whole-field, so a `VARCHAR` holding the sentence
/// "check the #REF! column" is text, which it is.
fn is_worksheet_error(text: &str) -> bool {
    matches!(
        text,
        "#DIV/0!"
            | "#N/A"
            | "#NAME?"
            | "#NULL!"
            | "#NUM!"
            | "#REF!"
            | "#VALUE!"
            | "#GETTING_DATA"
            | "#SPILL!"
            | "#CALC!"
    )
}

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
    let more = json_records(r, &mut columns, limit, |fields, _| {
        rows.push(fields);
        true
    })?;
    Ok(Sample {
        columns,
        rows,
        more,
    })
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
/// strings of the rows actually read — not the sheet. 4× is that measurement
/// with headroom for a workbook shaped unlike either, and it is still an
/// over-estimate rather than an under-one, which is the direction a warning has
/// to be wrong in.
pub const XLSX_MEMORY_FACTOR: u64 = 4;

/// Past this file size, [`xlsx_memory_warning`] speaks up.
///
/// Higher than it was (40 MB), because the thing it was warning about is gone:
/// at 25× a 40 MB workbook was estimated at 1 GB and measured 1,085 MB, and it
/// now costs 64 MB. Set where [`JSON_WARN_BYTES`] is set — the point at which
/// the estimate stops being something a machine absorbs without noticing — and
/// with the same factor, so the two warnings again fire at the same estimated
/// footprint.
pub const XLSX_WARN_BYTES: u64 = 200 * 1024 * 1024;

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
/// `format` is here for one reason: a worksheet's formula errors
/// ([`IssueKind::CellError`]) are wrong for every column type, and only a
/// worksheet has them.
pub fn coerce_record(
    fields: &[Field],
    mapping: &Mapping,
    table: &TableInfo,
    nulls: &NullRule,
    dialect: SqlDialect,
    format: ImportFormat,
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
            let field = match field_of[ci].and_then(|fi| fields.get(fi)) {
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
            // dispatch to catch it with. Format-gated, because only a worksheet
            // has formula errors: `#N/A` typed into a CSV is text.
            if format == ImportFormat::Xlsx && is_worksheet_error(field) {
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
    for_each_record(r, format, cfg, |mut fields, line| {
        out.rows += 1;
        // See `trim_to_mapping` — the check must see exactly what the import will.
        fields.truncate(trim_to_mapping(&fields, format, mapping));
        let (_, issues) = coerce_record(&fields, mapping, table, &nulls, dialect, format, line);
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
        // Excel sides with CSV, not JSON: a worksheet's columns are fixed by its
        // used range, so every row is already the same width and a count
        // mismatch is a real one worth reporting — there is no key union here
        // that could widen a later row.
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
    fn row(&self, fields: &[Field], line: u64) -> Result<Vec<Value>, String> {
        let fields = &fields[..trim_to_mapping(fields, self.format, &self.mapping)];
        let (values, issues) = coerce_record(
            fields,
            &self.mapping,
            &self.table,
            &self.nulls,
            self.dialect,
            self.format,
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
    Buffered(std::vec::IntoIter<(Vec<Field>, u64)>),
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
            json_records(r, &mut keys, usize::MAX, |fields, n| {
                rows.push((fields, n));
                true
            })?;
            RowSourceIter::Buffered(rows.into_iter())
        }
        ImportFormat::Xlsx => {
            let mut names = Vec::new();
            let mut rows = Vec::new();
            xlsx_records(r, cfg, &mut names, usize::MAX, |fields, n| {
                rows.push((fields, n));
                true
            })?;
            RowSourceIter::Buffered(rows.into_iter())
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
                        self.ctx.row(&fields, line)
                    }
                    Err(e) => Err(e.to_string()),
                })
            }
            RowSourceIter::Buffered(rows) => {
                let (fields, line) = rows.next()?;
                Some(self.ctx.row(&fields, line))
            }
        }
    }
}

/// Rows per `INSERT`. Bulk import is one transaction of batched statements
/// rather than the grid write-back's statement-per-row: at 100k rows that's 100k
/// server round-trips, which is minutes on a remote host.
pub const INSERT_BATCH_ROWS: usize = 500;

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

        // The seam, both ends: what the reader writes for an error cell is what
        // the coercion recognises. `rust_xlsxwriter` cannot author an error cell,
        // so the fixture is the reader's own output rather than a workbook — and
        // joining the two here is the point, since each half looked right alone.
        for e in [CellErrorType::NA, CellErrorType::Div0, CellErrorType::Ref] {
            let text = cell_text(&Data::Error(e)).expect("an error cell is not a null");
            assert!(is_worksheet_error(&text), "{text}");
        }
        let na = cell_text(&Data::Error(CellErrorType::NA)).unwrap();

        let (_, issues) = coerce_record(
            &f(&["2", &na]),
            &mapping,
            &table,
            &NullRule::default(),
            MySql,
            ImportFormat::Xlsx,
            3,
        );
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].kind, IssueKind::CellError);
        assert_eq!(issues[0].column, "note");
        assert_eq!(issues[0].line, 3);

        // …and the same text in a CSV is text: only a worksheet has formula
        // errors, and this must not start refusing files that never had one.
        let (_, issues) = coerce_record(
            &f(&["2", &na]),
            &mapping,
            &table,
            &NullRule::default(),
            MySql,
            ImportFormat::Csv,
            3,
        );
        assert!(issues.is_empty(), "{issues:?}");

        // A sentence that merely mentions one is text on every format.
        let (_, issues) = coerce_record(
            &f(&["2", "check the #REF! column"]),
            &mapping,
            &table,
            &NullRule::default(),
            MySql,
            ImportFormat::Xlsx,
            3,
        );
        assert!(issues.is_empty(), "{issues:?}");
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
            &m,
            &t,
            &NullRule::default(),
            MySql,
            ImportFormat::Csv,
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
            &m,
            &t,
            &NullRule::default(),
            MySql,
            ImportFormat::Csv,
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
        let (vals, issues) = coerce_record(
            &f(&["7"]),
            &m,
            &t,
            &NullRule::default(),
            MySql,
            ImportFormat::Csv,
            3,
        );
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
