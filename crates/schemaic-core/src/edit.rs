//! Result-set editability analysis — pure over [`ResultSet`] + schema, no UI.
//!
//! From each column's wire provenance (real table/column + key flags, see
//! [`crate::model::ColumnOrigin`]) this decides which columns can be written
//! back and, per base table, which result columns reconstruct a row's `WHERE`
//! key. It is deliberately conservative: anything it can't identify uniquely
//! and safely is read-only. This is the most safety-critical logic in the app
//! (a wrong key misdirects an UPDATE), so it lives here with tests rather than
//! welded to Floem signals in the UI.

use crate::model::{RefetchTemplate, ResultSet};
use crate::schema::TableInfo;

/// A base table the result can write back to, plus the result-column indices
/// whose (original) values form the row-identity `WHERE`.
#[derive(Clone, Debug)]
pub struct EditTable {
    pub database: String,
    pub table: String,
    pub key_cols: Vec<usize>,
}

/// Which result columns are editable, and to which base table each writes.
/// `col_table[ci]` is the index into `tables` for column `ci`, or `None` if the
/// column is read-only (an expression/aggregate, a binary column, or one whose
/// table has no reconstructible row key).
#[derive(Default, Debug)]
pub struct EditModel {
    col_table: Vec<Option<usize>>,
    tables: Vec<EditTable>,
}

impl EditModel {
    /// Can result column `ci` be edited?
    pub fn editable(&self, ci: usize) -> bool {
        self.col_table.get(ci).copied().flatten().is_some()
    }

    /// The `tables` index that column `ci` writes to, if editable.
    pub fn table_index(&self, ci: usize) -> Option<usize> {
        self.col_table.get(ci).copied().flatten()
    }

    /// The base table at `tables` index `idx`.
    pub fn table(&self, idx: usize) -> Option<&EditTable> {
        self.tables.get(idx)
    }

    /// The sole base table an `INSERT` would target, if the result maps to exactly
    /// one writable table (the destination for a new row). `None` for a
    /// multi-table join or a non-editable / read-only result.
    pub fn insert_target(&self) -> Option<&EditTable> {
        match self.tables.as_slice() {
            [only] => Some(only),
            _ => None,
        }
    }
}

/// If every result column has a real origin from a *single* base table (so the
/// whole row can be re-`SELECT`ed by real column name), return the template for
/// re-fetching edited rows after a commit. `None` — an expression/aggregate
/// column, a join across two writable tables, or no usable key — means the
/// caller should re-run the whole query instead of splicing.
///
/// Requires `model` to have been computed from `rs` (it reads the model's single
/// table + its resolved WHERE key).
pub fn refetch_template(rs: &ResultSet, model: &EditModel) -> Option<RefetchTemplate> {
    // Exactly one writable base table (with a resolved key), else not spliceable.
    if model.tables.len() != 1 {
        return None;
    }
    let tbl = &model.tables[0];
    // Every result column must originate from that one table — no expression /
    // second-table columns — so `SELECT <real cols>` reproduces the row 1:1.
    let mut columns = Vec::with_capacity(rs.columns.len());
    for col in &rs.columns {
        let o = col.origin.as_ref()?;
        if o.database != tbl.database || o.table != tbl.table {
            return None;
        }
        columns.push(o.column.clone());
    }
    Some(RefetchTemplate {
        database: tbl.database.clone(),
        table: tbl.table.clone(),
        columns,
        key_cols: tbl.key_cols.clone(),
    })
}

/// Compute the [`EditModel`]. `schema_for(database, table)` returns the loaded
/// schema for a base table (or `None` if unknown) — the UI supplies a closure
/// that reads its schema signals; tests supply a plain map.
pub fn analyze_edit(
    rs: &ResultSet,
    schema_for: impl Fn(&str, &str) -> Option<TableInfo>,
) -> EditModel {
    let ncols = rs.columns.len();
    let mut col_table: Vec<Option<usize>> = vec![None; ncols];
    let mut tables: Vec<EditTable> = Vec::new();

    // Distinct (database, table) in first-seen order → its result columns.
    let mut groups: Vec<((String, String), Vec<usize>)> = Vec::new();
    for (ci, col) in rs.columns.iter().enumerate() {
        let Some(o) = &col.origin else { continue };
        let key = (o.database.clone(), o.table.clone());
        if let Some(g) = groups.iter_mut().find(|(k, _)| *k == key) {
            g.1.push(ci);
        } else {
            groups.push((key, vec![ci]));
        }
    }

    for ((db, table), cis) in &groups {
        if let Some(key_cols) = resolve_key(&schema_for, db, table, cis, rs) {
            let idx = tables.len();
            tables.push(EditTable {
                database: db.clone(),
                table: table.clone(),
                key_cols,
            });
            for &ci in cis {
                // C2: binary columns can't round-trip as text → never editable,
                // even when their table has a usable key.
                let binary = rs.columns[ci]
                    .origin
                    .as_ref()
                    .map(|o| o.binary)
                    .unwrap_or(false);
                if !binary {
                    col_table[ci] = Some(idx);
                }
            }
        }
    }
    EditModel { col_table, tables }
}

/// Find the result-column indices forming a usable row key for one base table,
/// or `None` if the table's rows can't be identified safely (read-only).
fn resolve_key(
    schema_for: &impl Fn(&str, &str) -> Option<TableInfo>,
    db: &str,
    table: &str,
    cis: &[usize],
    rs: &ResultSet,
) -> Option<Vec<usize>> {
    // C1: if the same base column is exposed more than once for this table (a
    // self-join collapsing two aliases, or `id, id AS id2`), an edit can't be
    // attributed to one row — refuse the whole table.
    let mut seen = std::collections::HashSet::new();
    for &ci in cis {
        if let Some(o) = rs.columns[ci].origin.as_ref()
            && !seen.insert(o.column.clone())
        {
            return None;
        }
    }

    // Map a real column name → the result column of THIS table exposing it.
    let col_ci = |name: &str| -> Option<usize> {
        cis.iter()
            .copied()
            .find(|&ci| rs.columns[ci].origin.as_ref().map(|o| o.column.as_str()) == Some(name))
    };
    // All names present as result columns of this table → their indices.
    let all_present =
        |names: &[String]| -> Option<Vec<usize>> { names.iter().map(|n| col_ci(n)).collect() };

    let candidate: Option<Vec<usize>> = if let Some(t) = schema_for(db, table) {
        // Primary key, if it's fully present in the result.
        let pk: Vec<String> = t
            .columns
            .iter()
            .filter(|c| c.primary_key)
            .map(|c| c.name.clone())
            .collect();
        if !pk.is_empty() && all_present(&pk).is_some() {
            all_present(&pk)
        } else {
            // Else a unique, non-foreign index whose columns are all present and
            // all NOT NULL (so it uniquely identifies a row).
            t.indexes
                .iter()
                .filter(|ix| ix.unique && !ix.foreign)
                .filter(|ix| {
                    ix.columns.iter().all(|c| {
                        t.columns
                            .iter()
                            .find(|tc| &tc.name == c)
                            .map(|tc| !tc.nullable)
                            .unwrap_or(false)
                    })
                })
                .find_map(|ix| all_present(&ix.columns))
        }
    } else {
        // No schema loaded: trust the wire PK flags on the returned columns.
        let flagged: Vec<usize> = cis
            .iter()
            .copied()
            .filter(|&ci| {
                rs.columns[ci]
                    .origin
                    .as_ref()
                    .map(|o| o.flags.primary_key)
                    .unwrap_or(false)
            })
            .collect();
        (!flagged.is_empty()).then_some(flagged)
    };

    let key = candidate?;
    // C2/C4: a binary or floating-point key column can't be matched reliably in
    // a WHERE (lossy bytes / FLOAT↔DOUBLE precision), so the table is read-only.
    for &kci in &key {
        if rs.columns[kci]
            .origin
            .as_ref()
            .map(|o| o.binary)
            .unwrap_or(false)
        {
            return None;
        }
        let ty = rs.columns[kci].type_name.to_ascii_uppercase();
        if ty.starts_with("FLOAT") || ty.starts_with("DOUBLE") {
            return None;
        }
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, ColumnFlags, ColumnOrigin};
    use crate::schema::ColumnInfo;

    /// A result column with a base-table origin.
    fn col(name: &str, ty: &str, table: &str, pk: bool, binary: bool) -> Column {
        Column {
            name: name.to_string(),
            type_name: ty.to_string(),
            origin: Some(ColumnOrigin {
                database: "db".to_string(),
                table: table.to_string(),
                column: name.to_string(),
                flags: ColumnFlags {
                    primary_key: pk,
                    not_null: pk,
                    ..Default::default()
                },
                binary,
            }),
        }
    }

    fn rs(columns: Vec<Column>) -> ResultSet {
        ResultSet {
            columns,
            rows: Vec::new(),
            elapsed_ms: 0,
            truncated: false,
            affected: None,
        }
    }

    /// Schema table with the given primary-key column names (INT, NOT NULL).
    fn schema_with_pk(table: &str, pk: &[&str], cols: &[(&str, &str)]) -> TableInfo {
        TableInfo {
            name: table.to_string(),
            columns: cols
                .iter()
                .map(|(n, ty)| ColumnInfo {
                    name: n.to_string(),
                    type_name: ty.to_string(),
                    nullable: !pk.contains(n),
                    primary_key: pk.contains(n),
                })
                .collect(),
            indexes: Vec::new(),
            is_view: false,
            view_definition: None,
        }
    }

    #[test]
    fn happy_path_int_pk_is_editable() {
        let r = rs(vec![
            col("id", "INT", "users", true, false),
            col("name", "VARCHAR", "users", false, false),
        ]);
        let schema = |_db: &str, t: &str| {
            (t == "users")
                .then(|| schema_with_pk("users", &["id"], &[("id", "int"), ("name", "varchar")]))
        };
        let m = analyze_edit(&r, schema);
        assert!(m.editable(0));
        assert!(m.editable(1));
    }

    #[test]
    fn c1_self_join_duplicate_column_is_readonly() {
        // Two aliases of `users` both expose `id` + `name` → ambiguous identity.
        let r = rs(vec![
            col("id", "INT", "users", true, false),
            col("name", "VARCHAR", "users", false, false),
            col("id", "INT", "users", true, false),
            col("name", "VARCHAR", "users", false, false),
        ]);
        let schema = |_db: &str, t: &str| {
            (t == "users")
                .then(|| schema_with_pk("users", &["id"], &[("id", "int"), ("name", "varchar")]))
        };
        let m = analyze_edit(&r, schema);
        assert!(!m.editable(0));
        assert!(!m.editable(1));
    }

    #[test]
    fn c2_binary_column_not_editable_binary_key_readonly() {
        // A binary (BLOB) non-key column: read-only, but the INT PK stays editable.
        let r = rs(vec![
            col("id", "INT", "docs", true, false),
            col("blob", "BLOB", "docs", false, true),
        ]);
        let schema = |_db: &str, t: &str| {
            (t == "docs")
                .then(|| schema_with_pk("docs", &["id"], &[("id", "int"), ("blob", "blob")]))
        };
        let m = analyze_edit(&r, schema);
        assert!(m.editable(0), "INT PK editable");
        assert!(!m.editable(1), "BLOB column read-only");

        // A binary PK makes the whole table read-only (can't build a safe WHERE).
        let r2 = rs(vec![
            col("id", "VARBINARY", "b", true, true),
            col("v", "INT", "b", false, false),
        ]);
        let schema2 = |_db: &str, t: &str| {
            (t == "b").then(|| schema_with_pk("b", &["id"], &[("id", "varbinary"), ("v", "int")]))
        };
        let m2 = analyze_edit(&r2, schema2);
        assert!(!m2.editable(0));
        assert!(!m2.editable(1));
    }

    #[test]
    fn c4_float_key_is_readonly() {
        let r = rs(vec![
            col("id", "FLOAT", "m", true, false),
            col("v", "INT", "m", false, false),
        ]);
        let schema = |_db: &str, t: &str| {
            (t == "m").then(|| schema_with_pk("m", &["id"], &[("id", "float"), ("v", "int")]))
        };
        let m = analyze_edit(&r, schema);
        assert!(!m.editable(0));
        assert!(!m.editable(1));
    }

    #[test]
    fn expression_columns_are_readonly() {
        let mut expr = col("cnt", "BIGINT", "", false, false);
        expr.origin = None; // aggregate / expression
        let r = rs(vec![col("id", "INT", "t", true, false), expr]);
        let schema =
            |_db: &str, t: &str| (t == "t").then(|| schema_with_pk("t", &["id"], &[("id", "int")]));
        let m = analyze_edit(&r, schema);
        assert!(m.editable(0));
        assert!(!m.editable(1));
    }

    #[test]
    fn refetch_template_single_table() {
        let r = rs(vec![
            col("id", "INT", "users", true, false),
            col("name", "VARCHAR", "users", false, false),
        ]);
        let schema = |_db: &str, t: &str| {
            (t == "users")
                .then(|| schema_with_pk("users", &["id"], &[("id", "int"), ("name", "varchar")]))
        };
        let m = analyze_edit(&r, schema);
        let t = super::refetch_template(&r, &m).expect("single-table result is spliceable");
        assert_eq!(t.table, "users");
        assert_eq!(t.columns, vec!["id".to_string(), "name".to_string()]);
        assert_eq!(t.key_cols, vec![0]); // `id` is the WHERE key
    }

    #[test]
    fn refetch_template_none_with_expression_column() {
        // An aggregate/expression column can't be re-selected by real name.
        let mut expr = col("cnt", "BIGINT", "", false, false);
        expr.origin = None;
        let r = rs(vec![col("id", "INT", "t", true, false), expr]);
        let schema =
            |_db: &str, t: &str| (t == "t").then(|| schema_with_pk("t", &["id"], &[("id", "int")]));
        let m = analyze_edit(&r, schema);
        assert!(super::refetch_template(&r, &m).is_none());
    }

    #[test]
    fn refetch_template_none_with_two_tables() {
        // A join across two writable tables → ambiguous single-table re-fetch.
        let r = rs(vec![
            col("id", "INT", "a", true, false),
            col("bid", "INT", "b", true, false),
        ]);
        let schema = |_db: &str, t: &str| match t {
            "a" => Some(schema_with_pk("a", &["id"], &[("id", "int")])),
            "b" => Some(schema_with_pk("b", &["bid"], &[("bid", "int")])),
            _ => None,
        };
        let m = analyze_edit(&r, schema);
        assert!(super::refetch_template(&r, &m).is_none());
    }

    #[test]
    fn insert_target_single_vs_multi_table() {
        // Single writable table → that table is the insert destination.
        let one = rs(vec![
            col("id", "INT", "users", true, false),
            col("name", "VARCHAR", "users", false, false),
        ]);
        let schema = |_db: &str, t: &str| {
            (t == "users")
                .then(|| schema_with_pk("users", &["id"], &[("id", "int"), ("name", "varchar")]))
        };
        let m = analyze_edit(&one, schema);
        assert_eq!(m.insert_target().map(|t| t.table.as_str()), Some("users"));

        // Two writable tables → ambiguous, no single insert destination.
        let two = rs(vec![
            col("id", "INT", "a", true, false),
            col("bid", "INT", "b", true, false),
        ]);
        let schema2 = |_db: &str, t: &str| match t {
            "a" => Some(schema_with_pk("a", &["id"], &[("id", "int")])),
            "b" => Some(schema_with_pk("b", &["bid"], &[("bid", "int")])),
            _ => None,
        };
        let m2 = analyze_edit(&two, schema2);
        assert!(m2.insert_target().is_none());

        // Read-only / non-editable (empty model) → no destination.
        assert!(EditModel::default().insert_target().is_none());
    }

    #[test]
    fn table_index_and_table_accessors() {
        let r = rs(vec![
            col("id", "INT", "users", true, false),
            col("name", "VARCHAR", "users", false, false),
        ]);
        let schema = |_db: &str, t: &str| {
            (t == "users")
                .then(|| schema_with_pk("users", &["id"], &[("id", "int"), ("name", "varchar")]))
        };
        let m = analyze_edit(&r, schema);
        // Both columns map to table index 0.
        assert_eq!(m.table_index(0), Some(0));
        assert_eq!(m.table_index(1), Some(0));
        // Out-of-range column → None.
        assert_eq!(m.table_index(99), None);
        // table(idx) resolves the EditTable.
        assert_eq!(m.table(0).map(|t| t.table.as_str()), Some("users"));
        assert!(m.table(1).is_none());
    }

    #[test]
    fn no_schema_falls_back_to_wire_pk_flags() {
        // schema_for returns None (schema not loaded) but the wire marks `id` PK.
        let r = rs(vec![
            col("id", "INT", "users", true, false),
            col("name", "VARCHAR", "users", false, false),
        ]);
        let no_schema = |_db: &str, _t: &str| None;
        let m = analyze_edit(&r, no_schema);
        assert!(m.editable(0), "wire PK flag makes the table editable");
        assert!(m.editable(1));
        let t = refetch_template(&r, &m).expect("spliceable via wire PK");
        assert_eq!(t.key_cols, vec![0]);

        // No schema AND no PK flag anywhere → read-only (no reconstructible key).
        let r2 = rs(vec![
            col("a", "INT", "t", false, false),
            col("b", "INT", "t", false, false),
        ]);
        let m2 = analyze_edit(&r2, no_schema);
        assert!(!m2.editable(0));
        assert!(!m2.editable(1));
    }

    #[test]
    fn unique_not_null_index_is_the_key_when_no_pk() {
        // Table has no primary key but a UNIQUE, non-foreign, NOT NULL index on
        // `email` → that becomes the WHERE key.
        let r = rs(vec![
            col("email", "VARCHAR", "users", false, false),
            col("name", "VARCHAR", "users", false, false),
        ]);
        let schema = |_db: &str, t: &str| {
            (t == "users").then(|| TableInfo {
                name: "users".to_string(),
                columns: vec![
                    ColumnInfo {
                        name: "email".to_string(),
                        type_name: "varchar".to_string(),
                        nullable: false, // NOT NULL — required for the unique-index key
                        primary_key: false,
                    },
                    ColumnInfo {
                        name: "name".to_string(),
                        type_name: "varchar".to_string(),
                        nullable: true,
                        primary_key: false,
                    },
                ],
                indexes: vec![crate::schema::IndexInfo {
                    name: "email_uq".to_string(),
                    columns: vec!["email".to_string()],
                    unique: true,
                    foreign: false,
                }],
                is_view: false,
                view_definition: None,
            })
        };
        let m = analyze_edit(&r, schema);
        assert!(m.editable(0));
        assert!(m.editable(1));
        let t = refetch_template(&r, &m).expect("unique NOT NULL index is a usable key");
        assert_eq!(t.key_cols, vec![0]); // email

        // A NULLABLE unique index is NOT a safe key → read-only.
        let schema_nullable = |_db: &str, t: &str| {
            (t == "users").then(|| TableInfo {
                name: "users".to_string(),
                columns: vec![
                    ColumnInfo {
                        name: "email".to_string(),
                        type_name: "varchar".to_string(),
                        nullable: true, // nullable → can't uniquely identify a row
                        primary_key: false,
                    },
                    ColumnInfo {
                        name: "name".to_string(),
                        type_name: "varchar".to_string(),
                        nullable: true,
                        primary_key: false,
                    },
                ],
                indexes: vec![crate::schema::IndexInfo {
                    name: "email_uq".to_string(),
                    columns: vec!["email".to_string()],
                    unique: true,
                    foreign: false,
                }],
                is_view: false,
                view_definition: None,
            })
        };
        let m2 = analyze_edit(&r, schema_nullable);
        assert!(!m2.editable(0));
        assert!(!m2.editable(1));
    }
}
