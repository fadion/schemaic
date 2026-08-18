//! Result-set editability analysis — pure over [`ResultSet`] + schema, no UI.
//!
//! From each column's wire provenance (real table/column + key flags, see
//! [`crate::model::ColumnOrigin`]) this decides which columns can be written
//! back and, per base table, which result columns reconstruct a row's `WHERE`
//! key. It is deliberately conservative: anything it can't identify uniquely
//! and safely is read-only. This is the most safety-critical logic in the app
//! (a wrong key misdirects an UPDATE), so it lives here with tests rather than
//! welded to Floem signals in the UI.

use crate::model::{RefetchTemplate, ResultSet, Value};
use crate::schema::TableInfo;
use std::collections::HashMap;

/// A base table the result can write back to, plus the result-column indices
/// whose (original) values form the row-identity `WHERE`.
#[derive(Clone, Debug)]
pub struct EditTable {
    pub database: String,
    /// PostgreSQL namespace of `table` (`None` on MySQL). Carried through to the
    /// staged `RowEdit`/`RowInsert`/`RowDelete` so the write names the same table
    /// the row was read from, not whatever `search_path` resolves.
    pub schema: Option<String>,
    pub table: String,
    pub key_cols: Vec<usize>,
    /// Result columns whose **original** values the `WHERE` must also match, on
    /// top of `key_cols`. Empty unless the key is an implicit one.
    ///
    /// **A rowid is not a row identity.** SQLite hands one out per row, and it
    /// reassigns them: the twelve-step rebuild renumbers a keyless table, a
    /// delete frees the highest one for the next insert, `VACUUM` compacts them.
    /// Nothing re-runs an open result tab when any of that happens, so the grid
    /// can hold a number that now names a *different* row — and an `UPDATE`
    /// keyed on it affects exactly 1 row, which is the number
    /// [`crate::model::one_row_verdict`] is looking for. The safety net's whole
    /// premise is that a stale key matches **zero** rows.
    ///
    /// So the rowid keeps identifying the row and these columns confirm it: the
    /// values the grid actually read, `AND`ed onto the same `WHERE`. A
    /// renumbered or reused rowid now matches nothing and the net fires. This is
    /// not "match on every value" — that scheme can't tell two identical rows
    /// apart, and this one never has to, because the rowid already did.
    pub confirm_cols: Vec<usize>,
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
        if o.database != tbl.database || o.schema != tbl.schema || o.table != tbl.table {
            return None;
        }
        columns.push(o.column.clone());
    }
    Some(RefetchTemplate {
        database: tbl.database.clone(),
        schema: tbl.schema.clone(),
        table: tbl.table.clone(),
        columns,
        key_cols: tbl.key_cols.clone(),
    })
}

/// The `WHERE` key identifying data row `di` **after** an edit, for re-fetching
/// it into the grid.
///
/// `edited` is that row's changed result columns → their new value (`None` = SQL
/// `NULL`); a key column among them is looked up by the value it was changed
/// **to**, since that is what the just-committed `UPDATE` left in the table.
/// Every other key column reads its original value out of `rs`.
///
/// This is the single builder for both write paths. There used to be two, and
/// only the staged-batch one handled an edited key column; the row panel's built
/// the key from the pre-edit row on the stated precondition that *"the editor
/// blocks PK edits"* — which it does not (`EditModel::editable` asks only
/// whether a column maps to a base table). So changing `id` there wrote
/// correctly, re-fetched nothing, and left the grid showing the old key, after
/// which every later edit to that row missed and rolled its batch back.
pub fn refetch_key(
    template: &RefetchTemplate,
    rs: &ResultSet,
    di: usize,
    edited: &HashMap<usize, Option<String>>,
) -> Vec<Value> {
    template
        .key_cols
        .iter()
        .map(|&kci| match edited.get(&kci) {
            // Bound as text, exactly as the `UPDATE`'s own SET value was.
            Some(Some(text)) => Value::Str(text.clone()),
            Some(None) => Value::Null,
            None => rs
                .cell(di, kci)
                .map(|c| c.to_value())
                .unwrap_or(Value::Null),
        })
        .collect()
}

/// Compute the [`EditModel`]. `schema_for(database, schema, table)` returns the
/// loaded schema for a base table (or `None` if unknown) — the UI supplies a
/// closure that reads its schema signals; tests supply a plain map. `schema` is
/// the PostgreSQL namespace, `None` on MySQL.
pub fn analyze_edit(
    rs: &ResultSet,
    schema_for: impl Fn(&str, Option<&str>, &str) -> Option<TableInfo>,
) -> EditModel {
    let ncols = rs.columns.len();
    let mut col_table: Vec<Option<usize>> = vec![None; ncols];
    let mut tables: Vec<EditTable> = Vec::new();

    // Distinct (database, schema, table) in first-seen order → its result
    // columns. The namespace is part of the key: `sales.orders` and
    // `archive.orders` are different tables, and merging them would let one
    // table's key columns address the other's rows.
    type TableKey = (String, Option<String>, String);
    let mut groups: Vec<(TableKey, Vec<usize>)> = Vec::new();
    for (ci, col) in rs.columns.iter().enumerate() {
        let Some(o) = &col.origin else { continue };
        let key = (o.database.clone(), o.schema.clone(), o.table.clone());
        if let Some(g) = groups.iter_mut().find(|(k, _)| *k == key) {
            g.1.push(ci);
        } else {
            groups.push((key, vec![ci]));
        }
    }

    for ((db, schema, table), cis) in &groups {
        if let Some(key_cols) = resolve_key(&schema_for, db, schema.as_deref(), table, cis, rs) {
            let idx = tables.len();
            let confirm_cols = confirm_columns(&key_cols, cis, rs);
            tables.push(EditTable {
                database: db.clone(),
                schema: schema.clone(),
                table: table.clone(),
                key_cols,
                confirm_cols,
            });
            for &ci in cis {
                // C2: binary columns can't round-trip as text → never editable,
                // even when their table has a usable key. An implicit key is
                // excluded for a different reason (see `ColumnOrigin`): it is no
                // column of the table, so there is nothing to write to.
                let excluded = rs.columns[ci]
                    .origin
                    .as_ref()
                    .map(|o| o.binary || o.implicit_key)
                    .unwrap_or(false);
                if !excluded {
                    col_table[ci] = Some(idx);
                }
            }
        }
    }
    EditModel { col_table, tables }
}

/// The `WHERE` identity of data row `di` in `rs` for base table `tbl`: each key
/// column's real name paired with the row's **original** value, followed by the
/// table's [`EditTable::confirm_cols`] in the same shape.
///
/// The one builder for it. Every write the grid issues — update, delete, and the
/// row panel's immediate save — is aimed at the row this names, so a difference
/// between copies is a statement aimed somewhere else. It lives here rather than
/// in the grid because the confirming columns are part of the row's identity,
/// and identity is what this module is for.
pub fn row_key(rs: &ResultSet, tbl: &EditTable, di: usize) -> Vec<(String, Value)> {
    tbl.key_cols
        .iter()
        .chain(tbl.confirm_cols.iter())
        .map(|&kci| {
            let name = rs
                .columns
                .get(kci)
                .and_then(|c| c.origin.as_ref())
                .map(|o| o.column.clone())
                .unwrap_or_default();
            let val = rs
                .cell(di, kci)
                .map(|c| c.to_value())
                .unwrap_or(Value::Null);
            (name, val)
        })
        .collect()
}

/// The result columns whose original values must confirm an **implicit** key —
/// see [`EditTable::confirm_cols`]. Empty for every real key, on every engine.
///
/// A binary column is left out: its cell is a placeholder, not the value, so
/// comparing it would refuse every write to the table rather than only the
/// misdirected ones. Everything else the grid read goes in, including a column
/// the user is editing — the value compared is the one that was *read*, which is
/// what the row was when its rowid was.
fn confirm_columns(key_cols: &[usize], cis: &[usize], rs: &ResultSet) -> Vec<usize> {
    let implicit = key_cols.iter().any(|&kci| {
        rs.columns[kci]
            .origin
            .as_ref()
            .is_some_and(|o| o.implicit_key)
    });
    if !implicit {
        return Vec::new();
    }
    cis.iter()
        .copied()
        .filter(|ci| !key_cols.contains(ci))
        .filter(|&ci| {
            rs.columns[ci]
                .origin
                .as_ref()
                .is_some_and(|o| !o.binary && !o.implicit_key)
        })
        .collect()
}

/// Find the result-column indices forming a usable row key for one base table,
/// or `None` if the table's rows can't be identified safely (read-only).
fn resolve_key(
    schema_for: &impl Fn(&str, Option<&str>, &str) -> Option<TableInfo>,
    db: &str,
    schema: Option<&str>,
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

    let candidate: Option<Vec<usize>> = if let Some(t) = schema_for(db, schema, table) {
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
                    ix.column_names().all(|c| {
                        t.columns
                            .iter()
                            .find(|tc| tc.name == c)
                            .map(|tc| !tc.nullable)
                            .unwrap_or(false)
                    })
                })
                .find_map(|ix| {
                    let names: Vec<String> = ix.column_names().map(str::to_string).collect();
                    all_present(&names)
                })
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

    // Last resort: a row key the table doesn't have a column for, asserted by the
    // backend and projected into the result (SQLite's `rowid` — see
    // [`crate::model::ColumnOrigin::implicit_key`]). It comes after the real keys
    // and never instead of one: a primary key is what the user means by the row's
    // identity, it survives a re-fetch, and it is stable in a way a rowid the
    // engine may reassign is not.
    let candidate = candidate.or_else(|| {
        cis.iter()
            .copied()
            .find(|&ci| {
                rs.columns[ci]
                    .origin
                    .as_ref()
                    .is_some_and(|o| o.implicit_key)
            })
            .map(|ci| vec![ci])
    });

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

    /// A result column with a base-table origin (no namespace — the MySQL shape).
    fn col(name: &str, ty: &str, table: &str, pk: bool, binary: bool) -> Column {
        col_in(None, name, ty, table, pk, binary)
    }

    /// As [`col`], but in an explicit PostgreSQL namespace.
    fn col_in(
        schema: Option<&str>,
        name: &str,
        ty: &str,
        table: &str,
        pk: bool,
        binary: bool,
    ) -> Column {
        Column {
            name: name.to_string(),
            type_name: ty.to_string(),
            origin: Some(ColumnOrigin {
                database: "db".to_string(),
                schema: schema.map(str::to_string),
                table: table.to_string(),
                column: name.to_string(),
                flags: ColumnFlags {
                    primary_key: pk,
                    not_null: pk,
                    ..Default::default()
                },
                binary,
                implicit_key: false,
            }),
        }
    }

    fn rs(columns: Vec<Column>) -> ResultSet {
        ResultSet::from_rows(columns, Vec::new())
    }

    /// Schema table with the given primary-key column names (INT, NOT NULL).
    fn schema_with_pk(table: &str, pk: &[&str], cols: &[(&str, &str)]) -> TableInfo {
        TableInfo {
            schema: None,
            name: table.to_string(),
            columns: cols
                .iter()
                .map(|(n, ty)| ColumnInfo {
                    name: n.to_string(),
                    type_name: ty.to_string(),
                    nullable: !pk.contains(n),
                    primary_key: pk.contains(n),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn happy_path_int_pk_is_editable() {
        let r = rs(vec![
            col("id", "INT", "users", true, false),
            col("name", "VARCHAR", "users", false, false),
        ]);
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "users")
                .then(|| schema_with_pk("users", &["id"], &[("id", "int"), ("name", "varchar")]))
        };
        let m = analyze_edit(&r, schema);
        assert!(m.editable(0));
        assert!(m.editable(1));
    }

    // ── multi-schema: the namespace is part of a table's identity ─────────

    #[test]
    fn same_table_name_in_two_schemas_stays_two_edit_tables() {
        // `sales.orders` joined to `archive.orders`. Keying groups on the table
        // name alone would fold them into one — and then one table's key columns
        // would be used to address the other's rows.
        let r = rs(vec![
            col_in(Some("sales"), "id", "INT", "orders", true, false),
            col_in(Some("sales"), "total", "INT", "orders", false, false),
            col_in(Some("archive"), "id", "INT", "orders", true, false),
        ]);
        let schema = |_db: &str, s: Option<&str>, t: &str| {
            (t == "orders").then(|| TableInfo {
                schema: s.map(str::to_string),
                ..schema_with_pk("orders", &["id"], &[("id", "int"), ("total", "int")])
            })
        };
        let m = analyze_edit(&r, schema);
        // Two distinct writable tables, each carrying its own namespace.
        assert_eq!(m.table(0).map(|t| t.schema.as_deref()), Some(Some("sales")));
        assert_eq!(
            m.table(1).map(|t| t.schema.as_deref()),
            Some(Some("archive"))
        );
        assert_eq!(m.table_index(0), Some(0));
        assert_eq!(m.table_index(2), Some(1));
        // Two writable tables → no single INSERT target.
        assert!(m.insert_target().is_none());
        // And the result isn't spliceable (a join across two base tables).
        assert!(refetch_template(&r, &m).is_none());
    }

    // ── the implicit key: a row identity outside the table's columns ──────

    /// A result column carrying a table's implicit row key (SQLite's `rowid`) —
    /// a real origin on the table, but no column of it.
    fn implicit_col(name: &str, table: &str) -> Column {
        let mut c = col(name, "", table, false, false);
        c.origin.as_mut().unwrap().implicit_key = true;
        c
    }

    /// A table with no primary key and no index at all — read-only on every
    /// engine, and the case an implicit key exists to rescue.
    fn schema_keyless(table: &str, cols: &[(&str, &str)]) -> TableInfo {
        schema_with_pk(table, &[], cols)
    }

    #[test]
    fn keyless_table_is_editable_through_its_implicit_key() {
        let r = rs(vec![
            implicit_col("rowid", "notes"),
            col("a", "TEXT", "notes", false, false),
            col("b", "TEXT", "notes", false, false),
        ]);
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "notes").then(|| schema_keyless("notes", &[("a", "text"), ("b", "text")]))
        };
        let m = analyze_edit(&r, schema);
        // The key is the implicit column, and the data columns are writable.
        assert_eq!(m.table(0).map(|t| t.key_cols.clone()), Some(vec![0]));
        assert!(m.editable(1));
        assert!(m.editable(2));
        // The key itself is not: it is the handle on the row, not the table's
        // data, and a new row has no value to offer for it.
        assert!(!m.editable(0));
        assert!(m.insert_target().is_some());
    }

    /// **The rowid identifies, the values confirm.** A rowid is reassigned — by
    /// the twelve-step rebuild, by an insert after a delete, by `VACUUM` — and
    /// nothing re-runs an open grid when it happens, so the number can come to
    /// name a different row. Keyed on the number alone, the `UPDATE` lands on
    /// that row and affects exactly 1, which is the number the safety net wants
    /// to see. The read values ride along so a moved rowid matches **zero**.
    #[test]
    fn an_implicit_key_carries_the_read_values_as_confirmation() {
        let r = rs(vec![
            implicit_col("rowid", "notes"),
            col("a", "TEXT", "notes", false, false),
            col("b", "TEXT", "notes", false, false),
        ]);
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "notes").then(|| schema_keyless("notes", &[("a", "text"), ("b", "text")]))
        };
        let m = analyze_edit(&r, schema);
        let tbl = m.insert_target().expect("writable");
        assert_eq!(tbl.key_cols, vec![0]);
        assert_eq!(tbl.confirm_cols, vec![1, 2]);
    }

    /// A binary column's cell is a placeholder, not the value, so comparing it
    /// would refuse every write to the table rather than only the misdirected
    /// ones.
    #[test]
    fn a_binary_column_is_not_used_as_confirmation() {
        let r = rs(vec![
            implicit_col("rowid", "notes"),
            col("a", "TEXT", "notes", false, false),
            col("blob", "BLOB", "notes", false, true),
        ]);
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "notes").then(|| schema_keyless("notes", &[("a", "text"), ("blob", "blob")]))
        };
        let m = analyze_edit(&r, schema);
        assert_eq!(
            m.insert_target().map(|t| t.confirm_cols.clone()),
            Some(vec![1])
        );
    }

    /// A real key needs no confirmation: it is the row's identity, it survives a
    /// rebuild because its *values* are copied, and a deleted-then-reinserted
    /// row does not silently inherit it. Every MySQL and PostgreSQL table is
    /// here, and so is every SQLite table with a key of its own.
    #[test]
    fn a_real_key_carries_no_confirmation_columns() {
        let r = rs(vec![
            col("id", "INT", "users", true, false),
            col("name", "VARCHAR", "users", false, false),
        ]);
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "users")
                .then(|| schema_with_pk("users", &["id"], &[("id", "int"), ("name", "varchar")]))
        };
        let m = analyze_edit(&r, schema);
        assert_eq!(
            m.insert_target().map(|t| t.confirm_cols.clone()),
            Some(vec![])
        );
    }

    /// The implicit key is the last resort, never a shortcut past a real one:
    /// the table's own key is what an `UPDATE` should match on, and it is what
    /// survives a re-fetch.
    #[test]
    fn a_real_key_still_wins_over_a_projected_implicit_one() {
        let r = rs(vec![
            implicit_col("rowid", "users"),
            col("id", "INT", "users", true, false),
            col("name", "VARCHAR", "users", false, false),
        ]);
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "users")
                .then(|| schema_with_pk("users", &["id"], &[("id", "int"), ("name", "varchar")]))
        };
        let m = analyze_edit(&r, schema);
        assert_eq!(m.table(0).map(|t| t.key_cols.clone()), Some(vec![1]));
    }

    /// Nothing changes for a table that has no implicit key to offer — a
    /// `WITHOUT ROWID` table, and every MySQL/PostgreSQL table there is.
    #[test]
    fn a_keyless_table_with_no_implicit_key_stays_read_only() {
        let r = rs(vec![
            col("a", "TEXT", "notes", false, false),
            col("b", "TEXT", "notes", false, false),
        ]);
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "notes").then(|| schema_keyless("notes", &[("a", "text"), ("b", "text")]))
        };
        let m = analyze_edit(&r, schema);
        assert!(!m.editable(0));
        assert!(!m.editable(1));
        assert!(m.insert_target().is_none());
    }

    /// A read-only key column is still part of the row, so the post-commit
    /// re-fetch must select it and match on it.
    #[test]
    fn refetch_template_keys_on_the_implicit_key() {
        let r = rs(vec![
            implicit_col("rowid", "notes"),
            col("a", "TEXT", "notes", false, false),
        ]);
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "notes").then(|| schema_keyless("notes", &[("a", "text")]))
        };
        let m = analyze_edit(&r, schema);
        let tpl = refetch_template(&r, &m).expect("single base table is spliceable");
        assert_eq!(tpl.columns, vec!["rowid".to_string(), "a".to_string()]);
        assert_eq!(tpl.key_cols, vec![0]);
    }

    #[test]
    fn refetch_template_carries_the_namespace() {
        let r = rs(vec![
            col_in(Some("sales"), "id", "INT", "orders", true, false),
            col_in(Some("sales"), "total", "INT", "orders", false, false),
        ]);
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "orders")
                .then(|| schema_with_pk("orders", &["id"], &[("id", "int"), ("total", "int")]))
        };
        let m = analyze_edit(&r, schema);
        let tpl = refetch_template(&r, &m).expect("single base table is spliceable");
        assert_eq!(tpl.schema.as_deref(), Some("sales"));
        assert_eq!(tpl.table, "orders");
    }

    #[test]
    fn analyze_edit_passes_the_namespace_to_the_schema_lookup() {
        // The lookup must be able to tell the two apart; a closure that only
        // answers for `sales` leaves an `archive` column read-only rather than
        // silently borrowing the other schema's key.
        let r = rs(vec![col_in(
            Some("archive"),
            "id",
            "INT",
            "orders",
            false,
            false,
        )]);
        let schema = |_db: &str, s: Option<&str>, t: &str| {
            (s == Some("sales") && t == "orders")
                .then(|| schema_with_pk("orders", &["id"], &[("id", "int")]))
        };
        let m = analyze_edit(&r, schema);
        // No schema for `archive.orders`, and the wire flags say it isn't a PK →
        // no usable key → read-only.
        assert!(!m.editable(0));
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
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
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
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
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
        let schema2 = |_db: &str, _s: Option<&str>, t: &str| {
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
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
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
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "t").then(|| schema_with_pk("t", &["id"], &[("id", "int")]))
        };
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
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "users")
                .then(|| schema_with_pk("users", &["id"], &[("id", "int"), ("name", "varchar")]))
        };
        let m = analyze_edit(&r, schema);
        let t = super::refetch_template(&r, &m).expect("single-table result is spliceable");
        assert_eq!(t.table, "users");
        assert_eq!(t.columns, vec!["id".to_string(), "name".to_string()]);
        assert_eq!(t.key_cols, vec![0]); // `id` is the WHERE key
    }

    // ── The post-edit re-fetch key (`refetch_key`) ──
    //
    // A key column *is* editable — `EditModel::editable` asks only whether the
    // column maps to a base table, and `happy_path_int_pk_is_editable` pins that
    // — so the re-fetch has to look for the row by the key the UPDATE just gave
    // it. The row panel's own builder assumed the opposite and silently left the
    // grid on the old value.

    /// A two-column `users` result (`id` PK, `name`) with one row, plus its
    /// re-fetch template.
    fn keyed_users_row() -> (ResultSet, RefetchTemplate) {
        let r = ResultSet::from_rows(
            vec![
                col("id", "INT", "users", true, false),
                col("name", "VARCHAR", "users", false, false),
            ],
            vec![vec![Value::Int(5), Value::Str("ada".to_string())]],
        );
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "users")
                .then(|| schema_with_pk("users", &["id"], &[("id", "int"), ("name", "varchar")]))
        };
        let m = analyze_edit(&r, schema);
        let tpl = super::refetch_template(&r, &m).expect("single-table result is spliceable");
        assert_eq!(tpl.key_cols, vec![0]);
        (r, tpl)
    }

    #[test]
    fn an_untouched_key_column_refetches_by_its_original_value() {
        let (r, tpl) = keyed_users_row();
        let edited: HashMap<usize, Option<String>> =
            [(1, Some("grace".to_string()))].into_iter().collect();
        assert_eq!(refetch_key(&tpl, &r, 0, &edited), vec![Value::Int(5)]);
    }

    #[test]
    fn an_edited_key_column_refetches_by_its_new_value() {
        // `UPDATE users SET id = 6 WHERE id = 5` committed; row 5 no longer
        // exists, so re-fetching by 5 finds nothing and the grid keeps showing
        // the stale key.
        let (r, tpl) = keyed_users_row();
        let edited: HashMap<usize, Option<String>> =
            [(0, Some("6".to_string()))].into_iter().collect();
        assert_eq!(
            refetch_key(&tpl, &r, 0, &edited),
            vec![Value::Str("6".to_string())]
        );
    }

    #[test]
    fn a_key_column_edited_to_null_refetches_by_null() {
        let (r, tpl) = keyed_users_row();
        let edited: HashMap<usize, Option<String>> = [(0, None)].into_iter().collect();
        assert_eq!(refetch_key(&tpl, &r, 0, &edited), vec![Value::Null]);
    }

    #[test]
    fn a_composite_key_takes_each_column_from_where_it_stands() {
        let r = ResultSet::from_rows(
            vec![
                col("a", "INT", "t", true, false),
                col("b", "INT", "t", true, false),
                col("v", "VARCHAR", "t", false, false),
            ],
            vec![vec![Value::Int(1), Value::Int(2), Value::Str("x".into())]],
        );
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "t").then(|| {
                schema_with_pk(
                    "t",
                    &["a", "b"],
                    &[("a", "int"), ("b", "int"), ("v", "varchar")],
                )
            })
        };
        let m = analyze_edit(&r, schema);
        let tpl = super::refetch_template(&r, &m).expect("spliceable");
        assert_eq!(tpl.key_cols, vec![0, 1]);
        // Only `b` was edited: `a` keeps its original, `b` takes the new value.
        let edited: HashMap<usize, Option<String>> =
            [(1, Some("9".to_string()))].into_iter().collect();
        assert_eq!(
            refetch_key(&tpl, &r, 0, &edited),
            vec![Value::Int(1), Value::Str("9".to_string())]
        );
    }

    #[test]
    fn a_row_past_the_end_keys_on_null_rather_than_panicking() {
        let (r, tpl) = keyed_users_row();
        assert_eq!(
            refetch_key(&tpl, &r, 99, &HashMap::new()),
            vec![Value::Null]
        );
    }

    #[test]
    fn refetch_template_none_with_expression_column() {
        // An aggregate/expression column can't be re-selected by real name.
        let mut expr = col("cnt", "BIGINT", "", false, false);
        expr.origin = None;
        let r = rs(vec![col("id", "INT", "t", true, false), expr]);
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "t").then(|| schema_with_pk("t", &["id"], &[("id", "int")]))
        };
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
        let schema = |_db: &str, _s: Option<&str>, t: &str| match t {
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
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
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
        let schema2 = |_db: &str, _s: Option<&str>, t: &str| match t {
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
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
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
        let no_schema = |_db: &str, _s: Option<&str>, _t: &str| None;
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
        let schema = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "users").then(|| TableInfo {
                schema: None,
                name: "users".to_string(),
                columns: vec![
                    ColumnInfo {
                        name: "email".to_string(),
                        type_name: "varchar".to_string(),
                        nullable: false, // NOT NULL — required for the unique-index key
                        ..Default::default()
                    },
                    ColumnInfo {
                        name: "name".to_string(),
                        type_name: "varchar".to_string(),
                        nullable: true,
                        ..Default::default()
                    },
                ],
                indexes: vec![crate::schema::IndexInfo::plain(
                    "email_uq",
                    vec!["email"],
                    true,
                )],
                ..Default::default()
            })
        };
        let m = analyze_edit(&r, schema);
        assert!(m.editable(0));
        assert!(m.editable(1));
        let t = refetch_template(&r, &m).expect("unique NOT NULL index is a usable key");
        assert_eq!(t.key_cols, vec![0]); // email

        // A NULLABLE unique index is NOT a safe key → read-only.
        let schema_nullable = |_db: &str, _s: Option<&str>, t: &str| {
            (t == "users").then(|| TableInfo {
                schema: None,
                name: "users".to_string(),
                columns: vec![
                    ColumnInfo {
                        name: "email".to_string(),
                        type_name: "varchar".to_string(),
                        nullable: true, // nullable → can't uniquely identify a row
                        ..Default::default()
                    },
                    ColumnInfo {
                        name: "name".to_string(),
                        type_name: "varchar".to_string(),
                        nullable: true,
                        ..Default::default()
                    },
                ],
                indexes: vec![crate::schema::IndexInfo::plain(
                    "email_uq",
                    vec!["email"],
                    true,
                )],
                ..Default::default()
            })
        };
        let m2 = analyze_edit(&r, schema_nullable);
        assert!(!m2.editable(0));
        assert!(!m2.editable(1));
    }
}
