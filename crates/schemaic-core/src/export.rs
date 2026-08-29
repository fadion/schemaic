//! Result-set export — pure over [`ResultSet`] + a display order, no UI.
//!
//! `order` is the display→data-row permutation (post-sort); callers pass the
//! grid's live order so exports match what's on screen.
//!
//! [`ExportFormat`] is the single value the grid's two menus dispatch on — Copy
//! (to the clipboard) and Download (to a file) — so the label, extension,
//! suggested file name and rendering can't drift between them.
//!
//! Every renderer comes in two shapes. The `*_to` functions write into any
//! [`std::io::Write`], which is what the file export uses: a 200k-row result
//! rendered into a `String` first is a second full copy of the data — hundreds of
//! megabytes on a wide result — held only to hand it to `fs::write`. Streaming it
//! into a `BufWriter` keeps that cost to the buffer. The `String`-returning
//! versions are thin wrappers over them, kept for the clipboard, which has no
//! streaming API to target. Both share one implementation, so the two paths can't
//! drift — a test asserts they agree byte-for-byte in every format.

use std::io::{self, Write};

use crate::intel::SqlDialect;
use crate::model::{ResultSet, Value};

/// Run a `*_to` renderer into a `String`. Writing into a `Vec<u8>` can't fail and
/// every renderer emits `&str`, so both the io error and the UTF-8 check are
/// unreachable — `unwrap_or_default` keeps that from becoming a panic path.
fn to_string(f: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> String {
    let mut buf = Vec::new();
    match f(&mut buf) {
        Ok(()) => String::from_utf8(buf).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// The formats the results grid can export a result set to — one value driving
/// the menu label, the file extension, the suggested file name, and the rendering,
/// so "copy to clipboard" and "save to file" can't drift apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Csv,
    /// `INSERT` statements, in the connection's dialect.
    Sql,
    Markdown,
    Html,
}

impl ExportFormat {
    /// Every format, in the order the grid's menus list them.
    pub const ALL: [ExportFormat; 5] = [
        ExportFormat::Json,
        ExportFormat::Csv,
        ExportFormat::Sql,
        ExportFormat::Markdown,
        ExportFormat::Html,
    ];

    /// The menu label.
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Json => "JSON",
            ExportFormat::Csv => "CSV",
            ExportFormat::Sql => "SQL",
            ExportFormat::Markdown => "Markdown",
            ExportFormat::Html => "HTML",
        }
    }

    /// The file extension, without the leading dot.
    pub fn extension(self) -> &'static str {
        self.extensions()[0]
    }

    /// The extensions as a `'static` slice, for a file dialog's type filter.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            ExportFormat::Json => &["json"],
            ExportFormat::Csv => &["csv"],
            ExportFormat::Sql => &["sql"],
            ExportFormat::Markdown => &["md", "markdown"],
            ExportFormat::Html => &["html", "htm"],
        }
    }

    /// Render `rs` (in display `order`) in this format. `source` is the result's
    /// real `(database, namespace, table)` when known — only [`ExportFormat::Sql`]
    /// uses it, to name the `INSERT` target.
    pub fn render(
        self,
        rs: &ResultSet,
        order: &[usize],
        source: Option<(&str, Option<&str>, &str)>,
        dialect: SqlDialect,
    ) -> String {
        to_string(|w| self.render_to(w, rs, order, source, dialect).map(|_| ()))
    }

    /// Stream the same rendering into `w` — the file export's path, so a large
    /// result never exists twice in memory. Identical output to [`Self::render`],
    /// returning what was written and what could not be carried
    /// ([`ExportTally`]).
    ///
    /// Errors are the writer's own (a full disk, a revoked permission). They must
    /// reach the user: unlike the buffered path, which either produced the whole
    /// text or nothing, a failure here leaves a **truncated file** that looks
    /// complete.
    pub fn render_to<W: Write>(
        self,
        w: &mut W,
        rs: &ResultSet,
        order: &[usize],
        source: Option<(&str, Option<&str>, &str)>,
        dialect: SqlDialect,
    ) -> io::Result<ExportTally> {
        self.stream_to(w, &mut OneChunk::new(rs, order), source, dialect)
    }

    /// Stream this rendering from a [`RowChunks`] source, returning what it wrote
    /// — what an export of a whole table uses, where the rows arrive from the
    /// server in blocks and must reach the file as they come rather than be
    /// gathered first.
    ///
    /// [`Self::render_to`] is this over a source of one chunk, so the two cannot
    /// drift: there is one renderer per format, not a buffered one and a streamed
    /// one that have to be kept in agreement.
    pub fn stream_to<W: Write>(
        self,
        w: &mut W,
        src: &mut dyn RowChunks,
        source: Option<(&str, Option<&str>, &str)>,
        dialect: SqlDialect,
    ) -> io::Result<ExportTally> {
        match self {
            ExportFormat::Json => export_json_chunks(w, src),
            ExportFormat::Csv => export_csv_chunks(w, src),
            ExportFormat::Sql => export_inserts_chunks(w, src, source, dialect),
            ExportFormat::Markdown => export_markdown_chunks(w, src),
            ExportFormat::Html => export_html_chunks(w, src),
        }
    }
}

/// What an export wrote, and what it **could not carry**.
///
/// The row count was the whole of an export's result until it turned out that
/// two kinds of loss are invisible in the file itself:
///
/// - **Withheld bytes.** A raw-bytes cell has no `Value` variant to hold it, so
///   it arrives as [`crate::model::binary_display`]'s `<n bytes>` placeholder,
///   and the formats Schemaic reads back must not write that text as data (see
///   [`dropped_binary_columns`]). The SQL emitter says so in a `-- NOTE:` line;
///   CSV and JSON have no comment syntax, so an empty field was the only trace
///   — and an empty field is what a NULL looks like. Those two formats are
///   exactly `import::ImportFormat`, so the export a user reaches for as a
///   portable copy was the one that silently dropped a column.
/// - **Blanked cells.** A column's text arena stops at 512 MiB and does not
///   fail: every cell past the ceiling reads back as the empty string
///   ([`ResultSet::capped_columns`]). The grid
///   surfaces that with its own note, but a streamed chunk is never mounted in a
///   grid, so a whole-table export of a wide-text column could write a file with
///   holes in it and report a full row count. `RowChunks`' own doc names this
///   hazard as the reason the trait exists; nothing read the flag.
///
/// Both are *columns*, named, because that is what a user needs to know which
/// part of the file to distrust. Neither is an error: the file is written and
/// the rows in it are real. What must not happen is the caveat going unsaid,
/// which is [`export_note`]'s job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportTally {
    /// Rows written to the file.
    pub rows: u64,
    /// Columns whose bytes could not be carried, in column order. Empty for
    /// Markdown and HTML, which keep the placeholder deliberately — nothing
    /// reads those back, and there the placeholder is the useful rendering.
    pub withheld: Vec<String>,
    /// Columns whose cells past the arena ceiling were written **blank**.
    pub blanked: Vec<String>,
}

impl ExportTally {
    /// Fold one chunk's losses in, keeping column order and never repeating a
    /// name: a streamed export sees the same column in every chunk, and a
    /// caveat that named `body` two hundred times would say less than one that
    /// names it once.
    fn note(&mut self, rs: &ResultSet, withheld: &[usize]) {
        for &ci in withheld {
            if let Some(c) = rs.columns.get(ci)
                && !self.withheld.contains(&c.name)
            {
                self.withheld.push(c.name.clone());
            }
        }
        for &ci in &rs.capped_columns {
            if let Some(c) = rs.columns.get(ci)
                && !self.blanked.contains(&c.name)
            {
                self.blanked.push(c.name.clone());
            }
        }
    }

    /// Did anything about this export need saying beyond the row count?
    pub fn has_caveat(&self) -> bool {
        !self.withheld.is_empty() || !self.blanked.is_empty()
    }

    /// Fold **another table's** tally into this one — a dump writes many tables
    /// into one file and reports one sentence about it.
    ///
    /// Rows sum; a column name is kept **once**, in first-seen order. Two tables
    /// that both have a `body` column too wide to carry say `body` once, and two
    /// tables with different withheld columns keep both — which is the whole
    /// content of a caveat that has to name what to distrust.
    ///
    /// It lives here rather than in the dump's writer loop because it is the same
    /// question [`ExportTally::note`] answers one level down, and a fold written
    /// out at the call site is one nothing can test: the caller needs a `Db`, a
    /// runtime handle and two channels to reach.
    pub fn absorb(&mut self, other: ExportTally) {
        self.rows += other.rows;
        for c in other.withheld {
            if !self.withheld.contains(&c) {
                self.withheld.push(c);
            }
        }
        for c in other.blanked {
            if !self.blanked.contains(&c) {
                self.blanked.push(c);
            }
        }
    }
}

/// One block of rows on its way out — a [`ResultSet`] and the rows to take from
/// it, as display indices into it.
///
/// Every chunk of one export must carry the **same columns**: the header is
/// written from the first chunk and never revisited, so a source that changed
/// them mid-stream would produce a file whose header describes only its opening
/// rows.
pub struct RowChunk<'a> {
    pub rs: &'a ResultSet,
    pub order: &'a [usize],
}

/// A pull source of rows for an export.
///
/// **The reason exports take one of these rather than a `&ResultSet`.** A result
/// the grid is showing is bounded by the row cap and already in memory, so
/// handing it over whole costs nothing; a *table* export is neither. Materialising
/// 2 million rows to render them would hold the table twice over and run into the
/// 512 MiB per-column arena ceiling, which does not fail — it blanks the cells
/// past it ([`ResultSet::capped_columns`]) — so the export would quietly write a
/// file with holes in it. Pulling chunks bounds the memory at one chunk and lets
/// the rows go to disk as they come off the wire.
///
/// **Pull and not push**, because JSON decides it. Its renderer holds a
/// `serde_json` sequence serializer open across the whole array, borrowing the
/// writer; a push API would have to store that across calls and it is not a type
/// that can be stored. Inverting it keeps the serializer a local in one function,
/// which is also what keeps the five formats to one implementation each — the
/// `export_*_to` entry points are this same code over a source of exactly one
/// chunk ([`OneChunk`]), not a second copy of it.
///
/// The lifetime on the returned chunk ties it to the borrow of `self`, so a
/// source may hand out a view of a buffer it reuses: only one chunk is alive at
/// a time, by construction.
pub trait RowChunks {
    /// The next block of rows, or `None` when the source is exhausted.
    ///
    /// An error aborts the export. It is an [`io::Error`] because that is what
    /// the writer already returns and what the caller must report either way — a
    /// source that fails for its own reasons (a dropped connection, a cancelled
    /// export) wraps that reason in one.
    fn next_chunk(&mut self) -> io::Result<Option<RowChunk<'_>>>;
}

/// A [`RowChunks`] over one already-materialised result — what the grid's export
/// uses.
///
/// It yields exactly one chunk, **even when the result has no rows**, which is
/// load-bearing rather than an edge case: CSV, Markdown and HTML take their
/// header from the first chunk, so a source that yielded nothing for an empty
/// result would write an empty file where the old code wrote a header.
pub struct OneChunk<'a> {
    rs: &'a ResultSet,
    order: &'a [usize],
    done: bool,
}

impl<'a> OneChunk<'a> {
    pub fn new(rs: &'a ResultSet, order: &'a [usize]) -> Self {
        OneChunk {
            rs,
            order,
            done: false,
        }
    }
}

impl RowChunks for OneChunk<'_> {
    fn next_chunk(&mut self) -> io::Result<Option<RowChunk<'_>>> {
        if self.done {
            return Ok(None);
        }
        self.done = true;
        Ok(Some(RowChunk {
            rs: self.rs,
            order: self.order,
        }))
    }
}

/// A [`RowChunks`] over whole result sets pulled from `next` — every row of each,
/// in the order the server sent them.
///
/// The adapter between a loader that produces results and an export that consumes
/// rows. `next` blocks: on the export's own thread that is the point, since the
/// alternative is a buffer that grows to whatever the disk is behind by.
///
/// The natural order is materialised as a `Vec<usize>` rather than threaded
/// through the renderers as an `Option`, and the buffer is **reused** across
/// chunks — a chunk is thousands of rows, so this is one allocation for the whole
/// export, against five renderers that would each need a second code path.
pub struct PullChunks<F> {
    next: F,
    cur: Option<ResultSet>,
    order: Vec<usize>,
}

impl<F> PullChunks<F>
where
    F: FnMut() -> io::Result<Option<ResultSet>>,
{
    pub fn new(next: F) -> Self {
        PullChunks {
            next,
            cur: None,
            order: Vec::new(),
        }
    }
}

impl<F> RowChunks for PullChunks<F>
where
    F: FnMut() -> io::Result<Option<ResultSet>>,
{
    fn next_chunk(&mut self) -> io::Result<Option<RowChunk<'_>>> {
        let Some(rs) = (self.next)()? else {
            return Ok(None);
        };
        let n = rs.row_count();
        self.order.clear();
        self.order.extend(0..n);
        self.cur = Some(rs);
        Ok(Some(RowChunk {
            // `cur` was just assigned, so the borrow is of a live value.
            rs: self.cur.as_ref().expect("just assigned"),
            order: &self.order,
        }))
    }
}

/// A default file name for saving a result: the source table's display name when
/// the tab has one, else `result` — plus the format's extension.
///
/// `base` is **sanitized**, not trusted: a table name is server-controlled and may
/// hold characters no filesystem accepts (`/`, `:`, `*`, …), so those become `_`.
/// Windows also rejects a trailing dot or space and reserves a handful of device
/// names, so the stem is trimmed and a reserved stem is prefixed. A base that
/// sanitizes away to nothing falls back to `result`.
pub fn suggested_filename(base: Option<&str>, format: ExportFormat) -> String {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let mut stem: String = base
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Windows won't accept a name ending in a dot or space.
    stem = stem.trim_end_matches(['.', ' ']).trim_start().to_string();
    // Keep the whole name comfortably inside the usual 255-byte component limit.
    if stem.chars().count() > 100 {
        stem = stem.chars().take(100).collect();
    }
    if stem.is_empty() {
        stem = "result".to_string();
    }
    if RESERVED.contains(&stem.to_ascii_uppercase().as_str()) {
        stem = format!("_{stem}");
    }
    format!("{stem}.{}", format.extension())
}

/// A cell as a JSON value (non-finite floats → null).
pub fn value_to_json(v: &Value) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        Value::Null => J::Null,
        Value::Int(i) => J::from(*i),
        Value::UInt(u) => J::from(*u),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(J::Number)
            .unwrap_or(J::Null),
        Value::Str(s) => J::String(s.clone()),
    }
}

/// Quote a CSV field if it contains a delimiter/quote/newline, and neutralize
/// spreadsheet formula/DDE injection (§7.5): a value a spreadsheet would evaluate
/// as a formula — leading `=`, `+`, `@`, `-`, or a `\t`/`\r` control char — is
/// prefixed with a single quote so Excel/Sheets import it as text (a cell
/// `=HYPERLINK(...)` otherwise executes on open).
///
/// **Leading `-` is guarded only when the value isn't a number.** It was once
/// skipped entirely, on the grounds that prefixing it would corrupt every
/// negative value — but that dichotomy isn't forced. `-1+1+cmd|' /C calc'!A0` is
/// a DDE payload and `-5.25` is a number, and [`is_negative_number`] tells them
/// apart, so both cases can be served.
/// Is `s` a plain negative number — the one leading-`-` shape a spreadsheet
/// should be allowed to evaluate?
///
/// Deliberately strict: a decimal or scientific-notation literal and nothing
/// else. Anything a formula could hide in — an operator, a cell reference, a
/// `|` DDE separator — fails, and a false negative only costs a leading
/// apostrophe on a value that wasn't a number anyway.
fn is_negative_number(s: &str) -> bool {
    // A lone `-` never reaches here (the caller skips it — there is nothing after
    // the sign for a formula to hide in), so `rest` is non-empty in practice.
    let rest = &s[1..];
    if rest.is_empty() {
        return false;
    }
    // At most one exponent, split on it; each part must look numeric.
    let (mantissa, exponent) = match rest.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (rest, None),
    };
    let mantissa_ok = !mantissa.is_empty()
        && mantissa.bytes().filter(|b| *b == b'.').count() <= 1
        && mantissa.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && mantissa.bytes().any(|b| b.is_ascii_digit());
    let exponent_ok = match exponent {
        None => true,
        Some(e) => {
            let digits = e.strip_prefix(['+', '-']).unwrap_or(e);
            !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
        }
    };
    mantissa_ok && exponent_ok
}

pub fn csv_field(s: &str) -> String {
    let guarded;
    let s = if matches!(
        s.as_bytes().first(),
        Some(b'=' | b'+' | b'@' | b'\t' | b'\r')
    ) || (s.len() > 1 && s.starts_with('-') && !is_negative_number(s))
    {
        guarded = format!("'{s}");
        guarded.as_str()
    } else {
        s
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// A cell as a SQL literal (non-finite float → NULL; strings escaped).
///
/// **Backslashes are dialect-critical.** MySQL treats `\` as an escape character
/// inside a string literal, so it must be doubled. PostgreSQL, under its default
/// `standard_conforming_strings = on` (since 9.1), takes a backslash literally —
/// doubling it there would silently *corrupt* the value (`C:\tmp` → `C:\\tmp`).
/// Doubling the single quote is the injection guard on both.
pub fn sql_literal(v: &Value, dialect: SqlDialect) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) if !f.is_finite() => "NULL".to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => {
            let escaped = match dialect {
                SqlDialect::MySql => s.replace('\\', "\\\\").replace('\'', "''"),
                // SQLite is standard-conforming like Postgres and has no
                // backslash escape at all, so doubling one would corrupt the
                // value exactly as it would there.
                SqlDialect::Postgres | SqlDialect::Sqlite => s.replace('\'', "''"),
            };
            format!("'{escaped}'")
        }
    }
}

/// Quote a SQL identifier in the connection's dialect, doubling the embedded
/// quote character: MySQL `` `name` ``, PostgreSQL and SQLite `"name"`. The
/// *other* dialect's quote char is an ordinary character and passes through
/// untouched.
///
/// SQLite accepts `"x"`, `` `x` `` and `[x]` when *reading* (the lexer knows all
/// three), but **emits only the standard form**: `"` is the one with a defined
/// escape, since a `]` cannot be written inside brackets at all, so a name
/// containing one would be unquotable in the form we chose to generate.
pub fn ident_sql(name: &str, dialect: SqlDialect) -> String {
    match dialect {
        SqlDialect::MySql => format!("`{}`", name.replace('`', "``")),
        SqlDialect::Postgres | SqlDialect::Sqlite => {
            format!("\"{}\"", name.replace('"', "\"\""))
        }
    }
}

/// The same, but only when leaving the name bare would name something else.
///
/// A bare identifier is safe exactly when it is a plain lower-case ASCII word and
/// not reserved: PostgreSQL folds an unquoted name to lower case, so anything with
/// an upper-case letter, a space, punctuation or a non-ASCII byte has to be quoted
/// there, and a reserved word has to be quoted on either engine.
///
/// This is the rule for SQL Schemaic *generates for the user to read* — the
/// completion layer's auto-join and star expansion, and `filter::table_query`'s
/// `ORDER BY`. Quoting unconditionally would be simpler, but it would put
/// backticks around every ordinary MySQL name in text the user is about to edit.
/// For SQL that is only executed, [`ident_sql`] and its unconditional quoting is
/// the right choice and stays what the write paths use.
pub fn ident_if_needed(name: &str, dialect: SqlDialect) -> String {
    let plain = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !name.as_bytes()[0].is_ascii_digit()
        // The identifier question, not the alias one: `CAST`/`IF`/`RAISE` are
        // valid SQLite aliases but cannot be a bare column or table name.
        && !crate::intel::must_quote_ident(name, dialect);
    if plain {
        name.to_string()
    } else {
        ident_sql(name, dialect)
    }
}

/// The whole result as a pretty JSON array of row objects (keyed by column name).
/// Duplicate column names (e.g. `a.id, b.id` from a join) are suffixed `_2`,
/// `_3`, … so a JSON object doesn't silently drop all but the last (§7.4).
pub fn export_json(rs: &ResultSet, order: &[usize]) -> String {
    to_string(|w| export_json_to(w, rs, order))
}

/// One row as a JSON object, with the keys in **column order**.
///
/// Not a `serde_json::Map`: that's a `BTreeMap` (the `preserve_order` feature
/// isn't on), so building one sorts the keys alphabetically and a `SELECT id,
/// name` exported as `{"name": …, "id": …}` — the column order the user chose,
/// silently discarded. Emitting the entries directly keeps it.
struct RowObject<'a> {
    rs: &'a ResultSet,
    keys: &'a [String],
    /// Which columns' text stands in for bytes this result never carried —
    /// [`binary_mask`]. Computed once for the whole export, not per row.
    mask: &'a [bool],
    di: usize,
}

impl serde::Serialize for RowObject<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(Some(self.keys.len()))?;
        for (ci, key) in self.keys.iter().enumerate() {
            let v = self
                .rs
                .cell(self.di, ci)
                // JSON is a format Schemaic reads back, so a blob's placeholder
                // becomes `null` rather than the string — see
                // [`dropped_binary_columns`].
                .filter(|c| !withheld_binary(self.mask, ci, c))
                .map(|c| value_to_json(&c.to_value()))
                .unwrap_or(serde_json::Value::Null);
            m.serialize_entry(key, &v)?;
        }
        m.end()
    }
}

/// [`export_json`], streamed.
///
/// This is the one format that genuinely had to buffer: it built the entire array
/// as a `serde_json::Value` before `to_string_pretty` could see it, so a large
/// export held the rows a third time (result set, `Value` tree, output string).
/// Serializing the array element-by-element through a `serde_json::Serializer`
/// emits the same pretty output while only ever holding one row.
pub fn export_json_to<W: Write>(w: &mut W, rs: &ResultSet, order: &[usize]) -> io::Result<()> {
    export_json_chunks(w, &mut OneChunk::new(rs, order)).map(|_| ())
}

/// [`export_json_to`] over a [`RowChunks`] source — the form the whole-table
/// export uses, and the one the single-result form above is a special case of.
/// Returns the number of rows written.
///
/// The `serialize_seq` handle stays open across every chunk, so the array is one
/// array however many chunks it took: the keys are derived from the first chunk's
/// columns and reused, since a source that changed them mid-stream would already
/// have broken every other format's header.
pub fn export_json_chunks<W: Write>(w: &mut W, src: &mut dyn RowChunks) -> io::Result<ExportTally> {
    use serde::ser::{SerializeSeq, Serializer as _};

    let mut ser = serde_json::Serializer::pretty(w);
    let mut seq = ser.serialize_seq(None).map_err(io::Error::other)?;
    let mut keys: Vec<String> = Vec::new();
    let mut first = true;
    let mut tally = ExportTally::default();
    loop {
        // **A source error closes the array before it propagates.** Every other
        // format leaves a usable prefix when a stream is cancelled or the
        // connection drops — CSV, Markdown and SQL are line-based and a browser
        // closes an HTML table itself — while JSON's `]` is written once, after
        // the loop, so a `?` inside it left `…},` on disk and every parser
        // rejects the 90% of rows that did arrive. "Incomplete" reads very
        // differently for a file with most of the data in it than for one with
        // none.
        //
        // The source's error, not the writer's: a writer that has failed cannot
        // be asked to write a bracket, and `seq.end()`'s own failure is dropped
        // for the same reason.
        let chunk = match src.next_chunk() {
            Ok(chunk) => chunk,
            Err(e) => {
                let _ = seq.end();
                return Err(e);
            }
        };
        let Some(c) = chunk else { break };
        if first {
            keys = unique_column_keys(c.rs);
            first = false;
        }
        // Per chunk, not once: whether a binary column's text is a placeholder
        // this codebase generated is a fact about the *rows in hand*, and a
        // streamed export never has them all at once. A column carrying real
        // bytes in one chunk and a placeholder in the next is withheld only
        // where it is a placeholder, which is the same rule the one-shot path
        // applies to the only chunk it has.
        let dropped = dropped_binary_columns(c.rs, c.order);
        let mask = binary_mask(c.rs, &dropped);
        tally.note(c.rs, &dropped);
        for &di in c.order.iter().filter(|&&di| di < c.rs.row_count()) {
            seq.serialize_element(&RowObject {
                rs: c.rs,
                keys: &keys,
                mask: &mask,
                di,
            })
            .map_err(io::Error::other)?;
            tally.rows += 1;
        }
    }
    seq.end().map_err(io::Error::other)?;
    Ok(tally)
}

/// Column names made unique for use as JSON object keys: a repeated name gets a
/// `_2`/`_3`/… suffix (first occurrence keeps the bare name).
fn unique_column_keys(rs: &ResultSet) -> Vec<String> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    rs.columns
        .iter()
        .map(|c| {
            let n = seen.entry(c.name.clone()).or_insert(0);
            *n += 1;
            if *n == 1 {
                c.name.clone()
            } else {
                format!("{}_{}", c.name, n)
            }
        })
        .collect()
}

/// One column's values as a JSON array (for building arrays out of a column).
pub fn export_column_json(rs: &ResultSet, order: &[usize], ci: usize) -> String {
    to_string(|w| export_column_json_to(w, rs, order, ci))
}

/// [`export_column_json`], streamed.
pub fn export_column_json_to<W: Write>(
    w: &mut W,
    rs: &ResultSet,
    order: &[usize],
    ci: usize,
) -> io::Result<()> {
    use serde::ser::{SerializeSeq, Serializer as _};

    let mask = binary_mask(rs, &dropped_binary_columns(rs, order));
    let mut ser = serde_json::Serializer::pretty(w);
    let mut seq = ser.serialize_seq(None).map_err(io::Error::other)?;
    for &di in order {
        let v = rs
            .cell(di, ci)
            .filter(|c| !withheld_binary(&mask, ci, c))
            .map(|c| value_to_json(&c.to_value()))
            .unwrap_or(serde_json::Value::Null);
        seq.serialize_element(&v).map_err(io::Error::other)?;
    }
    seq.end().map_err(io::Error::other)
}

/// One column's values as a newline-separated list (a single-column CSV).
pub fn export_column_csv(rs: &ResultSet, order: &[usize], ci: usize) -> String {
    to_string(|w| export_column_csv_to(w, rs, order, ci))
}

/// [`export_column_csv`], streamed.
pub fn export_column_csv_to<W: Write>(
    w: &mut W,
    rs: &ResultSet,
    order: &[usize],
    ci: usize,
) -> io::Result<()> {
    // The same withholding the whole-result CSV does, and it matters as much:
    // this is the "copy this column" path, and the column being copied is
    // exactly the one a caller might paste into an `IN (…)` or a spreadsheet.
    let mask = binary_mask(rs, &dropped_binary_columns(rs, order));
    for &di in order {
        match rs.cell(di, ci) {
            None => {}
            Some(c) if c.is_null() => {}
            Some(c) if withheld_binary(&mask, ci, &c) => {}
            Some(c) => w.write_all(csv_field(c.display()).as_bytes())?,
        }
        w.write_all(b"\n")?;
    }
    Ok(())
}

/// The whole result as CSV (header row + data rows; NULL → empty field).
pub fn export_csv(rs: &ResultSet, order: &[usize]) -> String {
    to_string(|w| export_csv_to(w, rs, order))
}

/// [`export_csv`], streamed.
pub fn export_csv_to<W: Write>(w: &mut W, rs: &ResultSet, order: &[usize]) -> io::Result<()> {
    export_csv_chunks(w, &mut OneChunk::new(rs, order)).map(|_| ())
}

/// [`export_csv_to`] over a [`RowChunks`] source. Returns the rows written.
///
/// The header comes from the **first** chunk and is written once — which is why
/// [`OneChunk`] yields even for an empty result, so a header-only CSV stays a
/// header-only CSV rather than an empty file.
pub fn export_csv_chunks<W: Write>(w: &mut W, src: &mut dyn RowChunks) -> io::Result<ExportTally> {
    let mut first = true;
    let mut tally = ExportTally::default();
    while let Some(c) = src.next_chunk()? {
        if first {
            for (ci, col) in c.rs.columns.iter().enumerate() {
                if ci > 0 {
                    w.write_all(b",")?;
                }
                w.write_all(csv_field(&col.name).as_bytes())?;
            }
            w.write_all(b"\n")?;
            first = false;
        }
        // CSV is a format Schemaic reads back, so a blob's placeholder must not
        // go out in it — see [`dropped_binary_columns`]. An empty field, which
        // is already how this format renders NULL. Per chunk, for the reason
        // given in [`export_json_chunks`].
        let dropped = dropped_binary_columns(c.rs, c.order);
        let mask = binary_mask(c.rs, &dropped);
        tally.note(c.rs, &dropped);
        for &di in c.order {
            if di >= c.rs.row_count() {
                continue;
            }
            for ci in 0..c.rs.columns.len() {
                if ci > 0 {
                    w.write_all(b",")?;
                }
                match c.rs.cell(di, ci) {
                    None => {}
                    Some(cell) if cell.is_null() => {}
                    Some(cell) if withheld_binary(&mask, ci, &cell) => {}
                    Some(cell) => w.write_all(csv_field(cell.display()).as_bytes())?,
                }
            }
            w.write_all(b"\n")?;
            tally.rows += 1;
        }
    }
    Ok(tally)
}

/// Escape a Markdown table cell. A `|` starts a new column, so it must be
/// backslash-escaped; backslash is Markdown's escape char, so a literal `\`
/// doubles (else it would swallow a following `|`). Newlines would break the
/// row — GitHub renders `<br>` inside table cells, so map them there (a lone CR
/// is dropped so CRLF doesn't emit a double break).
pub fn md_cell(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\r', "")
        .replace('\n', "<br>")
}

/// Escape text for HTML element content. `&` is replaced first so the `&` in
/// the `&lt;`/`&gt;` entities isn't re-escaped.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The whole result as a GitHub-Flavored-Markdown table (header row + `---`
/// separator + data rows). Cells are escaped via [`md_cell`]; NULL renders as an
/// empty cell (matching [`export_csv`]).
pub fn export_markdown(rs: &ResultSet, order: &[usize]) -> String {
    to_string(|w| export_markdown_to(w, rs, order))
}

/// [`export_markdown`], streamed.
pub fn export_markdown_to<W: Write>(w: &mut W, rs: &ResultSet, order: &[usize]) -> io::Result<()> {
    export_markdown_chunks(w, &mut OneChunk::new(rs, order)).map(|_| ())
}

/// One row's cells, already escaped, as `| a | b |`. The separator row is a fixed
/// `---` per column, so no pass over the data is needed to size them — which is
/// what lets Markdown stream at all. Mirrors the original
/// `format!("| {} |\n", cells.join(" | "))` exactly, including the degenerate
/// zero-column case (`|  |`).
fn md_row_line<W: Write>(w: &mut W, cells: &mut dyn Iterator<Item = String>) -> io::Result<()> {
    w.write_all(b"| ")?;
    for (i, cell) in cells.enumerate() {
        if i > 0 {
            w.write_all(b" | ")?;
        }
        w.write_all(cell.as_bytes())?;
    }
    w.write_all(b" |\n")
}

/// [`export_markdown_to`] over a [`RowChunks`] source. Returns the rows written.
///
/// No withheld columns in its [`ExportTally`], deliberately — Markdown keeps the
/// `<n bytes>` placeholder because nothing reads it back and there the
/// placeholder is the useful rendering. A blanked column is still reported: an
/// empty cell reads as empty whatever the format.
pub fn export_markdown_chunks<W: Write>(
    w: &mut W,
    src: &mut dyn RowChunks,
) -> io::Result<ExportTally> {
    let mut first = true;
    let mut tally = ExportTally::default();
    while let Some(c) = src.next_chunk()? {
        tally.note(c.rs, &[]);
        let n = c.rs.columns.len();
        if first {
            md_row_line(w, &mut c.rs.columns.iter().map(|col| md_cell(&col.name)))?;
            md_row_line(w, &mut (0..n).map(|_| "---".to_string()))?;
            first = false;
        }
        for &di in c.order {
            if di >= c.rs.row_count() {
                continue;
            }
            md_row_line(
                w,
                &mut (0..n).map(|ci| match c.rs.cell(di, ci) {
                    None => String::new(),
                    Some(cell) if cell.is_null() => String::new(),
                    Some(cell) => md_cell(cell.display()),
                }),
            )?;
            tally.rows += 1;
        }
    }
    Ok(tally)
}

/// The whole result as an HTML `<table>` (thead + tbody). Cells/headers are
/// escaped via [`html_escape`]; NULL renders as an empty `<td>` (matching
/// [`export_csv`]).
pub fn export_html(rs: &ResultSet, order: &[usize]) -> String {
    to_string(|w| export_html_to(w, rs, order))
}

/// [`export_html`], streamed. The preamble and closing tags are fixed strings, so
/// nothing here needs to see the whole result first.
pub fn export_html_to<W: Write>(w: &mut W, rs: &ResultSet, order: &[usize]) -> io::Result<()> {
    export_html_chunks(w, &mut OneChunk::new(rs, order)).map(|_| ())
}

/// [`export_html_to`] over a [`RowChunks`] source. Returns the rows written.
///
/// The closing tags are written **only if the opening ones were** — a source that
/// yields no chunk at all produces an empty file rather than a `</tbody></table>`
/// closing a table that was never opened.
pub fn export_html_chunks<W: Write>(w: &mut W, src: &mut dyn RowChunks) -> io::Result<ExportTally> {
    let mut opened = false;
    let mut tally = ExportTally::default();
    while let Some(c) = src.next_chunk()? {
        tally.note(c.rs, &[]);
        if !opened {
            // The charset declaration is not optional. The bytes written here are
            // UTF-8, but for a `file://` URL with no declaration and no BOM the
            // HTML spec leaves the default to the user agent — windows-1252 in
            // Western locales — so `José` opened as `JosÃ©`. Chrome dropped its
            // manual encoding override in 2014, so there was no in-browser
            // workaround; the user had to edit the file.
            w.write_all(b"<meta charset=\"utf-8\">\n")?;
            w.write_all(b"<table>\n<thead>\n<tr>")?;
            for col in &c.rs.columns {
                w.write_all(b"<th>")?;
                w.write_all(html_escape(&col.name).as_bytes())?;
                w.write_all(b"</th>")?;
            }
            w.write_all(b"</tr>\n</thead>\n<tbody>\n")?;
            opened = true;
        }
        for &di in c.order {
            if di >= c.rs.row_count() {
                continue;
            }
            w.write_all(b"<tr>")?;
            for ci in 0..c.rs.columns.len() {
                w.write_all(b"<td>")?;
                match c.rs.cell(di, ci) {
                    None => {}
                    Some(cell) if cell.is_null() => {}
                    Some(cell) => w.write_all(html_escape(cell.display()).as_bytes())?,
                }
                w.write_all(b"</td>")?;
            }
            w.write_all(b"</tr>\n")?;
            tally.rows += 1;
        }
    }
    if opened {
        w.write_all(b"</tbody>\n</table>\n")?;
    }
    Ok(tally)
}

/// The result as `INSERT` statements, in the connection's dialect. `source` is
/// the real `(database, namespace, table)` when known; otherwise a `table`
/// placeholder is emitted for the user to fill in.
///
/// Identifiers and literals are quoted per `dialect` (see [`ident_sql`] and
/// [`sql_literal`]) so the output pastes straight into a client for that engine —
/// backticks and backslash-escaping for MySQL, double quotes and literal
/// backslashes for PostgreSQL.
///
/// A PostgreSQL namespace qualifies the table *instead of* the database — a PG
/// connection is bound to one database, so `schema.table` is the addressable
/// name, exactly as everywhere else in the app.
pub fn export_inserts(
    rs: &ResultSet,
    order: &[usize],
    source: Option<(&str, Option<&str>, &str)>,
    dialect: SqlDialect,
) -> String {
    to_string(|w| export_inserts_to(w, rs, order, source, dialect))
}

/// How generated SQL addresses a table, per engine — **the one rule**, shared by
/// the SQL export and by [`crate::import::build_insert`], which had it twice.
///
/// MySQL qualifies with the **database**, since its connection is server-level.
/// PostgreSQL qualifies with the **namespace** (the connection is already bound
/// to one database), falling back to the database when no namespace is given.
///
/// SQLite names the table **bare**, and that is the case worth stating: a
/// connection *is* one file, so there is nothing to disambiguate — and `main` is
/// not a name the user chose but SQLite's word for "the file you opened", so
/// emitting `"main"."t"` is noise on every row of an export and actively wrong
/// the moment that SQL is pasted into a session where a *different* file is
/// attached under that name. Same reasoning as [`crate::filter::table_query`]'s.
pub fn qualified_table(
    database: &str,
    schema: Option<&str>,
    table: &str,
    dialect: SqlDialect,
) -> String {
    let q = |s: &str| ident_sql(s, dialect);
    match dialect {
        SqlDialect::Sqlite => q(table),
        _ => match schema {
            Some(ns) => format!("{}.{}", q(ns), q(table)),
            // **An empty `database` means "don't qualify"**, and no engine has a
            // database whose name is the empty string, so nothing legal collides
            // with it. The dump needs this: its `CREATE`/`DROP` name a MySQL table
            // bare so the file's own `USE` line is the one thing to edit to
            // retarget it, and an `INSERT` that qualified with the *source*
            // database refilled the database the rows came from — a success
            // report, an empty target, and no duplicate-key error to stop it.
            None if database.is_empty() => q(table),
            None => format!("{}.{}", q(database), q(table)),
        },
    }
}

/// Columns whose exported cells stand in for bytes this result never carried.
///
/// A raw-bytes cell renders as [`crate::model::binary_display`]'s `<n bytes>`,
/// because a `Value` has no bytes variant to hold the real thing — so writing
/// that text into an `INSERT` produces a script that *silently stores the
/// placeholder* as the column's data on re-import. This is the pre-pass that
/// finds it, and it deliberately requires **both** signals to agree: the
/// column carries bytes, and the cell's text is one this codebase
/// generated. Either alone is wrong in a way that loses data — a SQLite `BLOB`
/// column is only an affinity and may hold ordinary text, and a user's prose
/// can spell `<12 bytes>` without being a blob.
///
/// **"The column carries bytes" has two sources, and the second is why SQLite
/// works here at all.** [`crate::model::Column::is_binary`] reads the type name
/// and the wire origin, and on SQLite neither can answer: `origin` is
/// unconditionally `None` and `decl_type()` is an affinity or nothing, so an
/// untyped column — the ordinary shape for a blob store — said "not binary" and
/// the placeholder went into CSV, JSON *and* SQL as though it were the data.
/// [`ResultSet::binary_columns`] is the backend's own per-value assertion for
/// that case. It **widens** the type signal rather than replacing it: the cell
/// test still has to agree, so real text in a column that held a blob elsewhere
/// is untouched.
///
/// Returns column *indices*, not names: two result columns can share a name
/// (`SELECT a.data, b.data`) and only one of them may be the blob.
///
/// **Which exports honour this, and which deliberately don't.** The formats
/// Schemaic itself reads back — `import::ImportFormat` is CSV and JSON — must
/// not carry the placeholder, because a round trip through one stores it *as*
/// the column's data; the SQL export must not for the same reason. Markdown and
/// HTML keep it: nothing reads those back, and there the placeholder is the
/// useful rendering, since blanking it would make a 4 MB blob indistinguishable
/// from an empty cell.
fn dropped_binary_columns(rs: &ResultSet, order: &[usize]) -> Vec<usize> {
    (0..rs.columns.len())
        .filter(|&ci| {
            (rs.columns[ci].is_binary() || rs.binary_columns.contains(&ci))
                && order.iter().any(|&di| {
                    di < rs.row_count()
                        && rs
                            .cell(di, ci)
                            .is_some_and(|c| crate::model::is_binary_display(c.text()))
                })
        })
        .collect()
}

/// [`dropped_binary_columns`] in the shape the **cell loop** needs: one `bool`
/// per column, indexed by `ci`.
///
/// Two shapes for one answer, because the two readers ask it differently. The
/// `-- NOTE:` line names the columns in order and wants the `Vec<usize>`; the
/// loop asks once per *cell*, and `Vec::contains` there is a linear scan over
/// the answer — 12M of them on a 200k × 60 result, to re-derive a per-column
/// fact that was already computed. Same hoist `Db::convert_row` and `pg_cell`
/// make for `Column::is_binary`.
fn binary_mask(rs: &ResultSet, dropped: &[usize]) -> Vec<bool> {
    let mut mask = vec![false; rs.columns.len()];
    for &ci in dropped {
        if let Some(slot) = mask.get_mut(ci) {
            *slot = true;
        }
    }
    mask
}

/// Is this cell one whose real bytes the result never carried?
///
/// **The one per-cell test**, shared by every export that withholds the
/// placeholder, so the two-signals rule (the column says bytes *and* the text is
/// one this codebase generated) cannot be spelled differently in one of them.
/// `mask` comes from [`binary_mask`].
fn withheld_binary(mask: &[bool], ci: usize, c: &crate::model::CellRef<'_>) -> bool {
    mask.get(ci).copied().unwrap_or(false) && crate::model::is_binary_display(c.text())
}

/// The sentence a finished export puts on the grid's bar, or `None` when there
/// is nothing to say.
///
/// **Silence is the default and a caveat overrides it.** A save of what is
/// already on screen has nothing to report that the screen doesn't show, and a
/// note on every save is a note nobody reads — so an ordinary `Fetched` export
/// stays quiet, and only a streamed one announces its row count. But a *loss*
/// is not silent in either scope: the whole failure this exists to fix was a CSV
/// of a blob column writing empty fields, reporting nothing, and being one of
/// the two formats Schemaic itself reads back.
///
/// The caveats read the way the grid's own arena note does — the column names,
/// then what happened to them — because a user comparing the file to the screen
/// needs to know *which* part of it to distrust, and "some data was lost" tells
/// them nothing.
pub fn export_note(t: &ExportTally, name: &str, streaming: bool) -> Option<String> {
    if !streaming && !t.has_caveat() {
        return None;
    }
    let n = t.rows as usize;
    let mut s = format!(
        "Exported {} {} to {name}",
        crate::text::human_count(n),
        crate::text::plural(n, "row", "rows")
    );
    if !t.withheld.is_empty() {
        s.push_str(&format!(
            " — {} {} not carried: a text export cannot hold raw bytes",
            crate::text::plural(t.withheld.len(), "binary column", "binary columns"),
            t.withheld.join(", ")
        ));
    }
    if !t.blanked.is_empty() {
        s.push_str(if t.withheld.is_empty() { " — " } else { "; " });
        s.push_str(&format!(
            "{} too large to hold in full: later rows are blank",
            t.blanked.join(", ")
        ));
    }
    Some(s)
}

/// The scratch name an export writes to before it is finished: the destination
/// with `.part` after it.
///
/// **The destination is not opened until the export has succeeded.** It used to be
/// opened first — `File::create` truncates — so a stream that died ten minutes in
/// had already destroyed whatever the user was overwriting, and all the bar could
/// do was say so. Writing a sibling and renaming it over the target on success is
/// the dance `persist` already does for every config file, and it is atomic on
/// both platforms *because it is a sibling*: a rename within one directory never
/// crosses a filesystem.
///
/// A visible, self-describing suffix rather than a hidden temp file, because when
/// an export does fail the fragment is the one thing the user may still want —
/// it is left behind and named in the message rather than swept away.
pub fn part_path(name: &str) -> String {
    format!("{name}.part")
}

/// What a failed export says. `partial` is the destination's file name when the
/// write had begun, and `None` when the export never got that far.
///
/// **A failure is the case where the user is least likely to look.** What it needs
/// to say has changed with [`part_path`]: the destination is no longer touched
/// until the export succeeds, so the sentence is no longer "your file is a
/// fragment" but "your file is untouched, and the rows that did arrive are in the
/// sibling". Both halves matter — the first is the reassurance, the second is
/// where the partial went.
///
/// `None` is not a formality: an export refused before the write starts (no
/// connection, one already running) must not mention a file at all.
pub fn export_failure_note(message: &str, partial: Option<&str>) -> String {
    match partial {
        Some(name) => format!(
            "{message} — {name} was not changed; the rows that were written are in {}",
            part_path(name)
        ),
        None => message.to_string(),
    }
}

/// What a **cancelled** export says.
///
/// The same two facts as [`export_failure_note`], in the voice the cancel arm has
/// always used: stopping was the user's own doing, so this is a note rather than an
/// error. It said `— {name} is incomplete`, which was true when the destination was
/// truncated at `t = 0` and is now the opposite of true.
pub fn export_cancel_note(name: &str) -> String {
    format!(
        "Export cancelled — {name} was not changed; the rows that were written are in {}",
        part_path(name)
    )
}

/// The `All rows` menu entry's label: the scope's size, and every way the file
/// it writes will differ from the grid it was launched from.
///
/// **A disclosure at the point of choice, because after the file is written is
/// too late.** Two differences, and neither is visible in the result:
///
/// - `sorted` — a column-header sort is a permutation of the rows in hand
///   (`compute_order`), not something the server was asked for, so the re-run
///   returns rows in the server's order.
/// - `manual_tx` — the re-run is a *second read on a fresh connection*
///   (`Db::stream_query`, deliberately outside the tab's pinned session), so a
///   manual-transaction tab's uncommitted rows are on screen and absent from the
///   file, and rows it deleted are in the file and gone from the screen.
///
/// `size` is pre-rendered by the caller (`~16k`, or empty when the total is not
/// known) because the estimate and its `~` belong to the stats line's vocabulary,
/// not to this decision.
pub fn all_rows_label(size: &str, sorted: bool, manual_tx: bool) -> String {
    let mut notes: Vec<&str> = Vec::new();
    if !size.is_empty() {
        notes.push(size);
    }
    if sorted {
        notes.push("server order");
    }
    if manual_tx {
        notes.push("committed rows only");
    }
    if notes.is_empty() {
        "All rows".to_string()
    } else {
        format!("All rows ({})", notes.join(", "))
    }
}

/// [`export_inserts`], streamed. One statement per row and no batching, so a row
/// carries no state into the next — the table and column lists are computed once
/// and repeated verbatim.
pub fn export_inserts_to<W: Write>(
    w: &mut W,
    rs: &ResultSet,
    order: &[usize],
    source: Option<(&str, Option<&str>, &str)>,
    dialect: SqlDialect,
) -> io::Result<()> {
    export_inserts_chunks(w, &mut OneChunk::new(rs, order), source, dialect).map(|_| ())
}

/// [`export_inserts_to`] over a [`RowChunks`] source. Returns the rows written.
///
/// **The `-- NOTE:` line is the one thing here that a stream cannot know up
/// front.** Which binary columns were withheld is a fact about the rows, and a
/// streamed export never holds them all: a column can carry its real bytes for a
/// million rows and hit a blob placeholder in the next chunk. So the note is
/// emitted the first time a column is actually withheld, naming the ones newly
/// discovered, and again later only if a further column joins them. Over a single
/// chunk that collapses to exactly the old behaviour — one note, before the first
/// `INSERT`, naming every withheld column — which is what
/// `a_chunked_export_matches_the_same_rows_in_one_go` pins.
///
/// A comment and not a refusal, either way: the script still runs, and the one
/// thing it must not do is pretend the placeholder was the data.
pub fn export_inserts_chunks<W: Write>(
    w: &mut W,
    src: &mut dyn RowChunks,
    source: Option<(&str, Option<&str>, &str)>,
    dialect: SqlDialect,
) -> io::Result<ExportTally> {
    let q = |s: &str| ident_sql(s, dialect);
    let table_sql = match source {
        Some((db, ns, table)) => qualified_table(db, ns, table, dialect),
        None => q("table"),
    };
    let mut cols = String::new();
    let mut noted: Vec<bool> = Vec::new();
    let mut first = true;
    let mut tally = ExportTally::default();
    while let Some(c) = src.next_chunk()? {
        if first {
            cols =
                c.rs.columns
                    .iter()
                    .map(|col| q(&col.name))
                    .collect::<Vec<_>>()
                    .join(", ");
            noted = vec![false; c.rs.columns.len()];
            first = false;
        }
        let dropped = dropped_binary_columns(c.rs, c.order);
        let mask = binary_mask(c.rs, &dropped);
        tally.note(c.rs, &dropped);
        let fresh: Vec<usize> = dropped
            .iter()
            .copied()
            .filter(|&ci| !noted.get(ci).copied().unwrap_or(true))
            .collect();
        if !fresh.is_empty() {
            for &ci in &fresh {
                noted[ci] = true;
            }
            writeln!(
                w,
                "-- NOTE: binary column{} {} exported as NULL — a text export cannot carry raw bytes.",
                if fresh.len() == 1 { "" } else { "s" },
                fresh
                    .iter()
                    .map(|&ci| q(&c.rs.columns[ci].name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
        }
        for &di in c.order {
            if di >= c.rs.row_count() {
                continue;
            }
            write!(w, "INSERT INTO {table_sql} ({cols}) VALUES (")?;
            for ci in 0..c.rs.columns.len() {
                if ci > 0 {
                    w.write_all(b", ")?;
                }
                let lit =
                    c.rs.cell(di, ci)
                        .map(|cell| {
                            if withheld_binary(&mask, ci, &cell) {
                                "NULL".to_string()
                            } else {
                                sql_literal(&cell.to_value(), dialect)
                            }
                        })
                        .unwrap_or_else(|| "NULL".to_string());
                w.write_all(lit.as_bytes())?;
            }
            w.write_all(b");\n")?;
            tally.rows += 1;
        }
    }
    Ok(tally)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Column;

    fn col(name: &str) -> Column {
        Column {
            name: name.to_string(),
            type_name: "VARCHAR".to_string(),
            origin: None,
        }
    }

    fn rs() -> ResultSet {
        ResultSet::from_rows(
            vec![col("id"), col("a`b")],
            vec![
                vec![Value::Int(1), Value::Str("x".to_string())],
                vec![Value::Null, Value::Str("y".to_string())],
            ],
        )
    }

    use crate::intel::SqlDialect::{MySql, Postgres, Sqlite};

    /// Rows with something for every renderer to trip over: a NULL, an embedded
    /// quote, a pipe and a newline (Markdown), an angle bracket (HTML), a
    /// backslash (the dialect-sensitive SQL literal), and a non-ASCII glyph.
    fn awkward_rows() -> (Vec<Column>, Vec<Vec<Value>>) {
        let cols = vec![col("id"), col("a`b")];
        let rows = vec![
            vec![Value::Int(1), Value::Str("x".to_string())],
            vec![Value::Null, Value::Str("y | z".to_string())],
            vec![Value::Int(3), Value::Str("line\nbreak".to_string())],
            vec![Value::Int(4), Value::Str("<b>&\"quoted\"".to_string())],
            vec![Value::Int(5), Value::Str("back\\slash".to_string())],
            vec![Value::Int(6), Value::Str("José".to_string())],
            vec![Value::Int(7), Value::Null],
        ];
        (cols, rows)
    }

    /// Split `rows` into `ResultSet`s of at most `size` rows, as a source.
    fn chunked(cols: &[Column], rows: &[Vec<Value>], size: usize) -> impl RowChunks {
        let mut queue: std::collections::VecDeque<ResultSet> = rows
            .chunks(size)
            .map(|part| ResultSet::from_rows(cols.to_vec(), part.to_vec()))
            .collect();
        PullChunks::new(move || Ok(queue.pop_front()))
    }

    /// **The seam this whole design rests on.** Every `export_*_to` is now the
    /// streaming renderer over a source of one chunk, so the only way the two can
    /// disagree is at a chunk boundary — a header written twice, a JSON array
    /// closed and reopened, a `-- NOTE:` line that moves. Splitting the same rows
    /// every possible way and demanding the same bytes is what catches that; a
    /// test of either half alone would not.
    #[test]
    fn a_chunked_export_matches_the_same_rows_in_one_go() {
        let (cols, rows) = awkward_rows();
        let whole = ResultSet::from_rows(cols.clone(), rows.clone());
        let order: Vec<usize> = (0..rows.len()).collect();
        let src_name = Some(("shop", None, "orders"));
        for format in ExportFormat::ALL {
            let one_go = to_string(|w| {
                format
                    .render_to(w, &whole, &order, src_name, MySql)
                    .map(|_| ())
            })
            .into_bytes();
            // Every split from one row per chunk up to one chunk for the lot,
            // plus a size that leaves a short final chunk.
            for size in 1..=rows.len() + 1 {
                let mut src = chunked(&cols, &rows, size);
                let mut buf = Vec::new();
                let n = format
                    .stream_to(&mut buf, &mut src, src_name, MySql)
                    .expect("writing to a Vec cannot fail");
                assert_eq!(
                    String::from_utf8_lossy(&buf),
                    String::from_utf8_lossy(&one_go),
                    "{} differs when chunked {size} rows at a time",
                    format.label()
                );
                assert_eq!(
                    n.rows,
                    rows.len() as u64,
                    "{} miscounted its rows at chunk size {size}",
                    format.label()
                );
            }
        }
    }

    /// **One empty chunk and no chunk at all are different things**, and the
    /// distinction is the whole reason [`OneChunk`] yields when it has nothing to
    /// yield. A result with no rows still knows its columns, so it gets its
    /// header; a source that ended before producing anything — a query that
    /// failed before its first block — knows nothing, and a header invented for
    /// it would describe columns nobody ever saw.
    #[test]
    fn an_empty_chunk_carries_a_header_and_no_chunk_carries_nothing() {
        let (cols, _) = awkward_rows();
        let empty = ResultSet::from_rows(cols.clone(), vec![]);
        for format in ExportFormat::ALL {
            // One empty chunk — what the grid exports for a result with no rows.
            let one_go = to_string(|w| format.render_to(w, &empty, &[], None, MySql).map(|_| ()));
            match format {
                ExportFormat::Csv => assert_eq!(one_go, "id,a`b\n"),
                ExportFormat::Markdown => assert_eq!(one_go, "| id | a`b |\n| --- | --- |\n"),
                ExportFormat::Json => assert_eq!(one_go, "[]"),
                ExportFormat::Html => {
                    assert!(one_go.contains("<th>id</th>"), "{one_go}");
                    assert!(one_go.ends_with("</tbody>\n</table>\n"), "{one_go}");
                }
                // SQL has no header to write — the column list lives on each
                // `INSERT`, so no rows means no output at all.
                ExportFormat::Sql => assert_eq!(one_go, ""),
            }

            // No chunk at all: `[].chunks(n)` yields nothing, so this is a source
            // that ended before its first block.
            let mut src = chunked(&cols, &[], usize::MAX);
            let mut buf = Vec::new();
            let n = format
                .stream_to(&mut buf, &mut src, None, MySql)
                .expect("writing to a Vec cannot fail");
            let streamed = String::from_utf8(buf).expect("utf-8");
            assert_eq!(n.rows, 0, "{} counted rows it never saw", format.label());
            match format {
                // The array framing is the format's own, not the data's, so an
                // empty array is still valid JSON — where a bare header row is a
                // claim about columns.
                ExportFormat::Json => assert_eq!(streamed, "[]"),
                _ => assert_eq!(
                    streamed,
                    "",
                    "{} wrote a header for a source that produced nothing",
                    format.label()
                ),
            }
        }
    }

    /// The `-- NOTE:` line is the one piece of SQL output that used to need the
    /// whole result. A column whose placeholder only shows up in a later chunk
    /// still gets a note — once, before the rows that dropped it — and a column
    /// already noted is not announced again.
    #[test]
    fn the_binary_note_finds_a_column_that_only_drops_later() {
        let mut blob = col("thumb");
        blob.type_name = "BLOB".to_string();
        let cols = vec![col("id"), blob];
        let rows = vec![
            vec![Value::Int(1), Value::Str("real text".to_string())],
            vec![Value::Int(2), Value::Str("still text".to_string())],
            vec![
                Value::Int(3),
                Value::Str(crate::model::binary_display(4096)),
            ],
            vec![
                Value::Int(4),
                Value::Str(crate::model::binary_display(8192)),
            ],
            // A second chunk that also drops the column — without these the
            // fixture never asks whether the note repeats, and a renderer with
            // no de-duplication at all passes.
            vec![Value::Int(5), Value::Str(crate::model::binary_display(16))],
            vec![Value::Int(6), Value::Str(crate::model::binary_display(32))],
        ];
        let mut src = chunked(&cols, &rows, 2);
        let mut buf = Vec::new();
        ExportFormat::Sql
            .stream_to(&mut buf, &mut src, Some(("shop", None, "img")), MySql)
            .expect("writing to a Vec cannot fail");
        let out = String::from_utf8(buf).expect("utf-8");
        assert_eq!(
            out.matches("-- NOTE:").count(),
            1,
            "the note should be announced once, not per chunk: {out}"
        );
        // The rows carrying real text keep it; only the placeholder rows are
        // nulled. A note emitted for the whole export up front would have been
        // right about the columns and wrong about these two rows.
        assert!(out.contains("(1, 'real text')"), "{out}");
        assert!(out.contains("(2, 'still text')"), "{out}");
        assert!(out.contains("(3, NULL)"), "{out}");
        assert!(!out.contains("4096 bytes"), "placeholder exported: {out}");
        // And the note precedes the first row it applies to, not the file.
        let note_at = out.find("-- NOTE:").expect("a note");
        let row3_at = out.find("(3, NULL)").expect("row 3");
        assert!(
            note_at < row3_at,
            "the note came after the dropped row: {out}"
        );
    }

    /// **A display order can name a row that is gone.** All five renderers guard
    /// it — four with `if di >= c.rs.row_count() { continue }`, JSON with a
    /// `filter` — and no test passed an out-of-range `order`, so dropping any one
    /// guard left a green suite and a format that panics on `rs.cell`'s slice
    /// arithmetic or writes a short file.
    ///
    /// It is unreachable on the *streaming* path (`PullChunks` rebuilds `order` as
    /// `0..row_count` per chunk) and live on the one-shot path, where the order is
    /// the caller's: the grid reads `gs.rs` and `gs.order` as two separate
    /// untracked reads, so an order can outlive the result it indexes.
    #[test]
    fn an_order_naming_a_row_that_is_gone_is_skipped_by_every_format() {
        let rs = ResultSet::from_rows(
            vec![col("id"), col("note")],
            vec![
                vec![Value::Int(1), Value::Str("one".into())],
                vec![Value::Int(2), Value::Str("two".into())],
            ],
        );
        // Two real rows with a stale index between them — the shape a result
        // spliced smaller under a held order produces.
        let order = [0usize, 5, 1];
        for format in ExportFormat::ALL {
            let mut buf = Vec::new();
            let tally = format
                .render_to(&mut buf, &rs, &order, Some(("shop", None, "t")), MySql)
                .expect("writing to a Vec cannot fail");
            assert_eq!(
                tally.rows,
                2,
                "{}: only the rows that exist are written",
                format.label()
            );
            let out = String::from_utf8(buf).expect("utf-8");
            assert!(out.contains("one"), "{}: {out}", format.label());
            assert!(out.contains("two"), "{}: {out}", format.label());
            // Nothing invented for the missing index: two data rows, not three.
            if format == ExportFormat::Sql {
                assert_eq!(out.matches("INSERT INTO").count(), 2, "{out}");
            }
            if format == ExportFormat::Json {
                let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid json");
                assert_eq!(parsed.as_array().map(Vec::len), Some(2), "{out}");
            }
            if format == ExportFormat::Csv {
                assert_eq!(out.lines().count(), 3, "header + two rows: {out}");
            }
        }
    }

    /// A source that fails mid-stream must not be reported as a finished export.
    /// This is the dropped connection and the cancelled table export, which the
    /// one-shot path could never produce because its source was already in memory.
    #[test]
    fn a_source_failing_mid_stream_fails_the_export() {
        let (cols, rows) = awkward_rows();
        for format in ExportFormat::ALL {
            let mut sent = 0;
            let cols = cols.clone();
            let rows = rows.clone();
            let mut src = PullChunks::new(move || {
                sent += 1;
                match sent {
                    1 => Ok(Some(ResultSet::from_rows(cols.clone(), rows[..2].to_vec()))),
                    _ => Err(io::Error::other("connection reset")),
                }
            });
            let mut buf = Vec::new();
            let err = format
                .stream_to(&mut buf, &mut src, None, MySql)
                .expect_err("a failing source must fail the export");
            assert!(
                err.to_string().contains("connection reset"),
                "{}: the source's reason should survive: {err}",
                format.label()
            );
            // **And what is on disk is still openable.** JSON's `]` is written
            // once, after the loop, so a source error used to leave `…},` — a
            // file every parser rejects, i.e. the one format whose partial export
            // is worth nothing. The rows that did arrive are the point of a
            // cancel.
            if format == ExportFormat::Json {
                let text = String::from_utf8(buf).expect("utf-8");
                let parsed: serde_json::Value =
                    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
                assert_eq!(
                    parsed.as_array().map(Vec::len),
                    Some(2),
                    "the rows that arrived before the failure: {text}"
                );
            }
        }
    }

    /// A writer that fails after `ok_bytes` bytes — stands in for a full disk or a
    /// revoked permission part-way through a large export.
    struct FailingWriter {
        written: usize,
        ok_bytes: usize,
    }

    impl std::io::Write for FailingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.written >= self.ok_bytes {
                return Err(std::io::Error::other("disk full"));
            }
            let n = buf.len().min(self.ok_bytes - self.written);
            self.written += n;
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// The streaming and buffered paths must agree byte-for-byte, in every format.
    /// This is what lets the file export stream while the clipboard keeps a
    /// `String` without the two drifting — the same guarantee `ExportFormat`
    /// itself exists to give the Copy and Download menus.
    #[test]
    fn streaming_render_matches_the_string_render_in_every_format() {
        let rs = rs();
        let order = [1usize, 0];
        let source = Some(("db", None, "t"));
        for f in ExportFormat::ALL {
            let mut buf: Vec<u8> = Vec::new();
            f.render_to(&mut buf, &rs, &order, source, MySql).unwrap();
            assert_eq!(
                String::from_utf8(buf).unwrap(),
                f.render(&rs, &order, source, MySql),
                "{:?} streamed output differs from the buffered one",
                f
            );
        }
    }

    /// A result whose every cell needs escaping *somewhere*: a CSV delimiter and
    /// quote, an HTML entity, a Markdown pipe and backslash, a SQL quote and
    /// backslash, a formula trigger, a newline, and non-ASCII text.
    fn nasty_rs() -> ResultSet {
        ResultSet::from_rows(
            vec![col("a,b"), col("c|d"), col("e<f>")],
            vec![
                vec![
                    Value::Str("he\"llo, world".to_string()),
                    Value::Str(r"pipe | and \ backslash".to_string()),
                    Value::Str("<script>&amp;".to_string()),
                ],
                vec![
                    Value::Str("=HYPERLINK(\"x\")".to_string()),
                    Value::Str("-1+1+cmd|' /C calc'!A0".to_string()),
                    Value::Str("line\nbreak\ttab".to_string()),
                ],
                vec![
                    Value::Str("it's a 'quote'".to_string()),
                    Value::Str(r"C:\temp".to_string()),
                    Value::Str("José 東京 €".to_string()),
                ],
            ],
        )
    }

    /// The anti-drift gate above ran on data that exercised **none** of the
    /// escaping paths — plain `x`/`y` strings — so the two renderers could have
    /// disagreed on every escape in the codebase and it would still have passed.
    /// Escaping is exactly where a streamed and a buffered writer diverge, since
    /// that is where each one decides what bytes to emit.
    #[test]
    fn streaming_and_buffered_agree_on_data_that_needs_escaping() {
        let rs = nasty_rs();
        let order = [2usize, 0, 1];
        let source = Some(("db", None, "t"));
        for dialect in [MySql, Postgres] {
            for f in ExportFormat::ALL {
                let mut buf: Vec<u8> = Vec::new();
                f.render_to(&mut buf, &rs, &order, source, dialect).unwrap();
                assert_eq!(
                    String::from_utf8(buf).unwrap(),
                    f.render(&rs, &order, source, dialect),
                    "{f:?}/{dialect:?} streamed output differs from the buffered one"
                );
            }
        }
    }

    /// …and that the escaping actually fired, so the fixture can't quietly stop
    /// being nasty.
    #[test]
    fn the_escaping_fixture_really_exercises_each_escape() {
        let rs = nasty_rs();
        let order = [0usize, 1, 2];
        let csv = ExportFormat::Csv.render(&rs, &order, None, MySql);
        assert!(csv.contains("\"he\"\"llo, world\""), "CSV quote doubling");
        assert!(csv.contains("'=HYPERLINK"), "CSV formula guard");
        assert!(csv.contains("'-1+1+cmd"), "CSV leading-dash guard");

        let html = ExportFormat::Html.render(&rs, &order, None, MySql);
        assert!(html.contains("&lt;script&gt;&amp;amp;"), "HTML entities");
        assert!(html.contains("José 東京 €"), "HTML non-ASCII");

        let md = ExportFormat::Markdown.render(&rs, &order, None, MySql);
        assert!(md.contains(r"\|"), "Markdown pipe escape");

        let my = ExportFormat::Sql.render(&rs, &order, Some(("db", None, "t")), MySql);
        assert!(my.contains(r"'C:\\temp'"), "MySQL backslash doubling");
        assert!(my.contains("'it''s a ''quote'''"), "SQL quote doubling");

        let pg = ExportFormat::Sql.render(&rs, &order, Some(("db", None, "t")), Postgres);
        assert!(pg.contains(r"'C:\temp'"), "PostgreSQL leaves backslashes");
    }

    #[test]
    fn streaming_column_exports_match_the_string_versions() {
        let rs = rs();
        let order = [0usize, 1];
        for ci in 0..2 {
            let mut csv: Vec<u8> = Vec::new();
            export_column_csv_to(&mut csv, &rs, &order, ci).unwrap();
            assert_eq!(
                String::from_utf8(csv).unwrap(),
                export_column_csv(&rs, &order, ci)
            );
            let mut json: Vec<u8> = Vec::new();
            export_column_json_to(&mut json, &rs, &order, ci).unwrap();
            assert_eq!(
                String::from_utf8(json).unwrap(),
                export_column_json(&rs, &order, ci)
            );
        }
    }

    /// The JSON array is emitted incrementally rather than built as one
    /// `serde_json::Value`, so pin the exact pretty-printed shape — a formatting
    /// drift here would silently change every exported file.
    ///
    /// Keys follow **column order** (`id` before `a\`b`), not alphabetical order —
    /// see [`RowObject`]. Going through `serde_json::Map` would sort them.
    #[test]
    fn streaming_json_keeps_the_pretty_array_layout() {
        let rs = rs();
        let mut buf: Vec<u8> = Vec::new();
        export_json_to(&mut buf, &rs, &[0, 1]).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[\n  {\n    \"id\": 1,\n    \"a`b\": \"x\"\n  },\n  \
             {\n    \"id\": null,\n    \"a`b\": \"y\"\n  }\n]"
        );
    }

    /// The export must preserve the order the user selected. A `SELECT` names its
    /// columns for a reason, and an alphabetically-sorted export silently throws
    /// that away — worst on a wide result, where `id` ends up buried mid-object.
    #[test]
    fn json_keys_follow_column_order_not_alphabetical() {
        let rs = ResultSet::from_rows(
            vec![col("zebra"), col("apple"), col("middle")],
            vec![vec![Value::Int(1), Value::Int(2), Value::Int(3)]],
        );
        let out = export_json(&rs, &[0]);
        let z = out.find("zebra").unwrap();
        let a = out.find("apple").unwrap();
        let m = out.find("middle").unwrap();
        assert!(z < a && a < m, "keys were reordered:\n{out}");
    }

    #[test]
    fn streaming_json_of_no_rows_is_an_empty_array() {
        let rs = rs();
        let mut buf: Vec<u8> = Vec::new();
        export_json_to(&mut buf, &rs, &[]).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "[]");
        assert_eq!(export_json(&rs, &[]), "[]");
    }

    /// A write failure must surface. Buffering into a `String` first made the
    /// whole export either succeed or never start; streaming can fail half-way
    /// through, and a caller that ignored that would leave a truncated file
    /// looking like a complete one.
    #[test]
    fn a_failing_writer_reports_the_error_in_every_format() {
        let rs = rs();
        for f in ExportFormat::ALL {
            let mut w = FailingWriter {
                written: 0,
                ok_bytes: 4,
            };
            let err = f.render_to(&mut w, &rs, &[0, 1], None, MySql).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::Other, "{:?}", f);
        }
    }

    #[test]
    fn ident_quotes_per_dialect() {
        // MySQL backticks (doubling an embedded backtick); Postgres double-quotes
        // (doubling an embedded double-quote).
        assert_eq!(ident_sql("a`b", MySql), "`a``b`");
        assert_eq!(ident_sql("plain", Postgres), "\"plain\"");
        assert_eq!(ident_sql("a\"b", Postgres), "\"a\"\"b\"");
        // The other dialect's quote char is NOT special — it's just a character.
        assert_eq!(ident_sql("a\"b", MySql), "`a\"b`");
        assert_eq!(ident_sql("a`b", Postgres), "\"a`b\"");
    }

    /// How a table is addressed in generated SQL, per engine — and the reason
    /// SQLite names it **bare**.
    #[test]
    fn a_table_is_qualified_the_way_its_engine_needs() {
        // MySQL: a connection is server-level, so the database qualifies.
        assert_eq!(
            qualified_table("shop", None, "orders", MySql),
            "`shop`.`orders`"
        );
        // Postgres: the connection is already bound to the database, so the
        // *namespace* qualifies — and `public` is dropped by the caller.
        assert_eq!(
            qualified_table("shop", Some("sales"), "orders", Postgres),
            "\"sales\".\"orders\""
        );
        assert_eq!(
            qualified_table("shop", None, "orders", Postgres),
            "\"shop\".\"orders\""
        );
        // SQLite: a connection *is* one file. `main` is not a name the user
        // chose — it is SQLite's word for "the file you opened" — so qualifying
        // with it is noise here and actively wrong in a session where another
        // file is attached under that name.
        assert_eq!(
            qualified_table("main", None, "orders", Sqlite),
            "\"orders\""
        );
        // …and it stays bare whatever it is asked to qualify with.
        assert_eq!(
            qualified_table("other", Some("x"), "orders", Sqlite),
            "\"orders\""
        );
    }

    /// SQLite *reads* `"x"`, `` `x` `` and `[x]`, but emits only the standard
    /// form: `"` is the one of the three with a defined escape, and a name
    /// carrying a `]` could not be written in brackets at all.
    #[test]
    fn sqlite_emits_the_standard_quoting_of_its_three() {
        assert_eq!(ident_sql("plain", Sqlite), "\"plain\"");
        assert_eq!(ident_sql("a\"b", Sqlite), "\"a\"\"b\"");
        // The two compatibility quotings are ordinary characters on the way out.
        assert_eq!(ident_sql("a`b", Sqlite), "\"a`b\"");
        assert_eq!(ident_sql("a]b", Sqlite), "\"a]b\"");
    }

    /// SQLite has no backslash escape, so doubling one would corrupt the value —
    /// the same reason Postgres doesn't, and the same failure (`C:\tmp` →
    /// `C:\\tmp` written into the row).
    #[test]
    fn sql_literal_does_not_touch_a_backslash_on_sqlite() {
        let v = Value::Str("C:\\tmp".to_string());
        assert_eq!(sql_literal(&v, Sqlite), "'C:\\tmp'");
        assert_eq!(sql_literal(&v, Postgres), "'C:\\tmp'");
        assert_eq!(sql_literal(&v, MySql), "'C:\\\\tmp'");
        // The injection guard is doubling the quote, and that is on everywhere.
        assert_eq!(
            sql_literal(&Value::Str("O'Hara".to_string()), Sqlite),
            "'O''Hara'"
        );
    }

    /// Every identifier-quoting entry point in the workspace answers to
    /// `ident_sql`. The *reading* side has had a single-lexer invariant since
    /// round 1; the writing side had four implementations that had each
    /// independently arrived at the same escaping — which is the drift hazard,
    /// not the reassurance, and one of them (`ddl_string`, the literal half) had
    /// already been found wrong.
    ///
    /// The `schemaic-db` pair is bound by a matching test in that crate, since
    /// its functions are private to it.
    #[test]
    fn every_identifier_quoter_agrees_with_ident_sql() {
        let nasty = [
            "plain",
            "MixedCase",
            "with space",
            "a`b",
            "a\"b",
            "both`and\"",
            "sélect",
            "",
        ];
        for name in nasty {
            for d in [MySql, Postgres, Sqlite] {
                assert_eq!(
                    crate::schema::ddl_ident_in(name, d),
                    ident_sql(name, d),
                    "ddl_ident_in({name:?}, {d:?})"
                );
            }
            // `filter`'s is `ident_sql` itself now; assert the re-export really
            // is the same function rather than a lookalike that could be swapped
            // back to a local copy.
            assert_eq!(
                crate::filter::quoted_ident_for_test(name, MySql),
                ident_sql(name, MySql)
            );
        }
    }

    #[test]
    fn sql_literal_handles_nonfinite_and_escapes() {
        assert_eq!(sql_literal(&Value::Float(f64::NAN), MySql), "NULL");
        assert_eq!(sql_literal(&Value::Float(f64::INFINITY), MySql), "NULL");
        assert_eq!(
            sql_literal(&Value::Str("O'Hara".to_string()), MySql),
            "'O''Hara'"
        );
    }

    #[test]
    fn sql_literal_only_escapes_backslashes_on_mysql() {
        // MySQL treats `\` as an escape inside a string, so it must be doubled.
        assert_eq!(
            sql_literal(&Value::Str(r"C:\tmp".to_string()), MySql),
            r"'C:\\tmp'"
        );
        // Postgres (standard_conforming_strings = on, the default since 9.1) takes
        // a backslash literally — doubling it would silently CORRUPT the value,
        // turning `C:\tmp` into `C:\\tmp`.
        assert_eq!(
            sql_literal(&Value::Str(r"C:\tmp".to_string()), Postgres),
            r"'C:\tmp'"
        );
        // Quote-doubling is the injection guard on both.
        assert_eq!(
            sql_literal(&Value::Str("x'; DROP TABLE t; --".to_string()), Postgres),
            "'x''; DROP TABLE t; --'"
        );
    }

    #[test]
    fn c5_inserts_use_real_table_and_escape_identifiers() {
        let out = export_inserts(&rs(), &[0, 1], Some(("shop", None, "cust")), MySql);
        // Real qualified table, not a `table` placeholder; column `a`b` escaped.
        assert!(out.contains("INSERT INTO `shop`.`cust` (`id`, `a``b`) VALUES"));
        assert!(out.contains("(1, 'x')"));
        assert!(out.contains("(NULL, 'y')"));
        // Placeholder only when the source is unknown.
        assert!(export_inserts(&rs(), &[0], None, MySql).contains("INSERT INTO `table` ("));
    }

    #[test]
    fn inserts_qualify_by_namespace_instead_of_database() {
        // A PostgreSQL connection is bound to one database, so the namespace is
        // what makes the name resolvable — `schema.table`, not `db.table`.
        let out = export_inserts(
            &rs(),
            &[0],
            Some(("warehouse", Some("sales"), "orders")),
            Postgres,
        );
        assert!(out.contains("INSERT INTO \"sales\".\"orders\" ("), "{out}");
        assert!(!out.contains("warehouse"), "{out}");
    }

    #[test]
    fn inserts_for_postgres_are_valid_postgres() {
        // The whole statement has to be pasteable into a PG client: every
        // identifier double-quoted, none backtick-quoted. Note the fixture's
        // column is literally named "a`b" — on Postgres that backtick is an
        // ordinary character inside the name, so the check is that no identifier
        // is *wrapped* in backticks, not that none appears at all.
        let out = export_inserts(
            &rs(),
            &[0, 1],
            Some(("db", Some("public"), "cust")),
            Postgres,
        );
        assert!(
            out.contains("INSERT INTO \"public\".\"cust\" (\"id\", \"a`b\") VALUES"),
            "{out}"
        );
        assert!(!out.contains("`id`"), "MySQL quoting leaked: {out}");
        assert!(!out.contains("`cust`"), "MySQL quoting leaked: {out}");
        // The unknown-source placeholder follows the dialect too.
        let ph = export_inserts(&rs(), &[0], None, Postgres);
        assert!(ph.contains("INSERT INTO \"table\" ("), "{ph}");
        assert!(!ph.contains("`table`"), "{ph}");
    }

    /// A BLOB has no text form, so the grid shows `<n bytes>` — and an INSERT
    /// export that copies that placeholder through as a string literal produces
    /// a script which *silently writes the wrong bytes* into the column on
    /// re-import. `NULL` is wrong too, but visibly so, and the header comment
    /// says which columns it happened to.
    #[test]
    fn inserts_never_write_a_binary_placeholder_as_the_data() {
        let mut blob = col("thumb");
        blob.type_name = "BLOB".to_string();
        let rs = ResultSet::from_rows(
            vec![col("id"), blob],
            vec![vec![
                Value::Int(1),
                Value::Str(crate::model::binary_display(4096)),
            ]],
        );
        let out = export_inserts(&rs, &[0], Some(("shop", None, "img")), MySql);
        assert!(
            !out.contains("4096 bytes"),
            "placeholder exported as data: {out}"
        );
        assert!(out.contains("(1, NULL)"), "{out}");
        assert!(
            out.contains("thumb"),
            "the note should name the column: {out}"
        );
    }

    /// The note is a *comment*, so a script with a nulled BLOB still runs. And
    /// it must not appear for a result with no binary column at all — a note on
    /// every export is a note nobody reads.
    #[test]
    fn the_binary_note_is_a_comment_and_only_when_there_is_one() {
        let mut blob = col("thumb");
        blob.type_name = "BLOB".to_string();
        let with_blob = ResultSet::from_rows(
            vec![blob],
            vec![vec![Value::Str(crate::model::binary_display(1))]],
        );
        let out = export_inserts(&with_blob, &[0], None, MySql);
        assert!(out.starts_with("--"), "{out}");
        assert!(!export_inserts(&rs(), &[0], None, MySql).contains("--"));
    }

    /// A text column whose value happens to read like the placeholder is not a
    /// blob, and nulling it would delete real data. Both signals must agree.
    #[test]
    fn a_text_column_that_merely_looks_like_a_placeholder_is_left_alone() {
        let rs = ResultSet::from_rows(
            vec![col("note")],
            vec![vec![Value::Str("<12 bytes>".to_string())]],
        );
        let out = export_inserts(&rs, &[0], None, MySql);
        assert!(out.contains("'<12 bytes>'"), "{out}");
    }

    /// The inverse: a genuinely binary column holding a value that is *not* the
    /// placeholder (PostgreSQL's `bytea_output = escape`, say) still carries its
    /// own text, and replacing that with NULL would be the data loss this test
    /// exists to prevent.
    #[test]
    fn a_binary_column_with_real_text_is_not_nulled() {
        let mut b = col("payload");
        b.type_name = "BYTEA".to_string();
        let rs = ResultSet::from_rows(vec![b], vec![vec![Value::Str("\\x4869".to_string())]]);
        let out = export_inserts(&rs, &[0], None, Postgres);
        assert!(out.contains("4869"), "{out}");
    }

    fn blob_rs() -> ResultSet {
        let mut blob = col("thumb");
        blob.type_name = "BLOB".to_string();
        ResultSet::from_rows(
            vec![col("id"), blob],
            vec![vec![
                Value::Int(1),
                Value::Str(crate::model::binary_display(4096)),
            ]],
        )
    }

    /// **The two formats Schemaic itself re-imports.** `import::ImportFormat` is
    /// CSV and JSON, so a blob exported to either and read back stores the
    /// placeholder *as the column's data* — the same round trip
    /// `inserts_never_write_a_binary_placeholder_as_the_data` closed for the SQL
    /// export, left open at the other two emitters.
    ///
    /// Blank rather than the text, which is how both formats already render
    /// NULL: it is the honest claim, since the bytes genuinely aren't here.
    #[test]
    fn the_re_importable_formats_never_carry_a_binary_placeholder() {
        let rs = blob_rs();
        let csv = export_csv(&rs, &[0]);
        assert!(!csv.contains("4096 bytes"), "csv: {csv}");
        assert_eq!(csv, "id,thumb\n1,\n");

        let json = export_json(&rs, &[0]);
        assert!(!json.contains("4096 bytes"), "json: {json}");
        assert!(json.contains("\"thumb\": null"), "{json}");

        // …and the single-column forms, which are the "copy this column" paths.
        let ccsv = export_column_csv(&rs, &[0], 1);
        assert!(!ccsv.contains("4096 bytes"), "column csv: {ccsv}");
        assert_eq!(ccsv, "\n");
        let cjson = export_column_json(&rs, &[0], 1);
        assert!(!cjson.contains("4096 bytes"), "column json: {cjson}");
        assert!(cjson.contains("null"), "{cjson}");
    }

    /// **Markdown and HTML keep it, deliberately.** Neither is a format anything
    /// reads back — they are for a person to look at — and there the placeholder
    /// is the *useful* rendering: blanking it would make a 4 MB blob
    /// indistinguishable from an empty cell and from NULL, which is strictly
    /// less than what the grid itself shows.
    #[test]
    fn the_presentation_formats_keep_the_placeholder() {
        let rs = blob_rs();
        assert!(export_markdown(&rs, &[0]).contains("<4096 bytes>"));
        // HTML-escaped, but still there.
        assert!(export_html(&rs, &[0]).contains("&lt;4096 bytes&gt;"));
    }

    /// Both signals must agree in *every* format, not only in the SQL one: a
    /// text column whose value reads like the placeholder is real data, and
    /// blanking it in a CSV would delete it.
    #[test]
    fn a_text_column_that_looks_like_a_placeholder_survives_every_format() {
        let rs = ResultSet::from_rows(
            vec![col("note")],
            vec![vec![Value::Str("<12 bytes>".to_string())]],
        );
        assert!(export_csv(&rs, &[0]).contains("<12 bytes>"));
        assert!(export_json(&rs, &[0]).contains("<12 bytes>"));
        assert!(export_column_csv(&rs, &[0], 0).contains("<12 bytes>"));
        assert!(export_column_json(&rs, &[0], 0).contains("<12 bytes>"));
    }

    /// **A bit-field's number must reach SQL as a number.** `8ed98fe` took `BIT`
    /// off the binary list on the stated grounds that a bit-field "has a lossless
    /// text form (its number, which is also what MySQL accepts back)" — true of
    /// the bare token `10`, false of `'10'`, which is the only thing an export can
    /// emit for a `Value::Str`. MySQL's `Field_bit::store(const char*, …)` takes
    /// the raw bits of a string's bytes, so `'10'` is `0x3132` = 12594 on a
    /// `BIT(16)` and "Data too long" on a `BIT(8)`: the round trip that change
    /// was made to enable wrote wrong data instead of withholding it.
    ///
    /// The composition, not the pieces: `bit_display` had a full table of tests
    /// and was right all along, and `a_bit_fields_bytes_read_as_a_big_endian_number`
    /// is green against this bug. What nothing asked was which `Value` the
    /// loader wraps the number in.
    #[test]
    fn a_bit_field_exports_as_a_bare_number() {
        use crate::model::bit_cell;
        // What the MySQL loader stores for `BIT(16)` holding 10 …
        let cell = bit_cell(&[0x00, 0x0A]);
        // … reaches SQL unquoted, on every dialect.
        for d in [MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert_eq!(sql_literal(&cell, d), "10", "{d:?}");
        }
        // …and JSON as a number, not a string.
        assert_eq!(value_to_json(&cell), serde_json::json!(10));
        // The widest field there is, and the empty run a server should never
        // send — both still numbers.
        assert_eq!(
            sql_literal(&bit_cell(&[0xFF; 8]), MySql),
            u64::MAX.to_string()
        );
        assert_eq!(sql_literal(&bit_cell(&[]), MySql), "0");
        // And it is still the same digits the grid shows.
        let rs = {
            let mut b = col("flags");
            b.type_name = "BIT(16)".to_string();
            ResultSet::from_rows(vec![b], vec![vec![bit_cell(&[0x00, 0x0A])]])
        };
        assert_eq!(export_csv(&rs, &[0]), "flags\n10\n");
        assert!(
            export_inserts(&rs, &[0], None, MySql).contains("VALUES (10)"),
            "{}",
            export_inserts(&rs, &[0], None, MySql)
        );
    }

    // ── What the export says it could not carry ───────────────────────────
    //
    // The loss itself is right (above); these are about *saying so*. CSV and
    // JSON have no comment syntax, so the withheld column left no trace in the
    // file at all — and an empty CSV field is what a NULL looks like.

    /// **The formats that drop the bytes report the columns they dropped.** The
    /// SQL emitter writes a `-- NOTE:` line for exactly this loss; the two
    /// formats Schemaic itself re-imports wrote nothing anywhere.
    #[test]
    fn a_withholding_export_reports_the_columns_it_blanked() {
        let rs = blob_rs();
        for format in [ExportFormat::Csv, ExportFormat::Json, ExportFormat::Sql] {
            let mut buf = Vec::new();
            let t = format
                .render_to(&mut buf, &rs, &[0], None, MySql)
                .expect("writing to a Vec cannot fail");
            assert_eq!(t.rows, 1, "{}", format.label());
            assert_eq!(t.withheld, vec!["thumb".to_string()], "{}", format.label());
            assert!(t.has_caveat(), "{}", format.label());
        }
        // Markdown and HTML keep the placeholder on purpose, so they have
        // nothing to disclose — reporting a loss they did not take would send
        // the user looking for data that is in the file.
        for format in [ExportFormat::Markdown, ExportFormat::Html] {
            let mut buf = Vec::new();
            let t = format
                .render_to(&mut buf, &rs, &[0], None, MySql)
                .expect("writing to a Vec cannot fail");
            assert!(t.withheld.is_empty(), "{}", format.label());
            assert!(!t.has_caveat(), "{}", format.label());
        }
    }

    /// A column named once, however many chunks carried it — a streamed export
    /// sees the same blob column in every block.
    #[test]
    fn a_withheld_column_is_named_once_across_a_stream() {
        let rs = blob_rs();
        // Three chunks of the same one-row result: the same column withheld in
        // each. `chunked` is the same helper the round-trip test uses.
        struct Thrice<'a>(&'a ResultSet, &'a [usize], usize);
        impl RowChunks for Thrice<'_> {
            fn next_chunk(&mut self) -> io::Result<Option<RowChunk<'_>>> {
                if self.2 == 0 {
                    return Ok(None);
                }
                self.2 -= 1;
                Ok(Some(RowChunk {
                    rs: self.0,
                    order: self.1,
                }))
            }
        }
        let mut src = Thrice(&rs, &[0], 3);
        let mut buf = Vec::new();
        let t = ExportFormat::Csv
            .stream_to(&mut buf, &mut src, None, MySql)
            .expect("writing to a Vec cannot fail");
        assert_eq!(t.rows, 3);
        assert_eq!(t.withheld, vec!["thumb".to_string()]);
    }

    /// **The arena ceiling, which nothing read.** A column whose 512 MiB text
    /// arena filled up renders blank from that row on; the grid says so in its
    /// own note, but a streamed chunk is never mounted in a grid, so a
    /// whole-table export could write a file with holes in it and report a full
    /// row count. Every format reports it — an empty cell reads as empty
    /// whatever the syntax around it.
    #[test]
    fn a_column_blanked_by_the_arena_ceiling_is_reported_by_every_format() {
        let mut rs = blob_rs();
        // The blanking is `ColumnData::finish_cell`'s and is tested there; what
        // reaches an exporter is the flag.
        rs.capped_columns = vec![1];
        for format in ExportFormat::ALL {
            let mut buf = Vec::new();
            let t = format
                .render_to(&mut buf, &rs, &[0], None, MySql)
                .expect("writing to a Vec cannot fail");
            assert_eq!(t.blanked, vec!["thumb".to_string()], "{}", format.label());
            assert!(t.has_caveat(), "{}", format.label());
        }
    }

    /// A dump writes many tables into one file and reports one sentence about
    /// it. The fold used to be written out inside the writer loop, where nothing
    /// could reach it: the caller needs a `Db`, a runtime handle and two
    /// channels.
    #[test]
    fn absorbing_another_tables_tally_sums_rows_and_names_a_column_once() {
        let mut total = ExportTally {
            rows: 10,
            withheld: vec!["body".to_string()],
            blanked: vec!["notes".to_string()],
        };
        // A second table with the *same* wide column: named once, not twice.
        total.absorb(ExportTally {
            rows: 5,
            withheld: vec!["body".to_string()],
            blanked: vec![],
        });
        assert_eq!(total.rows, 15);
        assert_eq!(total.withheld, vec!["body".to_string()]);

        // A third with a different one: both kept, in first-seen order — the
        // caveat has to name every part of the file to distrust.
        total.absorb(ExportTally {
            rows: 1,
            withheld: vec!["thumb".to_string()],
            blanked: vec!["notes".to_string(), "memo".to_string()],
        });
        assert_eq!(total.rows, 16);
        assert_eq!(
            total.withheld,
            vec!["body".to_string(), "thumb".to_string()]
        );
        assert_eq!(total.blanked, vec!["notes".to_string(), "memo".to_string()]);
        assert!(total.has_caveat());
    }

    #[test]
    fn absorbing_a_clean_table_says_nothing_new() {
        let mut total = ExportTally::default();
        total.absorb(ExportTally {
            rows: 7,
            ..Default::default()
        });
        assert_eq!(total.rows, 7);
        assert!(!total.has_caveat());
    }

    /// The sentence the bar shows. **Silence is the default and a caveat
    /// overrides it**: a save of what is already on screen says nothing, a
    /// streamed one says its count, and a loss is said in either scope.
    #[test]
    fn the_export_note_says_what_could_not_be_carried() {
        let clean = ExportTally {
            rows: 2,
            ..Default::default()
        };
        assert_eq!(export_note(&clean, "docs.csv", false), None);
        assert_eq!(
            export_note(&clean, "docs.csv", true).as_deref(),
            Some("Exported 2 rows to docs.csv")
        );

        let one = ExportTally {
            rows: 2,
            withheld: vec!["file".to_string()],
            blanked: Vec::new(),
        };
        assert_eq!(
            export_note(&one, "docs.csv", false).as_deref(),
            Some(
                "Exported 2 rows to docs.csv — binary column file not carried: a text export \
                 cannot hold raw bytes"
            )
        );
        let two = ExportTally {
            rows: 1,
            withheld: vec!["file".to_string(), "thumb".to_string()],
            blanked: Vec::new(),
        };
        assert_eq!(
            export_note(&two, "docs.csv", true).as_deref(),
            Some(
                "Exported 1 row to docs.csv — binary columns file, thumb not carried: a text \
                 export cannot hold raw bytes"
            )
        );
        let blanked = ExportTally {
            rows: 2_000_000,
            withheld: Vec::new(),
            blanked: vec!["body".to_string()],
        };
        assert_eq!(
            export_note(&blanked, "docs.csv", true).as_deref(),
            Some(
                "Exported 2m rows to docs.csv — body too large to hold in full: later rows are blank"
            )
        );
        // Both losses, one sentence.
        let both = ExportTally {
            rows: 3,
            withheld: vec!["file".to_string()],
            blanked: vec!["body".to_string()],
        };
        let msg = export_note(&both, "docs.csv", true).expect("a caveat is always said");
        assert!(msg.contains("binary column file not carried"), "{msg}");
        assert!(msg.contains("; body too large to hold in full"), "{msg}");
    }

    /// **A failure leaves the destination alone**, which is the sentence's whole
    /// job now: the rows go to a `.part` sibling and it is renamed over the target
    /// only when the export finished, so the file the user was overwriting is
    /// intact — and the rows that did arrive are still somewhere they can be had.
    ///
    /// It used to say `— orders.csv is incomplete`, which was true when
    /// `File::create` truncated the destination at `t = 0` and is now the opposite
    /// of true. And an export refused *before* the write still must not mention a
    /// file at all.
    #[test]
    fn a_failed_write_says_the_destination_survived_and_where_the_rows_went() {
        let msg = export_failure_note("Export failed: No space left on device", Some("orders.csv"));
        assert_eq!(
            msg,
            "Export failed: No space left on device — orders.csv was not changed; \
             the rows that were written are in orders.csv.part"
        );
        // The two facts, named rather than pattern-matched, so a reworded sentence
        // that drops one of them fails here.
        assert!(msg.contains("was not changed"), "{msg}");
        assert!(msg.contains("orders.csv.part"), "{msg}");
        assert!(
            !msg.contains("is incomplete"),
            "the destination is not a fragment any more: {msg}"
        );

        assert_eq!(
            export_failure_note("An export is already running.", None),
            "An export is already running."
        );
    }

    /// The cancel arm says the same two things, in the voice it has always used —
    /// stopping was the user's own doing, so it is a note and not an error.
    #[test]
    fn a_cancelled_export_says_the_same_two_things() {
        let msg = export_cancel_note("orders.csv");
        assert_eq!(
            msg,
            "Export cancelled — orders.csv was not changed; \
             the rows that were written are in orders.csv.part"
        );
        assert!(msg.starts_with("Export cancelled"), "{msg}");
        assert!(!msg.contains("is incomplete"), "{msg}");
    }

    /// The sibling is the destination plus a suffix and nothing cleverer — a
    /// rename inside one directory is what makes the publish atomic, so the name
    /// must not move the file anywhere.
    #[test]
    fn the_part_file_is_a_sibling_of_the_destination() {
        assert_eq!(part_path("orders.csv"), "orders.csv.part");
        // Extensions and dots in the stem are left exactly alone: the suffix is
        // appended, never substituted, so nothing can collide with a real file the
        // user has by having its extension replaced.
        assert_eq!(part_path("a.b.c.json"), "a.b.c.json.part");
        assert_eq!(part_path("no-extension"), "no-extension.part");
        // No separator is introduced, on either platform's spelling.
        for name in ["orders.csv", "a.b.c.json", "no-extension"] {
            let p = part_path(name);
            assert!(p.starts_with(name), "{p}");
            assert!(!p[name.len()..].contains('/'), "{p}");
            assert!(!p[name.len()..].contains('\\'), "{p}");
        }
    }

    /// **Every way the file will differ from the screen, at the point of
    /// choice.** The sort was disclosed; the transaction was not, and it is the
    /// larger of the two — an `All rows` export re-runs on a fresh connection,
    /// so a manual-transaction tab's uncommitted rows are on screen and absent
    /// from the file.
    #[test]
    fn the_all_rows_label_discloses_every_way_the_file_differs() {
        assert_eq!(all_rows_label("", false, false), "All rows");
        assert_eq!(all_rows_label("~16k", false, false), "All rows (~16k)");
        assert_eq!(
            all_rows_label("", true, false),
            "All rows (server order)",
            "the sort was already disclosed and must stay so"
        );
        assert_eq!(
            all_rows_label("~16k", true, false),
            "All rows (~16k, server order)"
        );
        assert_eq!(
            all_rows_label("", false, true),
            "All rows (committed rows only)"
        );
        assert_eq!(
            all_rows_label("~16k", false, true),
            "All rows (~16k, committed rows only)"
        );
        assert_eq!(
            all_rows_label("", true, true),
            "All rows (server order, committed rows only)"
        );
        assert_eq!(
            all_rows_label("~16k", true, true),
            "All rows (~16k, server order, committed rows only)"
        );
    }

    /// And the inverse, again in every format: a binary column carrying real
    /// text is not blanked.
    #[test]
    fn a_binary_column_with_real_text_survives_every_format() {
        let mut b = col("payload");
        b.type_name = "BYTEA".to_string();
        let rs = ResultSet::from_rows(vec![b], vec![vec![Value::Str("\\x4869".to_string())]]);
        assert!(export_csv(&rs, &[0]).contains("4869"));
        assert!(export_json(&rs, &[0]).contains("4869"));
        assert!(export_column_csv(&rs, &[0], 0).contains("4869"));
    }

    // ── export formats + save-file naming ─────────────────────────────────

    #[test]
    fn every_format_has_a_distinct_label_and_extension() {
        let labels: Vec<&str> = ExportFormat::ALL.iter().map(|f| f.label()).collect();
        let exts: Vec<&str> = ExportFormat::ALL.iter().map(|f| f.extension()).collect();
        for v in [&labels, &exts] {
            let mut s = v.clone();
            s.sort_unstable();
            s.dedup();
            assert_eq!(s.len(), v.len(), "duplicates in {v:?}");
        }
        // No leading dot — Floem's FileSpec adds it.
        assert!(exts.iter().all(|e| !e.starts_with('.')));
    }

    #[test]
    fn format_render_matches_the_direct_call() {
        // The enum is the single dispatch point for both menus, so it must agree
        // with the functions it fronts.
        let (rs, order) = (rs(), [0, 1][..].to_vec());
        let src = Some(("shop", None, "cust"));
        for f in ExportFormat::ALL {
            let via_enum = f.render(&rs, &order, src, MySql);
            let direct = match f {
                ExportFormat::Json => export_json(&rs, &order),
                ExportFormat::Csv => export_csv(&rs, &order),
                ExportFormat::Sql => export_inserts(&rs, &order, src, MySql),
                ExportFormat::Markdown => export_markdown(&rs, &order),
                ExportFormat::Html => export_html(&rs, &order),
            };
            assert_eq!(via_enum, direct, "{}", f.label());
        }
        // Only SQL is dialect- and source-sensitive.
        assert_ne!(
            ExportFormat::Sql.render(&rs, &order, src, MySql),
            ExportFormat::Sql.render(&rs, &order, src, Postgres)
        );
        assert_eq!(
            ExportFormat::Csv.render(&rs, &order, src, MySql),
            ExportFormat::Csv.render(&rs, &order, None, Postgres)
        );
    }

    #[test]
    fn suggested_filename_uses_the_source_table() {
        assert_eq!(
            suggested_filename(Some("orders"), ExportFormat::Csv),
            "orders.csv"
        );
        // A schema-qualified name keeps its dot — it's a legal file-name char.
        assert_eq!(
            suggested_filename(Some("sales.orders"), ExportFormat::Json),
            "sales.orders.json"
        );
        // No source (an arbitrary SELECT) → a neutral default.
        assert_eq!(suggested_filename(None, ExportFormat::Sql), "result.sql");
        assert_eq!(
            suggested_filename(Some(""), ExportFormat::Markdown),
            "result.md"
        );
    }

    #[test]
    fn suggested_filename_sanitizes_a_hostile_table_name() {
        // A table name comes from the server, so it can hold anything. None of it
        // may become a path separator or an illegal component.
        let out = suggested_filename(Some("a/b\\c:d*e?f\"g<h>i|j"), ExportFormat::Csv);
        assert_eq!(out, "a_b_c_d_e_f_g_h_i_j.csv");
        assert!(!out.contains(['/', '\\']), "{out}");
        // Control characters too.
        assert_eq!(
            suggested_filename(Some("a\nb\tc"), ExportFormat::Csv),
            "a_b_c.csv"
        );
        // A name that sanitizes to nothing falls back rather than yielding ".csv".
        assert_eq!(
            suggested_filename(Some("..."), ExportFormat::Csv),
            "result.csv"
        );
        assert_eq!(
            suggested_filename(Some("   "), ExportFormat::Csv),
            "result.csv"
        );
        // Windows rejects a trailing dot/space.
        assert_eq!(
            suggested_filename(Some("orders. "), ExportFormat::Csv),
            "orders.csv"
        );
        // Reserved device names are escaped, case-insensitively.
        assert_eq!(
            suggested_filename(Some("CON"), ExportFormat::Csv),
            "_CON.csv"
        );
        assert_eq!(
            suggested_filename(Some("nul"), ExportFormat::Csv),
            "_nul.csv"
        );
        // A very long name is capped (component limits are ~255 bytes).
        let long = "x".repeat(400);
        let out = suggested_filename(Some(&long), ExportFormat::Csv);
        assert!(out.len() < 120, "{} chars", out.len());
    }

    #[test]
    fn csv_quotes_only_when_needed() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("he\"llo"), "\"he\"\"llo\"");
    }

    #[test]
    fn csv_neutralizes_formula_injection() {
        // Leading formula/DDE triggers get a `'` prefix (then quoted if needed).
        assert_eq!(csv_field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_field("+1+2"), "'+1+2");
        assert_eq!(csv_field("@SUM(A1)"), "'@SUM(A1)");
        // Tab isn't a CSV delimiter, so the guarded value isn't additionally quoted.
        assert_eq!(csv_field("\tcmd"), "'\tcmd");
        // A `=` mid-value is harmless — only leading chars trigger a formula.
        assert_eq!(csv_field("a=b"), "a=b");
    }

    /// A leading `-` was let through on the grounds that guarding it would
    /// corrupt every negative number. That dichotomy isn't forced: a number and
    /// a formula are distinguishable, so guard the one and leave the other.
    #[test]
    fn csv_guards_a_leading_dash_that_is_not_a_number() {
        // The DDE payload the finding was written from.
        assert_eq!(
            csv_field("-1+1+cmd|' /C calc'!A0"),
            "'-1+1+cmd|' /C calc'!A0"
        );
        assert_eq!(csv_field("-A1"), "'-A1");
        assert_eq!(csv_field("-=1"), "'-=1");
    }

    /// …and every shape of negative number still exports unguarded, which is the
    /// whole reason the character was skipped in the first place.
    #[test]
    fn csv_leaves_negative_numbers_alone() {
        for n in [
            "-5",
            "-0",
            "-5.25",
            "-.5",
            "-1e10",
            "-1E-10",
            "-1234567890123456789",
        ] {
            assert_eq!(csv_field(n), n, "{n} is a number, not a formula");
        }
        // A bare dash isn't a formula either — it's a common "no value" marker.
        assert_eq!(csv_field("-"), "-");
    }

    #[test]
    fn json_suffixes_duplicate_columns() {
        let rs = ResultSet::from_rows(
            vec![col("id"), col("id"), col("id")],
            vec![vec![Value::Int(1), Value::Int(2), Value::Int(3)]],
        );
        let v: serde_json::Value = serde_json::from_str(&export_json(&rs, &[0])).unwrap();
        assert_eq!(v[0]["id"], 1);
        assert_eq!(v[0]["id_2"], 2);
        assert_eq!(v[0]["id_3"], 3);
    }

    #[test]
    fn json_respects_display_order() {
        // order [1, 0] → the NULL-id row first.
        let out = export_json(&rs(), &[1, 0]);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v[0]["id"].is_null());
        assert_eq!(v[1]["id"], 1);
    }

    #[test]
    fn value_to_json_maps_each_variant_and_nulls_nonfinite() {
        use serde_json::Value as J;
        assert_eq!(value_to_json(&Value::Null), J::Null);
        assert_eq!(value_to_json(&Value::Int(-3)), J::from(-3i64));
        assert_eq!(value_to_json(&Value::UInt(3)), J::from(3u64));
        assert_eq!(value_to_json(&Value::Float(1.5)), J::from(1.5));
        assert_eq!(
            value_to_json(&Value::Str("s".into())),
            J::String("s".into())
        );
        // Non-finite floats have no JSON representation → null.
        assert_eq!(value_to_json(&Value::Float(f64::NAN)), J::Null);
        assert_eq!(value_to_json(&Value::Float(f64::INFINITY)), J::Null);
    }

    #[test]
    fn export_csv_has_header_and_nulls_are_empty() {
        let out = export_csv(&rs(), &[0, 1]);
        let lines: Vec<&str> = out.lines().collect();
        // Header quotes the backtick column only because... it has no comma; stays bare.
        assert_eq!(lines[0], "id,a`b");
        assert_eq!(lines[1], "1,x");
        // NULL id renders as an empty leading field.
        assert_eq!(lines[2], ",y");
    }

    #[test]
    fn export_column_csv_is_newline_separated_with_blank_nulls() {
        // Column 0 (id): 1, then NULL → blank line.
        let out = export_column_csv(&rs(), &[0, 1], 0);
        assert_eq!(out, "1\n\n");
    }

    #[test]
    fn md_cell_escapes_pipe_backslash_and_newline() {
        // A pipe would start a new column — escape it. Backslash is Markdown's
        // escape char, so a literal `\` must double (else it'd escape the `|`).
        assert_eq!(md_cell("a|b"), "a\\|b");
        assert_eq!(md_cell("C:\\x"), "C:\\\\x");
        assert_eq!(md_cell("a\\|b"), "a\\\\\\|b");
        // Newlines would break the row → GFM `<br>`; a lone CR is dropped.
        assert_eq!(md_cell("a\nb"), "a<br>b");
        assert_eq!(md_cell("a\r\nb"), "a<br>b");
        assert_eq!(md_cell("plain"), "plain");
    }

    #[test]
    fn export_markdown_has_header_separator_and_orders_rows() {
        // order [1, 0] → NULL-id row first; NULL renders as an empty cell.
        let out = export_markdown(&rs(), &[1, 0]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "| id | a`b |");
        assert_eq!(lines[1], "| --- | --- |");
        assert_eq!(lines[2], "|  | y |");
        assert_eq!(lines[3], "| 1 | x |");
    }

    #[test]
    fn html_escape_orders_ampersand_first() {
        // `&` must be escaped before `<`/`>` or `&lt;` would become `&amp;lt;`.
        assert_eq!(html_escape("a<b>&c"), "a&lt;b&gt;&amp;c");
        assert_eq!(html_escape("plain"), "plain");
    }

    #[test]
    fn export_html_escapes_entities_and_nulls_are_empty() {
        let rs = ResultSet::from_rows(
            vec![col("a<b>")],
            vec![vec![Value::Str("x&y".to_string())], vec![Value::Null]],
        );
        let out = export_html(&rs, &[0, 1]);
        assert!(out.contains("<th>a&lt;b&gt;</th>"));
        assert!(out.contains("<td>x&amp;y</td>"));
        // NULL → empty cell, not the literal "NULL".
        assert!(out.contains("<td></td>"));
        // Well-formed table scaffolding, behind the charset declaration.
        assert!(out.trim_start().starts_with("<meta charset=\"utf-8\">"));
        assert!(out.contains("<table>"));
        assert!(out.contains("<thead>") && out.contains("<tbody>"));
        assert!(out.trim_end().ends_with("</table>"));
    }

    /// Without a declared encoding, a browser opening the saved `file://` HTML
    /// falls back to windows-1252 in Western locales and renders `José` as
    /// `JosÃ©`. The bytes were always correct UTF-8; nothing said so.
    #[test]
    fn export_html_declares_utf8_so_non_ascii_survives() {
        let rs = ResultSet::from_rows(
            vec![col("name")],
            vec![vec![Value::Str("José 東京 €".to_string())]],
        );
        let out = export_html(&rs, &[0]);
        assert!(
            out.trim_start().starts_with("<meta charset=\"utf-8\">"),
            "the declaration must precede any content:\n{out}"
        );
        assert!(out.contains("José 東京 €"), "and the text passes through");
    }

    #[test]
    fn export_column_json_is_array_in_display_order() {
        // Column 1 (a`b) in reversed order.
        let out = export_column_json(&rs(), &[1, 0], 1);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v[0], "y");
        assert_eq!(v[1], "x");
        // Column 0 with a NULL becomes JSON null.
        let out = export_column_json(&rs(), &[0, 1], 0);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v[0], 1);
        assert!(v[1].is_null());
    }
}
