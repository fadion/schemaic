//! Result-set editability analysis — pure over [`ResultSet`] + schema, no UI.
//!
//! It also owns the **display↔data row mapping** the grid's destructive row
//! actions run on ([`selected_data_rows`], [`attach_span`]): a sorted grid draws
//! rows in `order` while every write addresses a *data* row, and getting that
//! backwards deletes a row the user was not pointing at. The 1-row write-back
//! net checks the *count*, never the identity, so nothing downstream would
//! notice.
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
                // An index with no *column* keys keys nothing — the same guard
                // `schema::browse_key_columns` states, and for the same
                // PostgreSQL expression index. Without it `all_present(&[])`
                // answers `Some(vec![])`, and an empty write key builds
                // `… WHERE ` with nothing after it.
                .filter(|ix| ix.column_names().next().is_some())
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

/// The **data** rows a gutter gesture at display row `pos` acts on, in display
/// order.
///
/// `order` is the grid's display→data map, `selection` the display-row range the
/// user has highlighted. When the click landed outside that range the gesture
/// means the row it pointed at and nothing else — a menu must act on what was
/// clicked, not on a selection somewhere up the list.
///
/// **Pending new rows are left out.** They live past `order.len()` and have no
/// committed row to duplicate or mark for deletion.
///
/// Pure and here rather than in the grid because it decides *which rows are
/// deleted*: on a sorted grid the display index and the data index are different
/// numbers, and the write-back's 1-row net checks the count, not the identity —
/// so an inverted mapping deletes the wrong row and reports success.
pub fn selected_data_rows(
    order: &[usize],
    selection: Option<(usize, usize)>,
    pos: usize,
) -> Vec<usize> {
    let (r0, r1) = match selection {
        Some((r0, r1)) if pos >= r0 && pos <= r1 => (r0, r1),
        _ => (pos, pos),
    };
    (r0..=r1)
        .filter(|i| *i < order.len())
        .map(|i| order.get(i).copied().unwrap_or(i))
        .collect()
}

/// What a cell menu's **Copy** takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyScope {
    /// The one cell the menu was opened on.
    Cell,
    /// The whole highlighted block, exactly as Ctrl+C takes it.
    Selection,
}

impl CopyScope {
    /// The entry's label. **The two amounts get two words**: "Copy" over a block
    /// means the block, and one cell out of a block is a different act that has
    /// to say so.
    pub fn label(self) -> &'static str {
        match self {
            CopyScope::Cell => "Copy value",
            CopyScope::Selection => "Copy",
        }
    }
}

/// What Copy means for a right-click at display cell `(i, ci)`, given the
/// selection `(r0, c0, r1, c1)`.
///
/// A right-click **inside** a multi-cell selection no longer collapses it — that
/// was the point of preserving it — so the menu is about the block, and an entry
/// reading "Copy" that took one cell out of nine said the same word Ctrl+C and
/// the gutter menu's own Copy say for three different amounts.
pub fn copy_scope(
    selection: Option<(usize, usize, usize, usize)>,
    i: usize,
    ci: usize,
) -> CopyScope {
    match selection {
        Some((r0, c0, r1, c1))
            if (r0 != r1 || c0 != c1) && (r0..=r1).contains(&i) && (c0..=c1).contains(&ci) =>
        {
            CopyScope::Selection
        }
        _ => CopyScope::Cell,
    }
}

/// The literal a staged SQL NULL reads as, everywhere a surface reads the grid.
/// The cell paints it, so the clipboard and an attachment say it too.
const STAGED_NULL: &str = "NULL";

/// The grid's cell values as plain data: what the view's signals hold, borrowed
/// for one read.
///
/// **The surfaces that *read* the grid resolve a cell here, not in the view.**
/// The painted cell, the clipboard and an AI attachment are all answering "what
/// is in this cell", so a second spelling of the resolution is a second chance
/// to disagree — and each time it has. `attached_rows` first read `rs.cell` and
/// never `dirty`, so a green uncommitted edit was on screen while the pre-edit
/// value went to the model; the fix for *that* left the resolution in the view
/// where nothing could test it, and it then went out one source short again —
/// no [`crate::format::apply`], so a `Timestamp` column sent `1709294400` where
/// the grid showed `2024-03-01 12:00:00`, with the sent-attachment card
/// agreeing with the wrong copy because it is built from the same rows.
///
/// The painter itself is not a caller: it runs per cell per frame inside a
/// reactive closure and reads the signals one at a time. It stays the reference
/// implementation, and [`GridCells::text`] is written to match it.
pub struct GridCells<'a> {
    pub rs: &'a ResultSet,
    /// Display → data row map (`compute_order`); shorter than the result only
    /// before the first sort, where the display index *is* the data index.
    pub order: &'a [usize],
    /// The saved formatter per result column, as the painter reads it.
    pub formats: &'a [crate::format::ColumnFormat],
    /// Staged edits, keyed by (**data** row, column).
    pub dirty: &'a HashMap<(usize, usize), Option<String>>,
    /// Pending new rows, in the display order they are drawn past the real ones.
    pub new_rows: &'a [HashMap<usize, Option<String>>],
}

impl GridCells<'_> {
    /// The text **display** row `i`, column `ci` shows.
    ///
    /// Resolves the sources in the painter's order: a pending new row's typed
    /// value, then a staged edit, then the stored cell.
    ///
    /// `formatted` says whether the column's saved formatter applies. An
    /// attachment passes `true` — its whole promise is that the model is
    /// answering about what the user is looking at. The clipboard passes
    /// `false`: Ctrl+C is raw **by design**, and the cell menu offers *Copy
    /// formatted* as its own entry.
    ///
    /// A staged value is never formatted, because the painter doesn't format
    /// one either — it is the text the user typed, still uncommitted.
    ///
    /// **Raw is [`crate::model::CellRef::display`], not `apply(None, …)`**,
    /// though the painter uses the latter for every column. The two differ on
    /// one thing: `apply` goes through [`crate::model::CellRef::to_value`], so a
    /// `Float` cell the server sent as `1.50` comes back `1.5`. That is a
    /// pre-existing one-glyph divergence between the painter and the clipboard,
    /// and quietly widening it into what Ctrl+C yields is not this reader's to
    /// do.
    pub fn text(&self, i: usize, ci: usize, formatted: bool) -> String {
        let nreal = self.rs.row_count();
        // Display rows past the real ones are the pending new rows, whose values
        // live in `new_rows` — resolving one through `order` would fall back to
        // the display index and read a committed row that isn't it. A pending
        // row has no stored value at all: an unset cell is empty, because what
        // it will hold is a server default the cell previews as `<auto>`.
        if i >= nreal {
            return match self.new_rows.get(i - nreal).and_then(|r| r.get(&ci)) {
                Some(Some(t)) => t.clone(),
                Some(None) => STAGED_NULL.to_string(),
                None => String::new(),
            };
        }
        let di = self.order.get(i).copied().unwrap_or(i);
        match self.dirty.get(&(di, ci)) {
            Some(Some(t)) => t.clone(),
            Some(None) => STAGED_NULL.to_string(),
            None => {
                let fmt = match formatted {
                    true => self.formats.get(ci).copied().unwrap_or_default(),
                    false => crate::format::ColumnFormat::None,
                };
                match self.rs.cell(di, ci) {
                    None => String::new(),
                    Some(c) if fmt == crate::format::ColumnFormat::None => c.display().to_string(),
                    Some(c) => crate::format::apply(fmt, &c.to_value()),
                }
            }
        }
    }

    /// The block `(r0, c0, r1, c1)` as TSV, for the clipboard. Raw values — see
    /// [`GridCells::text`]. [`parse_tsv_block`] is its exact inverse.
    ///
    /// Columns in the order they are **drawn** ([`visual_cols`]), not in index
    /// order: whoever receives the block reads it left to right, and under a
    /// freeze the two disagree.
    pub fn tsv(
        &self,
        (r0, c0, r1, c1): (usize, usize, usize, usize),
        frozen: Option<usize>,
    ) -> String {
        let cols = selected_cols((c0, c1), self.rs.col_count(), frozen);
        let mut out = String::new();
        for i in r0..=r1 {
            if i > r0 {
                out.push('\n');
            }
            for (n, &ci) in cols.iter().enumerate() {
                if n > 0 {
                    out.push('\t');
                }
                out.push_str(&self.text(i, ci, false));
            }
        }
        out
    }

    /// The block `(r0, c0, r1, c1)` as an AI attachment: its column names, its
    /// rows **as the user sees them**, and how many rows were selected in all.
    ///
    /// Two figures, and the header says the second: `cap`
    /// ([`crate::prompt::ATTACH_ROW_CAP`]) is about the context window, not
    /// about consent, so going over it is *reported* rather than silently
    /// applied.
    /// Columns in the order they are **drawn** ([`visual_cols`]), for the same
    /// reason [`GridCells::tsv`] is: the model reads the block as a table, and a
    /// table whose columns are in an order the user never saw is answered about
    /// as though it were the one on screen.
    pub fn attached(
        &self,
        (r0, c0, r1, c1): (usize, usize, usize, usize),
        cap: usize,
        frozen: Option<usize>,
    ) -> (Vec<String>, Vec<Vec<String>>, usize) {
        let cols = selected_cols((c0, c1), self.rs.col_count(), frozen);
        let columns: Vec<String> = cols
            .iter()
            .map(|&ci| {
                self.rs
                    .columns
                    .get(ci)
                    .map(|c| c.name.clone())
                    .unwrap_or_default()
            })
            .collect();
        let (send, total) = attach_span(r0, r1, cap);
        let rows: Vec<Vec<String>> = (r0..=r1)
            .take(send)
            .map(|i| cols.iter().map(|&ci| self.text(i, ci, true)).collect())
            .collect();
        (columns, rows, total)
    }
}

/// How many rows an attachment **sends** and how many the user **selected**,
/// given the cap.
///
/// Two numbers because they differ, and the header has to say the second one:
/// `crate::prompt::ATTACH_ROW_CAP` is about the context window, not about
/// consent, so going over it is reported rather than silently applied. Returning
/// only the capped figure would tell the user 200 rows went when they picked
/// 900.
pub fn attach_span(r0: usize, r1: usize, cap: usize) -> (usize, usize) {
    let total = r1.saturating_sub(r0) + 1;
    (total.min(cap), total)
}

/// The result's columns in the order they are **drawn**: the frozen column
/// first, then every other column in index order.
///
/// A frozen column is pinned to the left of the grid in its own always-visible
/// pane while the data pane renders `(0..ncols).filter(|ci| Some(*ci) != frozen)`
/// — and the cells in it keep their **absolute** index, deliberately, so
/// selection, sort and resize stay consistent. The consequence is that with
/// `frozen = Some(f)`, `f > 0`, visual order is `[f, 0, 1, …, f-1, f+1, …]` and
/// index order is unchanged, so anything that walks a column *range* is walking
/// it in an order the user is not looking at.
///
/// That is fine for a rectangle's *membership* (which columns are selected) and
/// wrong for everything that is about adjacency or reading order:
///
/// - a paste extends past the anchor into the columns drawn beside it, not the
///   ones indexed beside it — otherwise a block pasted onto `email` writes into
///   the frozen column at the far left, which the user never pointed at;
/// - a clipboard or attachment block is read left to right by whoever receives
///   it, so its columns have to be in the order they were on screen.
///
/// One list, because the two have to agree: the grid's own
/// `scroll_active_into_view` already sums widths with the same filter, and a
/// second spelling of the order is a second chance to disagree with it.
///
/// A `frozen` index at or past `ncols` is ignored — it cannot be drawn, so it
/// does not move anything.
pub fn visual_cols(ncols: usize, frozen: Option<usize>) -> Vec<usize> {
    match frozen.filter(|f| *f < ncols) {
        Some(f) => std::iter::once(f)
            .chain((0..ncols).filter(move |c| *c != f))
            .collect(),
        None => (0..ncols).collect(),
    }
}

/// The columns a selection rectangle covers, in the order they are drawn.
///
/// Membership is the absolute range `c0..=c1` — that is what the grid paints as
/// highlighted, and which columns are selected is settled; only their order is
/// in question here. `ncols` is widened to include `c1` so a rectangle reaching
/// past the result's last column still yields the same positions it always did
/// (the cells there read as empty rather than vanishing).
fn selected_cols((c0, c1): (usize, usize), ncols: usize, frozen: Option<usize>) -> Vec<usize> {
    visual_cols(ncols.max(c1 + 1), frozen)
        .into_iter()
        .filter(|ci| (c0..=c1).contains(ci))
        .collect()
}

/// A clipboard block, parsed for pasting into the grid: rows of cell text.
///
/// **The exact inverse of [`GridCells::tsv`]** — lines split on `\n` (a trailing
/// `\r` dropped, so a Windows clipboard behaves), cells split on `\t`, and *no
/// quote interpretation at all*. The symmetry is the whole rule and it is worth
/// stating, because a CSV-style parser here would be the obvious mistake: this
/// codebase's copy side emits no quoting, so there is none to undo, and
/// unquoting would silently turn a cell whose value genuinely is `"hello"` —
/// ordinary in a database, and exactly what a user is most likely to be moving
/// between rows — into `hello`. The cost is that a spreadsheet cell containing
/// a newline arrives as two rows; that is the rarer wrong answer, and it is
/// visible in the grid rather than silent.
///
/// A trailing newline (which every spreadsheet appends) is not a row. Text that
/// is entirely empty yields no rows at all.
pub fn parse_tsv_block(text: &str) -> Vec<Vec<String>> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    let body = body.strip_suffix('\r').unwrap_or(body);
    if body.is_empty() {
        return Vec::new();
    }
    body.split('\n')
        .map(|line| {
            line.strip_suffix('\r')
                .unwrap_or(line)
                .split('\t')
                .map(str::to_string)
                .collect()
        })
        .collect()
}

/// The most cells one paste may stage.
///
/// **A paste is not bounded by what was copied.** A single copied cell fills the
/// whole selection by design (see [`plan_paste`]), and Ctrl+A selects every
/// display cell — so on a result at the 200k-row cap, Ctrl+A then Ctrl+V of one
/// cell asked for ~6M staged edits: a plan of 6M cloned strings, 6M entries in
/// `dirty`, every derived view recomputing over them, and a commit of 200k
/// `UPDATE`s behind one transaction. The window stops answering long before the
/// user gets to the Discard button that would undo it.
///
/// 50k is chosen to be far above any paste somebody assembled by hand and far
/// below the point where the grid stops being interactive. It is not a
/// correctness boundary: what it refuses is *reported* ([`PasteReport`]), because
/// a paste that quietly stopped two thirds of the way down a column is the
/// failure this whole file's counters exist to prevent. Anyone who genuinely
/// means "set this column for every row" wants one `UPDATE`, not 200k of them.
pub const PASTE_CELL_CAP: usize = 50_000;

/// The most **positions** one paste may walk, staged or not.
///
/// [`PASTE_CELL_CAP`] bounds what a paste costs in memory; this bounds what it
/// costs in time, and they are not the same gesture. A cell skipped as off-grid
/// or read-only rightly consumes no staging budget — so a result with no editable
/// column (every column an expression: a `CONCAT`, a `YEAR(…)`, a `salary*1.1`)
/// never reaches the staged cap at all, and Ctrl+A then Ctrl+V asked the walk to
/// visit every display cell of the whole result: 600k positions on a 200k × 3,
/// twelve million on a 200k × 60. On the UI thread, for a paste whose entire
/// outcome is "Nothing pasted".
///
/// Four times the staged cap: a paste may skip four positions for every one it
/// lands and still complete, which covers a mis-aimed selection with room to
/// spare, and what it declines to visit is *reported* ([`PastePlan::capped`])
/// rather than dropped quietly.
pub const PASTE_VISIT_CAP: usize = 4 * PASTE_CELL_CAP;

/// Where a pasted block lands, and what of it doesn't.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PastePlan {
    /// `(display row, column, value)` for every cell that will be staged.
    pub cells: Vec<(usize, usize, String)>,
    /// Cells the block carried that fell off the bottom or right of the grid.
    /// **Reported, never silent**: a paste that quietly discarded half a
    /// spreadsheet would look like it worked.
    pub dropped: usize,
    /// Cells that landed on a column no edit can reach (an expression, a binary
    /// column, a generated column). Skipped in place rather than shifted, so
    /// the columns either side still line up with what was copied.
    pub read_only: usize,
    /// Cells past [`PASTE_CELL_CAP`]. Counted whole and **not** classified: the
    /// plan stops walking at the cap rather than visiting six million positions
    /// to find out which of them would also have been off-grid.
    pub capped: usize,
}

/// Lay a parsed clipboard block over the grid, anchored at the selection.
///
/// Two shapes, because they are the two things people actually do:
///
/// - **A single copied cell fills the whole selection.** Selecting a column's
///   worth of cells and pasting one value is how a column gets set to a
///   constant, and refusing it would leave the user pasting the same thing N
///   times.
/// - **Anything larger extends from the selection's top-left**, whatever the
///   selection's own size. A block is a shape; honouring the selection's shape
///   instead would mean either truncating what was copied or tiling it, and
///   both guess at an intent the user did not express.
///
/// Everything is clipped to the grid — `rows` counts the *display* rows,
/// pending new rows included, so a paste can fill rows the user just added —
/// and what falls outside is counted rather than dropped quietly. `editable`
/// answers per column; a cell over a read-only column is skipped **in place**,
/// never shifted onto the next column, which would write the wrong data into a
/// column that happens to accept it.
///
/// **`frozen` is what "extends" means.** A block extends into the columns drawn
/// beside the anchor, which under a freeze are not the ones indexed beside it
/// ([`visual_cols`]): with `ssn` frozen out of `(id, name, email, ssn, notes)`
/// the grid draws `[ssn][id][name][email][notes]`, so a two-wide block dropped
/// on `email` fills `email` and `notes`. Walking the index range instead put the
/// second value into `ssn` — a column at the far *left* of the screen that the
/// user never pointed at, and one `UPDATE … SET ssn = 'hello'` away from
/// destroying it. The row half of the same walk has always converted display →
/// data through `order`; this is the column half of that conversion.
///
/// The **single-value** case is the exception, and not one: it fills the
/// selection, so its columns are the selected ones — the cells already painted
/// as highlighted — and no translation applies.
pub fn plan_paste(
    block: &[Vec<String>],
    (r0, c0, r1, c1): (usize, usize, usize, usize),
    rows: usize,
    cols: usize,
    frozen: Option<usize>,
    editable: impl Fn(usize) -> bool,
) -> PastePlan {
    let mut plan = PastePlan::default();
    if block.is_empty() || rows == 0 || cols == 0 {
        return plan;
    }
    let single = block.len() == 1 && block[0].len() == 1;
    // A single value covers the selection; anything else covers its own shape.
    let (span_r, span_c) = if single {
        (r1.saturating_sub(r0) + 1, c1.saturating_sub(c0) + 1)
    } else {
        (
            block.len(),
            block.iter().map(Vec::len).max().unwrap_or_default(),
        )
    };
    // Every position the block actually carries a value for — the ragged gaps of
    // a short row are not in it. Known up front so the walk can stop at the cap
    // and still say how much it left, without visiting the rest.
    let carried: usize = if single {
        span_r.saturating_mul(span_c)
    } else {
        block.iter().map(Vec::len).sum()
    };
    // The column the `dc`-th value of a row lands in. A single value fills the
    // selection, so those are the selected columns; a block extends from the
    // anchor in **draw** order, which is what the user pointed along.
    let target_cols: Vec<usize> = if single {
        (c0..=c1).collect()
    } else {
        let visual = visual_cols(cols, frozen);
        match visual.iter().position(|&ci| ci == c0) {
            Some(at) => visual[at..].to_vec(),
            // The anchor is not a column of this result. Nothing lands, and the
            // walk counts every value as dropped rather than sliding the block
            // onto whatever column happens to be first.
            None => Vec::new(),
        }
    };
    let mut seen = 0usize;
    'block: for dr in 0..span_r {
        for dc in 0..span_c {
            // **Two ceilings, because there are two costs.** The staged count is
            // the memory one, and a cell skipped as off-grid or read-only costs
            // nothing to stage, so it must not consume that budget. `seen` is the
            // *walk*: a selection is the user's whole result, so a paste that
            // stages nothing can still be asked to visit millions of positions —
            // Ctrl+A on a 200k-row expression result is 600k of them, and the
            // gesture `PASTE_CELL_CAP`'s doc names is exactly this one.
            if plan.cells.len() >= PASTE_CELL_CAP || seen >= PASTE_VISIT_CAP {
                plan.capped = carried.saturating_sub(seen);
                break 'block;
            }
            // **Does the block carry a value here at all**, asked without taking
            // one. A ragged block's short row has nothing to write and nothing
            // lost — the cell already there stays.
            if !single && block.get(dr).and_then(|r| r.get(dc)).is_none() {
                continue;
            }
            seen += 1;
            let row = r0 + dr;
            let Some(&col) = target_cols.get(dc) else {
                // Past the right edge of the grid — the same drop the index walk
                // counted when `c0 + dc` ran off the end.
                plan.dropped += 1;
                continue;
            };
            if row >= rows || col >= cols {
                plan.dropped += 1;
                continue;
            }
            if !editable(col) {
                plan.read_only += 1;
                continue;
            }
            // **Cloned last.** The two tests above need neither the value nor an
            // allocation, and they were below it: a paste over a result with no
            // editable column performed one heap allocation per selected position
            // and then discarded it, which is a visible freeze for a gesture whose
            // whole outcome is "Nothing pasted".
            let value = if single {
                block[0][0].clone()
            } else {
                block[dr][dc].clone()
            };
            plan.cells.push((row, col, value));
        }
    }
    plan
}

/// What a finished paste has to say for itself, and **in which voice**.
///
/// The grid's bottom bar has two surfaces: a red one for a failure, and the
/// ordinary chrome for a note. A partial paste went out on the red one, so
/// "Pasted 5 cells, skipping 1 in read-only columns." — an ordinary success with
/// a caveat — was indistinguishable from a write-back that failed. The
/// distinction is not a rendering detail: the red bar is the one that means
/// *nothing landed and you must do something*.
///
/// Split out of the view so the decision can be tested. It is the whole of it —
/// which sentence, and which surface — and the caller only picks a signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteReport {
    /// Every cell landed. The bar stays down.
    Clean,
    /// Some landed and some didn't: a success, with what was skipped named.
    Notice(String),
    /// **Nothing** landed. A read-only result (a join, an expression column) is
    /// the case, and it is the one where the user most needs telling why the
    /// paste appeared to do nothing at all.
    Failed(String),
}

/// Everything [`paste_report`] needs from a [`PastePlan`], taken **before**
/// staging consumes its `cells`.
///
/// A snapshot rather than a borrow of the plan, because the caller drains the
/// cell list into the grid — reading `plan.cells.len()` afterwards would report
/// every paste as having landed nothing. Named fields rather than three
/// positional `usize`s, which transpose silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasteCounts {
    pub dropped: usize,
    pub read_only: usize,
    pub capped: usize,
}

/// What a **blank** value means when it is staged into a pending new row.
///
/// A pending row has no original to diff against, so a blank has to mean
/// something chosen rather than something derived — and the two callers want
/// different things:
///
/// * [`BlankCell::UnsetsIt`] — a *typed* blank. The user cleared a cell they had
///   typed into, so the column goes back to unset and the `INSERT` omits it,
///   letting the server's default fill it. Undoing an edit is what clearing a
///   field means.
/// * [`BlankCell::IsAValue`] — a **pasted** blank. The clipboard says this cell is
///   empty, and a paste is an assertion about values rather than an undo. This is
///   the arm that did not exist: `stage_new_many` applied `UnsetsIt` to everything,
///   while `stage_many` (the real-row half, three lines away) staged `''`. So one
///   block wrote `''` above the pending-row boundary and the *server default*
///   below it — the same clipboard cell, two stored values, decided by a line the
///   user cannot see. It also discarded a value the user had typed into a pending
///   row when the pasted cell over it happened to be empty.
///
/// NULL is a separate question and is not this one's: a pasted cell reading `NULL`
/// stages the four-character string, deliberately, and whether it should is the
/// product decision `S5-L1-01` carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlankCell {
    /// A blank clears the cell back to "let the server decide".
    UnsetsIt,
    /// A blank is the empty string, as it is on a real row.
    IsAValue,
}

/// The entry a staged value produces in a pending row: `None` means *remove the
/// column* (so the `INSERT` omits it), `Some(v)` means store `v`.
///
/// Only a blank is ambiguous — see [`BlankCell`]. SQL NULL (`None`) is an explicit
/// value on either reading and is always stored.
pub fn pending_cell(val: Option<String>, blank: BlankCell) -> Option<Option<String>> {
    match &val {
        Some(s) if s.is_empty() && blank == BlankCell::UnsetsIt => None,
        _ => Some(val),
    }
}

impl PastePlan {
    /// This plan's counts, for [`paste_report`]. Call it before staging.
    pub fn counts(&self) -> PasteCounts {
        PasteCounts {
            dropped: self.dropped,
            read_only: self.read_only,
            capped: self.capped,
        }
    }
}

/// Turn a plan's counts, plus what the caller itself skipped, into the sentence
/// and the surface.
///
/// `skipped_deleted` is the view's own: a row marked for deletion is a display
/// state the plan knows nothing about. What actually *landed* is what decides
/// between the two voices — not whether anything was skipped.
///
/// **`staged` is counted by the caller, after staging, and is not derived here.**
/// It used to be `planned - skipped_deleted`, which is what the plan *intended* —
/// and staging drops entries the plan cannot know about: `stage_many` un-stages a
/// cell pasted back over its own original value, and `stage_new_many` removes a
/// column whose pasted cell is blank. So pasting a column's own values over
/// itself reported `Pasted N cells` while `dirty` gained nothing at all. The two
/// `stage_*_many` calls return what they changed and the caller sums them.
///
/// **Every figure is grouped and every noun agrees**, because this sentence
/// shares a bar with the stats line: `Pasted 1 cells` four pixels from
/// `200k of ~292.02k rows` reads as a bug in the paste, and the range's own code
/// says why the helpers are not optional — `save_export`'s note carries a comment
/// about `plural` returning the noun and nothing else, after forgetting the count
/// once produced "Exported rows to employees.csv".
pub fn paste_report(counts: PasteCounts, skipped_deleted: usize, staged: usize) -> PasteReport {
    let n = crate::text::human_count;
    let mut notes = Vec::new();
    if counts.dropped > 0 {
        notes.push(format!("{} outside the grid", n(counts.dropped)));
    }
    if counts.read_only > 0 {
        notes.push(format!("{} in read-only columns", n(counts.read_only)));
    }
    if counts.capped > 0 {
        // The limit is named, not just the overflow: "skipping 5,950,000" with no
        // reason reads as a bug, and the number is what tells the user this was a
        // ceiling rather than something about their data.
        notes.push(format!(
            "{} over the {}-cell paste limit",
            n(counts.capped),
            n(PASTE_CELL_CAP)
        ));
    }
    if skipped_deleted > 0 {
        notes.push(format!(
            "{} in rows marked for deletion",
            n(skipped_deleted)
        ));
    }
    if notes.is_empty() {
        return PasteReport::Clean;
    }
    if staged == 0 {
        PasteReport::Failed(format!("Nothing pasted: {}.", notes.join(", ")))
    } else {
        PasteReport::Notice(format!(
            "Pasted {} {}, skipping {}.",
            n(staged),
            crate::text::plural(staged, "cell", "cells"),
            notes.join(", ")
        ))
    }
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

    /// C1's **other** shape, and the one it now carries the safety of. A
    /// `SELECT a, * FROM t` is one table, not a self-join: it exposes `a` twice
    /// and everything else once. Before `ed7e60c` widened `projection_of` such a
    /// statement had no origins at all and was read-only by construction; now
    /// every column is attributed and this check is the only thing left refusing
    /// it. Relax the rule to "a duplicate across two tables" and an `UPDATE t SET
    /// a = ?, a = ?` becomes reachable, with nothing else failing.
    #[test]
    fn c1_holds_for_one_column_duplicated_within_a_single_table() {
        let r = rs(vec![
            col("a", "TEXT", "t", false, false), // the leading item
            col("id", "INT", "t", true, false),  // …and the wildcard behind it
            col("a", "TEXT", "t", false, false),
        ]);
        let schema = |_db: &str, _s: Option<&str>, tbl: &str| {
            (tbl == "t").then(|| schema_with_pk("t", &["id"], &[("id", "int"), ("a", "text")]))
        };
        let m = analyze_edit(&r, schema);
        assert!(m.insert_target().is_none(), "no row can be identified");
        for ci in 0..3 {
            assert!(!m.editable(ci), "column {ci}");
        }
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

    /// **A sorted grid's display index is not its data index**, and the gutter's
    /// destructive entries act on the second. The write-back's 1-row net checks
    /// the *count*, never the identity, so an inverted mapping deletes the wrong
    /// row and reports success.
    #[test]
    fn a_gutter_gesture_acts_on_the_data_rows_the_display_rows_stand_for() {
        // Sorted descending: display 0 is data row 2.
        let order = [2usize, 1, 0];

        // Clicked inside the selection → the whole selection, in display order.
        assert_eq!(
            selected_data_rows(&order, Some((0, 1)), 1),
            vec![2, 1],
            "display rows 0..=1 are data rows 2 and 1"
        );
        // Clicked outside it → the row that was pointed at, and only that one.
        assert_eq!(selected_data_rows(&order, Some((0, 1)), 2), vec![0]);
        // No selection at all is the same gesture.
        assert_eq!(selected_data_rows(&order, None, 0), vec![2]);
        // Pending new rows live past `order` and have no committed row to act on.
        assert_eq!(selected_data_rows(&order, Some((1, 5)), 3), vec![1, 0]);
        assert!(selected_data_rows(&order, None, 9).is_empty());
        assert!(selected_data_rows(&[], None, 0).is_empty());
    }

    /// The cap is about the context window, not about consent — so it is
    /// reported rather than silently applied, which needs both figures.
    #[test]
    fn an_attachment_says_how_many_were_picked_as_well_as_how_many_go() {
        assert_eq!(attach_span(0, 2, 200), (3, 3));
        assert_eq!(attach_span(0, 899, 200), (200, 900));
        // One row is one row, not zero.
        assert_eq!(attach_span(5, 5, 200), (1, 1));
    }

    /// **A right-click inside a block is about the block, and the two amounts
    /// get two words.** Preserving the selection through a right-click was the
    /// point of the change; an entry reading "Copy" that took one cell out of
    /// nine said the same word Ctrl+C and the gutter menu's Copy say for three
    /// different amounts.
    ///
    /// Each case below has been wrong once: the `r0 != r1 || c0 != c1` guard is
    /// the easily-dropped first conjunct that keeps a lone cell out of
    /// `Selection`, a click outside the rectangle must be about the cell even
    /// when a block is live, and a one-row or one-column block is still a
    /// block. The labels are pinned by name because they read **inverted from
    /// intuition** — `Cell` is the one that says "Copy value".
    #[test]
    fn a_right_click_inside_a_block_copies_the_block() {
        let block = Some((0, 0, 2, 2));
        assert_eq!(copy_scope(block, 1, 1), CopyScope::Selection);
        // Outside the rectangle — the menu acts on what was clicked.
        assert_eq!(copy_scope(block, 5, 1), CopyScope::Cell);
        assert_eq!(copy_scope(block, 1, 5), CopyScope::Cell);
        // A degenerate "block" of one cell is a cell.
        assert_eq!(copy_scope(Some((1, 1, 1, 1)), 1, 1), CopyScope::Cell);
        // …but one row of four columns, or one column of four rows, is a block.
        assert_eq!(copy_scope(Some((0, 0, 0, 3)), 0, 2), CopyScope::Selection);
        assert_eq!(copy_scope(Some((0, 0, 3, 0)), 2, 0), CopyScope::Selection);
        // Nothing selected at all.
        assert_eq!(copy_scope(None, 0, 0), CopyScope::Cell);

        assert_eq!(CopyScope::Cell.label(), "Copy value");
        assert_eq!(CopyScope::Selection.label(), "Copy");
    }

    // ── what the surfaces that *read* the grid see ───────────────────────────

    /// `placed_at | note`, two committed rows, sorted so the display order is
    /// the reverse of the data order — the case where reading a cell by the
    /// display index silently answers about the wrong row.
    fn cells_fixture() -> (ResultSet, Vec<usize>, Vec<crate::format::ColumnFormat>) {
        let rs = ResultSet::from_rows(
            vec![
                Column {
                    name: "placed_at".into(),
                    type_name: "BIGINT".into(),
                    origin: None,
                },
                Column {
                    name: "note".into(),
                    type_name: "VARCHAR".into(),
                    origin: None,
                },
            ],
            vec![
                vec![Value::Int(1_709_294_400), Value::Str("first".into())],
                vec![Value::Int(1_709_380_800), Value::Str("second".into())],
            ],
        );
        (
            rs,
            vec![1, 0],
            vec![
                crate::format::ColumnFormat::Timestamp,
                crate::format::ColumnFormat::None,
            ],
        )
    }

    fn cells<'a>(
        rs: &'a ResultSet,
        order: &'a [usize],
        formats: &'a [crate::format::ColumnFormat],
        dirty: &'a HashMap<(usize, usize), Option<String>>,
        new_rows: &'a [HashMap<usize, Option<String>>],
    ) -> GridCells<'a> {
        GridCells {
            rs,
            order,
            formats,
            dirty,
            new_rows,
        }
    }

    /// **An attachment is answered about as though it were the grid**, which is
    /// the reason its doc gives for reading the displayed value rather than the
    /// stored one. It resolved three of the painter's four sources and never
    /// [`crate::format::apply`], so a `Timestamp` column sent `1709294400`
    /// where the cell showed `2024-03-01 12:00:00` — and the sent-attachment
    /// card the user opens to check agreed with the wrong copy, because it is
    /// built from the same rows.
    #[test]
    fn an_attachment_reads_the_column_the_way_the_grid_paints_it() {
        let (rs, order, formats) = cells_fixture();
        let (dirty, new_rows) = (HashMap::new(), Vec::new());
        let g = cells(&rs, &order, &formats, &dirty, &new_rows);
        let (columns, rows, total) = g.attached((0, 0, 1, 1), 200, None);
        assert_eq!(columns, vec!["placed_at", "note"]);
        assert_eq!(total, 2);
        // Display row 0 is data row 1 — `order` is not the identity here.
        assert_eq!(
            rows,
            vec![
                vec!["2024-03-02 12:00:00".to_string(), "second".to_string()],
                vec!["2024-03-01 12:00:00".to_string(), "first".to_string()],
            ]
        );
    }

    /// The other side of the same parameter: Ctrl+C is raw **by design** — the
    /// cell menu offers *Copy formatted* as its own entry — so the clipboard
    /// must not start following the formatter along with the attachment.
    #[test]
    fn the_clipboard_takes_the_stored_value_not_the_formatted_one() {
        let (rs, order, formats) = cells_fixture();
        let (dirty, new_rows) = (HashMap::new(), Vec::new());
        let g = cells(&rs, &order, &formats, &dirty, &new_rows);
        assert_eq!(
            g.tsv((0, 0, 1, 1), None),
            "1709380800\tsecond\n1709294400\tfirst"
        );
    }

    /// **Under a freeze, the clipboard reads left to right.** `note` frozen out
    /// of `(placed_at, note)` draws as `[note][placed_at]`, so a selection over
    /// both columns has to be written in that order: whoever receives the block
    /// — a spreadsheet, another grid, a text file — has nothing but the order to
    /// go on, and the columns are transposed against the screen without it.
    #[test]
    fn the_clipboard_writes_the_columns_in_the_order_they_are_drawn() {
        let (rs, order, formats) = cells_fixture();
        let (dirty, new_rows) = (HashMap::new(), Vec::new());
        let g = cells(&rs, &order, &formats, &dirty, &new_rows);
        assert_eq!(
            g.tsv((0, 0, 1, 1), Some(1)),
            "second\t1709380800\nfirst\t1709294400"
        );
        // The attachment says the same thing, names included — the model reads
        // it as a table.
        let (columns, rows, _) = g.attached((0, 0, 1, 1), 200, Some(1));
        assert_eq!(columns, vec!["note", "placed_at"]);
        assert_eq!(
            rows[0],
            vec!["second".to_string(), "2024-03-02 12:00:00".to_string()]
        );
        // Freezing the leftmost column changes nothing, and neither does a
        // selection that doesn't reach the frozen one.
        assert_eq!(
            g.tsv((0, 0, 1, 1), Some(0)),
            "1709380800\tsecond\n1709294400\tfirst"
        );
        assert_eq!(g.tsv((0, 0, 1, 0), Some(1)), "1709380800\n1709294400");
    }

    /// **The composition, which is where this bug lived.** Copy under a freeze
    /// and paste it straight back: the values have to return to the columns they
    /// came from. Both halves walk a column range, and either one alone reading
    /// draw order (or index order) puts them back transposed — a half-applied
    /// map is worse than none, so this is the test that holds the pair together.
    #[test]
    fn a_copy_and_a_paste_under_a_freeze_agree_on_the_columns() {
        let (rs, order, formats) = cells_fixture();
        let (dirty, new_rows) = (HashMap::new(), Vec::new());
        let g = cells(&rs, &order, &formats, &dirty, &new_rows);
        // Display row 0, both columns, with `note` (abs 1) frozen and therefore
        // drawn first.
        let copied = g.tsv((0, 0, 0, 1), Some(1));
        let block = parse_tsv_block(&copied);
        // Pasted back onto the cell it was copied from: the anchor is the
        // leftmost *drawn* column of the selection, which is the frozen one.
        let plan = plan_paste(&block, (0, 1, 0, 1), 2, 2, Some(1), all_editable);
        assert_eq!(
            plan.cells,
            vec![
                (0, 1, "second".to_string()),
                (0, 0, "1709380800".to_string()),
            ],
            "the round trip must restore `note` to `note` and `placed_at` to \
             `placed_at`; copied block was {copied:?}"
        );
    }

    /// A staged edit is on screen and uncommitted; both surfaces have to show
    /// it, and a staged SQL NULL reads as the word the cell paints. Neither is
    /// formatted — the painter doesn't format one either, since it is the text
    /// the user just typed.
    #[test]
    fn a_staged_edit_is_what_both_surfaces_read() {
        let (rs, order, formats) = cells_fixture();
        let new_rows = Vec::new();
        // Data row 1 = display row 0.
        let dirty: HashMap<(usize, usize), Option<String>> =
            [((1, 0), Some("999".to_string())), ((0, 1), None)]
                .into_iter()
                .collect();
        let g = cells(&rs, &order, &formats, &dirty, &new_rows);
        assert_eq!(g.tsv((0, 0, 1, 1), None), "999\tsecond\n1709294400\tNULL");
        assert_eq!(
            g.attached((0, 0, 1, 1), 200, None).1,
            vec![
                vec!["999".to_string(), "second".to_string()],
                vec!["2024-03-01 12:00:00".to_string(), "NULL".to_string()],
            ]
        );
    }

    /// A pending new row is drawn past the real ones and has no committed row
    /// behind it: resolving it through `order` would fall back to the display
    /// index and read a committed row that isn't it. An unset cell is empty,
    /// not `NULL` — what it will hold is a server default the cell previews as
    /// `<auto>`.
    #[test]
    fn a_pending_new_row_reads_only_what_was_typed() {
        let (rs, order, formats) = cells_fixture();
        let dirty = HashMap::new();
        let new_rows: Vec<HashMap<usize, Option<String>>> = vec![
            [(0, Some("42".to_string())), (1, None)]
                .into_iter()
                .collect(),
        ];
        let g = cells(&rs, &order, &formats, &dirty, &new_rows);
        assert_eq!(g.text(2, 0, true), "42");
        assert_eq!(g.text(2, 1, true), "NULL");
        // A row past `new_rows` too, and a column nobody typed into.
        let empty: Vec<HashMap<usize, Option<String>>> = Vec::new();
        let g = cells(&rs, &order, &formats, &dirty, &empty);
        assert_eq!(g.text(2, 0, true), "");
    }

    /// The cap is reported, not silently applied, and the columns come back
    /// whatever the row cap does.
    #[test]
    fn an_attachment_over_the_cap_still_says_how_many_were_picked() {
        let (rs, order, formats) = cells_fixture();
        let (dirty, new_rows) = (HashMap::new(), Vec::new());
        let g = cells(&rs, &order, &formats, &dirty, &new_rows);
        let (_, rows, total) = g.attached((0, 0, 1, 1), 1, None);
        assert_eq!(rows.len(), 1);
        assert_eq!(total, 2);
    }

    /// **An expression-only unique index keys nothing.** PostgreSQL models
    /// `CREATE UNIQUE INDEX ON u (lower(email))` as a real index over one
    /// expression, and `column_names()` skips expressions — so the `find_map`
    /// used to answer `all_present(&[]) == Some(vec![])`, an **empty write
    /// key**, which builds `… WHERE ` with nothing after it and hides the plain
    /// unique index sorted behind it.
    #[test]
    fn an_expression_only_unique_index_is_not_a_write_key() {
        let r = rs(vec![
            col("email", "VARCHAR", "u", false, false),
            col("code", "VARCHAR", "u", false, false),
        ]);
        let column = |name: &str| ColumnInfo {
            name: name.to_string(),
            type_name: "varchar".to_string(),
            nullable: false,
            ..Default::default()
        };
        let expr = crate::schema::IndexInfo {
            name: "a_expr".to_string(),
            unique: true,
            columns: vec![crate::schema::IndexColumn::expr("lower(email)")],
            ..Default::default()
        };
        let schema = move |_db: &str, _s: Option<&str>, t: &str| {
            (t == "u").then(|| TableInfo {
                schema: None,
                name: "u".to_string(),
                columns: vec![column("email"), column("code")],
                // The expression index sorts first, exactly as `index_list_sql`
                // returns it.
                indexes: vec![
                    expr.clone(),
                    crate::schema::IndexInfo::plain("b_code", vec!["code"], true),
                ],
                ..Default::default()
            })
        };
        let m = analyze_edit(&r, schema);
        let t = refetch_template(&r, &m).expect("the plain unique index is still a usable key");
        assert_eq!(t.key_cols, vec![1]); // code, not an empty key
    }

    // ── Pasting a clipboard block ────────────────────────────────────────────

    /// A lone value is the common paste, and it must arrive verbatim — not as
    /// an empty row, not trimmed.
    #[test]
    fn a_single_cell_parses_as_one_row_of_one() {
        assert_eq!(parse_tsv_block("hello"), vec![vec!["hello".to_string()]]);
        assert_eq!(parse_tsv_block(" 42 "), vec![vec![" 42 ".to_string()]]);
    }

    #[test]
    fn a_block_splits_on_tabs_and_newlines() {
        assert_eq!(
            parse_tsv_block("a\tb\nc\td"),
            vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string(), "d".to_string()]
            ]
        );
    }

    /// Every spreadsheet appends a newline to the block it copies, and Windows
    /// makes it a CRLF. Neither is a row, and neither belongs on the last cell.
    #[test]
    fn a_trailing_line_break_is_not_a_row() {
        assert_eq!(parse_tsv_block("a\tb\n").len(), 1);
        assert_eq!(
            parse_tsv_block("a\tb\r\n"),
            vec![vec!["a".to_string(), "b".to_string()]]
        );
        assert_eq!(
            parse_tsv_block("a\r\nb\r\n"),
            vec![vec!["a".to_string()], vec!["b".to_string()]]
        );
    }

    #[test]
    fn an_empty_clipboard_is_no_block_at_all() {
        assert!(parse_tsv_block("").is_empty());
        assert!(parse_tsv_block("\n").is_empty());
    }

    /// An empty cell in the middle of a row is a value — SQL empty string, or a
    /// cell the user cleared — and dropping it would shift every column after
    /// it one to the left.
    #[test]
    fn an_empty_cell_holds_its_place() {
        assert_eq!(
            parse_tsv_block("a\t\tc"),
            vec![vec!["a".to_string(), String::new(), "c".to_string()]]
        );
    }

    /// **The rule this parser exists to keep.** A CSV-style reader would
    /// unquote this and hand back `hello` — a silent edit to the user's data,
    /// on a value shape that is ordinary in a database. The copy side emits no
    /// quoting, so there is none to undo.
    #[test]
    fn a_quoted_looking_value_is_left_exactly_as_it_is() {
        assert_eq!(
            parse_tsv_block("\"hello\""),
            vec![vec!["\"hello\"".to_string()]]
        );
        assert_eq!(
            parse_tsv_block("{\"a\": 1}"),
            vec![vec!["{\"a\": 1}".to_string()]]
        );
    }

    fn all_editable(_: usize) -> bool {
        true
    }

    // ── The frozen column: draw order vs index order ─────────────────────────

    /// The list every surface that cares about adjacency or reading order shares
    /// with the data pane's own `(0..ncols).filter(|ci| Some(*ci) != frozen)`.
    #[test]
    fn the_frozen_column_is_drawn_first_and_the_rest_keep_their_order() {
        assert_eq!(visual_cols(5, Some(3)), vec![3, 0, 1, 2, 4]);
        // Frozen at the left edge, or nothing frozen: index order *is* draw order.
        assert_eq!(visual_cols(5, Some(0)), vec![0, 1, 2, 3, 4]);
        assert_eq!(visual_cols(5, None), vec![0, 1, 2, 3, 4]);
        // A column that cannot be drawn moves nothing.
        assert_eq!(visual_cols(3, Some(7)), vec![0, 1, 2]);
        assert_eq!(visual_cols(0, Some(0)), Vec::<usize>::new());
    }

    /// **The paste lands where the user pointed.** `(id, name, email, ssn,
    /// notes)` with `ssn` (abs 3) frozen draws as `[ssn][id][name][email][notes]`;
    /// a 1×2 block dropped on `email` must fill `email` and the column drawn to
    /// its right, `notes` (abs 4) — not `ssn`, which is at the far left of the
    /// screen and is the column the user froze to keep an eye on.
    #[test]
    fn a_paste_under_a_frozen_column_lands_where_the_user_pointed() {
        let block = parse_tsv_block("a@b.com\thello");
        let plan = plan_paste(&block, (0, 2, 0, 2), 4, 5, Some(3), all_editable);
        assert_eq!(
            plan.cells,
            vec![(0, 2, "a@b.com".to_string()), (0, 4, "hello".to_string()),]
        );
        assert_eq!(plan.dropped, 0);
        // Anchored *on* the frozen column, a 3-wide block fills the three
        // leftmost columns on screen: `ssn`, then `id`, then `name`.
        let block = parse_tsv_block("x\ty\tz");
        let plan = plan_paste(&block, (0, 3, 0, 3), 4, 5, Some(3), all_editable);
        assert_eq!(
            plan.cells,
            vec![
                (0, 3, "x".to_string()),
                (0, 0, "y".to_string()),
                (0, 1, "z".to_string()),
            ]
        );
        // And with nothing frozen the walk is the index walk, unchanged.
        let plan = plan_paste(&block, (0, 1, 0, 1), 4, 5, None, all_editable);
        assert_eq!(
            plan.cells,
            vec![
                (0, 1, "x".to_string()),
                (0, 2, "y".to_string()),
                (0, 3, "z".to_string()),
            ]
        );
    }

    /// A block anchored near the right edge still drops what falls off it, and
    /// counts the drop — the frozen column changes *which* columns are walked,
    /// never how much is silently lost.
    #[test]
    fn a_paste_past_the_last_drawn_column_is_still_dropped_and_counted() {
        let block = parse_tsv_block("x\ty\tz");
        // Visual order `[3, 0, 1, 2, 4]`, anchored on abs 2 (drawn 4th): only
        // `2` and `4` are left, so the third value has nowhere to go.
        let plan = plan_paste(&block, (0, 2, 0, 2), 4, 5, Some(3), all_editable);
        assert_eq!(
            plan.cells,
            vec![(0, 2, "x".to_string()), (0, 4, "y".to_string())]
        );
        assert_eq!(plan.dropped, 1);
    }

    /// **A single value fills the selection**, and the selection is what the
    /// grid paints as highlighted — the absolute range. Extending it along the
    /// draw order instead would write into cells that were never lit up.
    #[test]
    fn one_value_over_a_selection_that_crosses_the_freeze_fills_the_selection() {
        let block = parse_tsv_block("x");
        let plan = plan_paste(&block, (0, 1, 0, 3), 4, 5, Some(2), all_editable);
        let mut cols: Vec<usize> = plan.cells.iter().map(|(_, c, _)| *c).collect();
        cols.sort_unstable();
        assert_eq!(cols, vec![1, 2, 3]);
    }

    /// Copy a block out of the grid, paste it back: the same values, in the
    /// same places. The two functions are inverses and this is the assertion
    /// that says so — the property a reader of either one would assume.
    #[test]
    fn a_block_round_trips_through_the_clipboard_shape() {
        let block = parse_tsv_block("a\tb\tc\nd\te\tf");
        let plan = plan_paste(&block, (0, 0, 0, 0), 10, 10, None, all_editable);
        assert_eq!(plan.dropped, 0);
        assert_eq!(plan.cells.len(), 6);
        assert!(plan.cells.contains(&(0, 0, "a".to_string())));
        assert!(plan.cells.contains(&(1, 2, "f".to_string())));
    }

    /// Setting a whole column to one value is the reason this case exists; a
    /// paste that put the value in only the top-left cell would leave the user
    /// pasting N times.
    #[test]
    fn one_copied_cell_fills_the_whole_selection() {
        let block = parse_tsv_block("x");
        let plan = plan_paste(&block, (1, 1, 3, 2), 10, 10, None, all_editable);
        assert_eq!(plan.cells.len(), 6);
        for r in 1..=3 {
            for c in 1..=2 {
                assert!(plan.cells.contains(&(r, c, "x".to_string())), "({r},{c})");
            }
        }
    }

    /// A block bigger than one cell keeps **its own** shape. Honouring the
    /// selection's instead would mean truncating what was copied or tiling it,
    /// and both guess at an intent nobody expressed.
    #[test]
    fn a_multi_cell_block_ignores_the_selections_size() {
        let block = parse_tsv_block("a\tb\nc\td");
        // A single-cell selection, and a huge one, land the same four cells.
        let from_one = plan_paste(&block, (0, 0, 0, 0), 10, 10, None, all_editable);
        let from_many = plan_paste(&block, (0, 0, 9, 9), 10, 10, None, all_editable);
        assert_eq!(from_one, from_many);
        assert_eq!(from_one.cells.len(), 4);
    }

    /// Silently discarding half a pasted spreadsheet looks exactly like a paste
    /// that worked. The count is what lets the grid say otherwise.
    #[test]
    fn cells_that_fall_off_the_grid_are_counted_not_dropped_quietly() {
        let block = parse_tsv_block("a\tb\tc\nd\te\tf");
        // A 3-column block anchored on the last column of a 2-column grid.
        let plan = plan_paste(&block, (0, 1, 0, 1), 1, 2, None, all_editable);
        assert_eq!(plan.cells, vec![(0, 1, "a".to_string())]);
        assert_eq!(plan.dropped, 5, "{plan:?}");
    }

    /// A read-only column is skipped **in place**. Shifting past it would write
    /// column 2's values into column 3 — data in the wrong column, accepted by
    /// the database, and invisible until someone reads it.
    #[test]
    fn a_read_only_column_is_skipped_rather_than_shifted() {
        let block = parse_tsv_block("a\tb\tc");
        let plan = plan_paste(&block, (0, 0, 0, 0), 5, 5, None, |ci| ci != 1);
        assert_eq!(plan.read_only, 1);
        assert!(plan.cells.contains(&(0, 0, "a".to_string())));
        assert!(plan.cells.contains(&(0, 2, "c".to_string())));
        assert!(!plan.cells.iter().any(|(_, c, _)| *c == 1));
    }

    /// A ragged block — the shape a spreadsheet produces from trailing empty
    /// cells — must not report the gaps as lost data.
    #[test]
    fn a_short_row_leaves_the_cells_it_does_not_reach_alone() {
        let block = parse_tsv_block("a\tb\nc");
        let plan = plan_paste(&block, (0, 0, 0, 0), 5, 5, None, all_editable);
        assert_eq!(plan.dropped, 0);
        assert_eq!(plan.cells.len(), 3);
        assert!(!plan.cells.iter().any(|(r, c, _)| *r == 1 && *c == 1));
    }

    /// **The selection, not the clipboard, is what can be enormous here.** One
    /// copied cell fills whatever is selected, and Ctrl+A on a result at the
    /// 200k-row cap selects every display cell — which asked for a plan of
    /// millions of cloned values, as many entries in `dirty`, and a commit of
    /// 200k `UPDATE`s, with the window unresponsive well before the Discard
    /// button that would undo it.
    ///
    /// Two properties, and the second is the one that makes the first safe to
    /// have: the walk **stops** at the cap (it does not visit the rest to
    /// classify them), and everything it did not visit is still accounted for, so
    /// the report can say so.
    #[test]
    fn a_selection_bigger_than_the_cap_stops_at_it_and_counts_the_rest() {
        let block = parse_tsv_block("x");
        let rows = PASTE_CELL_CAP + 10;
        let plan = plan_paste(&block, (0, 0, rows - 1, 0), rows, 1, None, all_editable);
        assert_eq!(plan.cells.len(), PASTE_CELL_CAP, "staged up to the cap");
        assert_eq!(plan.capped, 10, "and the remainder is counted, not dropped");
        assert_eq!(plan.cells.len() + plan.capped, rows, "nothing unaccounted");
        // The cap is about volume, so it must not be reported as anything else.
        assert_eq!((plan.dropped, plan.read_only), (0, 0));
        assert_eq!(plan.counts().capped, 10, "and it survives into the report");
    }

    /// **A paste that stages nothing is bounded too.** The staged cap cannot
    /// stop this walk: a result whose every column is an expression has no
    /// editable column, so `plan.cells` never grows and the ceiling is never
    /// reached — while Ctrl+A selected every display cell and each one was
    /// *cloned* before being classified as read-only. That is a heap allocation
    /// per position for a gesture whose whole outcome is "Nothing pasted", on the
    /// UI thread.
    ///
    /// Two properties: the walk stops at [`PASTE_VISIT_CAP`], and what it did not
    /// visit is counted so the report can say so.
    #[test]
    fn a_paste_that_can_stage_nothing_still_stops() {
        let block = parse_tsv_block("x");
        // A million selected positions, none of them editable.
        let rows = 1_000_000;
        let plan = plan_paste(&block, (0, 0, rows - 1, 0), rows, 1, None, |_| false);
        assert!(plan.cells.is_empty(), "nothing could be staged");
        assert_eq!(
            plan.read_only, PASTE_VISIT_CAP,
            "the walk stops at the visit ceiling"
        );
        assert_eq!(
            plan.read_only + plan.capped,
            rows,
            "and everything is accounted for"
        );
        // The ordinary read-only paste — small, and fully classified — is
        // untouched by the ceiling.
        let plan = plan_paste(&block, (0, 0, 3, 0), 4, 1, None, |_| false);
        assert_eq!((plan.read_only, plan.capped), (4, 0));
    }

    /// A paste that fits is untouched by the cap — the ordinary case has to stay
    /// exactly what it was, including reporting nothing at all.
    #[test]
    fn a_paste_that_fits_is_not_capped() {
        let block = parse_tsv_block("a\tb\nc\td");
        let plan = plan_paste(&block, (0, 0, 0, 0), 10, 10, None, all_editable);
        assert_eq!(plan.capped, 0);
        assert_eq!(
            paste_report(plan.counts(), 0, plan.cells.len()),
            PasteReport::Clean
        );
    }

    #[test]
    fn an_empty_block_or_an_empty_grid_plans_nothing() {
        assert_eq!(
            plan_paste(&[], (0, 0, 0, 0), 5, 5, None, all_editable),
            PastePlan::default()
        );
        let block = parse_tsv_block("a");
        assert_eq!(
            plan_paste(&block, (0, 0, 0, 0), 0, 5, None, all_editable),
            PastePlan::default()
        );
        assert_eq!(
            plan_paste(&block, (0, 0, 0, 0), 5, 0, None, all_editable),
            PastePlan::default()
        );
    }

    /// Built through [`PastePlan::counts`] rather than by hand, so the test
    /// exercises the same snapshot the caller takes.
    ///
    /// `cells` is what the plan *held*, and it is deliberately no longer part of
    /// the snapshot: how many cells landed is the caller's own count, taken after
    /// staging, and is passed to `paste_report` separately. These fixtures hand
    /// both in so the two can be told apart — `report(cells, …)` is the honest
    /// case where everything the plan held was staged.
    fn planned(cells: usize, dropped: usize, read_only: usize) -> (PasteCounts, usize) {
        let plan = PastePlan {
            cells: (0..cells).map(|i| (i, 0, String::new())).collect(),
            dropped,
            read_only,
            capped: 0,
        };
        let staged = plan.cells.len();
        (plan.counts(), staged)
    }

    /// As [`planned`], for the one count that is a *ceiling* rather than
    /// something about the data.
    fn planned_over(cells: usize, capped: usize) -> (PasteCounts, usize) {
        let plan = PastePlan {
            cells: (0..cells).map(|i| (i, 0, String::new())).collect(),
            capped,
            ..PastePlan::default()
        };
        let staged = plan.cells.len();
        (plan.counts(), staged)
    }

    /// `paste_report` over a fixture, with everything the plan held having landed.
    fn report(f: (PasteCounts, usize), skipped_deleted: usize) -> PasteReport {
        let (counts, staged) = f;
        paste_report(
            counts,
            skipped_deleted,
            staged.saturating_sub(skipped_deleted),
        )
    }

    /// **The sentence has to count what landed, not what was planned.**
    ///
    /// Staging drops entries the plan cannot know about: `stage_many` un-stages a
    /// cell pasted back over its own original value, and `stage_new_many` removes a
    /// column whose pasted cell is blank. `staged` was derived here as
    /// `planned - skipped_deleted`, so pasting a column's own values over itself
    /// reported `Pasted N cells` while `dirty` gained nothing at all.
    #[test]
    fn the_report_counts_what_landed_and_not_what_was_planned() {
        let (counts, held) = planned(2, 1, 0);
        assert_eq!(held, 2, "the plan held two cells");
        // Both were staged: the ordinary case, and the one the old arithmetic got
        // right.
        assert_eq!(
            paste_report(counts, 0, 2),
            PasteReport::Notice("Pasted 2 cells, skipping 1 outside the grid.".to_string())
        );
        // One of the two was a no-op — pasted back over its own value. The plan
        // still held two.
        assert_eq!(
            paste_report(counts, 0, 1),
            PasteReport::Notice("Pasted 1 cell, skipping 1 outside the grid.".to_string())
        );
        // Neither landed: that is a failure, however many the plan held.
        assert_eq!(
            paste_report(counts, 0, 0),
            PasteReport::Failed("Nothing pasted: 1 outside the grid.".to_string())
        );
    }

    /// **A pasted blank is a value; a typed blank is an undo.**
    ///
    /// One block used to write `''` above the pending-row boundary and leave the
    /// column unset — so the *server default* — below it: the same clipboard cell,
    /// two stored values, decided by a line the user cannot see. It also threw away
    /// a value typed into a pending row when the pasted cell over it was empty.
    #[test]
    fn a_blank_means_what_the_gesture_means() {
        use BlankCell::*;
        // A paste: the empty cell is stored, the same as it would be on a real row.
        assert_eq!(
            pending_cell(Some(String::new()), IsAValue),
            Some(Some(String::new()))
        );
        // A typed clear: the column goes back to unset, so the INSERT omits it.
        assert_eq!(pending_cell(Some(String::new()), UnsetsIt), None);
        // Everything else is the same on either reading — an ordinary value...
        for blank in [IsAValue, UnsetsIt] {
            assert_eq!(
                pending_cell(Some("a".to_string()), blank),
                Some(Some("a".to_string())),
                "{blank:?}"
            );
            // ...and SQL NULL, which is explicit on both and is never an undo.
            assert_eq!(pending_cell(None, blank), Some(None), "{blank:?}");
        }
    }

    /// A paste that lost nothing says nothing. A bar on every paste is a bar
    /// nobody reads.
    #[test]
    fn a_clean_paste_reports_nothing() {
        assert_eq!(report(planned(6, 0, 0), 0), PasteReport::Clean);
    }

    /// **The finding this split exists for.** Five cells landed and one didn't:
    /// that is a success with a caveat, and it went out on the grid's *red*
    /// error surface — indistinguishable from a write-back that failed.
    #[test]
    fn a_partial_paste_is_a_notice_and_not_an_error() {
        let r = report(planned(5, 0, 1), 0);
        assert_eq!(
            r,
            PasteReport::Notice("Pasted 5 cells, skipping 1 in read-only columns.".into())
        );
        assert!(
            !matches!(r, PasteReport::Failed(_)),
            "a paste that landed is not a failure"
        );
    }

    /// **The limit names itself.** "skipping 5,950,000" with no reason reads as a
    /// bug in the paste; the number is what says this was a ceiling, and it is
    /// the difference between a user enlarging their selection again and a user
    /// filing a report.
    #[test]
    fn a_capped_paste_says_what_the_ceiling_was() {
        let r = report(planned_over(PASTE_CELL_CAP, 12), 0);
        assert_eq!(
            r,
            PasteReport::Notice(
                "Pasted 50k cells, skipping 12 over the 50k-cell paste limit.".to_string()
            )
        );
        // A big overflow, in the printer the stats line uses — this test's own
        // doc comment wrote the figure grouped, which is what the sentence was
        // always meant to read like.
        let r = report(planned_over(PASTE_CELL_CAP, 5_950_000), 0);
        assert_eq!(
            r,
            PasteReport::Notice(
                "Pasted 50k cells, skipping 5.95m over the 50k-cell paste limit.".to_string()
            )
        );
    }

    /// **"Pasted 1 cells."** The noun follows the figure, on a bar whose stats
    /// line four pixels away reads `200k of ~292.02k rows` — and no fixture in
    /// this module ever staged exactly one cell, so the singular was unpinned.
    #[test]
    fn one_pasted_cell_is_one_cell() {
        assert_eq!(
            report(planned(1, 1, 0), 0),
            PasteReport::Notice("Pasted 1 cell, skipping 1 outside the grid.".to_string())
        );
        assert_eq!(
            report(planned(2, 1, 0), 0),
            PasteReport::Notice("Pasted 2 cells, skipping 1 outside the grid.".to_string())
        );
    }

    /// Nothing landed: that *is* a failure, and the red surface is right for it
    /// — a read-only result is exactly where the user needs telling why the
    /// paste appeared to do nothing.
    #[test]
    fn a_paste_that_lands_nothing_is_a_failure() {
        assert_eq!(
            report(planned(0, 0, 4), 0),
            PasteReport::Failed("Nothing pasted: 4 in read-only columns.".into())
        );
    }

    /// The view's own count comes off the staged total, so a block whose cells
    /// *all* landed on rows marked for deletion is a failure rather than a
    /// notice claiming cells were pasted.
    #[test]
    fn rows_marked_for_deletion_count_against_what_landed() {
        assert_eq!(
            report(planned(3, 0, 0), 3),
            PasteReport::Failed("Nothing pasted: 3 in rows marked for deletion.".into())
        );
        assert_eq!(
            report(planned(3, 0, 0), 1),
            PasteReport::Notice("Pasted 2 cells, skipping 1 in rows marked for deletion.".into())
        );
    }

    /// Every reason is named, in one sentence, in a fixed order — so two pastes
    /// that lost the same things read the same way.
    #[test]
    fn every_reason_a_cell_was_skipped_is_named() {
        assert_eq!(
            report(planned(4, 2, 3), 1),
            PasteReport::Notice(
                "Pasted 3 cells, skipping 2 outside the grid, 3 in read-only columns, \
                 1 in rows marked for deletion."
                    .into()
            )
        );
    }
}
