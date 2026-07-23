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

/// Where a result column really came from — the MySQL wire protocol reports
/// this per column, even through aliases and joins. `None` (see [`Column`])
/// means the column has no single base column (an expression, aggregate, or
/// literal) and so cannot be edited.
#[derive(Clone, Debug)]
pub struct ColumnOrigin {
    /// Real schema (database) the column belongs to.
    pub database: String,
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

/// A fully materialized result set. `schemaic_db::Db` loads rows into memory up
/// to a caller-supplied row cap (`truncated` flags when more exist); true
/// streaming is a future change.
#[derive(Clone, Debug, Default)]
pub struct ResultSet {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Value>>,
    pub elapsed_ms: u128,
    /// True if the fetch stopped at the row cap (more rows may exist).
    pub truncated: bool,
    /// For a statement that returns no result set (UPDATE/INSERT/DELETE/DDL),
    /// the number of rows the server reports affected. `None` for a row-
    /// returning result (a SELECT grid), so the UI can tell the two apart.
    pub affected: Option<u64>,
}

impl ResultSet {
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    pub fn col_count(&self) -> usize {
        self.columns.len()
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
            table: "t".to_string(),
            set: vec![],
            key: vec![],
        });
        assert!(!w.is_empty());

        let mut w = GridWrite::default();
        w.inserts.push(RowInsert {
            database: "d".to_string(),
            table: "t".to_string(),
            cols: vec![],
        });
        assert!(!w.is_empty());

        let mut w = GridWrite::default();
        w.deletes.push(RowDelete {
            database: "d".to_string(),
            table: "t".to_string(),
            key: vec![],
        });
        assert!(!w.is_empty());
    }

    #[test]
    fn resultset_counts_reflect_dimensions() {
        let rs = ResultSet {
            columns: vec![col("INT"), col("TEXT")],
            rows: vec![
                vec![Value::Int(1), Value::Str("a".to_string())],
                vec![Value::Int(2), Value::Str("b".to_string())],
                vec![Value::Int(3), Value::Str("c".to_string())],
            ],
            ..Default::default()
        };
        assert_eq!(rs.row_count(), 3);
        assert_eq!(rs.col_count(), 2);

        let empty = ResultSet::default();
        assert_eq!(empty.row_count(), 0);
        assert_eq!(empty.col_count(), 0);
    }
}
