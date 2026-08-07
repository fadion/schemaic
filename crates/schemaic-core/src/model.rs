//! Result-set model shared across the app.
//!
//! Cells arrive over the MySQL *text protocol* (every value as a string) and are
//! parsed into [`Value`]'s compact numeric variants where lossless; `DECIMAL`,
//! dates, JSON, and anything else MySQL sends as exact text stay a `Str` so
//! nothing is rounded or reformatted. Column provenance ([`ColumnOrigin`]) drives
//! the write-back editing system.

use std::sync::Arc;

/// A single result cell.
///
/// M2 parses the wire text into compact numeric variants (for tighter memory on
/// large results and right-aligned display); everything else — including
/// `DECIMAL` and dates, which MySQL already sends as exact text — stays a
/// `Str`, so nothing is rounded or reformatted lossily.
#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Text to render in a grid cell (NULLs render as the literal `NULL`,
    /// styled dim by the UI).
    pub fn display(&self) -> String {
        match self {
            Value::Null => "NULL".to_string(),
            Value::Int(v) => v.to_string(),
            Value::UInt(v) => v.to_string(),
            Value::Float(v) => v.to_string(),
            Value::Str(s) => s.clone(),
        }
    }
}

/// Compact per-cell type tag for the columnar [`ResultSet`] storage. The cell's
/// text lives in the owning column's arena; this tag says how to interpret it
/// (and whether it is SQL `NULL`). Mirrors the [`Value`] variants. Stored as the
/// top 3 bits of each cell's packed offset word (see [`ColumnData`]) — no
/// per-cell byte of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellTag {
    Null,
    Int,
    UInt,
    Float,
    Str,
}

impl CellTag {
    /// 3-bit encoding packed into the high bits of a column's offset word.
    fn to_bits(self) -> u32 {
        match self {
            CellTag::Null => 0,
            CellTag::Int => 1,
            CellTag::UInt => 2,
            CellTag::Float => 3,
            CellTag::Str => 4,
        }
    }

    /// Inverse of [`CellTag::to_bits`]. Any unused bit pattern maps to `Str` —
    /// the safe default (raw text), so a stray value can never mis-parse.
    fn from_bits(bits: u32) -> CellTag {
        match bits {
            0 => CellTag::Null,
            1 => CellTag::Int,
            2 => CellTag::UInt,
            3 => CellTag::Float,
            _ => CellTag::Str,
        }
    }
}

/// A borrowed view of a single result cell: its type tag plus its stored text
/// (empty for NULL). Cheap to copy, no allocation — this is what the grid reads
/// on the hot render/sort/scan paths. Reconstruct an owned, typed [`Value`] with
/// [`CellRef::to_value`] only where one is actually needed (edit keys, JSON).
#[derive(Clone, Copy, Debug)]
pub struct CellRef<'a> {
    pub tag: CellTag,
    text: &'a str,
}

impl<'a> CellRef<'a> {
    pub fn is_null(&self) -> bool {
        self.tag == CellTag::Null
    }

    /// The text to render in a grid cell: NULL renders as the literal `NULL`
    /// (matching [`Value::display`]); every other cell renders its stored text.
    /// Borrowed — no allocation.
    pub fn display(&self) -> &'a str {
        if self.is_null() { "NULL" } else { self.text }
    }

    /// The raw stored text — empty for NULL, the canonical value text otherwise.
    /// Used where NULL must render blank rather than as `NULL` (CSV/JSON/HTML).
    pub fn text(&self) -> &'a str {
        self.text
    }

    /// Reconstruct the owned, typed [`Value`] by parsing the stored text per the
    /// tag. A numeric cell whose text unexpectedly fails to parse degrades to a
    /// `Str` (defensive — the tag is set from a real parsed value at build time).
    pub fn to_value(&self) -> Value {
        match self.tag {
            CellTag::Null => Value::Null,
            CellTag::Int => self
                .text
                .parse()
                .map(Value::Int)
                .unwrap_or_else(|_| Value::Str(self.text.to_string())),
            CellTag::UInt => self
                .text
                .parse()
                .map(Value::UInt)
                .unwrap_or_else(|_| Value::Str(self.text.to_string())),
            CellTag::Float => self
                .text
                .parse()
                .map(Value::Float)
                .unwrap_or_else(|_| Value::Str(self.text.to_string())),
            CellTag::Str => Value::Str(self.text.to_string()),
        }
    }
}

/// Where a result column really came from — the MySQL wire protocol reports
/// this per column, even through aliases and joins. `None` (see [`Column`])
/// means the column has no single base column (an expression, aggregate, or
/// literal) and so cannot be edited.
#[derive(Clone, Debug)]
pub struct ColumnOrigin {
    /// Real schema (database) the column belongs to.
    pub database: String,
    /// PostgreSQL namespace the table lives in (`public`, `sales`, …), from the
    /// prepared column's `table_oid`. `None` on MySQL, which has no level between
    /// database and table. Part of the table's identity: without it, same-named
    /// tables in two schemas collapse into one and an `UPDATE` could hit the
    /// wrong one.
    pub schema: Option<String>,
    /// Real table (`org_table`), not the query alias.
    pub table: String,
    /// Real column name (`org_name`), not the query alias.
    pub column: String,
    /// Key/nullability flags carried on the column definition.
    pub flags: ColumnFlags,
    /// Raw-bytes column (BLOB/BINARY/VARBINARY/BIT — binary charset). Such a
    /// value can't round-trip through the text protocol without loss, so the
    /// editing system treats it as read-only and refuses it as a WHERE key.
    pub binary: bool,
}

/// Per-column key/nullability flags from the wire column definition. Used by the
/// editing system to decide updatability and build a safe `WHERE`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ColumnFlags {
    pub primary_key: bool,
    pub unique_key: bool,
    pub not_null: bool,
    pub auto_increment: bool,
    /// The column has **no** default value (MySQL `NO_DEFAULT_VALUE_FLAG`): a new
    /// row must supply it, or the `INSERT` errors ("Field 'x' doesn't have a
    /// default value"). Nullable columns have an implicit `NULL` default, so this
    /// is only set for NOT-NULL, non-auto-increment columns without a `DEFAULT`.
    pub no_default: bool,
}

/// Column metadata from a result.
#[derive(Clone, Debug)]
pub struct Column {
    pub name: String,
    /// SQL type name as reported by the driver (e.g. `VARCHAR`, `INT`).
    pub type_name: String,
    /// Provenance: the real base column this maps to, if any. `None` for
    /// expressions/aggregates/literals (not editable).
    pub origin: Option<ColumnOrigin>,
}

impl Column {
    /// Coarse heuristic: is this a numeric column? (Used later for right
    /// alignment; kept simple for M1.)
    pub fn is_numeric(&self) -> bool {
        let t = self.type_name.to_ascii_uppercase();
        [
            "TINYINT",
            "SMALLINT",
            "MEDIUMINT",
            "INT",
            "BIGINT",
            "DECIMAL",
            "NUMERIC",
            "FLOAT",
            "DOUBLE",
            "YEAR",
            "BIT",
        ]
        .iter()
        .any(|k| t.contains(k))
    }
}

/// Width of the offset field in a packed cell word; the remaining top 3 bits
/// carry the [`CellTag`]. 29 bits caps a column's arena at 512 MiB of text.
const OFFSET_BITS: u32 = 29;
/// Mask for the offset field of a packed cell word.
const OFFSET_MASK: u32 = (1 << OFFSET_BITS) - 1;
/// Max text bytes per column arena (the largest representable offset).
const MAX_ARENA: usize = OFFSET_MASK as usize;

/// One column's cells in the compact **columnar** layout: a single `u32` per
/// cell (its [`CellTag`] packed into the top 3 bits, its text's end offset into
/// the arena in the low 29 bits) plus one `arena` holding every cell's canonical
/// text back-to-back. Cell `i` spans `arena[start..end]`, where `end` is the low
/// 29 bits of `ends[i]` and `start` those of `ends[i - 1]` (or `0` for the first
/// cell); a NULL cell has an empty span and a `Null` tag, and a numeric cell
/// stores its canonical display text — so a cell's rendered text is a zero-copy
/// slice of the arena.
///
/// This replaces the old row-major `Vec<Value>` (a 32-byte `Value` per cell plus
/// a heap allocation per string): **4 bytes** of fixed overhead per cell plus the
/// text bytes, and one arena allocation per column instead of one per string.
#[derive(Clone, Debug, Default)]
struct ColumnData {
    /// Per cell: `(tag << OFFSET_BITS) | end_offset`. The tag rides in the top 3
    /// bits so there's no separate tag byte; the offset (low 29 bits) reaches
    /// [`MAX_ARENA`], unreachable within the 200k-row cap for any real cell.
    ends: Vec<u32>,
    arena: String,
}

impl ColumnData {
    fn with_capacity(rows: usize) -> Self {
        ColumnData {
            ends: Vec::with_capacity(rows),
            arena: String::new(),
        }
    }

    /// Finalize the cell whose text has just been appended to `arena`, recording
    /// its tag + end offset as one packed word. If the arena ever exceeds the
    /// 29-bit ceiling (a single 512 MiB column — unreachable within the row cap),
    /// it's truncated at a char boundary so an offset can never collide with the
    /// tag bits; graceful capping, never corruption.
    fn finish_cell(&mut self, tag: CellTag) {
        if self.arena.len() > MAX_ARENA {
            let mut cut = MAX_ARENA;
            while !self.arena.is_char_boundary(cut) {
                cut -= 1;
            }
            self.arena.truncate(cut);
        }
        let end = self.arena.len() as u32;
        self.ends.push((tag.to_bits() << OFFSET_BITS) | end);
    }

    /// Append one value: write its canonical text into the arena (nothing for
    /// NULL) and record its tag. Numerics are written via `Display` — identical
    /// to [`Value::display`] — so the stored text is exactly what the grid shows.
    fn push(&mut self, v: &Value) {
        use std::fmt::Write as _;
        let tag = match v {
            Value::Null => CellTag::Null,
            Value::Int(n) => {
                let _ = write!(self.arena, "{n}");
                CellTag::Int
            }
            Value::UInt(n) => {
                let _ = write!(self.arena, "{n}");
                CellTag::UInt
            }
            Value::Float(f) => {
                let _ = write!(self.arena, "{f}");
                CellTag::Float
            }
            Value::Str(s) => {
                self.arena.push_str(s);
                CellTag::Str
            }
        };
        self.finish_cell(tag);
    }

    /// Append a borrowed cell verbatim (tag + text) — copies a cell from one
    /// column buffer into another (used when rebuilding a column on splice).
    fn push_ref(&mut self, c: CellRef<'_>) {
        self.arena.push_str(c.text);
        self.finish_cell(c.tag);
    }

    fn cell(&self, row: usize) -> Option<CellRef<'_>> {
        let packed = *self.ends.get(row)?;
        let tag = CellTag::from_bits(packed >> OFFSET_BITS);
        let end = (packed & OFFSET_MASK) as usize;
        let start = if row == 0 {
            0
        } else {
            (self.ends[row - 1] & OFFSET_MASK) as usize
        };
        Some(CellRef {
            tag,
            text: &self.arena[start..end],
        })
    }
}

/// A fully materialized result set, stored **columnar** (one [`ColumnData`] per
/// column) rather than row-major. `schemaic_db::Db` loads rows into memory up to
/// a caller-supplied row cap (`truncated` flags when more exist) via
/// [`ResultBuilder`]; true streaming is a future change.
///
/// Cells are read through [`ResultSet::cell`] (a borrowed [`CellRef`]); the field
/// is private so the columnar invariant (each column's packed `ends` words stay
/// in lock-step with its `arena`) can't be violated from outside.
#[derive(Clone, Debug, Default)]
pub struct ResultSet {
    pub columns: Vec<Column>,
    cols: Vec<ColumnData>,
    n_rows: usize,
    pub elapsed_ms: u128,
    /// True if the fetch stopped at the row cap (more rows may exist).
    pub truncated: bool,
    /// For a statement that returns no result set (UPDATE/INSERT/DELETE/DDL),
    /// the number of rows the server reports affected. `None` for a row-
    /// returning result (a SELECT grid), so the UI can tell the two apart.
    pub affected: Option<u64>,
}

impl ResultSet {
    /// Build a row-returning result from row-major data (columns + rows). Each
    /// inner `Vec<Value>` is one row in column order; a row shorter than
    /// `columns` is padded with NULL and extra cells are ignored. Used by tests
    /// and the splice/refetch paths — the DB loader uses [`ResultBuilder`]
    /// directly to avoid ever holding a row-major copy.
    pub fn from_rows(columns: Vec<Column>, rows: Vec<Vec<Value>>) -> Self {
        let mut b = ResultBuilder::with_capacity(columns, rows.len());
        for row in &rows {
            b.push_row(row);
        }
        b.finish()
    }

    /// Build a no-result-set outcome (UPDATE/INSERT/DELETE/DDL): the server's
    /// reported affected-row count, no grid.
    pub fn affected_rows(columns: Vec<Column>, n: u64) -> Self {
        ResultSet {
            columns,
            affected: Some(n),
            ..Default::default()
        }
    }

    /// Builder-style setter for the query's elapsed time.
    pub fn with_elapsed(mut self, ms: u128) -> Self {
        self.elapsed_ms = ms;
        self
    }

    /// Builder-style setter for the truncated (hit-the-row-cap) flag.
    pub fn with_truncated(mut self, truncated: bool) -> Self {
        self.truncated = truncated;
        self
    }

    pub fn row_count(&self) -> usize {
        self.n_rows
    }
    pub fn col_count(&self) -> usize {
        self.columns.len()
    }

    /// Borrowed view of the cell at `(row, col)` in **data** (unsorted) order, or
    /// `None` if either index is out of range.
    pub fn cell(&self, row: usize, col: usize) -> Option<CellRef<'_>> {
        self.cols.get(col)?.cell(row)
    }

    /// Replace whole data rows in place — `(data_row, new cells)` with cells
    /// aligned to the columns — rebuilding each column buffer with the
    /// substitutions applied. Used by the grid's in-place edit splice (post-commit
    /// re-fetch), so scroll/selection survive without a full query re-run. Rows
    /// not listed keep their existing cells; a replacement shorter than `columns`
    /// leaves the missing columns' cells unchanged.
    pub fn splice_rows(&mut self, rows: &[(usize, Vec<Value>)]) {
        if rows.is_empty() {
            return;
        }
        let repl: std::collections::HashMap<usize, &[Value]> =
            rows.iter().map(|(di, v)| (*di, v.as_slice())).collect();
        let n = self.n_rows;
        for (ci, cd) in self.cols.iter_mut().enumerate() {
            let mut nb = ColumnData::with_capacity(n);
            for r in 0..n {
                match repl.get(&r).and_then(|cells| cells.get(ci)) {
                    Some(v) => nb.push(v),
                    None => match cd.cell(r) {
                        Some(c) => nb.push_ref(c),
                        None => nb.push(&Value::Null),
                    },
                }
            }
            *cd = nb;
        }
    }
}

/// Assembles a columnar [`ResultSet`] one row at a time, so a large result never
/// exists as a row-major `Vec<Vec<Value>>` in memory: the DB loader converts each
/// wire row and pushes it straight into the per-column buffers.
pub struct ResultBuilder {
    columns: Vec<Column>,
    cols: Vec<ColumnData>,
    n_rows: usize,
    elapsed_ms: u128,
    truncated: bool,
}

impl ResultBuilder {
    pub fn new(columns: Vec<Column>) -> Self {
        Self::with_capacity(columns, 0)
    }

    pub fn with_capacity(columns: Vec<Column>, rows: usize) -> Self {
        let cols = (0..columns.len())
            .map(|_| ColumnData::with_capacity(rows))
            .collect();
        ResultBuilder {
            columns,
            cols,
            n_rows: 0,
            elapsed_ms: 0,
            truncated: false,
        }
    }

    /// The columns being built — the loader passes these to `convert_row`.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// Rows pushed so far (the loader compares this against the row cap).
    pub fn row_count(&self) -> usize {
        self.n_rows
    }

    /// Append one row of cells (column order). A row shorter than the column
    /// count is padded with NULL; extra cells are ignored.
    pub fn push_row(&mut self, cells: &[Value]) {
        for (ci, cd) in self.cols.iter_mut().enumerate() {
            match cells.get(ci) {
                Some(v) => cd.push(v),
                None => cd.push(&Value::Null),
            }
        }
        self.n_rows += 1;
    }

    pub fn set_elapsed(&mut self, ms: u128) {
        self.elapsed_ms = ms;
    }

    pub fn set_truncated(&mut self, truncated: bool) {
        self.truncated = truncated;
    }

    pub fn finish(self) -> ResultSet {
        ResultSet {
            columns: self.columns,
            cols: self.cols,
            n_rows: self.n_rows,
            elapsed_ms: self.elapsed_ms,
            truncated: self.truncated,
            affected: None,
        }
    }
}

/// One row's staged edits, ready to execute as a single `UPDATE`. Built by the
/// grid's editing system from the result's per-column provenance: `database` /
/// `table` are the real base table, `set` are the columns to change (new text,
/// bound as a parameter), and `key` is the WHERE identity (columns + their
/// *original* typed values). The executor runs each of these in one transaction
/// and requires every statement to affect exactly one row.
#[derive(Clone, Debug)]
pub struct RowEdit {
    pub database: String,
    /// PostgreSQL namespace of `table` (`None` on MySQL). The executor qualifies
    /// with it **unconditionally** — this statement is never shown to the user,
    /// so it must not depend on `search_path`.
    pub schema: Option<String>,
    pub table: String,
    /// Columns to set → new value. `Some(text)` is bound as a string param (the
    /// server coerces to the column type); `None` sets SQL `NULL`.
    pub set: Vec<(String, Option<String>)>,
    /// WHERE identity: key columns → their original typed values.
    pub key: Vec<(String, Value)>,
}

/// One new row staged for `INSERT`. Built by the grid from the result's single
/// base table: `database` / `table` are that table, and `cols` are the columns
/// the user set → value (`Some(text)` bound as a string param; `None` = SQL
/// `NULL`). Columns *omitted* from `cols` take their DB default (auto-increment,
/// `DEFAULT`, or `NULL`). The executor runs each in the same transaction as the
/// updates and requires it to affect exactly one row.
#[derive(Clone, Debug)]
pub struct RowInsert {
    pub database: String,
    /// PostgreSQL namespace of `table` — see [`RowEdit::schema`].
    pub schema: Option<String>,
    pub table: String,
    pub cols: Vec<(String, Option<String>)>,
}

/// One row staged for `DELETE`, identified by its WHERE key (columns + their
/// original typed values) — the same row-identity model as [`RowEdit::key`]. The
/// executor runs it in the shared transaction and requires it to affect exactly
/// one row.
#[derive(Clone, Debug)]
pub struct RowDelete {
    pub database: String,
    /// PostgreSQL namespace of `table` — see [`RowEdit::schema`].
    pub schema: Option<String>,
    pub table: String,
    pub key: Vec<(String, Value)>,
}

/// A batch of staged grid mutations committed together in one transaction:
/// cell-edit `UPDATE`s, new-row `INSERT`s, and row `DELETE`s.
#[derive(Clone, Debug, Default)]
pub struct GridWrite {
    pub updates: Vec<RowEdit>,
    pub inserts: Vec<RowInsert>,
    pub deletes: Vec<RowDelete>,
}

impl GridWrite {
    /// No staged changes at all.
    pub fn is_empty(&self) -> bool {
        self.updates.is_empty() && self.inserts.is_empty() && self.deletes.is_empty()
    }
}

/// A template for re-`SELECT`ing just-edited rows so the grid can splice DB
/// truth back in without re-running the whole query (built by
/// [`crate::edit::refetch_template`]). Only produced when the result is a single
/// base table with every column having a real origin, so `SELECT <real cols> …`
/// reproduces the row 1:1.
#[derive(Clone, Debug)]
pub struct RefetchTemplate {
    pub database: String,
    /// PostgreSQL namespace of `table` — see [`RowEdit::schema`].
    pub schema: Option<String>,
    pub table: String,
    /// Real column name for every result column, in result-column order.
    pub columns: Vec<String>,
    /// Indices into `columns` forming the row-identity `WHERE` key.
    pub key_cols: Vec<usize>,
}

/// One row to re-fetch: the grid data-row to splice back into, plus that row's
/// *post-edit* key values (aligned to [`RefetchTemplate::key_cols`]).
#[derive(Clone, Debug)]
pub struct RefetchRow {
    pub data_row: usize,
    pub key: Vec<Value>,
}

/// A full re-fetch request handed to the commit path alongside the edits.
#[derive(Clone, Debug)]
pub struct RefetchRequest {
    pub template: RefetchTemplate,
    pub rows: Vec<RefetchRow>,
}

/// Outcome of a commit, delivered back to the grid on the UI thread.
#[derive(Clone, Debug)]
pub enum CommitDone {
    /// Splice these fresh rows in place — `(data_row, new cell values)`, the
    /// values aligned to the result columns. The grid overwrites those rows and
    /// clears its staged edits, preserving scroll/selection.
    Spliced(Vec<(usize, Vec<Value>)>),
    /// The whole query was re-run instead (not spliceable, or the re-fetch
    /// failed) — the grid is being rebuilt from fresh results, so it does nothing.
    FullReran,
    /// The commit failed; the message is shown and the staged edits are kept.
    Failed(String),
}

/// UI-facing lifecycle of a query in a tab. Shared between the app (writer)
/// and the UI (reader) through a Floem signal.
#[derive(Clone, Debug)]
pub enum QueryState {
    /// No query has run in this tab yet.
    Idle,
    Running,
    Loaded(Arc<ResultSet>),
    Failed(String),
    /// The query was cancelled by the user.
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(type_name: &str) -> Column {
        Column {
            name: "c".to_string(),
            type_name: type_name.to_string(),
            origin: None,
        }
    }

    #[test]
    fn value_is_null_only_for_null() {
        assert!(Value::Null.is_null());
        assert!(!Value::Int(0).is_null());
        assert!(!Value::UInt(0).is_null());
        assert!(!Value::Float(0.0).is_null());
        assert!(!Value::Str(String::new()).is_null());
    }

    #[test]
    fn value_display_covers_every_variant() {
        assert_eq!(Value::Null.display(), "NULL");
        assert_eq!(Value::Int(-42).display(), "-42");
        assert_eq!(Value::UInt(42).display(), "42");
        assert_eq!(Value::Str("hi".to_string()).display(), "hi");
        // Float uses f64::to_string — integral floats print without a fraction.
        assert_eq!(Value::Float(1.5).display(), "1.5");
        assert_eq!(Value::Float(2.0).display(), "2");
    }

    #[test]
    fn is_numeric_matches_numeric_types_case_insensitively() {
        for t in [
            "INT",
            "int",
            "BIGINT",
            "tinyint(1)",
            "DECIMAL(10,2)",
            "numeric",
            "FLOAT",
            "double",
            "YEAR",
            "BIT",
            "MEDIUMINT UNSIGNED",
        ] {
            assert!(col(t).is_numeric(), "{t} should be numeric");
        }
    }

    #[test]
    fn is_numeric_rejects_non_numeric_types() {
        for t in [
            "VARCHAR(255)",
            "TEXT",
            "DATETIME",
            "JSON",
            "BLOB",
            "ENUM('a')",
        ] {
            assert!(!col(t).is_numeric(), "{t} should not be numeric");
        }
    }

    #[test]
    fn gridwrite_is_empty_tracks_all_three_buckets() {
        let mut w = GridWrite::default();
        assert!(w.is_empty());
        w.updates.push(RowEdit {
            database: "d".to_string(),
            schema: None,
            table: "t".to_string(),
            set: vec![],
            key: vec![],
        });
        assert!(!w.is_empty());

        let mut w = GridWrite::default();
        w.inserts.push(RowInsert {
            database: "d".to_string(),
            schema: None,
            table: "t".to_string(),
            cols: vec![],
        });
        assert!(!w.is_empty());

        let mut w = GridWrite::default();
        w.deletes.push(RowDelete {
            database: "d".to_string(),
            schema: None,
            table: "t".to_string(),
            key: vec![],
        });
        assert!(!w.is_empty());
    }

    #[test]
    fn resultset_counts_reflect_dimensions() {
        let rs = ResultSet::from_rows(
            vec![col("INT"), col("TEXT")],
            vec![
                vec![Value::Int(1), Value::Str("a".to_string())],
                vec![Value::Int(2), Value::Str("b".to_string())],
                vec![Value::Int(3), Value::Str("c".to_string())],
            ],
        );
        assert_eq!(rs.row_count(), 3);
        assert_eq!(rs.col_count(), 2);

        let empty = ResultSet::default();
        assert_eq!(empty.row_count(), 0);
        assert_eq!(empty.col_count(), 0);
    }

    #[test]
    fn columnar_cell_roundtrips_every_variant() {
        let rs = ResultSet::from_rows(
            vec![col("INT"), col("BIGINT"), col("DOUBLE"), col("TEXT")],
            vec![
                vec![
                    Value::Int(-42),
                    Value::UInt(18_446_744_073_709_551_615),
                    Value::Float(1.5),
                    Value::Str("héllo".to_string()),
                ],
                vec![Value::Null, Value::Null, Value::Null, Value::Null],
            ],
        );
        // Tags survive; text is the canonical display; to_value round-trips.
        let c = rs.cell(0, 0).unwrap();
        assert_eq!(c.tag, CellTag::Int);
        assert_eq!(c.display(), "-42");
        assert!(matches!(c.to_value(), Value::Int(-42)));

        let c = rs.cell(0, 1).unwrap();
        assert_eq!(c.tag, CellTag::UInt);
        assert!(matches!(
            c.to_value(),
            Value::UInt(18_446_744_073_709_551_615)
        ));

        let c = rs.cell(0, 2).unwrap();
        assert_eq!(c.tag, CellTag::Float);
        assert!(matches!(c.to_value(), Value::Float(f) if f == 1.5));

        // Multi-byte UTF-8 slices at the right arena boundary.
        let c = rs.cell(0, 3).unwrap();
        assert_eq!(c.tag, CellTag::Str);
        assert_eq!(c.display(), "héllo");
        assert!(matches!(c.to_value(), Value::Str(s) if s == "héllo"));

        // NULL: dim `NULL` for display, empty raw text, `Value::Null` typed.
        let c = rs.cell(1, 0).unwrap();
        assert!(c.is_null());
        assert_eq!(c.display(), "NULL");
        assert_eq!(c.text(), "");
        assert!(c.to_value().is_null());
    }

    #[test]
    fn packed_tag_does_not_corrupt_offsets() {
        // Many cells of differing lengths: the tag packed into the high bits must
        // not disturb the low-29-bit offsets, so every cell slices back exactly —
        // including across a tag change (Str → Int) mid-column.
        let rs = ResultSet::from_rows(
            vec![col("MIXED")],
            vec![
                vec![Value::Str("a".to_string())],
                vec![Value::Str("bbbbbbbbbb".to_string())],
                vec![Value::Null],
                vec![Value::Str(String::new())],
                vec![Value::Int(1234567890)],
                vec![Value::Str("tail".to_string())],
            ],
        );
        assert_eq!(rs.cell(0, 0).unwrap().display(), "a");
        assert_eq!(rs.cell(1, 0).unwrap().display(), "bbbbbbbbbb");
        assert!(rs.cell(2, 0).unwrap().is_null());
        assert_eq!(rs.cell(3, 0).unwrap().display(), "");
        assert!(!rs.cell(3, 0).unwrap().is_null());
        assert_eq!(rs.cell(4, 0).unwrap().display(), "1234567890");
        assert_eq!(rs.cell(4, 0).unwrap().tag, CellTag::Int);
        assert_eq!(rs.cell(5, 0).unwrap().display(), "tail");
    }

    #[test]
    fn columnar_cell_out_of_range_is_none() {
        let rs = ResultSet::from_rows(vec![col("INT")], vec![vec![Value::Int(1)]]);
        assert!(rs.cell(0, 0).is_some());
        assert!(rs.cell(1, 0).is_none()); // row past end
        assert!(rs.cell(0, 1).is_none()); // col past end
    }

    #[test]
    fn empty_string_cell_is_distinct_from_null() {
        // Both have an empty arena span; the tag keeps them apart.
        let rs = ResultSet::from_rows(
            vec![col("TEXT"), col("TEXT")],
            vec![vec![Value::Str(String::new()), Value::Null]],
        );
        let empty = rs.cell(0, 0).unwrap();
        assert!(!empty.is_null());
        assert_eq!(empty.display(), "");
        let null = rs.cell(0, 1).unwrap();
        assert!(null.is_null());
        assert_eq!(null.display(), "NULL");
    }

    #[test]
    fn short_row_is_padded_with_null() {
        let rs = ResultSet::from_rows(
            vec![col("INT"), col("TEXT")],
            vec![vec![Value::Int(7)]], // only one cell for a two-column result
        );
        assert!(matches!(rs.cell(0, 0).unwrap().to_value(), Value::Int(7)));
        assert!(rs.cell(0, 1).unwrap().is_null());
    }

    #[test]
    fn splice_rows_replaces_only_listed_rows() {
        let mut rs = ResultSet::from_rows(
            vec![col("INT"), col("TEXT")],
            vec![
                vec![Value::Int(1), Value::Str("a".to_string())],
                vec![Value::Int(2), Value::Str("b".to_string())],
                vec![Value::Int(3), Value::Str("c".to_string())],
            ],
        );
        rs.splice_rows(&[(1, vec![Value::Int(20), Value::Str("B".to_string())])]);
        // Row 1 replaced; rows 0 and 2 untouched.
        assert_eq!(rs.cell(0, 1).unwrap().display(), "a");
        assert!(matches!(rs.cell(1, 0).unwrap().to_value(), Value::Int(20)));
        assert_eq!(rs.cell(1, 1).unwrap().display(), "B");
        assert_eq!(rs.cell(2, 1).unwrap().display(), "c");
        assert_eq!(rs.row_count(), 3);
    }

    #[test]
    fn builder_matches_from_rows_and_carries_flags() {
        let mut b = ResultBuilder::new(vec![col("INT")]);
        assert_eq!(b.row_count(), 0);
        assert_eq!(b.columns().len(), 1);
        b.push_row(&[Value::Int(1)]);
        b.push_row(&[Value::Null]);
        b.set_elapsed(12);
        b.set_truncated(true);
        let rs = b.finish();
        assert_eq!(rs.row_count(), 2);
        assert_eq!(rs.elapsed_ms, 12);
        assert!(rs.truncated);
        assert!(rs.affected.is_none());
    }
}
