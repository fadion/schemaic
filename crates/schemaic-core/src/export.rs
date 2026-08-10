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
        to_string(|w| self.render_to(w, rs, order, source, dialect))
    }

    /// Stream the same rendering into `w` — the file export's path, so a large
    /// result never exists twice in memory. Identical output to [`Self::render`].
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
    ) -> io::Result<()> {
        match self {
            ExportFormat::Json => export_json_to(w, rs, order),
            ExportFormat::Csv => export_csv_to(w, rs, order),
            ExportFormat::Sql => export_inserts_to(w, rs, order, source, dialect),
            ExportFormat::Markdown => export_markdown_to(w, rs, order),
            ExportFormat::Html => export_html_to(w, rs, order),
        }
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
                SqlDialect::Postgres => s.replace('\'', "''"),
            };
            format!("'{escaped}'")
        }
    }
}

/// Quote a SQL identifier in the connection's dialect, doubling the embedded
/// quote character: MySQL `` `name` ``, PostgreSQL `"name"`. The *other*
/// dialect's quote char is an ordinary character and passes through untouched.
pub fn ident_sql(name: &str, dialect: SqlDialect) -> String {
    match dialect {
        SqlDialect::MySql => format!("`{}`", name.replace('`', "``")),
        SqlDialect::Postgres => format!("\"{}\"", name.replace('"', "\"\"")),
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
    use serde::ser::{SerializeSeq, Serializer as _};

    let keys = unique_column_keys(rs);
    let mut ser = serde_json::Serializer::pretty(w);
    let mut seq = ser.serialize_seq(None).map_err(io::Error::other)?;
    for &di in order.iter().filter(|&&di| di < rs.row_count()) {
        seq.serialize_element(&RowObject {
            rs,
            keys: &keys,
            di,
        })
        .map_err(io::Error::other)?;
    }
    seq.end().map_err(io::Error::other)
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

    let mut ser = serde_json::Serializer::pretty(w);
    let mut seq = ser.serialize_seq(None).map_err(io::Error::other)?;
    for &di in order {
        let v = rs
            .cell(di, ci)
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
    for &di in order {
        match rs.cell(di, ci) {
            None => {}
            Some(c) if c.is_null() => {}
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
    for (ci, c) in rs.columns.iter().enumerate() {
        if ci > 0 {
            w.write_all(b",")?;
        }
        w.write_all(csv_field(&c.name).as_bytes())?;
    }
    w.write_all(b"\n")?;
    for &di in order {
        if di >= rs.row_count() {
            continue;
        }
        for ci in 0..rs.columns.len() {
            if ci > 0 {
                w.write_all(b",")?;
            }
            match rs.cell(di, ci) {
                None => {}
                Some(c) if c.is_null() => {}
                Some(c) => w.write_all(csv_field(c.display()).as_bytes())?,
            }
        }
        w.write_all(b"\n")?;
    }
    Ok(())
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
    let n = rs.columns.len();
    // One row's cells, already escaped, as `| a | b |`. The separator row is a
    // fixed `---` per column, so no pass over the data is needed to size them.
    // Mirrors the old `format!("| {} |\n", cells.join(" | "))` exactly, including
    // the degenerate zero-column case (`|  |`).
    let row_line = |w: &mut W, cells: &mut dyn Iterator<Item = String>| -> io::Result<()> {
        w.write_all(b"| ")?;
        for (i, cell) in cells.enumerate() {
            if i > 0 {
                w.write_all(b" | ")?;
            }
            w.write_all(cell.as_bytes())?;
        }
        w.write_all(b" |\n")
    };
    row_line(w, &mut rs.columns.iter().map(|c| md_cell(&c.name)))?;
    row_line(w, &mut (0..n).map(|_| "---".to_string()))?;
    for &di in order {
        if di >= rs.row_count() {
            continue;
        }
        row_line(
            w,
            &mut (0..n).map(|ci| match rs.cell(di, ci) {
                None => String::new(),
                Some(c) if c.is_null() => String::new(),
                Some(c) => md_cell(c.display()),
            }),
        )?;
    }
    Ok(())
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
    // The charset declaration is not optional. The bytes written here are UTF-8,
    // but for a `file://` URL with no declaration and no BOM the HTML spec leaves
    // the default to the user agent — windows-1252 in Western locales — so
    // `José` opened as `JosÃ©`. Chrome dropped its manual encoding override in
    // 2014, so there was no in-browser workaround; the user had to edit the file.
    w.write_all(b"<meta charset=\"utf-8\">\n")?;
    w.write_all(b"<table>\n<thead>\n<tr>")?;
    for c in &rs.columns {
        w.write_all(b"<th>")?;
        w.write_all(html_escape(&c.name).as_bytes())?;
        w.write_all(b"</th>")?;
    }
    w.write_all(b"</tr>\n</thead>\n<tbody>\n")?;
    for &di in order {
        if di >= rs.row_count() {
            continue;
        }
        w.write_all(b"<tr>")?;
        for ci in 0..rs.columns.len() {
            w.write_all(b"<td>")?;
            match rs.cell(di, ci) {
                None => {}
                Some(c) if c.is_null() => {}
                Some(c) => w.write_all(html_escape(c.display()).as_bytes())?,
            }
            w.write_all(b"</td>")?;
        }
        w.write_all(b"</tr>\n")?;
    }
    w.write_all(b"</tbody>\n</table>\n")
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
    let q = |s: &str| ident_sql(s, dialect);
    let table_sql = match source {
        Some((_, Some(ns), table)) => format!("{}.{}", q(ns), q(table)),
        Some((db, None, table)) => format!("{}.{}", q(db), q(table)),
        None => q("table"),
    };
    let cols = rs
        .columns
        .iter()
        .map(|c| q(&c.name))
        .collect::<Vec<_>>()
        .join(", ");
    for &di in order {
        if di >= rs.row_count() {
            continue;
        }
        write!(w, "INSERT INTO {table_sql} ({cols}) VALUES (")?;
        for ci in 0..rs.columns.len() {
            if ci > 0 {
                w.write_all(b", ")?;
            }
            let lit = rs
                .cell(di, ci)
                .map(|c| sql_literal(&c.to_value(), dialect))
                .unwrap_or_else(|| "NULL".to_string());
            w.write_all(lit.as_bytes())?;
        }
        w.write_all(b");\n")?;
    }
    Ok(())
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

    use crate::intel::SqlDialect::{MySql, Postgres};

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
