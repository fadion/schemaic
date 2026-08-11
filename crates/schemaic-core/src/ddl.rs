//! Schema editing: the table you *want*, the difference from the table that's
//! there, and the SQL that closes it.
//!
//! Shaped like [`edit`](crate::edit): a pure analysis ([`diff`]) turning two
//! states into a reviewable model ([`ChangeSet`]), and a pure emitter
//! ([`ChangeSet::emit`]) turning that model into statements. Nothing here talks
//! to a server, so every rule — what counts as a change, what counts as
//! destructive, what SQL each engine needs — is unit-testable, and the UI stays
//! a thin shell over it.
//!
//! Three things make this more than a struct comparison:
//!
//! * **Identity survives renaming.** A draft column carries the name it had on
//!   the server ([`ColumnDraft::original`]), not just the name it has now — the
//!   only thing that tells `rename id → user_id` apart from `drop id, add
//!   user_id`, which are the same edit to a plain field-by-field compare and
//!   very different to the data.
//! * **MySQL's `MODIFY COLUMN` replaces a column outright**, so every alteration
//!   restates the whole definition through the one shared emitter
//!   ([`ColumnInfo::definition_sql`]). Anything it didn't restate would be
//!   silently destroyed — the default, the comment, the collation, the
//!   auto-increment.
//! * **Types have to compare by meaning, not by text.** MariaDB says `int(11)`
//!   where MySQL 8 says `int`, and PostgreSQL says `character varying(45)` where
//!   everyone types `varchar(45)`. Comparing the strings would show a phantom
//!   change on every column the moment a designer opened.

use std::collections::{HashMap, HashSet};

use crate::intel::SqlDialect;
use crate::pairs;
use crate::schema::{
    ColumnInfo, ForeignKeyInfo, IndexInfo, TableInfo, ViewOptions, ddl_ident_in, ddl_string,
    definer_sql, sql_qualifier,
};

// ── The desired state ────────────────────────────────────────────────────────

/// Where a column sits in the table's column order. MySQL only — PostgreSQL
/// can't move a column at all, so a draft's ordering there is display-only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Position {
    First,
    After(String),
}

impl Position {
    fn sql(&self, dialect: SqlDialect) -> String {
        match self {
            Position::First => " FIRST".to_string(),
            Position::After(c) => format!(" AFTER {}", ddl_ident_in(c, dialect)),
        }
    }
}

/// A column as the designer holds it: its desired definition, plus the name it
/// answers to on the server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnDraft {
    /// The column's name in the introspected table, or `None` for one the user
    /// added. This is *identity*, not a name — renaming edits `info.name` and
    /// leaves this alone, which is the only thing that distinguishes a rename
    /// from a drop-plus-add.
    pub original: Option<String>,
    pub info: ColumnInfo,
    /// The name this column's primary-key, index and foreign-key references
    /// currently spell — the draft's answer to "which column is that entry?".
    ///
    /// Normally identical to `info.name`. The two diverge for exactly as long as
    /// the user is typing *through* a name another column already holds: the
    /// designer writes back on every keystroke, so renaming `b` to `ab` walks the
    /// draft through `""`, `"a"`, `"ab"`, and while `info.name` reads `a` this
    /// column's dependents still say what they said before. Rewriting them then
    /// would have claimed the *other* `a`'s key membership.
    ///
    /// Private because it is bookkeeping, not state a caller should set:
    /// [`TableDraft::rename_column`] is the only thing that moves it, and it
    /// maintains the invariant the whole scheme rests on — **`key_name` is unique
    /// across a draft's columns even when `info.name` is not.**
    key_name: String,
}

impl ColumnDraft {
    /// A column that already exists, unchanged.
    pub fn existing(info: ColumnInfo) -> Self {
        Self {
            original: Some(info.name.clone()),
            key_name: info.name.clone(),
            info,
        }
    }

    /// A column the user is adding.
    pub fn new(info: ColumnInfo) -> Self {
        Self {
            original: None,
            key_name: info.name.clone(),
            info,
        }
    }
}

/// An index as the designer holds it. Same identity rule as [`ColumnDraft`],
/// except an index is never *altered*: any change is a drop and a re-create,
/// because neither engine can edit an index's key list in place.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexDraft {
    pub original: Option<String>,
    pub info: IndexInfo,
}

impl IndexDraft {
    pub fn existing(info: IndexInfo) -> Self {
        Self {
            original: Some(info.name.clone()),
            info,
        }
    }
    pub fn new(info: IndexInfo) -> Self {
        Self {
            original: None,
            info,
        }
    }
}

/// A foreign key as the designer holds it. Like an index, only droppable and
/// re-creatable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForeignKeyDraft {
    pub original: Option<String>,
    pub info: ForeignKeyInfo,
}

impl ForeignKeyDraft {
    pub fn existing(info: ForeignKeyInfo) -> Self {
        Self {
            original: Some(info.name.clone()),
            info,
        }
    }
    pub fn new(info: ForeignKeyInfo) -> Self {
        Self {
            original: None,
            info,
        }
    }
}

/// The whole desired shape of one table.
///
/// The primary key lives here as an **ordered list** rather than as the
/// per-column [`ColumnInfo::primary_key`] flag: a composite key's column order
/// is part of the key (it decides which prefixes the index serves), and a set of
/// booleans can't carry it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableDraft {
    /// The table's name on the server, or `None` when this draft is a new table.
    pub original: Option<String>,
    pub name: String,
    /// PostgreSQL namespace; `None` on MySQL.
    pub schema: Option<String>,
    pub columns: Vec<ColumnDraft>,
    /// Primary-key columns in key order, named as they are in this draft (so a
    /// renamed column appears under its new name).
    pub primary_key: Vec<String>,
    /// Non-primary indexes. The primary key is [`TableDraft::primary_key`], not
    /// an entry here.
    pub indexes: Vec<IndexDraft>,
    pub foreign_keys: Vec<ForeignKeyDraft>,
    /// MySQL storage engine. Always `None` on PostgreSQL.
    pub engine: Option<String>,
    /// MySQL table collation. Always `None` on PostgreSQL.
    pub collation: Option<String>,
    pub comment: Option<String>,
}

impl TableDraft {
    /// The draft that describes an introspected table exactly — the designer's
    /// starting point, and the input to the round-trip gate (diffing this
    /// against its own source must produce nothing).
    pub fn from_table(t: &TableInfo) -> TableDraft {
        TableDraft {
            original: Some(t.name.clone()),
            name: t.name.clone(),
            schema: t.schema.clone(),
            columns: t
                .columns
                .iter()
                .cloned()
                .map(ColumnDraft::existing)
                .collect(),
            primary_key: primary_key_of(t),
            indexes: t
                .indexes
                .iter()
                .filter(|ix| !ix.is_primary())
                .cloned()
                .map(IndexDraft::existing)
                .collect(),
            foreign_keys: t
                .foreign_keys
                .iter()
                .cloned()
                .map(ForeignKeyDraft::existing)
                .collect(),
            engine: t.engine.clone(),
            collation: t.collation.clone(),
            comment: t.comment.clone(),
        }
    }

    /// An empty draft for a brand-new table in `schema`.
    pub fn blank(name: impl Into<String>, schema: Option<String>) -> TableDraft {
        TableDraft {
            original: None,
            name: name.into(),
            schema,
            ..Default::default()
        }
    }

    /// The draft's column names, in order.
    pub fn column_names(&self) -> Vec<String> {
        self.columns.iter().map(|c| c.info.name.clone()).collect()
    }

    /// Rename the column at `idx`, carrying every reference to it along.
    ///
    /// The draft describes the table it *wants*, so its keys and indexes name
    /// draft columns. Renaming the column alone would leave them pointing at a
    /// name that no longer exists — which reads to [`diff`] as "the index
    /// changed", and turns a free rename into a rebuild of every index over it.
    ///
    /// **References move by identity, not by name.** The designer writes back on
    /// every keystroke, so a rename routinely passes *through* names other
    /// columns hold: renaming `b` to `ab` in a table that also has an `a` walks
    /// the draft through `""`, `"a"`, `"ab"`. A plain string match then rewrote
    /// `a`'s key membership to point at `ab` — and the draft validated clean
    /// afterwards, because the clash it would have complained about was gone. So
    /// the dependents follow [`ColumnDraft::key_name`], and it only advances to a
    /// name no *other* column's `key_name` already claims. Uniqueness of
    /// `key_name` is therefore preserved by induction, which is what makes a
    /// reference unambiguous however tangled `info.name` gets in between.
    pub fn rename_column(&mut self, idx: usize, new_name: &str) {
        if idx >= self.columns.len() {
            return;
        }
        let taken = self
            .columns
            .iter()
            .enumerate()
            .any(|(i, c)| i != idx && c.key_name == new_name);
        let c = &mut self.columns[idx];
        c.info.name = new_name.to_string();
        if taken || c.key_name == new_name {
            // Mid-flight through another column's identity: show what the user
            // typed, but leave the dependents where they are. `validate` is what
            // surfaces the clash if the user stops here.
            return;
        }
        let old = std::mem::replace(&mut c.key_name, new_name.to_string());
        self.move_references(&old, new_name);
        self.settle_key_names();
    }

    /// Point every key, index and foreign-key reference to `old` at `new`.
    fn move_references(&mut self, old: &str, new: &str) {
        let swap = |s: &mut String| {
            if s == old {
                *s = new.to_string();
            }
        };
        self.primary_key.iter_mut().for_each(swap);
        for ix in &mut self.indexes {
            ix.info.columns.iter_mut().for_each(|c| swap(&mut c.name));
        }
        for fk in &mut self.foreign_keys {
            fk.info.columns.iter_mut().for_each(swap);
        }
    }

    /// Let any column whose `key_name` is still behind its `info.name` catch up,
    /// now that some other edit may have freed the name it was waiting for.
    ///
    /// Without this the divergence outlives the clash that caused it: rename `b`
    /// to `a` (blocked by the existing `a`), then rename that `a` to something
    /// else, and `b`'s references would sit on `b` forever — a name no column
    /// answers to any more, which `validate` reports as a primary key naming a
    /// column the user can no longer see. Repeated because one column catching up
    /// frees its old name for the next.
    fn settle_key_names(&mut self) {
        for _ in 0..self.columns.len() {
            let Some((idx, old, new)) = self.columns.iter().enumerate().find_map(|(i, c)| {
                (c.key_name != c.info.name
                    && !self
                        .columns
                        .iter()
                        .enumerate()
                        .any(|(j, o)| j != i && o.key_name == c.info.name))
                .then(|| (i, c.key_name.clone(), c.info.name.clone()))
            }) else {
                return;
            };
            self.columns[idx].key_name = new.clone();
            self.move_references(&old, &new);
        }
    }

    /// Is the column at `idx` part of the primary key?
    ///
    /// By identity, not by name — asking with `primary_key.contains(&name)` gets
    /// the wrong answer while a rename is passing through another column's name.
    pub fn is_in_primary_key(&self, idx: usize) -> bool {
        self.columns
            .get(idx)
            .is_some_and(|c| self.primary_key.contains(&c.key_name))
    }

    /// Add the column at `idx` to the primary key (appended in click order) or
    /// take it out. The counterpart to [`TableDraft::is_in_primary_key`], and for
    /// the same reason: a by-name `retain` mid-rename removes the *other*
    /// column's membership.
    pub fn set_in_primary_key(&mut self, idx: usize, member: bool) {
        let Some(name) = self.columns.get(idx).map(|c| c.key_name.clone()) else {
            return;
        };
        match member {
            true if !self.primary_key.contains(&name) => self.primary_key.push(name),
            true => {}
            false => self.primary_key.retain(|p| *p != name),
        }
    }

    /// Drop the column at `idx` and every key, index and foreign key that stood
    /// on it — an index over a column that no longer exists can't be created,
    /// and both engines drop one on the user's behalf anyway.
    pub fn remove_column(&mut self, idx: usize) {
        if idx >= self.columns.len() {
            return;
        }
        // By `key_name`, for the same reason [`TableDraft::rename_column`] moves
        // references by it: mid-rename `info.name` can be another column's.
        let name = self.columns.remove(idx).key_name;
        self.primary_key.retain(|c| *c != name);
        self.indexes
            .retain(|ix| !ix.info.columns.iter().any(|c| c.name == name));
        self.foreign_keys
            .retain(|fk| !fk.info.columns.contains(&name));
        // Removing a column frees its name for anyone mid-rename onto it.
        self.settle_key_names();
    }

    /// Problems that would make the generated SQL nonsense, in plain language —
    /// what the designer refuses to hand to the preview. Empty means "emittable",
    /// **not** "the server will accept it": type names, expressions and defaults
    /// are the server's to judge, exactly as with import's coercion.
    pub fn validate(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.name.trim().is_empty() {
            out.push("The table needs a name.".to_string());
        }
        if self.columns.is_empty() {
            out.push("A table needs at least one column.".to_string());
        }
        let mut seen: HashSet<String> = HashSet::new();
        for c in &self.columns {
            let name = c.info.name.trim();
            if name.is_empty() {
                out.push("Every column needs a name.".to_string());
            } else if !seen.insert(name.to_ascii_lowercase()) {
                out.push(format!("Two columns are both called {name}."));
            }
            if c.info.type_name.trim().is_empty() {
                out.push(format!("Column {name} needs a type."));
            }
        }
        let names: HashSet<String> = self
            .columns
            .iter()
            .map(|c| c.info.name.to_ascii_lowercase())
            .collect();
        let known = |c: &String| names.contains(&c.to_ascii_lowercase());
        for c in &self.primary_key {
            if !known(c) {
                out.push(format!("The primary key names {c}, which isn't a column."));
            }
        }
        let mut index_names: HashSet<String> = HashSet::new();
        for ix in &self.indexes {
            if ix.info.name.trim().is_empty() {
                out.push("Every index needs a name.".to_string());
            } else if !index_names.insert(ix.info.name.to_ascii_lowercase()) {
                out.push(format!("Two indexes are both called {}.", ix.info.name));
            }
            if ix.info.columns.is_empty() {
                out.push(format!("Index {} has no columns.", ix.info.name));
            }
            for c in &ix.info.columns {
                // An expression key names no column by design, so there is
                // nothing to look up — `(lower(email))` is the index's *value*,
                // not a reference to something in the table above.
                if !c.expression && !known(&c.name) {
                    out.push(format!(
                        "Index {} names {}, which isn't a column.",
                        ix.info.name, c.name
                    ));
                }
            }
        }
        for fk in &self.foreign_keys {
            if fk.info.name.trim().is_empty() {
                out.push("Every foreign key needs a name.".to_string());
            }
            if fk.info.ref_table.trim().is_empty() {
                out.push(format!("Foreign key {} references no table.", fk.info.name));
            }
            if fk.info.columns.is_empty() || fk.info.columns.len() != fk.info.ref_columns.len() {
                out.push(format!(
                    "Foreign key {} must pair each column with one it references.",
                    fk.info.name
                ));
            }
            for c in &fk.info.columns {
                if !known(c) {
                    out.push(format!(
                        "Foreign key {} names {c}, which isn't a column.",
                        fk.info.name
                    ));
                }
            }
        }
        out
    }
}

// ── The desired state of a view ──────────────────────────────────────────────

/// The whole desired shape of one view: a name, a `SELECT`, and the options that
/// have to be carried along whether or not anyone edits them.
///
/// Carried along is the point. A view is redefined by *replacing* it, so a
/// `CREATE OR REPLACE` that doesn't restate the definer, the security type or
/// the check option resets them — see [`ViewOptions`]. The draft holds them for
/// the same reason [`ColumnDraft`] holds a whole [`ColumnInfo`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewDraft {
    /// The view's name on the server, or `None` when this draft is a new view.
    /// Identity, not a name: editing `name` is a rename.
    pub original: Option<String>,
    pub name: String,
    /// PostgreSQL namespace; `None` on MySQL.
    pub schema: Option<String>,
    /// The body, without a trailing semicolon (see [`view_body`]).
    pub select: String,
    pub options: ViewOptions,
    /// **PostgreSQL only.** Apply the edit by dropping and re-creating the view
    /// instead of replacing it in place.
    ///
    /// `CREATE OR REPLACE VIEW` there can only *append* columns, and whether a
    /// given body still produces the old ones can't always be read off the
    /// statement ([`pg_replaceable`]). When it can't, the plan replaces and lets
    /// the server judge — and this is the user's answer when the server says no.
    /// Ignored on MySQL, which replaces anything.
    pub force_recreate: bool,
}

impl ViewDraft {
    /// The draft that describes an introspected view exactly — the editor's
    /// starting point, and the input to the round-trip gate. `None` for a base
    /// table, which has no view to draft.
    pub fn from_table(t: &TableInfo) -> Option<ViewDraft> {
        if !t.is_view {
            return None;
        }
        Some(ViewDraft {
            original: Some(t.name.clone()),
            name: t.name.clone(),
            schema: t.schema.clone(),
            select: view_body(t.view_definition.as_deref().unwrap_or_default()),
            options: t.view_options.clone().unwrap_or_default(),
            force_recreate: false,
        })
    }

    /// An empty draft for a brand-new view in `schema`.
    pub fn blank(name: impl Into<String>, schema: Option<String>) -> ViewDraft {
        ViewDraft {
            original: None,
            name: name.into(),
            schema,
            ..Default::default()
        }
    }

    /// Problems that would make the generated SQL nonsense, in plain language.
    /// Empty means "emittable", **not** "the server will accept it" — whether the
    /// body's tables and columns exist is the server's judgement, as everywhere
    /// else here.
    pub fn validate(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.name.trim().is_empty() {
            out.push("The view needs a name.".to_string());
        }
        let body = self.select.trim();
        if body.is_empty() {
            out.push("A view needs a SELECT to define it.".to_string());
        } else if !can_be_view_body(body) {
            // Head-keyword only: anything past that is the parser's job, and a
            // body it can't parse mid-edit still has to be emittable.
            out.push(
                "A view's body has to be a query — it starts with SELECT, WITH, VALUES or TABLE."
                    .to_string(),
            );
        }
        if self.options.materialized {
            out.push(
                "Schemaic can't edit a materialized view — PostgreSQL has no \
                 CREATE OR REPLACE for one."
                    .to_string(),
            );
        }
        out
    }
}

/// Could `body` be a view's definition — does it start like a query?
///
/// The set is every head keyword a view body may legitimately take on either
/// engine. Head-keyword only, deliberately: it's the same answer mid-edit as it
/// is on a complete statement, which is what both callers need — the draft's
/// [`validate`](ViewDraft::validate), and the editor's right-click menu deciding
/// whether "Create view" applies to the statement under the cursor.
pub fn can_be_view_body(body: &str) -> bool {
    let head = body
        .split(|c: char| c.is_whitespace() || c == '(')
        .find(|w| !w.is_empty())
        .unwrap_or_default();
    ["SELECT", "WITH", "VALUES", "TABLE"]
        .iter()
        .any(|k| head.eq_ignore_ascii_case(k))
}

/// A view body as it goes into a `CREATE VIEW`: trimmed, with the terminating
/// semicolon off.
///
/// `pg_get_viewdef` hands back a *statement*, semicolon and all, and pasting
/// that in front of `WITH CASCADED CHECK OPTION` is a syntax error. Only a
/// genuinely trailing `;` is removed — one inside a string literal is data.
pub fn view_body(sql: &str) -> String {
    sql.trim().trim_end_matches(';').trim_end().to_string()
}

/// Can PostgreSQL redefine a view whose columns are `current` with the body
/// `select`, in place?
///
/// `CREATE OR REPLACE VIEW` there may only **append**: every existing column has
/// to still be produced, under the same name, in the same position. `None` means
/// the answer can't be read off the statement (a `*`, a set operation, an
/// unnamed expression) — and uncertainty must resolve to "replace and let the
/// server refuse", never to "drop it and find out".
pub fn pg_replaceable(current: &[String], select: &str, dialect: SqlDialect) -> Option<bool> {
    let names = crate::intel::select_output_names(select, dialect)?;
    if names.len() < current.len() {
        return Some(false);
    }
    Some(
        current
            .iter()
            .zip(&names)
            .all(|(a, b)| a.eq_ignore_ascii_case(b)),
    )
}

/// A table's primary-key columns in key order: the `PRIMARY` index when the
/// server reported one (authoritative for a composite key's order), else the
/// flagged columns in column order.
pub fn primary_key_of(t: &TableInfo) -> Vec<String> {
    match t.indexes.iter().find(|ix| ix.is_primary()) {
        Some(ix) => ix.column_names().map(str::to_string).collect(),
        None => t
            .columns
            .iter()
            .filter(|c| c.primary_key)
            .map(|c| c.name.clone())
            .collect(),
    }
}

// ── The difference ───────────────────────────────────────────────────────────

/// One reviewable step between the table that's there and the table that's
/// wanted. Every change is independently describable ([`Change::summary`]) and
/// independently judged for danger ([`Change::is_destructive`]) — that pair is
/// what the preview modal renders, so a user never approves a wall of SQL they
/// haven't been told the consequences of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    CreateTable(Box<TableDraft>),
    DropTable,
    TruncateTable,
    RenameTable {
        to: String,
    },
    AddColumn {
        column: Box<ColumnInfo>,
        position: Option<Position>,
    },
    DropColumn {
        name: String,
        type_name: String,
    },
    /// A column changed. `from.name != to.name` is a rename; `position` is set
    /// only when the column also moved (MySQL only).
    AlterColumn {
        from: Box<ColumnInfo>,
        to: Box<ColumnInfo>,
        position: Option<Position>,
    },
    /// The primary key changed. Either side may be empty (adding a key to a
    /// table that had none, or dropping one). `drop_constraint` is the name
    /// PostgreSQL needs to drop the old key — it has no `DROP PRIMARY KEY`.
    PrimaryKey {
        from: Vec<String>,
        to: Vec<String>,
        drop_constraint: Option<String>,
    },
    AddIndex(Box<IndexInfo>),
    DropIndex {
        name: String,
        /// The constraint the index backs, when it is one — PostgreSQL refuses
        /// `DROP INDEX` on those.
        constraint: Option<String>,
    },
    /// An edit to an index [`IndexInfo::lossy`] marks as only partly readable was
    /// **withheld**. Emits no SQL.
    ///
    /// This is a change rather than a silent omission on purpose. The user asked
    /// for something and it isn't in the plan; a preview that simply didn't
    /// mention it would be the same class of dishonesty as the destruction it
    /// replaces. So it carries a summary and a risk line, and the modal renders
    /// both.
    KeepLossyIndex {
        name: String,
    },
    AddForeignKey(Box<ForeignKeyInfo>),
    DropForeignKey {
        name: String,
    },
    /// MySQL table options / the table comment. Each field carries **only what
    /// changed**: `None` means "not part of this change", so the emitter restates
    /// nothing the user didn't edit. An empty `comment` clears it — that is a
    /// real state, unlike an empty engine or collation, which the differ treats
    /// as "leave it alone".
    TableOptions {
        engine: Option<String>,
        collation: Option<String>,
        comment: Option<String>,
    },
    /// Create a view that doesn't exist yet — plain `CREATE VIEW`, so a name
    /// already taken fails instead of silently replacing someone else's view.
    CreateView(Box<ViewDraft>),
    /// Redefine an existing view. `recreate` ⇒ the engine can't replace it in
    /// place, so this is a `DROP` followed by a `CREATE` — one change, because
    /// it's one edit, with the cost stated in [`Change::is_destructive`].
    ReplaceView {
        draft: Box<ViewDraft>,
        recreate: bool,
    },
    RenameView {
        to: String,
    },
    DropView {
        materialized: bool,
    },
}

/// What a view loses when it's dropped — the sentence behind both the plain
/// `DROP VIEW` and the recreate that has to drop first.
const VIEW_DROP_COST: &str =
    "Dependent views, rules and grants on it are dropped with it and aren't restored.";

impl Change {
    /// One line of plain language for the preview's change list.
    pub fn summary(&self) -> String {
        match self {
            Change::CreateTable(d) => format!(
                "Create table {} with {} column{}",
                d.name,
                d.columns.len(),
                plural(d.columns.len())
            ),
            Change::DropTable => "Drop the table".to_string(),
            Change::TruncateTable => "Delete every row".to_string(),
            Change::RenameTable { to } => format!("Rename the table to {to}"),
            Change::AddColumn { column, .. } => {
                format!("Add column {} {}", column.name, column.type_name)
            }
            Change::DropColumn { name, .. } => format!("Drop column {name}"),
            Change::AlterColumn { from, to, position } => {
                if from.name != to.name {
                    format!("Rename column {} to {}", from.name, to.name)
                } else if from.as_ref() == to.as_ref() && position.is_some() {
                    match position {
                        Some(Position::First) => format!("Move column {} first", to.name),
                        Some(Position::After(a)) => format!("Move column {} after {a}", to.name),
                        None => format!("Move column {}", to.name),
                    }
                } else if from.type_name != to.type_name {
                    format!(
                        "Change column {} from {} to {}",
                        to.name, from.type_name, to.type_name
                    )
                } else {
                    format!("Change column {}", to.name)
                }
            }
            Change::PrimaryKey { from, to, .. } => match (from.is_empty(), to.is_empty()) {
                (true, _) => format!("Add primary key ({})", to.join(", ")),
                (_, true) => format!("Drop the primary key ({})", from.join(", ")),
                _ => format!(
                    "Change the primary key from ({}) to ({})",
                    from.join(", "),
                    to.join(", ")
                ),
            },
            Change::AddIndex(ix) => format!(
                "Add {}index {} on ({})",
                if ix.unique { "unique " } else { "" },
                ix.name,
                ix.column_names().collect::<Vec<_>>().join(", ")
            ),
            Change::DropIndex { name, .. } => format!("Drop index {name}"),
            Change::KeepLossyIndex { name } => {
                format!("Leave index {name} unchanged — Schemaic can't read all of it")
            }
            Change::AddForeignKey(fk) => format!(
                "Add foreign key {} → {}({})",
                fk.name,
                fk.ref_table,
                fk.ref_columns.join(", ")
            ),
            Change::DropForeignKey { name } => format!("Drop foreign key {name}"),
            // Name the options this change actually carries — "change the
            // table's options" couldn't say which, and used to be shown for an
            // edit the statement didn't make.
            Change::TableOptions {
                engine,
                collation,
                comment,
            } => {
                let mut parts = Vec::new();
                if let Some(e) = engine {
                    parts.push(format!("engine to {e}"));
                }
                if let Some(c) = collation {
                    parts.push(format!("collation to {c}"));
                }
                if let Some(cm) = comment {
                    parts.push(if cm.is_empty() {
                        "comment (cleared)".to_string()
                    } else {
                        "comment".to_string()
                    });
                }
                format!("Set the table's {}", parts.join(", "))
            }
            Change::CreateView(d) => format!("Create view {}", d.name),
            Change::ReplaceView { draft, recreate } => {
                if *recreate {
                    format!("Drop and re-create view {}", draft.name)
                } else {
                    format!("Redefine view {}", draft.name)
                }
            }
            Change::RenameView { to } => format!("Rename the view to {to}"),
            Change::DropView { materialized } => {
                format!(
                    "Drop the {}view",
                    if *materialized { "materialized " } else { "" }
                )
            }
        }
    }

    /// What this change destroys, in plain language — empty when nothing is at
    /// risk. These are the sentences the preview lists above the Apply button, so
    /// they name the consequence rather than the operation.
    ///
    /// A `Vec` rather than one sentence because a single change can carry more
    /// than one risk: an `AlterColumn` that narrows a column *and* makes it
    /// NOT NULL used to disclose only the second, and that sentence says the
    /// statement will fail — which reads as a promise that nothing can be lost.
    pub fn risks(&self) -> Vec<String> {
        match self {
            Change::DropTable => vec!["Drops the table and every row in it.".to_string()],
            Change::TruncateTable => {
                vec!["Deletes every row in the table. This can't be undone.".to_string()]
            }
            Change::DropColumn { name, .. } => {
                vec![format!("Drops column {name} and all the data in it.")]
            }
            Change::AlterColumn { from, to, .. } => alter_risks(from, to),
            Change::PrimaryKey { from, to, .. } if !from.is_empty() && to.is_empty() => {
                vec!["Leaves the table without a primary key — rows can no longer be edited from the grid.".to_string()]
            }
            Change::DropView { materialized } => vec![format!(
                "Drops the {}view. {VIEW_DROP_COST}",
                if *materialized { "materialized " } else { "" }
            )],
            // The one place a *redefinition* is destructive: PostgreSQL can only
            // append columns to a view, so an edit that renames, retypes or
            // reorders one can't be applied in place at all. Saying that here is
            // the whole reason the recreate isn't done quietly.
            Change::ReplaceView {
                draft,
                recreate: true,
            } => vec![format!(
                "Re-creating {} drops it first. {VIEW_DROP_COST} PostgreSQL can't \
                 replace a view whose columns changed name, type or order, so this \
                 is the only way to apply the edit.",
                draft.name
            )],
            // Not destructive — the *opposite* — but it belongs in the same
            // block, because the block is where the preview says what the plan
            // won't do for you. Silently omitting the user's edit would be the
            // dishonesty this change exists to avoid.
            Change::KeepLossyIndex { name } => vec![format!(
                "Your edit to index {name} is not included. It uses something \
                 Schemaic can't read back — an expression key column, an operator \
                 class, or a NULLS ordering — and applying the edit would mean \
                 re-creating the index without it. Edit this index in SQL instead."
            )],
            _ => Vec::new(),
        }
    }

    /// Whether this change destroys anything at all.
    pub fn is_destructive(&self) -> bool {
        !self.risks().is_empty()
    }
}

/// Everything an in-place column change puts at risk. Narrowing is the one that
/// silently loses data; the others fail loudly, which is safer but still worth
/// saying before the statement runs.
///
/// **All of them, not the first.** Nullability and type are independent halves of
/// one edit and a designer changes both at once routinely; returning early on the
/// nullability half hid the narrowing — the more dangerous of the two.
fn alter_risks(from: &ColumnInfo, to: &ColumnInfo) -> Vec<String> {
    let mut out = Vec::new();
    if from.nullable && !to.nullable {
        out.push(format!(
            "Column {} becomes NOT NULL — the statement fails if any row holds NULL.",
            to.name
        ));
    }
    let (fb, fa) = {
        let p = split_type(&from.type_name);
        (p.base, p.params)
    };
    let (tb, ta) = {
        let p = split_type(&to.type_name);
        (p.base, p.params)
    };
    if fb.is_empty() || tb.is_empty() || from.type_name == to.type_name {
        return out;
    }
    if fb == tb {
        // Same family: compare the parameters **pairwise**, because the two carry
        // different consequences. For `DECIMAL`/`NUMERIC` (and MySQL's
        // `FLOAT`/`DOUBLE` with parameters) they are `(precision, scale)`: a
        // smaller precision makes values not fit, but a smaller *scale* silently
        // **rounds** every value — `decimal(10,2)` → `decimal(10,0)` turns
        // `1234.56` into `1235`, succeeding with a warning rather than failing.
        // Looking only at the first parameter missed the rounding case, which is
        // the one that loses data without the statement complaining.
        let narrowed = |i: usize| matches!((fa.get(i), ta.get(i)), (Some(a), Some(b)) if b < a);
        let mut lost = Vec::new();
        if narrowed(0) {
            lost.push("truncates values that no longer fit");
        }
        if narrowed(1) {
            lost.push("rounds every value in the column");
        }
        if !lost.is_empty() {
            out.push(format!(
                "Narrowing {} from {} to {} {}.",
                to.name,
                from.type_name,
                to.type_name,
                lost.join(" and ")
            ));
        }
        return out;
    }
    out.push(format!(
        "Changing {} from {} to {} rewrites every value; it can fail or lose precision.",
        to.name, from.type_name, to.type_name
    ));
    out
}

/// A set of changes against one table, ready to review and emit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeSet {
    /// The table's name **on the server** — what an `ALTER` addresses. A rename
    /// is one of the changes, not a new identity here.
    pub table: String,
    pub schema: Option<String>,
    pub dialect: SqlDialect,
    pub changes: Vec<Change>,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// Every destructive consequence in this set, in change order — a change can
    /// contribute more than one.
    pub fn destructive(&self) -> Vec<String> {
        self.changes.iter().flat_map(Change::risks).collect()
    }

    /// The statements, in the order they must run. Ready to hand to the preview
    /// modal, the clipboard, or a query tab — they're the same text either way.
    pub fn emit(&self) -> Vec<String> {
        match self.dialect {
            SqlDialect::Postgres => self.emit_postgres(),
            _ => self.emit_mysql(),
        }
    }

    /// The statements as one script, blank-line separated — what "Copy" and
    /// "Open in editor" hand over.
    pub fn script(&self) -> String {
        self.emit().join("\n\n")
    }

    fn q(&self, name: &str) -> String {
        ddl_ident_in(name, self.dialect)
    }

    /// The table, schema-qualified when it isn't in PostgreSQL's `public`.
    fn qname(&self) -> String {
        qualified(&self.table, self.schema.as_deref(), self.dialect)
    }

    // ── MySQL ────────────────────────────────────────────────────────────────

    /// MySQL coalesces everything into **one** `ALTER TABLE`: it's the only way
    /// to get atomicity out of the engine, and it's also the only way some pairs
    /// of changes are legal at all (dropping and re-adding an index of the same
    /// name, say). Ordering within the clause list still matters — a foreign key
    /// has to go before the column it constrains.
    fn emit_mysql(&self) -> Vec<String> {
        let d = self.dialect;
        let mut out = self.view_statements();
        // Whole-table statements stand alone; they never share an ALTER.
        for c in &self.changes {
            match c {
                Change::CreateTable(draft) => out.extend(create_table_sql(draft, d)),
                Change::DropTable => out.push(format!("DROP TABLE {};", self.qname())),
                Change::TruncateTable => out.push(format!("TRUNCATE TABLE {};", self.qname())),
                _ => {}
            }
        }

        let mut cl: Vec<String> = Vec::new();
        // 1. Constraints and keys come off first — a column can't be dropped or
        //    retyped while a foreign key or index still stands on it.
        for c in &self.changes {
            if let Change::DropForeignKey { name } = c {
                cl.push(format!("DROP FOREIGN KEY {}", self.q(name)));
            }
        }
        for c in &self.changes {
            if let Change::DropIndex { name, .. } = c {
                cl.push(format!("DROP INDEX {}", self.q(name)));
            }
        }
        for c in &self.changes {
            if let Change::PrimaryKey { from, .. } = c
                && !from.is_empty()
            {
                cl.push("DROP PRIMARY KEY".to_string());
            }
        }
        // 2. Columns: drop, then change, then add. Adds come last so a new
        //    column's `AFTER` can name one added earlier in the same statement.
        for c in &self.changes {
            if let Change::DropColumn { name, .. } = c {
                cl.push(format!("DROP COLUMN {}", self.q(name)));
            }
        }
        for c in &self.changes {
            if let Change::AlterColumn { from, to, position } = c {
                let pos = position.as_ref().map(|p| p.sql(d)).unwrap_or_default();
                let def = to.definition_sql(d);
                // CHANGE restates the old name as well; MODIFY doesn't take one.
                // Either way the definition is restated in full — MySQL replaces
                // the column, so anything left out is destroyed.
                if from.name != to.name {
                    cl.push(format!("CHANGE COLUMN {} {def}{pos}", self.q(&from.name)));
                } else {
                    cl.push(format!("MODIFY COLUMN {def}{pos}"));
                }
            }
        }
        for c in &self.changes {
            if let Change::AddColumn { column, position } = c {
                let pos = position.as_ref().map(|p| p.sql(d)).unwrap_or_default();
                cl.push(format!("ADD COLUMN {}{pos}", column.definition_sql(d)));
            }
        }
        // 3. Keys and constraints back on, over the columns that now exist.
        for c in &self.changes {
            if let Change::PrimaryKey { to, .. } = c
                && !to.is_empty()
            {
                cl.push(format!("ADD PRIMARY KEY ({})", self.key_list(to)));
            }
        }
        for c in &self.changes {
            if let Change::AddIndex(ix) = c {
                let kw = if ix.unique { "UNIQUE INDEX" } else { "INDEX" };
                cl.push(format!("ADD {kw} {} ({})", self.q(&ix.name), ix.key_sql(d)));
            }
        }
        for c in &self.changes {
            if let Change::AddForeignKey(fk) = c {
                cl.push(format!("ADD {}", fk_clause(fk, d)));
            }
        }
        // 4. Table-level options.
        for c in &self.changes {
            if let Change::TableOptions {
                engine,
                collation,
                comment,
            } = c
            {
                // Each is present only when it changed, so nothing is restated.
                if let Some(e) = engine {
                    cl.push(format!("ENGINE={e}"));
                }
                if let Some(coll) = collation {
                    cl.push(format!("COLLATE={coll}"));
                }
                if let Some(cm) = comment {
                    cl.push(format!("COMMENT={}", ddl_string(cm, SqlDialect::MySql)));
                }
            }
        }
        if !cl.is_empty() {
            out.push(format!(
                "ALTER TABLE {}\n  {};",
                self.qname(),
                cl.join(",\n  ")
            ));
        }
        // The rename runs last, so everything above still addresses the table by
        // the name the server currently knows.
        for c in &self.changes {
            if let Change::RenameTable { to } = c {
                out.push(format!(
                    "ALTER TABLE {} RENAME TO {};",
                    self.qname(),
                    qualified(to, self.schema.as_deref(), d)
                ));
            }
        }
        out
    }

    // ── PostgreSQL ───────────────────────────────────────────────────────────

    /// PostgreSQL splits into several statements — renames and index work aren't
    /// `ALTER TABLE` actions at all — but its DDL is transactional, so
    /// [`run_ddl`](../../schemaic_db/struct.Db.html) wraps the whole list in one
    /// transaction and the split costs nothing in atomicity.
    fn emit_postgres(&self) -> Vec<String> {
        let d = self.dialect;
        let q = |s: &str| ddl_ident_in(s, d);
        let mut out = self.view_statements();
        for c in &self.changes {
            match c {
                Change::CreateTable(draft) => out.extend(create_table_sql(draft, d)),
                Change::DropTable => out.push(format!("DROP TABLE {};", self.qname())),
                Change::TruncateTable => out.push(format!("TRUNCATE TABLE {};", self.qname())),
                _ => {}
            }
        }
        // 1. Renames first, so every clause after this names the new column.
        for c in &self.changes {
            if let Change::AlterColumn { from, to, .. } = c
                && from.name != to.name
            {
                out.push(format!(
                    "ALTER TABLE {} RENAME COLUMN {} TO {};",
                    self.qname(),
                    q(&from.name),
                    q(&to.name)
                ));
            }
        }
        // 2. Plain indexes are dropped by their own statement; a constraint-backed
        //    one has to go through the table (PostgreSQL refuses DROP INDEX there).
        for c in &self.changes {
            if let Change::DropIndex {
                name,
                constraint: None,
            } = c
            {
                // The index lives in its table's schema, and `DROP INDEX
                // "s"."i"` is how it's addressed when that isn't the default.
                out.push(format!(
                    "DROP INDEX {};",
                    qualified(name, self.schema.as_deref(), d)
                ));
            }
        }

        let mut cl: Vec<String> = Vec::new();
        for c in &self.changes {
            match c {
                Change::DropForeignKey { name } => {
                    cl.push(format!("DROP CONSTRAINT {}", q(name)));
                }
                Change::DropIndex {
                    constraint: Some(name),
                    ..
                } => cl.push(format!("DROP CONSTRAINT {}", q(name))),
                // Only a *named* constraint can be dropped here — PostgreSQL has
                // no `DROP PRIMARY KEY`, so a key whose constraint name never
                // made it through introspection has nothing to emit.
                Change::PrimaryKey {
                    from,
                    drop_constraint: Some(name),
                    ..
                } if !from.is_empty() => cl.push(format!("DROP CONSTRAINT {}", q(name))),
                _ => {}
            }
        }
        for c in &self.changes {
            if let Change::DropColumn { name, .. } = c {
                cl.push(format!("DROP COLUMN {}", q(name)));
            }
        }
        for c in &self.changes {
            if let Change::AlterColumn { from, to, .. } = c {
                cl.extend(pg_column_clauses(from, to, d));
            }
        }
        for c in &self.changes {
            if let Change::AddColumn { column, .. } = c {
                cl.push(format!("ADD COLUMN {}", column.definition_sql(d)));
            }
        }
        for c in &self.changes {
            if let Change::PrimaryKey { to, .. } = c
                && !to.is_empty()
            {
                cl.push(format!("ADD PRIMARY KEY ({})", self.key_list(to)));
            }
        }
        for c in &self.changes {
            if let Change::AddForeignKey(fk) = c {
                cl.push(format!("ADD {}", fk_clause(fk, d)));
            }
        }
        for c in &self.changes {
            if let Change::TableOptions { .. } = c {
                // Engine and collation don't exist here; the comment is its own
                // statement, emitted below.
            }
        }
        if !cl.is_empty() {
            out.push(format!(
                "ALTER TABLE {}\n  {};",
                self.qname(),
                cl.join(",\n  ")
            ));
        }
        // 3. Indexes back on, over the columns that now exist.
        for c in &self.changes {
            if let Change::AddIndex(ix) = c {
                out.push(create_index_sql(ix, &self.qname(), d));
            }
        }
        // 4. Comments are never inline on PostgreSQL.
        for c in &self.changes {
            match c {
                Change::AlterColumn { from, to, .. } if from.comment != to.comment => {
                    out.push(comment_on_column(
                        &self.qname(),
                        &to.name,
                        to.comment.as_deref(),
                        d,
                    ));
                }
                Change::AddColumn { column, .. } if column.comment.is_some() => {
                    out.push(comment_on_column(
                        &self.qname(),
                        &column.name,
                        column.comment.as_deref(),
                        d,
                    ));
                }
                // An emptied comment means *no* comment, so `IS NULL` rather than
                // `IS ''` — the two are distinct to `pg_description`, and an
                // empty one would re-read as a comment the user didn't write.
                Change::TableOptions {
                    comment: Some(cm), ..
                } => out.push(format!(
                    "COMMENT ON TABLE {} IS {};",
                    self.qname(),
                    comment_literal(Some(cm.as_str()).filter(|c| !c.is_empty()), d)
                )),
                _ => {}
            }
        }
        for c in &self.changes {
            if let Change::RenameTable { to } = c {
                // `RENAME TO` takes a bare name — the table can't change schema.
                out.push(format!("ALTER TABLE {} RENAME TO {};", self.qname(), q(to)));
            }
        }
        out
    }

    fn key_list(&self, cols: &[String]) -> String {
        cols.iter()
            .map(|c| self.q(c))
            .collect::<Vec<_>>()
            .join(", ")
    }

    // ── Views ────────────────────────────────────────────────────────────────

    /// The view statements in this set, in the order they must run.
    ///
    /// One function for both engines rather than an arm in each: a view is a
    /// name and a `SELECT` everywhere, and the divergence is small enough to
    /// live inside [`create_view_sql`] — quoting, the `DEFINER`/storage clauses,
    /// and the rename verb. Views never share a set with column changes (they
    /// come out of [`diff_view`], which produces nothing else), so this runs
    /// before the `ALTER TABLE` builders without interleaving with them.
    fn view_statements(&self) -> Vec<String> {
        let d = self.dialect;
        let mut out = Vec::new();
        for c in &self.changes {
            match c {
                Change::CreateView(draft) => {
                    out.push(create_view_sql(draft, &draft.name, d, false))
                }
                Change::ReplaceView { draft, recreate } => {
                    // A replace addresses the view under the name the server
                    // knows; a re-create drops that one and builds the draft's,
                    // which is how a rename comes along for free.
                    let server_name = draft.original.as_deref().unwrap_or(&draft.name);
                    if *recreate {
                        out.push(drop_view_sql(
                            &qualified(server_name, draft.schema.as_deref(), d),
                            draft.options.materialized,
                        ));
                        out.push(create_view_sql(draft, &draft.name, d, false));
                    } else {
                        out.push(create_view_sql(draft, server_name, d, true));
                    }
                }
                Change::DropView { materialized } => {
                    out.push(drop_view_sql(&self.qname(), *materialized))
                }
                // MySQL has no `ALTER VIEW … RENAME`; `RENAME TABLE` is what it
                // renames a view with.
                Change::RenameView { to } => out.push(match d {
                    SqlDialect::Postgres => format!(
                        "ALTER VIEW {} RENAME TO {};",
                        self.qname(),
                        ddl_ident_in(to, d)
                    ),
                    _ => format!(
                        "RENAME TABLE {} TO {};",
                        self.qname(),
                        qualified(to, self.schema.as_deref(), d)
                    ),
                }),
                _ => {}
            }
        }
        out
    }
}

/// The `CREATE VIEW` for an introspected view — the **display and copy** path's
/// emitter, which is deliberately the same one the apply path uses. `None` for a
/// base table, which has no view to emit.
///
/// This exists because there used to be two view emitters. The other one — the
/// view branch of [`TableInfo::create_ddl`] — never read `view_options`, so
/// "Copy DDL" handed over a `CREATE OR REPLACE VIEW` stripped of `ALGORITHM`,
/// `DEFINER`, `SQL SECURITY` and the check option. Run, it *succeeded* (the
/// `OR REPLACE` saw to that) and silently turned an `INVOKER` view into a
/// `DEFINER` one — a privilege change — and stopped an updatable view checking
/// its own `WHERE`. The same text reached the AI through the MCP table-info tool
/// and every view at once through "Copy DDL" on a schema.
///
/// Plain `CREATE VIEW`, not `OR REPLACE`: a copied skeleton is meant to recreate
/// the object somewhere, and failing loudly on a name collision beats
/// overwriting whatever is already there.
pub fn view_ddl(t: &TableInfo, dialect: SqlDialect) -> Option<String> {
    let v = ViewDraft::from_table(t)?;
    Some(create_view_sql(&v, &v.name, dialect, false))
}

/// `CREATE [OR REPLACE] VIEW`, with every option restated.
///
/// `name` is the view the statement addresses, which isn't always
/// `draft.name` — a replace has to name the view the server already has, since
/// the rename runs after it.
fn create_view_sql(v: &ViewDraft, name: &str, dialect: SqlDialect, replace: bool) -> String {
    let pg = dialect == SqlDialect::Postgres;
    let o = &v.options;
    fn set(s: &Option<String>) -> Option<&str> {
        s.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }
    let mut sql = String::from("CREATE ");
    if replace {
        sql.push_str("OR REPLACE ");
    }
    if !pg {
        // MySQL's clause order is fixed: ALGORITHM, DEFINER, SQL SECURITY, VIEW.
        // `UNDEFINED` is the default and says nothing, so it isn't emitted.
        if let Some(a) = set(&o.algorithm).filter(|a| !a.eq_ignore_ascii_case("UNDEFINED")) {
            sql.push_str(&format!("ALGORITHM = {} ", a.to_ascii_uppercase()));
        }
        if let Some(def) = set(&o.definer) {
            sql.push_str(&definer_sql(def));
            sql.push(' ');
        }
        if let Some(s) = set(&o.security) {
            sql.push_str(&format!("SQL SECURITY {} ", s.to_ascii_uppercase()));
        }
    }
    if pg && o.materialized {
        sql.push_str("MATERIALIZED ");
    }
    sql.push_str("VIEW ");
    sql.push_str(&qualified(name, v.schema.as_deref(), dialect));
    if pg && !o.storage.is_empty() {
        sql.push_str(&format!(" WITH ({})", o.storage.join(", ")));
    }
    sql.push_str(" AS\n");
    sql.push_str(&view_body(&v.select));
    // A materialized view has no check option — it isn't updatable at all.
    if !o.materialized
        && let Some(co) = set(&o.check_option).filter(|c| !c.eq_ignore_ascii_case("NONE"))
    {
        sql.push_str(&format!("\nWITH {} CHECK OPTION", co.to_ascii_uppercase()));
    }
    // The body is the user's own SQL and may end in a line comment — a trailing
    // `-- note` is ordinary. Pushing `;` straight onto it puts the terminator
    // *inside* the comment, and then "Open in editor" hands over a script whose
    // splitter runs this statement joined to the next one. Asked of the shared
    // lexer rather than hand-checked for `--`/`#`, so the dialects can't drift.
    if pairs::region_at(&sql, sql.len().saturating_sub(1), dialect) == pairs::Region::Comment {
        sql.push('\n');
    }
    sql.push(';');
    sql
}

fn drop_view_sql(qname: &str, materialized: bool) -> String {
    format!(
        "DROP {}VIEW {qname};",
        if materialized { "MATERIALIZED " } else { "" }
    )
}

// ── Emitter helpers ──────────────────────────────────────────────────────────

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// A table (or index) name, schema-qualified when the namespace isn't the one
/// the server resolves to anyway.
fn qualified(name: &str, schema: Option<&str>, dialect: SqlDialect) -> String {
    match sql_qualifier(schema) {
        Some(s) => format!(
            "{}.{}",
            ddl_ident_in(s, dialect),
            ddl_ident_in(name, dialect)
        ),
        None => ddl_ident_in(name, dialect),
    }
}

/// `CONSTRAINT … FOREIGN KEY (…) REFERENCES … (…) [ON DELETE …] [ON UPDATE …]`,
/// the one form both engines share (MySQL puts `ADD` in front of it, so does
/// PostgreSQL; only the quoting differs).
fn fk_clause(fk: &ForeignKeyInfo, dialect: SqlDialect) -> String {
    let q = |s: &str| ddl_ident_in(s, dialect);
    let cols = |v: &[String]| v.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ");
    // On MySQL the referenced schema is a *database*; on PostgreSQL a namespace,
    // where `public` resolves unqualified.
    let target = match dialect {
        SqlDialect::Postgres => qualified(&fk.ref_table, fk.ref_schema.as_deref(), dialect),
        _ => match fk.ref_schema.as_deref().filter(|s| !s.is_empty()) {
            Some(s) => format!("{}.{}", q(s), q(&fk.ref_table)),
            None => q(&fk.ref_table),
        },
    };
    let mut out = format!(
        "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {target} ({})",
        q(&fk.name),
        cols(&fk.columns),
        cols(&fk.ref_columns)
    );
    // `NO ACTION` is the unwritten default on both engines, so `None` emits
    // nothing and an untouched key round-trips exactly.
    if let Some(a) = &fk.on_delete {
        out.push_str(&format!(" ON DELETE {a}"));
    }
    if let Some(a) = &fk.on_update {
        out.push_str(&format!(" ON UPDATE {a}"));
    }
    out
}

fn create_index_sql(ix: &IndexInfo, qtable: &str, dialect: SqlDialect) -> String {
    let uniq = if ix.unique { "UNIQUE " } else { "" };
    let using = match &ix.method {
        Some(m) => format!(" USING {m}"),
        None => String::new(),
    };
    let filter = match &ix.predicate {
        Some(p) => format!(" WHERE {p}"),
        None => String::new(),
    };
    // The index name is never qualified — PostgreSQL puts an index in its
    // table's schema automatically and rejects `CREATE INDEX "s"."i"`.
    format!(
        "CREATE {uniq}INDEX {} ON {qtable}{using} ({}){filter};",
        ddl_ident_in(&ix.name, dialect),
        ix.key_sql(dialect)
    )
}

fn comment_literal(c: Option<&str>, dialect: SqlDialect) -> String {
    match c.filter(|s| !s.is_empty()) {
        Some(s) => ddl_string(s, dialect),
        None => "NULL".to_string(),
    }
}

fn comment_on_column(
    qtable: &str,
    column: &str,
    comment: Option<&str>,
    dialect: SqlDialect,
) -> String {
    format!(
        "COMMENT ON COLUMN {qtable}.{} IS {};",
        ddl_ident_in(column, dialect),
        comment_literal(comment, dialect)
    )
}

/// The `ALTER COLUMN` actions PostgreSQL needs to turn `from` into `to`.
///
/// Unlike MySQL there's no "replace the column" verb, so each attribute moves on
/// its own — which is also why nothing is destroyed by omission here. The one
/// exception is a generated expression, which PostgreSQL can't change in place
/// at all: that becomes a drop and a re-add of a column whose values were
/// derived anyway.
fn pg_column_clauses(from: &ColumnInfo, to: &ColumnInfo, d: SqlDialect) -> Vec<String> {
    let q = |s: &str| ddl_ident_in(s, d);
    let mut out = Vec::new();
    if from.generated != to.generated {
        out.push(format!("DROP COLUMN {}", q(&to.name)));
        out.push(format!("ADD COLUMN {}", to.definition_sql(d)));
        return out;
    }
    let name = q(&to.name);
    if !types_equal(&from.type_name, &to.type_name, d) || from.collation != to.collation {
        let coll = match to.collation.as_deref().filter(|c| !c.is_empty()) {
            Some(c) => format!(" COLLATE {}", q(c)),
            None => String::new(),
        };
        out.push(format!(
            "ALTER COLUMN {name} TYPE {}{coll} USING {name}::{}",
            to.type_name, to.type_name
        ));
    }
    if from.nullable != to.nullable {
        out.push(format!(
            "ALTER COLUMN {name} {} NOT NULL",
            if to.nullable { "DROP" } else { "SET" }
        ));
    }
    if !defaults_equal(from.default.as_deref(), to.default.as_deref()) {
        out.push(match norm_default(to.default.as_deref()) {
            Some(dv) => format!("ALTER COLUMN {name} SET DEFAULT {dv}"),
            None => format!("ALTER COLUMN {name} DROP DEFAULT"),
        });
    }
    if from.auto_increment != to.auto_increment {
        out.push(if to.auto_increment {
            format!("ALTER COLUMN {name} ADD GENERATED BY DEFAULT AS IDENTITY")
        } else {
            format!("ALTER COLUMN {name} DROP IDENTITY IF EXISTS")
        });
    }
    out
}

/// `CREATE TABLE` (plus, on PostgreSQL, the statements its `CREATE TABLE` can't
/// carry: indexes and comments).
fn create_table_sql(d: &TableDraft, dialect: SqlDialect) -> Vec<String> {
    let pg = dialect == SqlDialect::Postgres;
    let q = |s: &str| ddl_ident_in(s, dialect);
    let qname = qualified(&d.name, d.schema.as_deref(), dialect);
    let mut lines: Vec<String> = d
        .columns
        .iter()
        .map(|c| format!("  {}", c.info.definition_sql(dialect)))
        .collect();
    if !d.primary_key.is_empty() {
        lines.push(format!(
            "  PRIMARY KEY ({})",
            d.primary_key
                .iter()
                .map(|c| q(c))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !pg {
        // MySQL inlines its indexes; PostgreSQL can't and emits them after.
        for ix in &d.indexes {
            let kw = if ix.info.unique { "UNIQUE KEY" } else { "KEY" };
            lines.push(format!(
                "  {kw} {} ({})",
                q(&ix.info.name),
                ix.info.key_sql(dialect)
            ));
        }
    }
    for fk in &d.foreign_keys {
        lines.push(format!("  {}", fk_clause(&fk.info, dialect)));
    }
    let mut head = format!("CREATE TABLE {qname} (\n{}\n)", lines.join(",\n"));
    if !pg {
        if let Some(e) = d.engine.as_deref().filter(|e| !e.is_empty()) {
            head.push_str(&format!(" ENGINE={e}"));
        }
        if let Some(c) = d.collation.as_deref().filter(|c| !c.is_empty()) {
            head.push_str(&format!(" COLLATE={c}"));
        }
        if let Some(c) = d.comment.as_deref().filter(|c| !c.is_empty()) {
            head.push_str(&format!(" COMMENT={}", ddl_string(c, dialect)));
        }
    }
    head.push(';');
    let mut out = vec![head];
    if pg {
        for ix in &d.indexes {
            out.push(create_index_sql(&ix.info, &qname, dialect));
        }
        if let Some(c) = d.comment.as_deref().filter(|c| !c.is_empty()) {
            out.push(format!(
                "COMMENT ON TABLE {qname} IS {};",
                ddl_string(c, dialect)
            ));
        }
        for c in &d.columns {
            if let Some(cm) = c.info.comment.as_deref().filter(|s| !s.is_empty()) {
                out.push(comment_on_column(&qname, &c.info.name, Some(cm), dialect));
            }
        }
    }
    out
}

// ── Text forms the designer edits ────────────────────────────────────────────

/// An index's key columns as one editable line — `bio(20), age DESC`, the same
/// shape [`IndexInfo::key_sql`] emits, minus the quoting.
///
/// One field rather than a column picker because the picker can't express what
/// this has to: order, MySQL prefix lengths, and per-column `DESC`. The syntax is
/// already the one the user reads in generated DDL.
pub fn key_list_text(cols: &[crate::schema::IndexColumn]) -> String {
    cols.iter()
        .map(|c| {
            // Parenthesised, which is both how PostgreSQL writes it and what
            // tells `parse_key_list` this piece is an expression rather than a
            // column whose name happens to contain brackets.
            let mut s = if c.expression {
                format!("({})", c.name)
            } else {
                c.name.clone()
            };
            if let Some(n) = c.prefix {
                s.push_str(&format!("({n})"));
            }
            if c.descending {
                s.push_str(" DESC");
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The inverse of [`key_list_text`]. Unparseable pieces come back as a plain
/// column name, so a typo surfaces as "that isn't a column"
/// ([`TableDraft::validate`]) rather than as silently dropped input.
pub fn parse_key_list(s: &str) -> Vec<crate::schema::IndexColumn> {
    split_keys(s)
        .into_iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| {
            // ` DESC` / ` ASC` suffix first, then a `(n)` prefix length.
            let (head, descending) = match p.rsplit_once(char::is_whitespace) {
                Some((h, tail)) if tail.eq_ignore_ascii_case("desc") => (h.trim(), true),
                Some((h, tail)) if tail.eq_ignore_ascii_case("asc") => (h.trim(), false),
                _ => (p, false),
            };
            // A piece wrapped in its own parentheses is an expression key —
            // `(lower(email))`. Checked before the prefix rule below, which reads
            // `(` as the start of a MySQL prefix length.
            if let Some(inner) = unwrap_parens(head) {
                return crate::schema::IndexColumn {
                    descending,
                    ..crate::schema::IndexColumn::expr(inner)
                };
            }
            let prefix = match (head.find('('), head.ends_with(')')) {
                (Some(i), true) => head[i + 1..head.len() - 1].trim().parse::<u32>().ok(),
                _ => None,
            };
            // Only a length that actually parsed is taken off the name — `bio(x)`
            // stays `bio(x)` so validation reports it instead of quietly
            // creating an index on `bio`.
            let name = match (prefix, head.find('(')) {
                (Some(_), Some(i)) => head[..i].trim(),
                _ => head,
            };
            crate::schema::IndexColumn {
                name: name.to_string(),
                prefix,
                descending,
                expression: false,
            }
        })
        .collect()
}

/// `s` without the parentheses enclosing the **whole** of it, or `None` when it
/// isn't enclosed by a single matching pair.
///
/// The matching part is the point: `(a) + (b)` starts with `(` and ends with `)`
/// and stripping both leaves `a) + (b`. Shared by the designer's key-list parser
/// and PostgreSQL's index introspection, which both have to decide whether a
/// piece of SQL is a parenthesised expression, and got different answers when
/// each had its own copy.
pub fn unwrap_parens(s: &str) -> Option<&str> {
    let s = s.trim();
    if !s.starts_with('(') {
        return None;
    }
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return (i + 1 == s.len()).then(|| s[1..i].trim());
                }
            }
            _ => {}
        }
    }
    None // unbalanced
}

/// Split a key list on the commas that separate *keys*, ignoring those inside
/// parentheses — `coalesce(a, b)` is one key, not two.
fn split_keys(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut depth) = (0usize, 0i32);
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = (depth - 1).max(0),
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// A comma-separated list of bare names (a foreign key's columns), trimmed and
/// with empties dropped.
pub fn parse_name_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// The referential actions a foreign key can take, in menu order. `None` is
/// `NO ACTION` — the default both engines leave unwritten.
pub const FK_ACTIONS: [Option<&str>; 5] = [
    None,
    Some("RESTRICT"),
    Some("CASCADE"),
    Some("SET NULL"),
    Some("SET DEFAULT"),
];

/// Column types offered as a shortcut beside the free-form type field. A
/// *shortcut*, not a picker: the field stays free text and the server stays the
/// authority on what a type is, exactly as with import's coercion.
pub fn common_types(dialect: SqlDialect) -> &'static [&'static str] {
    match dialect {
        SqlDialect::Postgres => &[
            "integer",
            "bigint",
            "smallint",
            "numeric(10,2)",
            "real",
            "double precision",
            "boolean",
            "varchar(255)",
            "text",
            "char(1)",
            "date",
            "timestamp",
            "timestamptz",
            "time",
            "uuid",
            "json",
            "jsonb",
            "bytea",
        ],
        _ => &[
            "int",
            "bigint",
            "smallint",
            "tinyint(1)",
            "decimal(10,2)",
            "float",
            "double",
            "varchar(255)",
            "char(1)",
            "text",
            "longtext",
            "date",
            "datetime",
            "timestamp",
            "time",
            "year",
            "json",
            "blob",
        ],
    }
}

/// MySQL storage engines worth offering. Free-form elsewhere for the same reason
/// types are.
pub const MYSQL_ENGINES: [&str; 4] = ["InnoDB", "MyISAM", "MEMORY", "ARCHIVE"];

// ── Type + default equivalence ───────────────────────────────────────────────

/// A declared type taken apart: its base keyword(s) and whatever was inside the
/// parentheses.
struct TypeParts {
    /// `numeric(10,2)` → `numeric`, `int(11) unsigned` → `int unsigned`,
    /// `timestamp(3) without time zone` → `timestamp without time zone`. Lower-cased.
    base: String,
    /// The parameters when **every** one of them is an integer.
    params: Vec<i64>,
    /// The raw parameter text when they are not — an `ENUM`/`SET` value list.
    /// Kept verbatim (bar outer whitespace) because those values *are* the type:
    /// dropping them made every `ENUM` equal to every other one.
    values: Option<String>,
}

/// Split a declared type into its base keyword(s) and its parenthesised
/// parameters: `numeric(10,2)` → `("numeric", [10, 2])`, `int(11) unsigned` →
/// `("int unsigned", [11])`, `timestamp(3) without time zone` →
/// `("timestamp without time zone", [3])`, `enum('a','b')` →
/// `("enum", [], Some("'a','b'"))`.
fn split_type(t: &str) -> TypeParts {
    let t = t.trim();
    let (head, rest) = match t.find('(') {
        Some(i) => (&t[..i], &t[i + 1..]),
        None => (t, ""),
    };
    // The *last* `)`, not the first: an ENUM value may contain one.
    let (args, tail) = match rest.rfind(')') {
        Some(j) => (&rest[..j], &rest[j + 1..]),
        None => ("", ""),
    };
    let base = format!("{} {}", head.trim(), tail.trim())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let args = args.trim();
    let params: Vec<i64> = args
        .split(',')
        .filter_map(|p| p.trim().parse::<i64>().ok())
        .collect();
    // All-integer parameters get compared numerically (so `( 10 , 2 )` and
    // `(10,2)` agree). Anything else is a value list and is compared as text.
    let all_numeric = !args.is_empty() && params.len() == args.split(',').count();
    TypeParts {
        base,
        params: if all_numeric { params } else { Vec::new() },
        values: (!args.is_empty() && !all_numeric).then(|| args.to_string()),
    }
}

/// A declared type reduced to a canonical spelling, so two ways of writing the
/// same type compare equal.
///
/// This is the difference between a designer that opens clean and one that shows
/// a phantom change on every column: MariaDB reports `int(11)` where MySQL 8
/// reports `int`, PostgreSQL reports `character varying(45)` where every human
/// writes `varchar(45)`, and neither difference means anything.
pub fn normalize_type(t: &str, dialect: SqlDialect) -> String {
    let TypeParts {
        base,
        params,
        values,
    } = split_type(t);
    if base.is_empty() {
        return String::new();
    }
    let pg = dialect == SqlDialect::Postgres;
    // Suffixes MySQL appends to a numeric type (`int unsigned zerofill`) travel
    // with the base word, so alias only the leading keyword(s).
    let (word, suffix) = match base.split_once(' ') {
        Some((w, s)) if !pg => (w.to_string(), format!(" {s}")),
        _ => (base.clone(), String::new()),
    };
    let canon: &str = if pg {
        match word.as_str() {
            "character varying" | "varchar" => "varchar",
            "character" | "bpchar" | "char" => "char",
            "integer" | "int4" | "int" => "int",
            "bigint" | "int8" => "bigint",
            "smallint" | "int2" => "smallint",
            "boolean" | "bool" => "boolean",
            "double precision" | "float8" => "double precision",
            "real" | "float4" => "real",
            "decimal" | "numeric" => "numeric",
            "timestamp" | "timestamp without time zone" => "timestamp",
            "timestamptz" | "timestamp with time zone" => "timestamptz",
            "time" | "time without time zone" => "time",
            "timetz" | "time with time zone" => "timetz",
            other => other,
        }
    } else {
        match word.as_str() {
            "integer" => "int",
            "dec" | "fixed" | "numeric" => "decimal",
            // MySQL's REAL is a DOUBLE unless REAL_AS_FLOAT is set.
            "real" => "double",
            "character varying" => "varchar",
            other => other,
        }
    };
    // MySQL integer display widths (`int(11)`) carry no meaning and are
    // deprecated — except `tinyint(1)`, which is how BOOLEAN is stored and is
    // the one width a client can act on.
    let integer = matches!(
        canon,
        "tinyint" | "smallint" | "mediumint" | "int" | "bigint"
    );
    let drop_width = !pg && integer && !(canon == "tinyint" && params.first() == Some(&1));
    // `bool`/`boolean` is a MySQL alias for exactly `tinyint(1)`.
    if !pg && matches!(word.as_str(), "bool" | "boolean") {
        return format!("tinyint(1){suffix}");
    }
    // An `ENUM`/`SET` value list is re-emitted verbatim. Comparison is exact,
    // deliberately: splitting it into values needs a quote-aware scan, and a
    // value may itself contain a comma — so `enum('a', 'b')` vs `enum('a','b')`
    // is reported as a change rather than risk collapsing two genuinely
    // different types into one. A phantom change is visible and previewable; a
    // missed edit is silent, which is the bug this replaced.
    let args = match &values {
        Some(v) => format!("({v})"),
        None if params.is_empty() || drop_width => String::new(),
        None => format!(
            "({})",
            params
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
    };
    format!("{canon}{args}{suffix}")
}

/// Do these two declared types mean the same thing to `dialect`?
pub fn types_equal(a: &str, b: &str, dialect: SqlDialect) -> bool {
    normalize_type(a, dialect) == normalize_type(b, dialect)
}

/// A `DEFAULT` clause reduced to what would actually be emitted. An absent
/// default, an empty field and the word `NULL` all mean the same thing — no
/// default worth writing — so the designer's empty box doesn't read as a change.
pub fn norm_default(d: Option<&str>) -> Option<String> {
    let d = d?.trim();
    if d.is_empty() || d.eq_ignore_ascii_case("null") {
        return None;
    }
    Some(d.to_string())
}

/// Compare two defaults the way the server would. A quoted literal is compared
/// exactly (case *is* the value); anything else is an expression or a keyword,
/// where case and spacing are noise.
pub fn defaults_equal(a: Option<&str>, b: Option<&str>) -> bool {
    match (norm_default(a), norm_default(b)) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            if x.starts_with('\'') || y.starts_with('\'') {
                x == y
            } else {
                let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
                squash(&x).eq_ignore_ascii_case(&squash(&y))
            }
        }
        _ => false,
    }
}

/// Is this column definition the same as that one, as far as the server is
/// concerned? Attributes the dialect can't express are ignored rather than
/// compared — `ON UPDATE` doesn't exist on PostgreSQL, so a difference in it
/// there would be a change that could never be emitted.
fn columns_equal(a: &ColumnInfo, b: &ColumnInfo, d: SqlDialect) -> bool {
    let pg = d == SqlDialect::Postgres;
    a.name == b.name
        && types_equal(&a.type_name, &b.type_name, d)
        && a.nullable == b.nullable
        && defaults_equal(a.default.as_deref(), b.default.as_deref())
        && a.auto_increment == b.auto_increment
        && a.generated.as_deref().map(str::trim) == b.generated.as_deref().map(str::trim)
        && (pg || a.on_update == b.on_update)
        && blank_as_none(a.comment.as_deref()) == blank_as_none(b.comment.as_deref())
        && blank_as_none(a.collation.as_deref()) == blank_as_none(b.collation.as_deref())
}

fn blank_as_none(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.is_empty())
}

/// Two indexes are the same when they'd build the same structure. The name is
/// compared too — renaming an index is a drop and a create, same as any other
/// change to it.
fn indexes_equal(a: &IndexInfo, b: &IndexInfo) -> bool {
    a.name == b.name
        && a.unique == b.unique
        && a.columns == b.columns
        && a.method == b.method
        && a.predicate == b.predicate
}

fn fks_equal(a: &ForeignKeyInfo, b: &ForeignKeyInfo) -> bool {
    a.name == b.name
        && a.columns == b.columns
        && a.ref_table == b.ref_table
        && a.ref_columns == b.ref_columns
        && a.on_delete == b.on_delete
        && a.on_update == b.on_update
        // A `None` namespace means "the same one", so it matches the explicit form.
        && match (a.ref_schema.as_deref(), b.ref_schema.as_deref()) {
            (Some(x), Some(y)) => x == y,
            _ => true,
        }
}

// ── The diff ─────────────────────────────────────────────────────────────────

/// Everything that has to happen to turn `current` into `draft`.
///
/// Diffing a table against [`TableDraft::from_table`] of itself must produce
/// nothing — that's the round-trip gate, and it's what catches a model-fidelity
/// gap before a user ever sees a phantom change.
pub fn diff(current: &TableInfo, draft: &TableDraft, dialect: SqlDialect) -> ChangeSet {
    let mut changes: Vec<Change> = Vec::new();

    // Which server-side columns the draft still claims, and under what name.
    let mut renamed: HashMap<String, String> = HashMap::new();
    let claimed: HashSet<&str> = draft
        .columns
        .iter()
        .filter_map(|c| c.original.as_deref())
        .collect();
    let by_name: HashMap<&str, &ColumnInfo> = current
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    for c in &current.columns {
        if !claimed.contains(c.name.as_str()) {
            changes.push(Change::DropColumn {
                name: c.name.clone(),
                type_name: c.type_name.clone(),
            });
        }
    }
    // Positions are computed after the adds and drops are known, so they're
    // filled in below rather than here.
    let mut alter_at: HashMap<String, usize> = HashMap::new();
    let mut add_at: HashMap<String, usize> = HashMap::new();
    for c in &draft.columns {
        match c.original.as_deref().and_then(|n| by_name.get(n)) {
            Some(cur) => {
                if cur.name != c.info.name {
                    renamed.insert(cur.name.clone(), c.info.name.clone());
                }
                if !columns_equal(cur, &c.info, dialect) {
                    alter_at.insert(c.info.name.clone(), changes.len());
                    changes.push(Change::AlterColumn {
                        from: Box::new((*cur).clone()),
                        to: Box::new(c.info.clone()),
                        position: None,
                    });
                }
            }
            // Either genuinely new, or claiming a column that no longer exists —
            // both are "add it", which is the safe reading.
            None => {
                add_at.insert(c.info.name.clone(), changes.len());
                changes.push(Change::AddColumn {
                    column: Box::new(c.info.clone()),
                    position: None,
                });
            }
        }
    }

    // Column order — MySQL only. PostgreSQL can't move a column, so a reordered
    // draft there is a preference the server has no way to honor, and pretending
    // otherwise would emit statements that fail.
    if dialect != SqlDialect::Postgres {
        apply_positions(current, draft, &renamed, &alter_at, &add_at, &mut changes);
    }

    // Primary key. The current key is named in server-side terms, so map it
    // through the renames before comparing with the draft's.
    let current_pk: Vec<String> = primary_key_of(current)
        .into_iter()
        .map(|c| renamed.get(&c).cloned().unwrap_or(c))
        .collect();
    if current_pk != draft.primary_key {
        changes.push(Change::PrimaryKey {
            from: current_pk,
            to: draft.primary_key.clone(),
            drop_constraint: current
                .indexes
                .iter()
                .find(|ix| ix.is_primary())
                .and_then(|ix| ix.constraint.clone()),
        });
    }

    // Indexes. Nothing about an index is alterable, so a change is a drop and a
    // create — which also means the drop has to carry the *old* name.
    let ix_claimed: HashSet<&str> = draft
        .indexes
        .iter()
        .filter_map(|i| i.original.as_deref())
        .collect();
    let cur_indexes: Vec<&IndexInfo> = current
        .indexes
        .iter()
        .filter(|ix| !ix.is_primary())
        .collect();
    let mut dropped_ix: Vec<&IndexInfo> = Vec::new();
    let mut added_ix: Vec<IndexInfo> = Vec::new();
    for ix in &cur_indexes {
        if !ix_claimed.contains(ix.name.as_str()) {
            dropped_ix.push(ix);
        }
    }
    for d in &draft.indexes {
        let current_ix = d
            .original
            .as_deref()
            .and_then(|n| cur_indexes.iter().find(|ix| ix.name == n).copied());
        // Compare against the current index rewritten in the draft's names, so a
        // renamed column doesn't read as a changed index.
        match current_ix {
            Some(ix) if indexes_equal(&rename_index(ix, &renamed), &d.info) => {}
            // An index we could only partly read: recreating it from this model
            // would silently destroy the parts introspection never saw (an
            // expression key column, an operator class, a NULLS ordering). The
            // edit is withheld and said out loud instead. Removing the index
            // outright is still allowed — that path doesn't come through here,
            // because a deleted draft index leaves nothing to compare against.
            Some(ix) if ix.lossy => {
                changes.push(Change::KeepLossyIndex {
                    name: ix.name.clone(),
                });
            }
            Some(ix) => {
                dropped_ix.push(ix);
                added_ix.push(d.info.clone());
            }
            None => added_ix.push(d.info.clone()),
        }
    }
    for ix in dropped_ix {
        changes.push(Change::DropIndex {
            name: ix.name.clone(),
            constraint: ix.constraint.clone(),
        });
    }
    for ix in added_ix {
        changes.push(Change::AddIndex(Box::new(ix)));
    }

    // Foreign keys, on the same drop-and-recreate rule.
    let fk_claimed: HashSet<&str> = draft
        .foreign_keys
        .iter()
        .filter_map(|f| f.original.as_deref())
        .collect();
    let mut dropped_fk: Vec<String> = Vec::new();
    let mut added_fk: Vec<ForeignKeyInfo> = Vec::new();
    for fk in &current.foreign_keys {
        if !fk_claimed.contains(fk.name.as_str()) {
            dropped_fk.push(fk.name.clone());
        }
    }
    for d in &draft.foreign_keys {
        let cur = d
            .original
            .as_deref()
            .and_then(|n| current.foreign_keys.iter().find(|f| f.name == n));
        match cur {
            Some(fk) if fks_equal(&rename_fk(fk, &renamed), &d.info) => {}
            Some(fk) => {
                dropped_fk.push(fk.name.clone());
                added_fk.push(d.info.clone());
            }
            None => added_fk.push(d.info.clone()),
        }
    }
    for name in dropped_fk {
        changes.push(Change::DropForeignKey { name });
    }
    for fk in added_fk {
        changes.push(Change::AddForeignKey(Box::new(fk)));
    }

    // Table-level options. On PostgreSQL only the comment exists, and the draft
    // carries `None` for the other two on both sides, so nothing is emitted.
    //
    // Each field of the change carries **only what changed** — `None` means "not
    // part of this change" — so the summary, the change count and the emitted SQL
    // can't disagree. They did: clearing the Engine field counted as a change,
    // but the emitter skips an empty clause, so the statement didn't touch the
    // engine.
    //
    // Clearing engine or collation is therefore *not* a change at all: a MySQL
    // table always has both, so an emptied field means "leave it". A cleared
    // comment is different — "no comment" is a state a table can really be in.
    let set_to = |cur: &Option<String>, dr: &Option<String>| -> Option<String> {
        let dr = dr.as_deref().map(str::trim).filter(|v| !v.is_empty())?;
        (blank_as_none(cur.as_deref()).map(str::trim) != Some(dr)).then(|| dr.to_string())
    };
    let engine = set_to(&current.engine, &draft.engine);
    let collation = set_to(&current.collation, &draft.collation);
    let comment = {
        let (cur, dr) = (
            blank_as_none(current.comment.as_deref()),
            blank_as_none(draft.comment.as_deref()),
        );
        (cur != dr).then(|| dr.unwrap_or_default().to_string())
    };
    if engine.is_some() || collation.is_some() || comment.is_some() {
        changes.push(Change::TableOptions {
            engine,
            collation,
            comment,
        });
    }

    if draft.name != current.name && !draft.name.trim().is_empty() {
        changes.push(Change::RenameTable {
            to: draft.name.clone(),
        });
    }

    ChangeSet {
        table: current.name.clone(),
        schema: current.schema.clone(),
        dialect,
        changes,
    }
}

/// Fill in `AFTER`/`FIRST` on the columns whose position actually moved.
///
/// The simulation matters: `ALTER TABLE` applies its clauses in order, so a
/// position is only meaningful against the order as it stands *at that clause*.
/// Walking the target order and moving only the columns that are out of place
/// keeps the statement to the moves the user actually made, instead of restating
/// every column's position on every edit.
fn apply_positions(
    current: &TableInfo,
    draft: &TableDraft,
    renamed: &HashMap<String, String>,
    alter_at: &HashMap<String, usize>,
    add_at: &HashMap<String, usize>,
    changes: &mut Vec<Change>,
) {
    let target: Vec<String> = draft.column_names();
    // The order the table will be in once the drops and adds have run: surviving
    // columns keep their server-side order, new ones land at the end.
    let survivors: Vec<String> = current
        .columns
        .iter()
        .map(|c| {
            renamed
                .get(&c.name)
                .cloned()
                .unwrap_or_else(|| c.name.clone())
        })
        .filter(|n| target.contains(n))
        .collect();
    let mut sim: Vec<String> = survivors.clone();
    for n in &target {
        if !sim.contains(n) {
            sim.push(n.clone());
        }
    }
    if sim == target {
        return;
    }
    for (i, name) in target.iter().enumerate() {
        if sim.get(i) == Some(name) {
            continue;
        }
        let pos = match i {
            0 => Position::First,
            _ => Position::After(target[i - 1].clone()),
        };
        // Re-run the move on the simulated order so later comparisons see the
        // table as the server will.
        if let Some(at) = sim.iter().position(|c| c == name) {
            let moved = sim.remove(at);
            sim.insert(i.min(sim.len()), moved);
        }
        // Attach the position to the change that already touches this column,
        // or raise a definition-preserving `MODIFY` if nothing else does.
        if let Some(&ci) = add_at.get(name) {
            if let Change::AddColumn { position, .. } = &mut changes[ci] {
                *position = Some(pos);
            }
        } else if let Some(&ci) = alter_at.get(name) {
            if let Change::AlterColumn { position, .. } = &mut changes[ci] {
                *position = Some(pos);
            }
        } else if let Some(info) = draft
            .columns
            .iter()
            .find(|c| c.info.name == *name)
            .map(|c| c.info.clone())
        {
            changes.push(Change::AlterColumn {
                from: Box::new(info.clone()),
                to: Box::new(info),
                position: Some(pos),
            });
        }
    }
}

/// The same index with its key columns renamed — so an index on a column the
/// draft renamed compares as unchanged rather than as a drop and a create.
fn rename_index(ix: &IndexInfo, renamed: &HashMap<String, String>) -> IndexInfo {
    let mut out = ix.clone();
    for c in &mut out.columns {
        if let Some(n) = renamed.get(&c.name) {
            c.name = n.clone();
        }
    }
    out
}

fn rename_fk(fk: &ForeignKeyInfo, renamed: &HashMap<String, String>) -> ForeignKeyInfo {
    let mut out = fk.clone();
    for c in &mut out.columns {
        if let Some(n) = renamed.get(c) {
            *c = n.clone();
        }
    }
    out
}

// ── Ready-made change sets (the context-menu shortcuts) ──────────────────────

/// Everything that has to happen to turn the view `current` into `draft`.
///
/// Same round-trip gate as [`diff`]: a view diffed against its own draft must
/// produce nothing. The one decision here that isn't a comparison is *how* a
/// redefinition is applied — see [`pg_replaceable`].
pub fn diff_view(current: &TableInfo, draft: &ViewDraft, dialect: SqlDialect) -> ChangeSet {
    let mut changes: Vec<Change> = Vec::new();
    let server = ViewDraft::from_table(current);
    let old_body = server
        .as_ref()
        .map(|v| v.select.clone())
        .unwrap_or_default();
    let old_options = server.map(|v| v.options).unwrap_or_default();
    let renamed = draft.name != current.name && !draft.name.trim().is_empty();

    if view_body(&draft.select) != old_body || draft.options != old_options {
        // MySQL's `CREATE OR REPLACE VIEW` redefines anything, so the whole
        // question — and the override — is PostgreSQL's.
        let recreate = dialect == SqlDialect::Postgres
            && (draft.force_recreate || {
                let cols: Vec<String> = current.columns.iter().map(|c| c.name.clone()).collect();
                pg_replaceable(&cols, &draft.select, dialect) == Some(false)
            });
        changes.push(Change::ReplaceView {
            draft: Box::new(draft.clone()),
            recreate,
        });
        // A re-create already builds the view under its new name; renaming
        // after it would address a name nothing answers to.
        if renamed && !recreate {
            changes.push(Change::RenameView {
                to: draft.name.clone(),
            });
        }
    } else if renamed {
        changes.push(Change::RenameView {
            to: draft.name.clone(),
        });
    }

    ChangeSet {
        table: current.name.clone(),
        schema: current.schema.clone(),
        dialect,
        changes,
    }
}

/// The `CREATE VIEW` for a brand-new view.
pub fn create_view(draft: &ViewDraft, dialect: SqlDialect) -> ChangeSet {
    ChangeSet {
        table: draft.name.clone(),
        schema: draft.schema.clone(),
        dialect,
        changes: vec![Change::CreateView(Box::new(draft.clone()))],
    }
}

/// The `CREATE TABLE` for a brand-new table.
pub fn create(draft: &TableDraft, dialect: SqlDialect) -> ChangeSet {
    ChangeSet {
        table: draft.name.clone(),
        schema: draft.schema.clone(),
        dialect,
        changes: vec![Change::CreateTable(Box::new(draft.clone()))],
    }
}

/// A one-change set against a table — how every context-menu shortcut reaches
/// the preview without opening the designer.
pub fn single(table: &str, schema: Option<&str>, dialect: SqlDialect, change: Change) -> ChangeSet {
    ChangeSet {
        table: table.to_string(),
        schema: schema.map(str::to_string),
        dialect,
        changes: vec![change],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intel::SqlDialect::{MySql, Postgres};
    use crate::schema::IndexColumn;

    fn col(name: &str, ty: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: ty.into(),
            nullable: true,
            ..Default::default()
        }
    }

    /// A small but *complete* table: every field the model carries is populated,
    /// so the round-trip test can't pass by leaving something empty.
    fn users() -> TableInfo {
        TableInfo {
            name: "users".into(),
            schema: None,
            columns: vec![
                ColumnInfo {
                    name: "id".into(),
                    type_name: "int(11)".into(),
                    nullable: false,
                    primary_key: true,
                    auto_increment: true,
                    ..Default::default()
                },
                ColumnInfo {
                    name: "email".into(),
                    type_name: "varchar(255)".into(),
                    nullable: false,
                    collation: Some("utf8mb4_bin".into()),
                    comment: Some("login".into()),
                    ..Default::default()
                },
                ColumnInfo {
                    name: "status".into(),
                    type_name: "varchar(20)".into(),
                    nullable: false,
                    default: Some("'draft'".into()),
                    ..Default::default()
                },
                ColumnInfo {
                    name: "updated".into(),
                    type_name: "timestamp".into(),
                    nullable: false,
                    default: Some("CURRENT_TIMESTAMP".into()),
                    on_update: Some("CURRENT_TIMESTAMP".into()),
                    ..Default::default()
                },
            ],
            indexes: vec![
                IndexInfo::plain("PRIMARY", vec!["id"], true),
                IndexInfo {
                    name: "email_uq".into(),
                    columns: vec![IndexColumn::plain("email")],
                    unique: true,
                    ..Default::default()
                },
            ],
            foreign_keys: vec![ForeignKeyInfo {
                name: "fk_status".into(),
                columns: vec!["status".into()],
                ref_table: "statuses".into(),
                ref_columns: vec!["code".into()],
                on_delete: Some("CASCADE".into()),
                ..Default::default()
            }],
            engine: Some("InnoDB".into()),
            collation: Some("utf8mb4_general_ci".into()),
            comment: Some("people".into()),
            ..Default::default()
        }
    }

    // ── the round-trip gate ─────────────────────────────────────────────────

    #[test]
    fn a_table_diffed_against_itself_has_no_changes() {
        let t = users();
        for d in [MySql, Postgres] {
            let draft = TableDraft::from_table(&t);
            let cs = diff(&t, &draft, d);
            assert!(cs.is_empty(), "{d:?} phantom changes: {:?}", cs.changes);
            assert!(cs.emit().is_empty());
        }
    }

    #[test]
    fn a_postgres_table_round_trips_with_its_namespace_and_identity() {
        let t = TableInfo {
            name: "orders".into(),
            schema: Some("sales".into()),
            columns: vec![
                ColumnInfo {
                    name: "id".into(),
                    type_name: "integer".into(),
                    nullable: false,
                    primary_key: true,
                    auto_increment: true,
                    ..Default::default()
                },
                ColumnInfo {
                    name: "total".into(),
                    type_name: "numeric(10,2)".into(),
                    nullable: true,
                    default: Some("0".into()),
                    ..Default::default()
                },
            ],
            indexes: vec![IndexInfo {
                name: "PRIMARY".into(),
                columns: vec![IndexColumn::plain("id")],
                unique: true,
                constraint: Some("orders_pkey".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let cs = diff(&t, &TableDraft::from_table(&t), Postgres);
        assert!(cs.is_empty(), "{:?}", cs.changes);
    }

    #[test]
    fn an_empty_table_round_trips() {
        let t = TableInfo {
            name: "log".into(),
            columns: vec![col("msg", "text")],
            ..Default::default()
        };
        for d in [MySql, Postgres] {
            assert!(diff(&t, &TableDraft::from_table(&t), d).is_empty());
        }
    }

    // ── one field at a time ─────────────────────────────────────────────────

    /// The load-bearing one: MySQL replaces the column, so a widened `varchar`
    /// has to come back carrying its collation, comment, default — everything.
    #[test]
    fn widening_a_column_restates_its_whole_definition() {
        let t = users();
        let mut draft = TableDraft::from_table(&t);
        draft.columns[1].info.type_name = "varchar(320)".into();
        let cs = diff(&t, &draft, MySql);
        assert_eq!(cs.len(), 1);
        let sql = cs.script();
        assert!(sql.contains("MODIFY COLUMN `email` varchar(320)"), "{sql}");
        assert!(sql.contains("COLLATE utf8mb4_bin"), "{sql}");
        assert!(sql.contains("NOT NULL"), "{sql}");
        assert!(sql.contains("COMMENT 'login'"), "{sql}");
        // Widening loses nothing, so nothing is flagged.
        assert!(cs.destructive().is_empty());
    }

    #[test]
    fn narrowing_a_column_is_flagged_destructive() {
        let t = users();
        let mut draft = TableDraft::from_table(&t);
        draft.columns[1].info.type_name = "varchar(45)".into();
        let cs = diff(&t, &draft, MySql);
        let risk = cs.destructive();
        assert_eq!(risk.len(), 1);
        assert!(risk[0].contains("Narrowing email"), "{risk:?}");
        assert!(risk[0].contains("truncates"), "{risk:?}");
    }

    #[test]
    fn tightening_nullability_warns_before_it_fails() {
        let mut t = users();
        t.columns[1].nullable = true;
        let mut draft = TableDraft::from_table(&t);
        draft.columns[1].info.nullable = false;
        let risk = diff(&t, &draft, MySql).destructive();
        assert_eq!(risk.len(), 1);
        assert!(risk[0].contains("NOT NULL"), "{risk:?}");
    }

    #[test]
    fn renaming_a_column_is_a_change_not_a_drop_and_an_add() {
        let t = users();
        let mut draft = TableDraft::from_table(&t);
        draft.rename_column(1, "login_email");
        let cs = diff(&t, &draft, MySql);
        assert_eq!(cs.len(), 1, "{:?}", cs.changes);
        assert_eq!(
            cs.changes[0].summary(),
            "Rename column email to login_email"
        );
        let sql = cs.script();
        assert!(
            sql.contains("CHANGE COLUMN `email` `login_email` varchar(255)"),
            "{sql}"
        );
        // Nothing is dropped, so nothing is destroyed.
        assert!(cs.destructive().is_empty());
        // And the index over it follows the rename rather than being rebuilt.
        assert!(!sql.contains("DROP INDEX"), "{sql}");
    }

    #[test]
    fn renaming_a_column_on_postgres_is_its_own_statement() {
        let mut t = users();
        t.schema = Some("public".into());
        t.engine = None;
        t.collation = None;
        let mut draft = TableDraft::from_table(&t);
        draft.rename_column(1, "login_email");
        let sql = diff(&t, &draft, Postgres).script();
        assert_eq!(
            sql,
            "ALTER TABLE \"users\" RENAME COLUMN \"email\" TO \"login_email\";"
        );
    }

    #[test]
    fn dropping_a_column_says_what_it_costs() {
        let t = users();
        let mut draft = TableDraft::from_table(&t);
        // Removing the column takes the foreign key standing on it with it —
        // leaving that behind would emit a key over a column that's gone.
        draft.remove_column(2);
        assert!(draft.foreign_keys.is_empty());
        let cs = diff(&t, &draft, MySql);
        let sql = cs.script();
        // The foreign key standing on the column has to come off first.
        let fk_at = sql.find("DROP FOREIGN KEY").expect("fk dropped");
        let col_at = sql.find("DROP COLUMN").expect("column dropped");
        assert!(fk_at < col_at, "{sql}");
        assert!(
            cs.destructive()
                .iter()
                .any(|r| r.contains("Drops column status")),
            "{:?}",
            cs.destructive()
        );
    }

    #[test]
    fn adding_a_column_places_it_where_the_draft_puts_it() {
        let t = users();
        let mut draft = TableDraft::from_table(&t);
        draft.columns.insert(
            1,
            ColumnDraft::new(ColumnInfo {
                name: "nickname".into(),
                type_name: "varchar(40)".into(),
                nullable: true,
                ..Default::default()
            }),
        );
        let sql = diff(&t, &draft, MySql).script();
        assert!(
            sql.contains("ADD COLUMN `nickname` varchar(40) AFTER `id`"),
            "{sql}"
        );
        // PostgreSQL can't place a column, and mustn't pretend to.
        let pg = diff(&t, &draft, Postgres).script();
        assert!(pg.contains("ADD COLUMN \"nickname\""), "{pg}");
        assert!(!pg.contains("AFTER"), "{pg}");
    }

    #[test]
    fn moving_a_column_emits_one_modify_and_nothing_else() {
        let t = users();
        let mut draft = TableDraft::from_table(&t);
        let last = draft.columns.pop().expect("four columns");
        draft.columns.insert(0, last);
        let cs = diff(&t, &draft, MySql);
        assert_eq!(cs.len(), 1, "{:?}", cs.changes);
        assert_eq!(cs.changes[0].summary(), "Move column updated first");
        let sql = cs.script();
        assert!(sql.contains("MODIFY COLUMN `updated` timestamp"), "{sql}");
        assert!(sql.contains(" FIRST"), "{sql}");
        // Reordering on PostgreSQL isn't expressible, so it isn't claimed.
        assert!(diff(&t, &draft, Postgres).is_empty());
    }

    #[test]
    fn changing_the_primary_key_drops_then_adds_it() {
        let t = users();
        let mut draft = TableDraft::from_table(&t);
        draft.primary_key = vec!["id".into(), "email".into()];
        let cs = diff(&t, &draft, MySql);
        assert_eq!(cs.len(), 1);
        let sql = cs.script();
        let drop_at = sql.find("DROP PRIMARY KEY").expect("dropped");
        let add_at = sql.find("ADD PRIMARY KEY (`id`, `email`)").expect("added");
        assert!(drop_at < add_at, "{sql}");
    }

    #[test]
    fn postgres_drops_a_primary_key_by_its_constraint_name() {
        let mut t = users();
        t.engine = None;
        t.collation = None;
        t.indexes[0].constraint = Some("users_pkey".into());
        let mut draft = TableDraft::from_table(&t);
        draft.primary_key.clear();
        let cs = diff(&t, &draft, Postgres);
        let sql = cs.script();
        assert!(sql.contains("DROP CONSTRAINT \"users_pkey\""), "{sql}");
        assert!(!sql.contains("DROP PRIMARY KEY"), "{sql}");
        // Losing the key costs the grid its ability to edit rows.
        assert!(!cs.destructive().is_empty());
    }

    #[test]
    fn an_index_change_is_a_drop_and_a_create() {
        let t = users();
        let mut draft = TableDraft::from_table(&t);
        draft.indexes[0].info.unique = false;
        let cs = diff(&t, &draft, MySql);
        assert_eq!(cs.len(), 2);
        let sql = cs.script();
        assert!(sql.contains("DROP INDEX `email_uq`"), "{sql}");
        assert!(sql.contains("ADD INDEX `email_uq` (`email`)"), "{sql}");
    }

    #[test]
    fn postgres_creates_an_index_outside_the_alter() {
        let mut t = users();
        t.engine = None;
        t.collation = None;
        let mut draft = TableDraft::from_table(&t);
        draft.indexes.push(IndexDraft::new(IndexInfo {
            name: "status_ix".into(),
            columns: vec![IndexColumn::plain("status")],
            ..Default::default()
        }));
        let stmts = diff(&t, &draft, Postgres).emit();
        assert_eq!(
            stmts,
            vec!["CREATE INDEX \"status_ix\" ON \"users\" (\"status\");"]
        );
    }

    #[test]
    fn a_foreign_key_keeps_its_referential_actions_when_recreated() {
        let t = users();
        let mut draft = TableDraft::from_table(&t);
        draft.foreign_keys[0].info.ref_columns = vec!["id".into()];
        let sql = diff(&t, &draft, MySql).script();
        assert!(sql.contains("DROP FOREIGN KEY `fk_status`"), "{sql}");
        assert!(
            sql.contains(
                "ADD CONSTRAINT `fk_status` FOREIGN KEY (`status`) \
                 REFERENCES `statuses` (`id`) ON DELETE CASCADE"
            ),
            "{sql}"
        );
    }

    #[test]
    fn renaming_the_table_runs_last_and_under_the_old_name() {
        let t = users();
        let mut draft = TableDraft::from_table(&t);
        draft.name = "people".into();
        draft.columns[1].info.nullable = true;
        let stmts = diff(&t, &draft, MySql).emit();
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].starts_with("ALTER TABLE `users`"), "{stmts:?}");
        assert_eq!(stmts[1], "ALTER TABLE `users` RENAME TO `people`;");
    }

    #[test]
    fn table_options_are_one_change_but_restate_only_what_moved() {
        let t = users();
        let mut draft = TableDraft::from_table(&t);
        draft.comment = Some("staff".into());
        let cs = diff(&t, &draft, MySql);
        assert_eq!(cs.len(), 1, "the options are one change, not three");
        let sql = cs.script();
        assert!(sql.contains("COMMENT='staff'"), "{sql}");
        // The engine didn't change, so it isn't restated — a restated clause
        // reads as an edit the user didn't ask for.
        assert!(!sql.contains("ENGINE="), "{sql}");

        // …and two options edited at once still travel as one change.
        let mut draft = TableDraft::from_table(&t);
        draft.comment = Some("staff".into());
        draft.engine = Some("MyISAM".into());
        let cs = diff(&t, &draft, MySql);
        assert_eq!(cs.len(), 1);
        let sql = cs.script();
        assert!(
            sql.contains("ENGINE=MyISAM") && sql.contains("COMMENT='staff'"),
            "{sql}"
        );
    }

    #[test]
    fn postgres_writes_a_table_comment_as_its_own_statement() {
        let mut t = users();
        t.engine = None;
        t.collation = None;
        let mut draft = TableDraft::from_table(&t);
        draft.comment = Some("staff".into());
        let stmts = diff(&t, &draft, Postgres).emit();
        assert_eq!(stmts, vec!["COMMENT ON TABLE \"users\" IS 'staff';"]);
    }

    #[test]
    fn postgres_alters_type_nullability_and_default_in_one_statement() {
        let mut t = users();
        t.engine = None;
        t.collation = None;
        t.columns[2].collation = None;
        let mut draft = TableDraft::from_table(&t);
        draft.columns[2].info.type_name = "text".into();
        draft.columns[2].info.nullable = true;
        draft.columns[2].info.default = None;
        let stmts = diff(&t, &draft, Postgres).emit();
        assert_eq!(stmts.len(), 1, "{stmts:?}");
        let sql = &stmts[0];
        assert!(
            sql.contains("ALTER COLUMN \"status\" TYPE text USING \"status\"::text"),
            "{sql}"
        );
        assert!(
            sql.contains("ALTER COLUMN \"status\" DROP NOT NULL"),
            "{sql}"
        );
        assert!(
            sql.contains("ALTER COLUMN \"status\" DROP DEFAULT"),
            "{sql}"
        );
    }

    // ── CREATE TABLE ────────────────────────────────────────────────────────

    #[test]
    fn create_table_emits_columns_key_indexes_and_options() {
        let t = users();
        let mut draft = TableDraft::from_table(&t);
        draft.original = None;
        draft.name = "people".into();
        let sql = create(&draft, MySql).script();
        assert!(sql.starts_with("CREATE TABLE `people` ("), "{sql}");
        assert!(
            sql.contains("`id` int(11) NOT NULL AUTO_INCREMENT"),
            "{sql}"
        );
        assert!(sql.contains("PRIMARY KEY (`id`)"), "{sql}");
        assert!(sql.contains("UNIQUE KEY `email_uq` (`email`)"), "{sql}");
        assert!(sql.contains("CONSTRAINT `fk_status` FOREIGN KEY"), "{sql}");
        assert!(
            sql.contains(") ENGINE=InnoDB COLLATE=utf8mb4_general_ci"),
            "{sql}"
        );
    }

    #[test]
    fn create_table_on_postgres_splits_out_indexes_and_comments() {
        let mut t = users();
        t.engine = None;
        t.collation = None;
        let mut draft = TableDraft::from_table(&t);
        draft.original = None;
        let stmts = create(&draft, Postgres).emit();
        assert!(
            stmts[0].starts_with("CREATE TABLE \"users\" ("),
            "{stmts:?}"
        );
        assert!(
            stmts[0].contains("GENERATED BY DEFAULT AS IDENTITY"),
            "{stmts:?}"
        );
        assert!(
            stmts
                .iter()
                .any(|s| s.starts_with("CREATE UNIQUE INDEX \"email_uq\"")),
            "{stmts:?}"
        );
        assert!(
            stmts
                .iter()
                .any(|s| s == "COMMENT ON COLUMN \"users\".\"email\" IS 'login';"),
            "{stmts:?}"
        );
    }

    // ── shortcuts ───────────────────────────────────────────────────────────

    #[test]
    fn the_whole_table_shortcuts_stand_alone() {
        let drop = single("users", None, MySql, Change::DropTable);
        assert_eq!(drop.emit(), vec!["DROP TABLE `users`;"]);
        assert!(drop.destructive()[0].contains("every row"));

        let tr = single("users", Some("sales"), Postgres, Change::TruncateTable);
        assert_eq!(tr.emit(), vec!["TRUNCATE TABLE \"sales\".\"users\";"]);

        let rn = single(
            "users",
            None,
            MySql,
            Change::RenameTable {
                to: "people".into(),
            },
        );
        assert_eq!(rn.emit(), vec!["ALTER TABLE `users` RENAME TO `people`;"]);
        assert!(rn.destructive().is_empty());
    }

    #[test]
    fn a_dropped_index_shortcut_knows_which_verb_postgres_needs() {
        let plain = single(
            "users",
            None,
            Postgres,
            Change::DropIndex {
                name: "email_ix".into(),
                constraint: None,
            },
        );
        assert_eq!(plain.emit(), vec!["DROP INDEX \"email_ix\";"]);
        let backed = single(
            "users",
            None,
            Postgres,
            Change::DropIndex {
                name: "email_uq".into(),
                constraint: Some("users_email_key".into()),
            },
        );
        assert_eq!(
            backed.emit(),
            vec!["ALTER TABLE \"users\"\n  DROP CONSTRAINT \"users_email_key\";"]
        );
    }

    // ── type + default equivalence ──────────────────────────────────────────

    #[test]
    fn a_display_width_is_not_a_type_change() {
        assert!(types_equal("int(11)", "int", MySql));
        assert!(types_equal("BIGINT(20) UNSIGNED", "bigint unsigned", MySql));
        assert!(types_equal("INTEGER", "int", MySql));
        // tinyint(1) is how BOOLEAN is stored, so that width does mean something.
        assert!(types_equal("boolean", "tinyint(1)", MySql));
        assert!(!types_equal("tinyint(1)", "tinyint(4)", MySql));
        // Real differences survive.
        assert!(!types_equal("varchar(45)", "varchar(90)", MySql));
        assert!(!types_equal("int", "bigint", MySql));
        // A signed/unsigned change is a change.
        assert!(!types_equal("int unsigned", "int", MySql));
    }

    #[test]
    fn postgres_spellings_of_one_type_agree() {
        assert!(types_equal(
            "character varying(45)",
            "varchar(45)",
            Postgres
        ));
        assert!(types_equal("integer", "int4", Postgres));
        assert!(types_equal(
            "timestamp without time zone",
            "timestamp",
            Postgres
        ));
        assert!(types_equal("DOUBLE PRECISION", "float8", Postgres));
        assert!(types_equal("numeric(10,2)", "decimal(10,2)", Postgres));
        assert!(!types_equal("timestamp", "timestamptz", Postgres));
        // A PostgreSQL width is never a display width.
        assert!(!types_equal("int", "int(11)", Postgres));
    }

    #[test]
    fn normalize_type_survives_junk() {
        assert_eq!(normalize_type("", MySql), "");
        assert_eq!(normalize_type("   ", MySql), "");
        assert_eq!(normalize_type("weird_type", MySql), "weird_type");
        assert_eq!(
            normalize_type(" DECIMAL ( 10 , 2 ) ", MySql),
            "decimal(10,2)"
        );
    }

    // ── renaming a column carries its dependents by identity ────────────────

    /// Two columns, both in the primary key, with an index and a foreign key
    /// standing on the second one.
    fn ab_table() -> TableInfo {
        TableInfo {
            name: "t".into(),
            columns: vec![col("a", "int"), col("b", "int")],
            indexes: vec![
                IndexInfo::plain("PRIMARY", vec!["a", "b"], true),
                IndexInfo::plain("b_ix", vec!["b"], false),
            ],
            foreign_keys: vec![ForeignKeyInfo {
                name: "fk_b".into(),
                columns: vec!["b".into()],
                ref_table: "other".into(),
                ref_columns: vec!["id".into()],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn renaming_a_column_carries_its_key_index_and_foreign_key() {
        // The control: with no clash on the way, everything follows the name.
        let mut d = TableDraft::from_table(&ab_table());
        d.rename_column(1, "renamed");
        assert_eq!(d.primary_key, vec!["a", "renamed"]);
        assert_eq!(d.indexes[0].info.columns[0].name, "renamed");
        assert_eq!(d.foreign_keys[0].info.columns, vec!["renamed"]);
    }

    #[test]
    fn renaming_through_another_columns_name_leaves_that_column_alone() {
        // The designer writes back on every keystroke, so renaming `b` to `ab`
        // walks the draft through "", "a" and "ab" — and "a" is the *other*
        // column's name. Matching dependents by name rewrote `a`'s primary-key
        // membership to point at `ab`, and the draft then validated clean.
        let mut d = TableDraft::from_table(&ab_table());
        for keystroke in ["", "a", "ab"] {
            d.rename_column(1, keystroke);
        }
        assert_eq!(d.column_names(), vec!["a", "ab"]);
        assert_eq!(
            d.primary_key,
            vec!["a", "ab"],
            "column a's key membership must survive the transient clash"
        );
        // `from_table` lifts PRIMARY into `primary_key`, so `indexes` is b_ix alone.
        assert_eq!(d.indexes[0].info.columns[0].name, "ab");
        assert_eq!(d.foreign_keys[0].info.columns, vec!["ab"]);
        assert!(d.validate().is_empty(), "{:?}", d.validate());
    }

    #[test]
    fn two_columns_can_swap_names_a_keystroke_at_a_time() {
        // The harder shape: both halves pass through the other's name.
        let mut d = TableDraft::from_table(&ab_table());
        d.rename_column(1, "a"); // clash — b's dependents must not move
        d.rename_column(0, "b"); // clash the other way
        d.rename_column(1, "aa");
        d.rename_column(0, "bb");
        assert_eq!(d.column_names(), vec!["bb", "aa"]);
        assert_eq!(d.primary_key, vec!["bb", "aa"]);
        assert_eq!(d.foreign_keys[0].info.columns, vec!["aa"]);
    }

    #[test]
    fn a_column_catches_up_once_the_name_it_wanted_is_freed() {
        // b is renamed onto a's name (blocked), and then *a* moves away. b's
        // references must follow it to `a` rather than sitting on `b` forever —
        // a name no column answers to, which would block Preview with a message
        // naming a column the user can no longer see.
        let mut d = TableDraft::from_table(&ab_table());
        d.rename_column(1, "a");
        d.rename_column(0, "z");
        assert_eq!(d.column_names(), vec!["z", "a"]);
        assert_eq!(d.primary_key, vec!["z", "a"]);
        assert_eq!(d.foreign_keys[0].info.columns, vec!["a"]);
        assert!(d.validate().is_empty(), "{:?}", d.validate());
    }

    #[test]
    fn deleting_a_column_frees_its_name_for_one_mid_rename() {
        let mut d = TableDraft::from_table(&ab_table());
        d.rename_column(1, "a"); // blocked by column 0
        d.remove_column(0);
        assert_eq!(d.column_names(), vec!["a"]);
        assert_eq!(d.primary_key, vec!["a"]);
        assert_eq!(d.foreign_keys[0].info.columns, vec!["a"]);
        assert!(d.validate().is_empty(), "{:?}", d.validate());
    }

    #[test]
    fn a_rename_that_stops_on_a_duplicate_is_still_reported() {
        // The clash isn't silently swallowed — validate() is what blocks Preview.
        let mut d = TableDraft::from_table(&ab_table());
        d.rename_column(1, "a");
        assert!(
            d.validate().iter().any(|m| m.contains("both called a")),
            "{:?}",
            d.validate()
        );
    }

    #[test]
    fn the_primary_key_toggle_answers_for_the_column_it_was_asked_about() {
        // The designer's "Primary key" tick is the second door onto the same
        // defect: mid-rename, a by-name `retain` took the *other* column out.
        let mut d = TableDraft::from_table(&ab_table());
        d.rename_column(1, "a"); // column 1 now displays `a`, as does column 0
        assert!(d.is_in_primary_key(0) && d.is_in_primary_key(1));
        d.set_in_primary_key(1, false);
        assert!(d.is_in_primary_key(0), "column a keeps its membership");
        assert!(!d.is_in_primary_key(1));
        assert_eq!(d.primary_key, vec!["a"]);
        // Finishing the rename doesn't resurrect it.
        d.rename_column(1, "ab");
        assert_eq!(d.primary_key, vec!["a"]);
        d.set_in_primary_key(1, true);
        assert_eq!(d.primary_key, vec!["a", "ab"]);
    }

    #[test]
    fn removing_a_column_takes_the_dependents_it_still_owns() {
        let mut d = TableDraft::from_table(&ab_table());
        for keystroke in ["", "a", "ab"] {
            d.rename_column(1, keystroke);
        }
        d.remove_column(1);
        assert_eq!(d.column_names(), vec!["a"]);
        assert_eq!(d.primary_key, vec!["a"], "only ab's membership goes");
        assert!(d.foreign_keys.is_empty());
        assert!(
            d.indexes.is_empty(),
            "b_ix went with the column: {:?}",
            d.indexes
        );
    }

    // ── table options: what the differ reports is what the emitter does ─────

    /// `users()` and a draft of it, ready to have one option changed.
    fn users_and_draft() -> (TableInfo, TableDraft) {
        let t = users();
        let d = TableDraft::from_table(&t);
        (t, d)
    }

    #[test]
    fn clearing_the_engine_or_collation_is_not_a_change() {
        // A MySQL table always has both, so an emptied field means "leave it" —
        // and it must, because the emitter skips an empty clause. Reporting a
        // change here claimed an edit the statement doesn't perform.
        for clear in [
            (|d: &mut TableDraft| d.engine = None) as fn(&mut TableDraft),
            |d: &mut TableDraft| d.engine = Some(String::new()),
            |d: &mut TableDraft| d.engine = Some("   ".into()),
            |d: &mut TableDraft| d.collation = None,
            |d: &mut TableDraft| d.collation = Some(String::new()),
        ] {
            let (t, mut d) = users_and_draft();
            clear(&mut d);
            let cs = diff(&t, &d, MySql);
            assert!(cs.is_empty(), "clearing an option is not a change: {cs:?}");
        }
    }

    #[test]
    fn changing_the_engine_emits_only_the_engine() {
        let (t, mut d) = users_and_draft();
        d.engine = Some("MyISAM".into());
        let cs = diff(&t, &d, MySql);
        assert_eq!(cs.len(), 1);
        let sql = cs.emit().join("\n");
        assert!(sql.contains("ENGINE=MyISAM"), "{sql}");
        // The two options that didn't change are not restated. `COMMENT=` in
        // particular used to be pushed unconditionally.
        assert!(!sql.contains("COLLATE="), "{sql}");
        assert!(!sql.contains("COMMENT="), "{sql}");
    }

    #[test]
    fn the_options_summary_names_what_actually_changed() {
        let (t, mut d) = users_and_draft();
        d.engine = Some("MyISAM".into());
        assert_eq!(
            diff(&t, &d, MySql).changes[0].summary(),
            "Set the table's engine to MyISAM"
        );

        let (t, mut d) = users_and_draft();
        d.comment = Some("staff".into());
        assert_eq!(
            diff(&t, &d, MySql).changes[0].summary(),
            "Set the table's comment"
        );
    }

    #[test]
    fn clearing_the_comment_is_a_change_and_clears_it() {
        // Unlike the engine, "no comment" is a real state a table can be in.
        let (t, mut d) = users_and_draft();
        d.comment = None;
        let sql = diff(&t, &d, MySql).emit().join("\n");
        assert!(sql.contains("COMMENT=''"), "{sql}");
        assert!(!sql.contains("ENGINE="), "{sql}");
    }

    #[test]
    fn postgres_comments_are_their_own_statement_and_distinguish_empty_from_none() {
        let mut t = users();
        t.schema = Some("public".into());
        t.engine = None;
        t.collation = None;
        let base = TableDraft::from_table(&t);

        let mut d = base.clone();
        d.comment = Some("staff".into());
        let sql = diff(&t, &d, Postgres).emit().join("\n");
        assert!(sql.contains("COMMENT ON TABLE"), "{sql}");
        assert!(sql.contains("'staff'"), "{sql}");

        let mut d = base.clone();
        d.comment = None;
        let sql = diff(&t, &d, Postgres).emit().join("\n");
        assert!(
            sql.contains("IS NULL"),
            "cleared → no comment, not an empty one: {sql}"
        );
    }

    #[test]
    fn an_enum_or_set_value_list_is_part_of_the_type() {
        // The whole list, not the bare keyword: dropping it made every ENUM equal
        // to every other ENUM, so the designer reported no change and applied
        // nothing when the user edited the values.
        assert_eq!(normalize_type("enum('a','b')", MySql), "enum('a','b')");
        assert!(!types_equal("enum('a','b')", "enum('a')", MySql));
        assert!(!types_equal("enum('a','b')", "enum('a','b','c')", MySql));
        assert!(!types_equal(
            "enum('G','PG','PG-13','R','NC-17')",
            "enum('G','PG')",
            MySql
        ));
        // Order is part of a SET's (and an ENUM's) identity — the values are
        // stored by index.
        assert!(!types_equal("set('a','b')", "set('b','a')", MySql));
        assert!(!types_equal("set('a','b')", "set('a')", MySql));
        // The keyword's case is noise; a value's case is not.
        assert!(types_equal("ENUM('a','b')", "enum('a','b')", MySql));
        assert!(!types_equal("enum('a')", "enum('A')", MySql));
        // Still a different type from a SET with the same members.
        assert!(!types_equal("enum('a','b')", "set('a','b')", MySql));
    }

    #[test]
    fn the_numeric_equivalences_survive_the_value_list_fix() {
        // The regression net the finding named: these eight must not move.
        assert!(types_equal("int(11)", "int", MySql));
        assert!(types_equal("varchar(45)", "varchar(45)", MySql));
        assert!(types_equal(
            "character varying(45)",
            "varchar(45)",
            Postgres
        ));
        assert!(types_equal("numeric(10,2)", "decimal(10,2)", Postgres));
        assert!(!types_equal("varchar(45)", "varchar(50)", MySql));
        assert!(!types_equal("decimal(10,2)", "decimal(10,0)", MySql));
        assert!(!types_equal("int(11)", "bigint", MySql));
        assert!(!types_equal("enum('a','b')", "set('a','b')", MySql));
    }

    #[test]
    fn an_empty_default_is_no_default() {
        assert!(defaults_equal(None, Some("")));
        assert!(defaults_equal(None, Some("  ")));
        assert!(defaults_equal(None, Some("NULL")));
        assert!(defaults_equal(
            Some("current_timestamp"),
            Some("CURRENT_TIMESTAMP")
        ));
        // A quoted literal's case *is* its value.
        assert!(!defaults_equal(Some("'Draft'"), Some("'draft'")));
        assert!(defaults_equal(Some("'draft'"), Some("'draft'")));
        assert!(!defaults_equal(None, Some("0")));
    }

    // ── the designer's text fields ──────────────────────────────────────────

    #[test]
    fn a_key_list_round_trips_through_its_editable_form() {
        let cols = vec![
            IndexColumn {
                name: "bio".into(),
                prefix: Some(20),
                ..Default::default()
            },
            IndexColumn {
                name: "age".into(),
                descending: true,
                ..Default::default()
            },
            IndexColumn::plain("id"),
        ];
        let text = key_list_text(&cols);
        assert_eq!(text, "bio(20), age DESC, id");
        assert_eq!(parse_key_list(&text), cols);
    }

    #[test]
    fn parse_key_list_is_forgiving_about_spacing_and_case() {
        assert_eq!(
            parse_key_list("  bio ( 20 ) ,  age  desc , , id "),
            vec![
                IndexColumn {
                    name: "bio".into(),
                    prefix: Some(20),
                    descending: false,
                    expression: false,
                },
                IndexColumn {
                    name: "age".into(),
                    prefix: None,
                    descending: true,
                    expression: false,
                },
                IndexColumn::plain("id"),
            ]
        );
        // An explicit ASC is the default and disappears.
        assert_eq!(parse_key_list("id ASC"), vec![IndexColumn::plain("id")]);
        assert!(parse_key_list("   ").is_empty());
        // Junk stays a name, so validation can say it isn't a column.
        assert_eq!(parse_key_list("bio(x)"), vec![IndexColumn::plain("bio(x)")]);
    }

    /// The designer's key-list field is where an index the user *isn't* editing
    /// has to survive being shown and read back. An expression key is written
    /// parenthesised, and the commas inside it are not separators.
    #[test]
    fn an_expression_key_round_trips_through_the_designer_field() {
        let cols = vec![
            IndexColumn::expr("lower(email)"),
            IndexColumn {
                descending: true,
                ..IndexColumn::expr("coalesce(nick, name)")
            },
            IndexColumn::plain("id"),
        ];
        let text = key_list_text(&cols);
        assert_eq!(text, "(lower(email)), (coalesce(nick, name)) DESC, id");
        assert_eq!(parse_key_list(&text), cols, "read back unchanged");
    }

    /// A column whose name merely contains brackets is not an expression: only a
    /// piece the parens *enclose* is, which is exactly what `key_list_text`
    /// writes.
    #[test]
    fn parse_key_list_only_treats_an_enclosed_piece_as_an_expression() {
        assert_eq!(parse_key_list("bio(x)"), vec![IndexColumn::plain("bio(x)")]);
        assert_eq!(
            parse_key_list("(a) + (b)"),
            vec![IndexColumn::plain("(a) + (b)")],
            "not enclosed by one pair, so it stays a (bad) name for validation"
        );
    }

    /// An expression names no column, so the designer must not demand one — the
    /// check that would otherwise refuse to save any table carrying such an index.
    #[test]
    fn validate_does_not_look_for_a_column_behind_an_expression_key() {
        let mut d = TableDraft::from_table(&TableInfo {
            name: "t".into(),
            columns: vec![col("email", "text")],
            ..Default::default()
        });
        d.indexes = vec![IndexDraft::new(IndexInfo {
            name: "ix_lower_email".into(),
            columns: vec![IndexColumn::expr("lower(email)")],
            ..Default::default()
        })];
        assert!(
            !d.validate().iter().any(|e| e.contains("isn't a column")),
            "{:?}",
            d.validate()
        );
    }

    #[test]
    fn parse_name_list_trims_and_drops_empties() {
        assert_eq!(
            parse_name_list(" a , b ,, c "),
            vec!["a".to_string(), "b".into(), "c".into()]
        );
        assert!(parse_name_list("").is_empty());
    }

    // ── validation ──────────────────────────────────────────────────────────

    #[test]
    fn validate_catches_what_would_emit_nonsense() {
        let mut d = TableDraft::blank("", None);
        let errs = d.validate();
        assert!(errs.iter().any(|e| e.contains("needs a name")), "{errs:?}");
        assert!(
            errs.iter().any(|e| e.contains("at least one column")),
            "{errs:?}"
        );

        d.name = "t".into();
        d.columns = vec![
            ColumnDraft::new(col("a", "int")),
            ColumnDraft::new(col("A", "int")),
            ColumnDraft::new(col("b", "")),
        ];
        d.primary_key = vec!["ghost".into()];
        d.indexes = vec![IndexDraft::new(IndexInfo {
            name: "ix".into(),
            columns: vec![IndexColumn::plain("nope")],
            ..Default::default()
        })];
        let errs = d.validate();
        assert!(errs.iter().any(|e| e.contains("both called A")), "{errs:?}");
        assert!(
            errs.iter().any(|e| e.contains("b needs a type")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("isn't a column")),
            "{errs:?}"
        );
    }

    #[test]
    fn a_valid_draft_has_nothing_to_say() {
        let draft = TableDraft::from_table(&users());
        assert!(draft.validate().is_empty(), "{:?}", draft.validate());
    }

    /// The round-trip gate.
    ///
    /// Every table below is a captured shape from the databases this is
    /// developed against — classicmodels / sakila / employees / world on
    /// MariaDB, world / chinook on PostgreSQL — written out as the model the
    /// introspection produces for it. Building a draft from each and diffing it
    /// against its own source **must** produce nothing.
    ///
    /// That's the whole point: a designer opens by loading a table into a draft,
    /// so any gap between what the model reads and what the emitter writes shows
    /// up to the user as a change they didn't make, on a table they only wanted
    /// to look at. Checking it here keeps the suite DB-free while still failing
    /// the moment either side drifts.
    mod roundtrip {
        use super::*;

        fn c(name: &str, ty: &str, nullable: bool) -> ColumnInfo {
            ColumnInfo {
                name: name.into(),
                type_name: ty.into(),
                nullable,
                ..Default::default()
            }
        }
        fn pk(mut c: ColumnInfo) -> ColumnInfo {
            c.primary_key = true;
            c.nullable = false;
            c
        }
        fn auto(mut c: ColumnInfo) -> ColumnInfo {
            c.auto_increment = true;
            c
        }
        fn def(mut c: ColumnInfo, d: &str) -> ColumnInfo {
            c.default = Some(d.into());
            c
        }
        fn ix(name: &str, cols: Vec<&str>, unique: bool) -> IndexInfo {
            IndexInfo::plain(name, cols, unique)
        }
        fn fk(name: &str, cols: &[&str], table: &str, refs: &[&str]) -> ForeignKeyInfo {
            ForeignKeyInfo {
                name: name.into(),
                columns: cols.iter().map(|s| s.to_string()).collect(),
                ref_table: table.into(),
                ref_columns: refs.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            }
        }
        /// MySQL tables carry engine + collation on every row of
        /// `information_schema.TABLES`, so a fixture without them isn't what the
        /// introspection actually produces.
        fn innodb(mut t: TableInfo) -> TableInfo {
            t.engine = Some("InnoDB".into());
            t.collation = Some("utf8mb4_general_ci".into());
            t
        }

        /// `classicmodels.orderdetails` — composite key, two foreign keys, and
        /// the MariaDB display widths (`int(11)`) that a naive text compare
        /// trips over.
        fn orderdetails() -> TableInfo {
            innodb(TableInfo {
                name: "orderdetails".into(),
                columns: vec![
                    pk(c("orderNumber", "int(11)", false)),
                    pk(c("productCode", "varchar(15)", false)),
                    c("quantityOrdered", "int(11)", false),
                    c("priceEach", "decimal(10,2)", false),
                    c("orderLineNumber", "smallint(6)", false),
                ],
                indexes: vec![
                    ix("PRIMARY", vec!["orderNumber", "productCode"], true),
                    ix("productCode", vec!["productCode"], false),
                ],
                foreign_keys: vec![
                    fk(
                        "orderdetails_ibfk_1",
                        &["orderNumber"],
                        "orders",
                        &["orderNumber"],
                    ),
                    fk(
                        "orderdetails_ibfk_2",
                        &["productCode"],
                        "products",
                        &["productCode"],
                    ),
                ],
                ..Default::default()
            })
        }

        /// `sakila.film` — the awkward one: an unsigned auto-increment key, an
        /// `enum`, a `set`, a `year`, a `text`, and a timestamp that is both
        /// defaulted and `ON UPDATE`.
        fn film() -> TableInfo {
            innodb(TableInfo {
                name: "film".into(),
                columns: vec![
                    auto(pk(c("film_id", "smallint(5) unsigned", false))),
                    c("title", "varchar(128)", false),
                    c("description", "text", true),
                    c("release_year", "year(4)", true),
                    def(c("language_id", "tinyint(3) unsigned", false), "NULL"),
                    def(c("rental_duration", "tinyint(3) unsigned", false), "3"),
                    def(c("rental_rate", "decimal(4,2)", false), "4.99"),
                    def(
                        c("rating", "enum('G','PG','PG-13','R','NC-17')", true),
                        "'G'",
                    ),
                    c(
                        "special_features",
                        "set('Trailers','Commentaries','Deleted Scenes','Behind the Scenes')",
                        true,
                    ),
                    ColumnInfo {
                        default: Some("CURRENT_TIMESTAMP".into()),
                        on_update: Some("CURRENT_TIMESTAMP".into()),
                        ..c("last_update", "timestamp", false)
                    },
                ],
                indexes: vec![
                    ix("PRIMARY", vec!["film_id"], true),
                    ix("idx_title", vec!["title"], false),
                    ix("idx_fk_language_id", vec!["language_id"], false),
                ],
                foreign_keys: vec![ForeignKeyInfo {
                    on_update: Some("CASCADE".into()),
                    ..fk(
                        "fk_film_language",
                        &["language_id"],
                        "language",
                        &["language_id"],
                    )
                }],
                ..Default::default()
            })
        }

        /// `employees.salaries` — a composite key of a foreign key and a date,
        /// with `ON DELETE CASCADE` that a recreate must not quietly lose.
        fn salaries() -> TableInfo {
            innodb(TableInfo {
                name: "salaries".into(),
                columns: vec![
                    pk(c("emp_no", "int(11)", false)),
                    c("salary", "int(11)", false),
                    pk(c("from_date", "date", false)),
                    c("to_date", "date", false),
                ],
                indexes: vec![ix("PRIMARY", vec!["emp_no", "from_date"], true)],
                foreign_keys: vec![ForeignKeyInfo {
                    on_delete: Some("CASCADE".into()),
                    ..fk("salaries_ibfk_1", &["emp_no"], "employees", &["emp_no"])
                }],
                ..Default::default()
            })
        }

        /// `world.CountryLanguage` — a `char(3)` key, an enum default, a float
        /// with a scale, and a table comment.
        fn country_language() -> TableInfo {
            innodb(TableInfo {
                name: "CountryLanguage".into(),
                columns: vec![
                    def(pk(c("CountryCode", "char(3)", false)), "''"),
                    def(pk(c("Language", "char(30)", false)), "''"),
                    def(c("IsOfficial", "enum('T','F')", false), "'F'"),
                    def(c("Percentage", "float(4,1)", false), "0.0"),
                ],
                indexes: vec![ix("PRIMARY", vec!["CountryCode", "Language"], true)],
                foreign_keys: vec![fk(
                    "countryLanguage_ibfk_1",
                    &["CountryCode"],
                    "country",
                    &["Code"],
                )],
                comment: Some("language spoken".into()),
                ..Default::default()
            })
        }

        /// A prefix index over a `TEXT` column plus a generated column — the two
        /// shapes that fail outright when they're recreated without what makes
        /// them what they are.
        fn articles() -> TableInfo {
            innodb(TableInfo {
                name: "articles".into(),
                columns: vec![
                    auto(pk(c("id", "bigint(20) unsigned", false))),
                    c("body", "longtext", false),
                    ColumnInfo {
                        collation: Some("utf8mb4_bin".into()),
                        comment: Some("slug, cached".into()),
                        ..c("slug", "varchar(190)", true)
                    },
                    ColumnInfo {
                        generated: Some("char_length(`body`)".into()),
                        ..c("body_len", "int(11)", true)
                    },
                ],
                indexes: vec![
                    ix("PRIMARY", vec!["id"], true),
                    IndexInfo {
                        name: "body_prefix".into(),
                        columns: vec![IndexColumn {
                            name: "body".into(),
                            prefix: Some(64),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    IndexInfo {
                        name: "slug_desc".into(),
                        columns: vec![IndexColumn {
                            name: "slug".into(),
                            descending: true,
                            ..Default::default()
                        }],
                        unique: true,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            })
        }

        fn a_view() -> TableInfo {
            innodb(TableInfo {
                name: "film_list".into(),
                columns: vec![
                    c("FID", "smallint(5) unsigned", true),
                    c("title", "varchar(128)", true),
                ],
                is_view: true,
                view_definition: Some("select 1".into()),
                ..Default::default()
            })
        }

        /// `world.city` on PostgreSQL — a `serial` (a `nextval` default that also
        /// reads as auto-increment), a `character varying`, a defaulted integer,
        /// and a primary key that can only be dropped by its constraint name.
        fn pg_city() -> TableInfo {
            TableInfo {
                name: "city".into(),
                schema: Some("public".into()),
                columns: vec![
                    ColumnInfo {
                        auto_increment: true,
                        ..pk(c("id", "integer", false))
                    },
                    def(
                        c("name", "character varying(35)", false),
                        "''::character varying",
                    ),
                    def(c("countrycode", "character(3)", false), "''::bpchar"),
                    def(
                        c("district", "character varying(20)", false),
                        "''::character varying",
                    ),
                    def(c("population", "integer", false), "0"),
                ],
                indexes: vec![IndexInfo {
                    name: "PRIMARY".into(),
                    columns: vec![IndexColumn::plain("id")],
                    unique: true,
                    constraint: Some("city_pkey".into()),
                    ..Default::default()
                }],
                foreign_keys: vec![ForeignKeyInfo {
                    ref_schema: Some("public".into()),
                    ..fk(
                        "city_countrycode_fkey",
                        &["countrycode"],
                        "country",
                        &["code"],
                    )
                }],
                ..Default::default()
            }
        }

        /// `chinook.invoice` in a non-default namespace — an identity key, a
        /// `numeric`, a `timestamp`, a non-btree index and a partial one.
        fn pg_invoice() -> TableInfo {
            TableInfo {
                name: "invoice".into(),
                schema: Some("sales".into()),
                columns: vec![
                    ColumnInfo {
                        auto_increment: true,
                        comment: Some("invoice number".into()),
                        ..pk(c("invoice_id", "integer", false))
                    },
                    c("customer_id", "integer", false),
                    c("invoice_date", "timestamp without time zone", false),
                    ColumnInfo {
                        collation: Some("C".into()),
                        ..c("billing_country", "character varying(40)", true)
                    },
                    def(c("total", "numeric(10,2)", false), "0.00"),
                ],
                indexes: vec![
                    IndexInfo {
                        name: "PRIMARY".into(),
                        columns: vec![IndexColumn::plain("invoice_id")],
                        unique: true,
                        constraint: Some("invoice_pkey".into()),
                        ..Default::default()
                    },
                    IndexInfo {
                        name: "invoice_country_hash".into(),
                        columns: vec![IndexColumn::plain("billing_country")],
                        method: Some("hash".into()),
                        ..Default::default()
                    },
                    IndexInfo {
                        name: "invoice_big".into(),
                        columns: vec![IndexColumn::plain("total")],
                        predicate: Some("(total > (100)::numeric)".into()),
                        ..Default::default()
                    },
                ],
                foreign_keys: vec![ForeignKeyInfo {
                    ref_schema: Some("sales".into()),
                    on_delete: Some("SET NULL".into()),
                    ..fk(
                        "invoice_customer_fkey",
                        &["customer_id"],
                        "customer",
                        &["customer_id"],
                    )
                }],
                comment: Some("one invoice per order".into()),
                ..Default::default()
            }
        }

        fn fixtures() -> Vec<(SqlDialect, TableInfo)> {
            vec![
                (MySql, orderdetails()),
                (MySql, film()),
                (MySql, salaries()),
                (MySql, country_language()),
                (MySql, articles()),
                (MySql, a_view()),
                (Postgres, pg_city()),
                (Postgres, pg_invoice()),
                // Values that need escaping, on both engines — the gap A5-L6-03
                // named: without these the round-trip gate never sees one.
                (MySql, backslashes(MySql)),
                (Postgres, backslashes(Postgres)),
                (Postgres, pg_expression_index()),
            ]
        }

        /// A PostgreSQL table with the two index shapes the model only learned to
        /// hold in [B5-L5-02]'s second half: an **expression** key and a
        /// **descending** one. Both used to come back wrong — the expression
        /// silently missing (no `pg_attribute` row to join to) and DESC read as
        /// ASC — which is why such an index had to be refused as lossy rather
        /// than recreated.
        fn pg_expression_index() -> TableInfo {
            TableInfo {
                name: "person".into(),
                schema: Some("public".into()),
                columns: vec![
                    pk(auto(c("id", "integer", false))),
                    c("email", "text", true),
                    c("last_name", "text", true),
                    c("created_at", "timestamp with time zone", true),
                ],
                indexes: vec![
                    ix("PRIMARY", vec!["id"], true),
                    IndexInfo {
                        name: "ix_person_lower_email".into(),
                        columns: vec![IndexColumn::expr("lower(email)")],
                        unique: true,
                        ..Default::default()
                    },
                    IndexInfo {
                        name: "ix_person_created_desc".into(),
                        columns: vec![IndexColumn {
                            name: "created_at".into(),
                            descending: true,
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    // Mixed: a column and an expression in one key.
                    IndexInfo {
                        name: "ix_person_name_email".into(),
                        columns: vec![
                            IndexColumn::plain("last_name"),
                            IndexColumn::expr("lower(email)"),
                        ],
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }
        }

        #[test]
        fn every_fixture_diffs_to_nothing_against_itself() {
            for (dialect, t) in fixtures() {
                let cs = diff(&t, &TableDraft::from_table(&t), dialect);
                assert!(
                    cs.is_empty(),
                    "{}.{} shows a phantom change on {dialect:?}: {:#?}",
                    t.schema.as_deref().unwrap_or("-"),
                    t.name,
                    cs.changes
                );
            }
        }

        /// A table whose comment and default both contain a backslash.
        ///
        /// The default is stored the way introspection stores it — as
        /// ready-to-emit SQL text, so already quoted and escaped for its engine.
        fn backslashes(dialect: SqlDialect) -> TableInfo {
            let mut col = c("path", "varchar(255)", true);
            col.default = Some(crate::schema::ddl_string(r"C:\temp", dialect));
            col.comment = Some(r"windows path, e.g. C:\temp".into());
            let mut t = TableInfo {
                name: "paths".into(),
                columns: vec![pk(auto(c("id", "int(11)", false))), col],
                comment: Some(r"paths like C:\temp".into()),
                ..Default::default()
            };
            if dialect == MySql {
                t = innodb(t);
            }
            t
        }

        /// **No DDL test fed a value that needed escaping**, which is why the
        /// missing backslash handling in `ddl_string` survived — and the
        /// round-trip gate structurally cannot catch it, since both sides of
        /// that comparison go through the same emitter.
        ///
        /// So assert on the emitted text: MySQL treats `\` as an escape inside a
        /// literal and must double it; PostgreSQL takes it literally and must
        /// not, or the value is corrupted the other way.
        /// Emit the full CREATE for a table, through the real emitter — which is
        /// where the table-level comment is written, unlike
        /// `TableInfo::create_ddl`.
        fn emit_create(t: &TableInfo, dialect: SqlDialect) -> String {
            create_table_sql(&TableDraft::from_table(t), dialect).join("\n")
        }

        #[test]
        fn emitted_ddl_escapes_a_backslash_per_dialect() {
            let my = emit_create(&backslashes(MySql), MySql);
            assert!(
                my.contains(r"COMMENT 'windows path, e.g. C:\\temp'"),
                "MySQL column comment must double the backslash:\n{my}"
            );
            // The default is ready-to-emit text by the time it reaches here, so
            // this pins the *introspection* quoting (`db::mysql_column` calls
            // `ddl_string`) surviving into the emitted statement — not the
            // emitter's own escaping, which the two comments above cover.
            assert!(
                my.contains(r"DEFAULT 'C:\\temp'"),
                "MySQL default must double the backslash:\n{my}"
            );
            assert!(
                my.contains(r"COMMENT='paths like C:\\temp'"),
                "MySQL table comment must double the backslash:\n{my}"
            );

            let pg = emit_create(&backslashes(Postgres), Postgres);
            assert!(
                pg.contains(r"IS 'paths like C:\temp'"),
                "PostgreSQL must leave the backslash alone:\n{pg}"
            );
            assert!(
                !pg.contains(r"C:\\temp"),
                "doubling on PostgreSQL corrupts the value:\n{pg}"
            );
        }

        /// A trailing backslash is the case that malforms the *statement* rather
        /// than merely the value: unescaped, it escapes the closing quote.
        #[test]
        fn a_trailing_backslash_does_not_escape_the_closing_quote() {
            let mut t = backslashes(MySql);
            t.comment = Some(r"ends with a slash\".into());
            let sql = emit_create(&t, MySql);
            assert!(sql.contains(r"COMMENT='ends with a slash\\'"), "{sql}");
        }

        #[test]
        fn every_fixture_is_a_valid_draft() {
            for (_, t) in fixtures() {
                let d = TableDraft::from_table(&t);
                assert!(d.validate().is_empty(), "{}: {:?}", t.name, d.validate());
            }
        }

        /// The converse: touch exactly one thing and exactly one change comes
        /// back, saying what it is. A differ that under-reports is as dangerous
        /// as one that over-reports — it applies half of what the user asked for.
        #[test]
        fn one_edit_is_one_change() {
            type Edit = (&'static str, fn(&mut TableDraft), &'static str);
            let edits: Vec<Edit> = vec![
                (
                    "retype",
                    |d| d.columns[1].info.type_name = "varchar(500)".into(),
                    "varchar(500)",
                ),
                // Dropping NOT NULL is visible by what's *no longer* restated —
                // MySQL replaces the column, so the absence is the change.
                (
                    "nullability",
                    |d| d.columns[1].info.nullable = true,
                    "MODIFY COLUMN `title` varchar(128);",
                ),
                (
                    "default",
                    |d| d.columns[1].info.default = Some("'x'".into()),
                    "DEFAULT 'x'",
                ),
                (
                    "comment",
                    |d| d.columns[1].info.comment = Some("noted".into()),
                    "noted",
                ),
                ("rename", |d| d.rename_column(1, "renamed"), "renamed"),
                (
                    "add column",
                    |d| d.columns.push(ColumnDraft::new(c("extra", "int", true))),
                    "extra",
                ),
                // `description` is the one column nothing else stands on —
                // dropping an indexed column is legitimately two changes.
                (
                    "drop column",
                    |d| d.remove_column(2),
                    "DROP COLUMN `description`",
                ),
                (
                    "rename table",
                    |d| d.name = "renamed_table".into(),
                    "RENAME TO",
                ),
                (
                    "add index",
                    |d| {
                        d.indexes
                            .push(IndexDraft::new(ix("brand_new", vec!["title"], false)))
                    },
                    "brand_new",
                ),
            ];
            // `film` has a nullable-friendly second column (`title`) and no
            // dependency on it, so each edit above is genuinely one change.
            for (label, edit, expect) in edits {
                let t = film();
                let mut draft = TableDraft::from_table(&t);
                edit(&mut draft);
                let cs = diff(&t, &draft, MySql);
                assert_eq!(cs.len(), 1, "{label}: {:#?}", cs.changes);
                let sql = cs.script();
                assert!(sql.contains(expect), "{label}: {sql}");
            }
        }

        /// …and dropping an *indexed* column takes the index with it, in the
        /// order that works: the index first, or the column drop is refused.
        #[test]
        fn dropping_an_indexed_column_takes_its_index_first() {
            let t = film();
            let mut draft = TableDraft::from_table(&t);
            draft.remove_column(1); // `title`, which `idx_title` stands on
            let cs = diff(&t, &draft, MySql);
            assert_eq!(cs.len(), 2, "{:#?}", cs.changes);
            let sql = cs.script();
            let ix_at = sql.find("DROP INDEX `idx_title`").expect("index dropped");
            let col_at = sql.find("DROP COLUMN `title`").expect("column dropped");
            assert!(ix_at < col_at, "{sql}");
        }

        /// A view drafts and diffs to nothing on both engines, exactly like a
        /// table — the same gate, for the model that carries a view's options.
        #[test]
        fn every_view_fixture_diffs_to_nothing_against_itself() {
            for (dialect, t) in super::views::view_fixtures() {
                let draft = ViewDraft::from_table(&t).expect("a view drafts");
                let cs = diff_view(&t, &draft, dialect);
                assert!(
                    cs.is_empty(),
                    "{} shows a phantom change on {dialect:?}: {:#?}",
                    t.name,
                    cs.changes
                );
                assert!(draft.validate().is_empty(), "{:?}", draft.validate());
            }
        }

        /// Every fixture can be re-created from its own draft, and what comes out
        /// carries the attributes that make the table what it is.
        #[test]
        fn every_fixture_recreates_itself() {
            for (dialect, t) in fixtures() {
                if t.is_view {
                    continue; // A view is `CREATE VIEW`, not this path.
                }
                let mut draft = TableDraft::from_table(&t);
                draft.original = None;
                let stmts = create(&draft, dialect).emit();
                let sql = stmts.join("\n");
                assert!(sql.starts_with("CREATE TABLE "), "{}: {sql}", t.name);
                for col in &t.columns {
                    assert!(
                        sql.contains(&col.name),
                        "{}: {} missing from\n{sql}",
                        t.name,
                        col.name
                    );
                }
                if !primary_key_of(&t).is_empty() {
                    assert!(sql.contains("PRIMARY KEY ("), "{}: {sql}", t.name);
                }
                for fk in &t.foreign_keys {
                    assert!(sql.contains(&fk.name), "{}: {sql}", t.name);
                    if let Some(a) = &fk.on_delete {
                        assert!(sql.contains(&format!("ON DELETE {a}")), "{}: {sql}", t.name);
                    }
                }
            }
        }
    }

    /// Views: a name and a `SELECT`, and everything that makes redefining one
    /// more dangerous than it looks — the options a replace resets, and the
    /// engine that can't replace at all.
    mod views {
        use super::*;

        fn col(name: &str) -> ColumnInfo {
            ColumnInfo {
                name: name.into(),
                type_name: "int".into(),
                nullable: true,
                ..Default::default()
            }
        }

        /// A MySQL view carrying every option `information_schema.VIEWS` reports
        /// — the ones a `CREATE OR REPLACE` that didn't restate them would reset.
        pub(super) fn my_view() -> TableInfo {
            TableInfo {
                name: "active_staff".into(),
                columns: vec![col("staff_id"), col("name")],
                is_view: true,
                view_definition: Some(
                    "select `s`.`staff_id` AS `staff_id`,`s`.`name` AS `name` \
                     from `staff` `s` where `s`.`active` = 1"
                        .into(),
                ),
                view_options: Some(ViewOptions {
                    check_option: Some("CASCADED".into()),
                    definer: Some("root@localhost".into()),
                    security: Some("DEFINER".into()),
                    algorithm: Some("MERGE".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }
        }

        /// A PostgreSQL view, whose body arrives pretty-printed *and terminated*
        /// from `pg_get_viewdef`.
        pub(super) fn pg_view() -> TableInfo {
            TableInfo {
                name: "big_city".into(),
                schema: Some("public".into()),
                columns: vec![col("id"), col("name")],
                is_view: true,
                view_definition: Some(
                    " SELECT city.id,\n    city.name\n   FROM city\n  WHERE (city.pop > 1000);"
                        .into(),
                ),
                view_options: Some(ViewOptions {
                    storage: vec!["security_barrier=true".into()],
                    ..Default::default()
                }),
                ..Default::default()
            }
        }

        pub(super) fn view_fixtures() -> Vec<(SqlDialect, TableInfo)> {
            vec![(MySql, my_view()), (Postgres, pg_view())]
        }

        #[test]
        fn a_body_ending_in_a_line_comment_still_terminates() {
            // A trailing `-- note` is ordinary hand-written SQL, and the `;` was
            // pushed straight onto it — landing *inside* the comment. Down the
            // "Open in editor" / Copy path that means the shared splitter finds
            // no terminator, and this statement runs joined to the next one.
            // `#` is a MySQL comment and a Postgres operator, so it is only the
            // same shape on MySQL — which is why the dialect is threaded through.
            for (dialect, comment) in [
                (MySql, "-- the active ones"),
                (Postgres, "-- the active ones"),
                (MySql, "# the active ones"),
            ] {
                let mut v = ViewDraft::blank("v", None);
                v.select = format!("select id from t {comment}");
                let script = create_view(&v, dialect).script();
                let joined = format!("{script}\n\nSELECT 1");
                let pos = joined.rfind("SELECT 1").unwrap();
                let (lo, hi) = crate::sql::statement_range(&joined, pos, dialect);
                assert_eq!(
                    &joined[lo..hi],
                    "SELECT 1",
                    "the view statement swallowed its terminator and ran on: {joined:?}"
                );
            }
        }

        #[test]
        fn an_ordinary_body_keeps_its_semicolon_where_it_was() {
            let mut v = ViewDraft::blank("v", None);
            v.select = "select id from t".into();
            assert!(create_view(&v, MySql).script().ends_with("from t;"));
        }

        /// The anti-drift test: the display/copy emitter and the apply emitter
        /// must agree, because there is only supposed to be one of them.
        #[test]
        fn copy_ddl_and_the_apply_path_emit_the_same_view() {
            for (dialect, t) in view_fixtures() {
                let draft = ViewDraft::from_table(&t).expect("a view drafts");
                let applied = create_view(&draft, dialect).script();
                let copied = t.create_ddl(dialect);
                assert_eq!(
                    copied.trim(),
                    applied.trim(),
                    "two view emitters disagreed on {} ({dialect:?})",
                    t.name
                );
            }
        }

        #[test]
        fn copy_ddl_restates_the_options_a_replace_would_reset() {
            // Omitting SQL SECURITY turns an INVOKER view into a DEFINER one —
            // a privilege change — and the copied statement says OR REPLACE, so
            // it lands silently on the existing view.
            let mut t = my_view();
            t.view_options = Some(ViewOptions {
                check_option: Some("CASCADED".into()),
                definer: Some("app@localhost".into()),
                security: Some("INVOKER".into()),
                algorithm: Some("MERGE".into()),
                ..Default::default()
            });
            let sql = t.create_ddl(MySql);
            assert!(sql.contains("SQL SECURITY INVOKER"), "{sql}");
            assert!(sql.contains("DEFINER = `app`@`localhost`"), "{sql}");
            assert!(sql.contains("WITH CASCADED CHECK OPTION"), "{sql}");
            assert!(sql.contains("ALGORITHM = MERGE"), "{sql}");
        }

        #[test]
        fn copy_ddl_of_a_materialized_view_says_so() {
            let mut t = pg_view();
            t.view_options = Some(ViewOptions {
                materialized: true,
                ..Default::default()
            });
            let sql = t.create_ddl(Postgres);
            assert!(sql.contains("MATERIALIZED VIEW"), "{sql}");
            // There is no `CREATE OR REPLACE MATERIALIZED VIEW`.
            assert!(!sql.contains("OR REPLACE"), "{sql}");
        }

        /// The draft is the view; a base table has no view to draft.
        #[test]
        fn only_a_view_drafts_as_one() {
            assert!(ViewDraft::from_table(&my_view()).is_some());
            let base = TableInfo {
                name: "staff".into(),
                ..Default::default()
            };
            assert!(ViewDraft::from_table(&base).is_none());
        }

        /// The bug this whole struct exists for: redefining a view without
        /// restating `DEFINER`/`SQL SECURITY` silently hands it the caller's
        /// privileges instead of its owner's.
        #[test]
        fn mysql_replace_restates_every_option() {
            let v = my_view();
            let mut draft = ViewDraft::from_table(&v).unwrap();
            draft.select = "select 1 as staff_id, 'x' as name".into();
            let cs = diff_view(&v, &draft, MySql);
            assert_eq!(cs.len(), 1, "{:#?}", cs.changes);
            let sql = cs.script();
            assert!(sql.starts_with("CREATE OR REPLACE "), "{sql}");
            assert!(sql.contains("ALGORITHM = MERGE"), "{sql}");
            assert!(sql.contains("DEFINER = `root`@`localhost`"), "{sql}");
            assert!(sql.contains("SQL SECURITY DEFINER"), "{sql}");
            assert!(sql.contains("VIEW `active_staff` AS"), "{sql}");
            assert!(
                sql.trim_end().ends_with("WITH CASCADED CHECK OPTION;"),
                "{sql}"
            );
            // Redefining in place takes nothing away.
            assert!(cs.destructive().is_empty(), "{:?}", cs.destructive());
        }

        /// `pg_get_viewdef` hands back a terminated statement; pasting it in
        /// front of `WITH … CHECK OPTION` would be a syntax error.
        #[test]
        fn a_body_loses_its_trailing_semicolon() {
            assert_eq!(view_body("  SELECT 1;  "), "SELECT 1");
            assert_eq!(view_body("SELECT 1"), "SELECT 1");
            assert_eq!(view_body("SELECT ';' ;"), "SELECT ';'");
            let mut v = pg_view();
            v.view_options = Some(ViewOptions {
                check_option: Some("LOCAL".into()),
                ..Default::default()
            });
            let mut draft = ViewDraft::from_table(&v).unwrap();
            draft.select = "SELECT city.id, city.name FROM city;".into();
            let sql = diff_view(&v, &draft, Postgres).script();
            assert!(!sql.contains(";\nWITH"), "{sql}");
            assert!(
                sql.trim_end().ends_with("WITH LOCAL CHECK OPTION;"),
                "{sql}"
            );
        }

        /// PostgreSQL's storage parameters are the same class of reset as
        /// MySQL's `DEFINER` — a replace that drops `security_barrier` widens
        /// what the view leaks.
        #[test]
        fn pg_replace_restates_storage_parameters() {
            let v = pg_view();
            let mut draft = ViewDraft::from_table(&v).unwrap();
            draft.select = "SELECT city.id, city.name, city.pop FROM city".into();
            let sql = diff_view(&v, &draft, Postgres).script();
            assert!(sql.contains("WITH (security_barrier=true)"), "{sql}");
        }

        /// Appending a column is the one redefinition PostgreSQL takes in place.
        #[test]
        fn pg_appending_a_column_replaces_in_place() {
            let v = pg_view();
            let mut draft = ViewDraft::from_table(&v).unwrap();
            draft.select = "SELECT city.id, city.name, city.pop FROM city".into();
            let cs = diff_view(&v, &draft, Postgres);
            assert_eq!(cs.len(), 1, "{:#?}", cs.changes);
            let sql = cs.script();
            assert!(sql.contains("CREATE OR REPLACE VIEW"), "{sql}");
            assert!(!sql.contains("DROP VIEW"), "{sql}");
            assert!(cs.destructive().is_empty(), "{:?}", cs.destructive());
        }

        /// Renaming, retyping or reordering a column is not something
        /// `CREATE OR REPLACE VIEW` can do there — and the drop it takes instead
        /// has to be *said*, not done quietly.
        #[test]
        fn pg_renaming_a_column_recreates_and_says_so() {
            let v = pg_view();
            let mut draft = ViewDraft::from_table(&v).unwrap();
            draft.select = "SELECT city.id, city.name AS city_name FROM city".into();
            let cs = diff_view(&v, &draft, Postgres);
            let sql = cs.script();
            let drop_at = sql.find("DROP VIEW").expect("dropped first");
            let create_at = sql.find("CREATE VIEW").expect("then created");
            assert!(drop_at < create_at, "{sql}");
            assert!(!sql.contains("OR REPLACE"), "{sql}");
            let risks = cs.destructive().join(" ");
            assert!(risks.contains("Dependent views"), "{risks}");
            assert!(risks.contains("grants"), "{risks}");
        }

        /// Dropping a column is the same story — and the *count* has to shrink
        /// for it to be one, so a shorter list can't read as a prefix match.
        #[test]
        fn pg_dropping_a_column_recreates() {
            let v = pg_view();
            let mut draft = ViewDraft::from_table(&v).unwrap();
            draft.select = "SELECT city.id FROM city".into();
            assert!(
                diff_view(&v, &draft, Postgres)
                    .script()
                    .contains("DROP VIEW")
            );
        }

        /// Uncertainty means "let the server judge", never "drop it and find
        /// out": a `SELECT *` body can't be resolved without the catalogue, so
        /// it replaces and fails loudly if PostgreSQL disagrees.
        #[test]
        fn pg_unreadable_columns_replace_rather_than_drop() {
            let v = pg_view();
            let mut draft = ViewDraft::from_table(&v).unwrap();
            draft.select = "SELECT * FROM city".into();
            let cs = diff_view(&v, &draft, Postgres);
            assert!(!cs.script().contains("DROP VIEW"), "{}", cs.script());
            assert!(cs.destructive().is_empty());
        }

        /// …which is why the user gets the override, and taking it says what it
        /// costs.
        #[test]
        fn pg_forced_recreate_is_honoured() {
            let v = pg_view();
            let mut draft = ViewDraft::from_table(&v).unwrap();
            draft.select = "SELECT city.id, city.name, city.pop FROM city".into();
            draft.force_recreate = true;
            let cs = diff_view(&v, &draft, Postgres);
            assert!(cs.script().contains("DROP VIEW"), "{}", cs.script());
            assert!(!cs.destructive().is_empty());
        }

        /// MySQL replaces whatever the edit is, so the override is a PostgreSQL
        /// rule and doesn't leak into the other engine.
        #[test]
        fn mysql_never_recreates() {
            let v = my_view();
            let mut draft = ViewDraft::from_table(&v).unwrap();
            draft.select = "select 1 as x".into();
            draft.force_recreate = true;
            let sql = diff_view(&v, &draft, MySql).script();
            assert!(!sql.contains("DROP VIEW"), "{sql}");
        }

        #[test]
        fn renaming_a_view_uses_each_engine_s_verb() {
            let v = my_view();
            let mut draft = ViewDraft::from_table(&v).unwrap();
            draft.name = "staff_on_duty".into();
            let cs = diff_view(&v, &draft, MySql);
            assert_eq!(cs.len(), 1, "{:#?}", cs.changes);
            assert!(
                cs.script()
                    .contains("RENAME TABLE `active_staff` TO `staff_on_duty`;"),
                "{}",
                cs.script()
            );

            let v = pg_view();
            let mut draft = ViewDraft::from_table(&v).unwrap();
            draft.name = "large_city".into();
            let sql = diff_view(&v, &draft, Postgres).script();
            assert!(
                sql.contains(r#"ALTER VIEW "big_city" RENAME TO "large_city";"#),
                "{sql}"
            );
        }

        /// A recreate already creates the view under its new name — emitting a
        /// rename after it would fail on a name nothing answers to.
        #[test]
        fn a_recreate_carries_the_rename_instead_of_repeating_it() {
            let v = pg_view();
            let mut draft = ViewDraft::from_table(&v).unwrap();
            draft.name = "large_city".into();
            draft.select = "SELECT city.id AS city_id FROM city".into();
            let sql = diff_view(&v, &draft, Postgres).script();
            assert!(sql.contains(r#"DROP VIEW "big_city";"#), "{sql}");
            assert!(sql.contains(r#"CREATE VIEW "large_city""#), "{sql}");
            assert!(!sql.contains("RENAME"), "{sql}");
        }

        /// A brand-new view is `CREATE VIEW`, not `CREATE OR REPLACE`: if the
        /// name is taken, that has to fail rather than silently replace someone
        /// else's view.
        #[test]
        fn a_new_view_never_replaces() {
            let draft = ViewDraft {
                select: "SELECT 1 AS one".into(),
                ..ViewDraft::blank("one_row", None)
            };
            let cs = create_view(&draft, MySql);
            let sql = cs.script();
            assert_eq!(sql, "CREATE VIEW `one_row` AS\nSELECT 1 AS one;");
            assert!(cs.destructive().is_empty());
        }

        #[test]
        fn dropping_a_view_names_what_goes_with_it() {
            let cs = single(
                "active_staff",
                None,
                MySql,
                Change::DropView {
                    materialized: false,
                },
            );
            assert_eq!(cs.script(), "DROP VIEW `active_staff`;");
            assert!(
                cs.destructive()[0].contains("Dependent"),
                "{:?}",
                cs.destructive()
            );

            let cs = single(
                "city_stats",
                Some("public"),
                Postgres,
                Change::DropView { materialized: true },
            );
            assert_eq!(cs.script(), r#"DROP MATERIALIZED VIEW "city_stats";"#);
        }

        /// What the designer refuses to hand to the preview.
        #[test]
        fn validate_catches_what_cannot_be_emitted() {
            let msgs = |d: &ViewDraft| d.validate().join(" | ");

            let mut d = ViewDraft::blank("", None);
            d.select = "SELECT 1".into();
            assert!(msgs(&d).contains("name"), "{}", msgs(&d));

            let mut d = ViewDraft::blank("v", None);
            d.select = "   ".into();
            assert!(msgs(&d).contains("SELECT"), "{}", msgs(&d));

            let mut d = ViewDraft::blank("v", None);
            d.select = "DELETE FROM staff".into();
            assert!(msgs(&d).contains("SELECT"), "{}", msgs(&d));

            // The forms a body may legitimately take.
            for body in [
                "SELECT 1",
                "with x as (select 1) select * from x",
                "VALUES (1)",
            ] {
                let mut d = ViewDraft::blank("v", None);
                d.select = body.into();
                assert!(d.validate().is_empty(), "{body}: {:?}", d.validate());
            }
        }

        /// The same predicate answers the editor menu's "is there a query here
        /// to make a view out of?", so it has to hold on a half-typed statement
        /// too — head keyword only, never a parse.
        #[test]
        fn can_be_view_body_reads_the_head_keyword() {
            for yes in [
                "SELECT 1",
                "  select a from t where b = 1",
                "WITH x AS (SELECT 1) SELECT * FROM x",
                "(SELECT 1)",
                "VALUES (1)",
                "TABLE city",
                "SELECT a FROM ", // mid-edit, still a query
            ] {
                assert!(can_be_view_body(yes), "{yes}");
            }
            for no in [
                "",
                "   ",
                "DELETE FROM t",
                "INSERT INTO t VALUES (1)",
                "CREATE VIEW v AS SELECT 1",
                "SELECTED",
            ] {
                assert!(!can_be_view_body(no), "{no}");
            }
        }

        /// A materialized view has no `CREATE OR REPLACE` and isn't editable
        /// here — better to say so than to open half a form over it.
        #[test]
        fn a_materialized_view_is_not_editable() {
            let mut v = pg_view();
            v.view_options = Some(ViewOptions {
                materialized: true,
                ..Default::default()
            });
            let draft = ViewDraft::from_table(&v).unwrap();
            assert!(draft.options.materialized);
            assert!(
                draft.validate().iter().any(|m| m.contains("materialized")),
                "{:?}",
                draft.validate()
            );
        }

        /// The PostgreSQL rule on its own: what a replace can and can't do.
        #[test]
        fn pg_replaceable_reads_the_new_column_list() {
            let cur = ["id".to_string(), "name".to_string()];
            let ok = |s: &str| pg_replaceable(&cur, s, Postgres);
            assert_eq!(ok("SELECT id, name FROM t"), Some(true));
            assert_eq!(ok("SELECT id, name, pop FROM t"), Some(true));
            assert_eq!(ok("SELECT id FROM t"), Some(false));
            assert_eq!(ok("SELECT name, id FROM t"), Some(false));
            assert_eq!(ok("SELECT id, name AS label FROM t"), Some(false));
            // Unknowable, either way.
            assert_eq!(ok("SELECT * FROM t"), None);
            assert_eq!(ok("SELECT id, count(*) FROM t GROUP BY id"), None);
            assert_eq!(ok("not sql at all"), None);
        }
    }

    // ── alter_risk: what an in-place type change costs ───────────────────────

    /// Every destructive sentence for `c from → to`, as the preview lists them.
    fn risks(from: &str, to: &str) -> Vec<String> {
        Change::AlterColumn {
            from: Box::new(col("c", from)),
            to: Box::new(col("c", to)),
            position: None,
        }
        .risks()
    }

    /// The same, flattened — for the cases that assert on wording rather than count.
    fn risk(from: &str, to: &str) -> Option<String> {
        let v = risks(from, to);
        (!v.is_empty()).then(|| v.join(" "))
    }

    #[test]
    fn a_narrowing_that_also_becomes_not_null_discloses_both() {
        // One edit, two attribute changes, both ordinary in a designer. Reporting
        // only the NOT NULL half is worse than saying nothing: that sentence says
        // the statement *fails*, which reads as a promise that nothing is lost.
        let from = col("c", "varchar(255)");
        let mut to = col("c", "varchar(10)");
        to.nullable = false;
        let got = Change::AlterColumn {
            from: Box::new(from),
            to: Box::new(to),
            position: None,
        }
        .risks();
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got.iter().any(|r| r.contains("NOT NULL")), "{got:?}");
        assert!(got.iter().any(|r| r.contains("truncates")), "{got:?}");
    }

    #[test]
    fn each_single_attribute_change_still_yields_exactly_one_risk() {
        assert_eq!(risks("varchar(255)", "varchar(10)").len(), 1);
        assert_eq!(risks("decimal(10,2)", "decimal(10,0)").len(), 1);
        assert_eq!(risks("varchar(50)", "int(11)").len(), 1);
        // Nullability alone.
        let mut to = col("c", "varchar(255)");
        to.nullable = false;
        let got = Change::AlterColumn {
            from: Box::new(col("c", "varchar(255)")),
            to: Box::new(to),
            position: None,
        }
        .risks();
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].contains("NOT NULL"));
    }

    #[test]
    fn a_harmless_alter_yields_no_risk_at_all() {
        assert!(risks("varchar(10)", "varchar(255)").is_empty());
    }

    #[test]
    fn shrinking_a_decimal_scale_warns_that_it_rounds() {
        // The parameters of DECIMAL are (precision, scale); only the scale
        // carries the fractional digits, and MySQL *rounds* rather than failing —
        // 1234.56 becomes 1235 with nothing but a warning on the wire.
        for (from, to) in [
            ("decimal(10,2)", "decimal(10,0)"),
            ("decimal(12,4)", "decimal(12,2)"),
            ("numeric(9,3)", "numeric(9,1)"),
        ] {
            let msg = risk(from, to)
                .unwrap_or_else(|| panic!("{from} → {to} must be reported as destructive"));
            assert!(
                msg.contains("rounds"),
                "{from} → {to}: a scale reduction rounds, it doesn't truncate — got {msg:?}"
            );
        }
    }

    #[test]
    fn shrinking_a_decimal_precision_warns_that_values_may_not_fit() {
        let msg = risk("decimal(12,2)", "decimal(6,2)").expect("precision ↓ is destructive");
        assert!(msg.contains("no longer fit"), "got {msg:?}");
    }

    #[test]
    fn shrinking_both_decimal_parameters_names_both_consequences() {
        let msg = risk("decimal(12,4)", "decimal(6,2)").expect("both ↓ is destructive");
        assert!(
            msg.contains("rounds") && msg.contains("no longer fit"),
            "got {msg:?}"
        );
    }

    #[test]
    fn widening_a_decimal_is_not_destructive() {
        assert_eq!(risk("decimal(10,2)", "decimal(12,2)"), None);
        assert_eq!(risk("decimal(10,2)", "decimal(12,4)"), None);
        assert_eq!(risk("decimal(10,2)", "decimal(10,2)"), None);
        // Unparameterised on one side: nothing to compare, so nothing claimed.
        assert_eq!(risk("decimal", "decimal(10,2)"), None);
    }

    #[test]
    fn narrowing_a_string_or_time_type_still_warns() {
        // The controls the finding used — these already worked and must keep working.
        assert!(risk("varchar(255)", "varchar(10)").is_some());
        assert!(risk("datetime(6)", "datetime(0)").is_some());
        assert_eq!(risk("varchar(10)", "varchar(255)"), None);
    }

    #[test]
    fn changing_type_family_still_warns_about_the_rewrite() {
        let msg = risk("varchar(50)", "int(11)").expect("a family change is destructive");
        assert!(msg.contains("rewrites every value"), "got {msg:?}");
    }
}

#[cfg(test)]
mod lossy_index_tests {
    use super::*;
    use crate::intel::SqlDialect::Postgres;
    use crate::schema::IndexColumn;

    /// A PostgreSQL table whose one secondary index carries something the
    /// introspected model can't hold — an expression key column, a non-default
    /// operator class, or a NULLS ordering. All three really occur and all three
    /// are invisible in `IndexInfo`.
    fn table_with_lossy_index() -> TableInfo {
        TableInfo {
            name: "person".into(),
            schema: Some("public".into()),
            columns: vec![
                ColumnInfo {
                    name: "id".into(),
                    type_name: "integer".into(),
                    nullable: false,
                    primary_key: true,
                    ..Default::default()
                },
                ColumnInfo {
                    name: "last_name".into(),
                    type_name: "text".into(),
                    nullable: true,
                    ..Default::default()
                },
            ],
            indexes: vec![
                IndexInfo::plain("PRIMARY", vec!["id"], true),
                IndexInfo {
                    name: "idx_person".into(),
                    // What survived introspection: `lower(email)` was silently
                    // dropped by the join, so the model holds only `last_name`.
                    columns: vec![IndexColumn::plain("last_name")],
                    lossy: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    /// The finding's own repro: edit *something else* about the index and the
    /// plan would drop it and recreate it from the half of it that survived
    /// introspection, silently destroying `lower(email)`.
    #[test]
    fn an_index_we_cannot_fully_read_is_never_dropped_and_recreated() {
        let cur = table_with_lossy_index();
        let mut draft = TableDraft::from_table(&cur);
        // Any edit at all: make it unique.
        let ix = draft
            .indexes
            .iter_mut()
            .find(|d| d.info.name == "idx_person")
            .unwrap();
        ix.info.unique = true;

        let cs = diff(&cur, &draft, Postgres);
        assert!(
            !cs.changes.iter().any(|c| matches!(
                c,
                Change::DropIndex { name, .. } if name == "idx_person"
            )),
            "a lossy index must not be dropped: {:?}",
            cs.changes
        );
        assert!(
            !cs.changes.iter().any(|c| matches!(
                c,
                Change::AddIndex(ix) if ix.name == "idx_person"
            )),
            "…nor recreated from the lossy model: {:?}",
            cs.changes
        );
    }

    /// Withholding the statement silently would be its own bug: the user asked
    /// for a change and has to be told why it isn't in the plan.
    #[test]
    fn the_refusal_is_stated_in_the_preview() {
        let cur = table_with_lossy_index();
        let mut draft = TableDraft::from_table(&cur);
        draft
            .indexes
            .iter_mut()
            .find(|d| d.info.name == "idx_person")
            .unwrap()
            .info
            .unique = true;

        let cs = diff(&cur, &draft, Postgres);
        let kept = cs
            .changes
            .iter()
            .find(|c| matches!(c, Change::KeepLossyIndex { .. }))
            .expect("the refusal is a change the preview can render");
        assert!(kept.summary().contains("idx_person"));
        assert!(
            !kept.risks().is_empty(),
            "it belongs in the destructive block — the user's edit is not being applied"
        );
        // And it emits no SQL.
        assert!(
            ChangeSet {
                changes: vec![kept.clone()],
                ..cs.clone()
            }
            .emit()
            .is_empty()
        );
    }

    /// An ordinary index is unaffected — the refusal must not become a general
    /// freeze on index editing.
    #[test]
    fn an_ordinary_index_still_drops_and_recreates() {
        let mut cur = table_with_lossy_index();
        cur.indexes[1].lossy = false;
        let mut draft = TableDraft::from_table(&cur);
        draft
            .indexes
            .iter_mut()
            .find(|d| d.info.name == "idx_person")
            .unwrap()
            .info
            .unique = true;

        let cs = diff(&cur, &draft, Postgres);
        assert!(cs.changes.iter().any(|c| matches!(
            c,
            Change::DropIndex { name, .. } if name == "idx_person"
        )));
    }

    /// Deleting a lossy index outright is the user saying so explicitly, and is
    /// allowed — the rule is "don't destroy it as a side effect of an edit".
    #[test]
    fn deliberately_removing_a_lossy_index_is_still_allowed() {
        let cur = table_with_lossy_index();
        let mut draft = TableDraft::from_table(&cur);
        draft.indexes.retain(|d| d.info.name != "idx_person");

        let cs = diff(&cur, &draft, Postgres);
        assert!(cs.changes.iter().any(|c| matches!(
            c,
            Change::DropIndex { name, .. } if name == "idx_person"
        )));
    }
}
