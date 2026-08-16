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
    CheckInfo, ColumnInfo, DomainInfo, EnumInfo, ForeignKeyInfo, IndexInfo, RoutineInfo,
    SequenceInfo, ServerFlavour, TableInfo, TriggerAction, TriggerEvent, TriggerInfo, TriggerLevel,
    TriggerTiming, ViewOptions, ddl_ident_in, ddl_string, definer_sql, sql_qualifier,
};
use crate::sql;

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

/// A `CHECK` constraint as the designer holds it. Like a foreign key: named,
/// droppable, and with no in-place alter on either engine — an edited predicate
/// is a drop and an add.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckDraft {
    pub original: Option<String>,
    pub info: CheckInfo,
}

impl CheckDraft {
    pub fn existing(info: CheckInfo) -> Self {
        Self {
            original: Some(info.name.clone()),
            info,
        }
    }
    pub fn new(info: CheckInfo) -> Self {
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
    /// `CHECK` constraints, table-level and — on MariaDB — column-level alike:
    /// the designer lists both, and [`CheckInfo::column_level`] is what tells
    /// the emitter which statement can express a given one.
    pub check_constraints: Vec<CheckDraft>,
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
            check_constraints: t
                .check_constraints
                .iter()
                .cloned()
                .map(CheckDraft::existing)
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

    /// Add the column at `idx` to the primary key or take it out. The
    /// counterpart to [`TableDraft::is_in_primary_key`], and for the same reason:
    /// a by-name `retain` mid-rename removes the *other* column's membership.
    ///
    /// A member is inserted at **its own column ordinal**, not appended, so that
    /// taking a column out and putting it straight back is a no-op on a key that
    /// is in column order. Appending was not: any path that wrote the toggle
    /// twice — and the app had one, an Enter on the focused switch that flipped
    /// it off and on within a single keypress — moved a *composite* key's column
    /// to the end, and a key that looks unchanged on screen emits `DROP PRIMARY
    /// KEY` + `ADD PRIMARY KEY (…reordered…)`, i.e. a clustered-index rebuild.
    /// The residual is deliberate and visible: a key the server reports out of
    /// column order (`PRIMARY KEY (b, a)`) is *normalized* by an off-then-on
    /// pair rather than restored, which takes two deliberate clicks and shows in
    /// the change count.
    pub fn set_in_primary_key(&mut self, idx: usize, member: bool) {
        let Some(name) = self.columns.get(idx).map(|c| c.key_name.clone()) else {
            return;
        };
        match member {
            true if !self.primary_key.contains(&name) => {
                // The first member whose column sits after this one; a member
                // naming no column at all (mid-rename) counts as after, so a
                // half-typed name never pushes a real column out of order.
                let at = self
                    .primary_key
                    .iter()
                    .position(|p| {
                        !self
                            .columns
                            .iter()
                            .position(|c| c.key_name == *p)
                            .is_some_and(|o| o < idx)
                    })
                    .unwrap_or(self.primary_key.len());
                self.primary_key.insert(at, name);
            }
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
        // Checks were the one section with no arm here, though every sibling
        // guards its equivalent — so a blank one reached the server as
        // `ADD CONSTRAINT `t_chk` CHECK ()`, two clicks from the designer.
        let mut check_names: HashSet<String> = HashSet::new();
        for ck in &self.check_constraints {
            let name = ck.info.name.trim();
            if name.is_empty() {
                out.push("Every check constraint needs a name.".to_string());
            } else if !check_names.insert(name.to_ascii_lowercase()) {
                out.push(format!("Two check constraints are both called {name}."));
            }
            if ck.info.expression.trim().is_empty() {
                out.push(format!("Check {name} has no predicate."));
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

// ── The desired state of a trigger ───────────────────────────────────────────

/// The whole desired shape of one trigger.
///
/// Carries a whole [`TriggerInfo`] rather than the fields someone might edit,
/// for the reason [`ViewDraft`] carries [`ViewOptions`]: **a trigger is
/// replaced, never altered.** Neither engine has an `ALTER` that can change a
/// trigger's timing, events or action, so every edit is a drop-and-create and
/// anything the draft doesn't hold is gone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TriggerDraft {
    /// The trigger's name on the server, or `None` when this draft is a new
    /// trigger. Identity, not a name: editing `info.name` is a rename — which,
    /// on both engines, is still a drop and a create.
    pub original: Option<String>,
    pub info: TriggerInfo,
}

impl TriggerDraft {
    /// The draft that describes an introspected trigger exactly — the editor's
    /// starting point, and the input to the round-trip gate.
    pub fn from_info(t: &TriggerInfo) -> TriggerDraft {
        TriggerDraft {
            original: Some(t.name.clone()),
            info: t.clone(),
        }
    }

    /// An empty draft for a brand-new trigger on `table`.
    pub fn blank(
        name: impl Into<String>,
        table: impl Into<String>,
        schema: Option<String>,
    ) -> Self {
        TriggerDraft {
            original: None,
            info: TriggerInfo {
                name: name.into(),
                table: table.into(),
                schema,
                ..Default::default()
            },
        }
    }

    /// Problems that would make the generated SQL nonsense, in plain language.
    ///
    /// Empty means "emittable", not "the server will accept it" — whether the
    /// body compiles and the columns exist stays the server's judgement, as
    /// everywhere else here. What this *does* own is the divergence between the
    /// engines, because the model deliberately holds both shapes: introspection
    /// must never have to lie about what a server reported, so the refusal lives
    /// here instead.
    pub fn validate(&self, dialect: SqlDialect, host: TriggerHost) -> Vec<String> {
        let t = &self.info;
        let pg = dialect == SqlDialect::Postgres;
        let sqlite = dialect == SqlDialect::Sqlite;
        let view = host == TriggerHost::View;
        let mut out = Vec::new();
        if t.name.trim().is_empty() {
            out.push("The trigger needs a name.".to_string());
        }
        if t.table.trim().is_empty() {
            out.push("A trigger needs a table to fire on.".to_string());
        }
        if t.events.is_empty() {
            out.push("A trigger needs at least one event to fire on.".to_string());
        }
        // A constraint trigger's *existence* is fine — see
        // `TriggerSetDraft::validate`, which is where the refusal lives now,
        // because it needs the server's copy to tell "this one is being
        // changed" from "this one is merely present".
        if pg {
            match &t.action {
                TriggerAction::Function { name, .. } if name.trim().is_empty() => {
                    out.push("A PostgreSQL trigger needs a function to execute.".to_string())
                }
                TriggerAction::Body(_) => out.push(
                    "A PostgreSQL trigger runs a function, not a body — pick or write \
                     one to execute."
                        .to_string(),
                ),
                _ => {}
            }
            // TRUNCATE fires once per statement; there are no rows to hand it.
            if t.events.contains(&TriggerEvent::Truncate) && t.level == TriggerLevel::Row {
                out.push("A TRUNCATE trigger has to be FOR EACH STATEMENT.".to_string());
            }
            if t.timing == TriggerTiming::InsteadOf && t.level != TriggerLevel::Row {
                out.push("An INSTEAD OF trigger has to be FOR EACH ROW.".to_string());
            }
            // What the timing may be depends on what it fires on, and the two
            // rules are exact opposites — measured on 16.14, verbatim:
            // `"t" is a table … Tables cannot have INSTEAD OF triggers` and
            // `"v" is a view … Views cannot have row-level BEFORE or AFTER
            // triggers`. A statement-level `BEFORE`/`AFTER` on a view is fine,
            // so this is narrower than "a view only takes INSTEAD OF".
            if t.timing == TriggerTiming::InsteadOf && !view {
                out.push(
                    "Only a view can have an INSTEAD OF trigger — a table takes \
                     BEFORE or AFTER."
                        .to_string(),
                );
            }
            if view && t.timing != TriggerTiming::InsteadOf && t.level == TriggerLevel::Row {
                out.push(
                    "A view's BEFORE or AFTER trigger has to be FOR EACH STATEMENT \
                     — use INSTEAD OF to act on rows."
                        .to_string(),
                );
            }
        } else if sqlite {
            // Every rule below is one SQLite states itself, quoted from 3.45:
            // `cannot create INSTEAD OF trigger on table: emp`, `cannot create
            // BEFORE trigger on view: v`, and a plain `syntax error` for the
            // rest. They are refused here rather than at Apply because the
            // modal can say which control is wrong, and because a `DROP` has
            // already run by the time a `CREATE` fails.
            if t.events.len() > 1 {
                out.push(
                    "SQLite fires a trigger on one event — make a separate trigger per event."
                        .to_string(),
                );
            }
            if t.events.contains(&TriggerEvent::Truncate) {
                out.push("SQLite has no TRUNCATE trigger.".to_string());
            }
            // The two halves are exact opposites, as they are on PostgreSQL, but
            // stricter: a view takes *only* INSTEAD OF here, at either level.
            if t.timing == TriggerTiming::InsteadOf && !view {
                out.push(
                    "Only a view can have an INSTEAD OF trigger — a table takes \
                     BEFORE or AFTER."
                        .to_string(),
                );
            }
            if view && t.timing != TriggerTiming::InsteadOf {
                out.push(
                    "A view's trigger has to be INSTEAD OF — SQLite has no BEFORE or \
                     AFTER trigger on a view."
                        .to_string(),
                );
            }
            if t.level != TriggerLevel::Row {
                out.push(
                    "SQLite has only FOR EACH ROW triggers — there is no statement-level one."
                        .to_string(),
                );
            }
            if !t.update_columns.is_empty() && !t.events.contains(&TriggerEvent::Update) {
                out.push("UPDATE OF names columns on an UPDATE trigger.".to_string());
            }
            match &t.action {
                TriggerAction::Body(b) if b.trim().is_empty() => {
                    out.push("A SQLite trigger needs a body.".to_string())
                }
                // Not a style rule: SQLite's grammar has no bare-statement form,
                // and `BEGIN END` with nothing between is a syntax error too.
                TriggerAction::Body(b) if !is_begin_end_block(b) => out.push(
                    "A SQLite trigger's body has to be a BEGIN … END block holding at \
                     least one statement."
                        .to_string(),
                ),
                TriggerAction::Function { .. } => out.push(
                    "A SQLite trigger runs a body, not a function — write the statements \
                     to run."
                        .to_string(),
                ),
                _ => {}
            }
        } else {
            if t.events.len() > 1 {
                out.push(
                    "MySQL fires a trigger on one event — make a separate trigger per event."
                        .to_string(),
                );
            }
            if t.events.contains(&TriggerEvent::Truncate) {
                out.push("MySQL has no TRUNCATE trigger.".to_string());
            }
            if t.timing == TriggerTiming::InsteadOf {
                out.push("MySQL has no INSTEAD OF trigger — those are PostgreSQL's.".to_string());
            }
            if t.condition.as_deref().is_some_and(|c| !c.trim().is_empty()) {
                out.push(
                    "MySQL has no WHEN condition — put the test inside the body with IF."
                        .to_string(),
                );
            }
            if !t.update_columns.is_empty() {
                out.push("MySQL has no UPDATE OF — a trigger sees every column.".to_string());
            }
            match &t.action {
                TriggerAction::Body(b) if b.trim().is_empty() => {
                    out.push("A MySQL trigger needs a body.".to_string())
                }
                TriggerAction::Function { .. } => out.push(
                    "A MySQL trigger runs a body, not a function — write the statements \
                     to run."
                        .to_string(),
                ),
                _ => {}
            }
        }
        out
    }
}

/// Is `body` a `BEGIN … END` block with something inside it?
///
/// SQLite's trigger grammar has **no bare-statement form** — `CREATE TRIGGER …
/// SELECT 1;` is a syntax error, and so is `BEGIN END` with nothing between —
/// so this is a rule of the engine rather than a house style. Asked through the
/// shared lexer, not by `starts_with`: a body may open with a comment, and
/// `BEGIN`/`END` inside a string literal are not the block's own.
fn is_begin_end_block(body: &str) -> bool {
    let b = body.as_bytes();
    let mut words: Vec<&str> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(j) = sql::skip_noncode(b, i, SqlDialect::Sqlite) {
            // A string or a quoted identifier is content; a comment is not.
            if b[i] != b'-' && b[i] != b'/' && b[i] != b'#' {
                words.push("");
            }
            i = j.max(i + 1);
            continue;
        }
        if !sql::is_word_start(b[i]) {
            if !b[i].is_ascii_whitespace() {
                words.push("");
            }
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 1;
        while end < b.len() && sql::is_word_byte(b[end]) {
            end += 1;
        }
        words.push(&body[start..end]);
        i = end;
    }
    matches!(words.first(), Some(w) if w.eq_ignore_ascii_case("BEGIN"))
        && matches!(words.last(), Some(w) if w.eq_ignore_ascii_case("END"))
        && words.len() > 2
}

/// What a trigger fires on — the half of the rules that isn't the dialect.
///
/// PostgreSQL's timing rules are exact opposites on the two, so a validator that
/// only knows the dialect has to guess, and guessed wrong in both directions:
/// the modal offered `INSTEAD OF` on tables, where the server always refuses it,
/// while a view's triggers were unreachable. `Table` is the default because it
/// is what a trigger fires on unless something says otherwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TriggerHost {
    #[default]
    Table,
    View,
}

impl TriggerHost {
    /// The host a `TableInfo` is. `is_view` covers a materialized view too, but
    /// PostgreSQL takes no trigger on one at all, so the distinction never
    /// reaches here.
    pub fn of(is_view: bool) -> TriggerHost {
        if is_view {
            TriggerHost::View
        } else {
            TriggerHost::Table
        }
    }
}

/// The desired state of **all** of one table's triggers.
///
/// A set rather than one trigger because that is how they're edited: the modal
/// lists a table's triggers and you add, remove and change them together, so one
/// plan carries the lot. [`ChangeSet`] already holds a `Vec<Change>`, and every
/// trigger change is a whole statement, so nothing had to bend to allow it.
///
/// Deliberately **not** part of [`TableDraft`]. A trigger can't be a clause of
/// `ALTER TABLE` — it needs its own statement — so folding it in would turn the
/// designer's single coalesced `ALTER TABLE` into an `ALTER` plus N trigger
/// statements, which on MySQL commit one at a time. Keeping the two plans apart
/// is what lets `DdlError::applied` still mean something.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TriggerSetDraft {
    /// PostgreSQL namespace of the table; `None` on MySQL.
    pub schema: Option<String>,
    pub table: String,
    /// Every trigger the table should end up with. Each carries its own
    /// `original`, which is what tells an edit from an addition — and a trigger
    /// *missing* from here is a drop.
    pub triggers: Vec<TriggerDraft>,
}

impl TriggerSetDraft {
    /// The draft that describes a table's triggers exactly — the modal's
    /// starting point, and the input to the round-trip gate.
    pub fn from_table(t: &TableInfo) -> TriggerSetDraft {
        TriggerSetDraft {
            schema: t.schema.clone(),
            table: t.name.clone(),
            triggers: t.triggers.iter().map(TriggerDraft::from_info).collect(),
        }
    }

    /// Problems across the whole set, in plain language: each trigger's own,
    /// plus the one that only exists for a set — two triggers can't share a
    /// name, and a list-plus-form makes that easy to do by accident.
    /// `current` is the server's copy of the table's triggers — what
    /// [`diff_triggers`] compares against. It is needed because one rule here is
    /// about a *change*, not about a state: a constraint trigger may not be
    /// edited (Schemaic doesn't model the deferral settings one carries), but it
    /// may perfectly well sit on the table while its neighbours are.
    ///
    /// Folding every member's messages in unconditionally made **one** constraint
    /// trigger lock the whole modal: the form renders `errs.first()` and gates
    /// `ready` on the set being empty, so Preview SQL was permanently disabled
    /// and the only way out was to select that trigger and drop it.
    pub fn validate(
        &self,
        current: &[TriggerInfo],
        dialect: SqlDialect,
        host: TriggerHost,
    ) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in &self.triggers {
            // An untouched member says nothing: it is already on the server in
            // exactly this shape, so nothing this plan does can be wrong about
            // it. (`diff_triggers` uses the same comparison to decide it emits
            // no statement.)
            let unchanged = t
                .original
                .as_deref()
                .and_then(|n| current.iter().find(|c| c.name == n))
                .is_some_and(|cur| t.info == *cur);
            if unchanged {
                continue;
            }
            if t.info.constraint {
                out.push(format!(
                    "Schemaic can't edit the constraint trigger {} — it doesn't model \
                     the deferral settings one carries.",
                    t.info.name
                ));
            }
            out.extend(t.validate(dialect, host));
        }
        let mut seen: Vec<&str> = Vec::new();
        for t in &self.triggers {
            let name = t.info.name.trim();
            if name.is_empty() {
                continue;
            }
            // MySQL scopes a trigger name to the database, PostgreSQL to the
            // table — but within one table both refuse a duplicate, and that is
            // the only scope this set covers.
            if seen.iter().any(|s| s.eq_ignore_ascii_case(name)) {
                out.push(format!("Two triggers are both called {name}."));
            } else {
                seen.push(name);
            }
        }
        out
    }
}

// ── The desired state of a function ──────────────────────────────────────────

/// The whole desired shape of one PostgreSQL function.
///
/// Carries a whole [`RoutineInfo`] for the reason [`ViewDraft`] carries
/// [`ViewOptions`]: `CREATE OR REPLACE FUNCTION` replaces the entire routine, so
/// anything the statement doesn't restate reverts to the server's default —
/// including the `SET search_path` that keeps a `SECURITY DEFINER` function from
/// being a privilege-escalation hole.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FunctionDraft {
    /// The function's name on the server, or `None` for a new one. Identity, not
    /// a name: editing `info.name` is a rename.
    pub original: Option<String>,
    pub info: RoutineInfo,
}

impl FunctionDraft {
    pub fn from_info(f: &RoutineInfo) -> FunctionDraft {
        FunctionDraft {
            original: Some(f.name.clone()),
            info: f.clone(),
        }
    }

    /// A new trigger function, pre-shaped: `plpgsql`, `returns trigger`, and the
    /// skeleton every one of them has to end with. A body that doesn't `RETURN`
    /// is the single most common way a first trigger function fails at runtime
    /// rather than at creation, so the starting point supplies it.
    pub fn blank_trigger(name: impl Into<String>, schema: Option<String>) -> FunctionDraft {
        FunctionDraft {
            original: None,
            info: RoutineInfo {
                name: name.into(),
                schema,
                arguments: String::new(),
                returns: "trigger".to_string(),
                language: "plpgsql".to_string(),
                body: "BEGIN\n    RETURN NEW;\nEND;".to_string(),
                ..Default::default()
            },
        }
    }

    /// Problems that would make the generated SQL nonsense. Whether the body
    /// compiles is the server's judgement — PostgreSQL doesn't even check a
    /// `plpgsql` body beyond syntax until it runs.
    pub fn validate(&self) -> Vec<String> {
        let f = &self.info;
        let mut out = Vec::new();
        if f.name.trim().is_empty() {
            out.push("The function needs a name.".to_string());
        }
        if f.language.trim().is_empty() {
            out.push("The function needs a language (plpgsql, sql).".to_string());
        }
        if f.returns.trim().is_empty() {
            out.push("The function needs a return type.".to_string());
        }
        if f.body.trim().is_empty() {
            out.push("The function needs a body.".to_string());
        }
        // Not a syntax error — it creates fine and then fails on every write.
        if f.is_trigger_function() && !f.arguments.trim().is_empty() {
            out.push(
                "A trigger function takes no declared arguments — they arrive in \
                 TG_ARGV."
                    .to_string(),
            );
        }
        out
    }
}

// ── The desired state of a standalone object ─────────────────────────────────

/// Which of PostgreSQL's standalone objects a shared change is about.
///
/// The three of them spell rename, drop and comment identically apart from one
/// keyword, so those get one [`Change`] arm each with this to say which — rather
/// than nine arms differing by a string. Everything that genuinely diverges
/// (adding an enum value, altering a domain's constraints, restarting a
/// sequence) keeps its own arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Enum,
    Domain,
    Sequence,
}

impl ObjectKind {
    /// What the object is called in a sentence a person reads.
    pub fn label(self) -> &'static str {
        match self {
            ObjectKind::Enum => "type",
            ObjectKind::Domain => "domain",
            ObjectKind::Sequence => "sequence",
        }
    }

    /// The keyword that addresses it in `ALTER`/`DROP`/`COMMENT ON`.
    ///
    /// A domain **is** a type, and `ALTER TYPE` will rename one — but `ALTER
    /// DOMAIN` is the documented spelling, is what `COMMENT ON DOMAIN` requires
    /// (`COMMENT ON TYPE` on a domain is an error), and keeps the emitted script
    /// readable as the thing it edits. One keyword per kind, everywhere.
    pub fn sql_keyword(self) -> &'static str {
        match self {
            ObjectKind::Enum => "TYPE",
            ObjectKind::Domain => "DOMAIN",
            ObjectKind::Sequence => "SEQUENCE",
        }
    }
}

/// One column that would have to be re-cast if the type under it were rebuilt.
///
/// PostgreSQL can't remove or reorder an enum's values, and can't change a
/// domain's base type, so those edits are a rename-create-recast-drop dance
/// rather than an `ALTER`. This is what the dance has to touch — read off the
/// introspected schema by [`type_dependents`] so the preview can *name* every
/// affected column instead of describing the risk in the abstract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeDependent {
    pub schema: Option<String>,
    pub table: String,
    pub column: String,
    /// The declared type — the bare name, or its `mood[]` array form, which needs
    /// a different cast.
    pub type_name: String,
    /// Restated after the re-cast. A column default is stored against the *old*
    /// type and has to come off before the column can be retyped, so a dance that
    /// didn't put it back would silently drop it.
    pub default_value: Option<String>,
}

impl TypeDependent {
    /// Whether the column holds an array of the type rather than the type.
    pub fn is_array(&self) -> bool {
        self.type_name.trim_end().ends_with("[]")
    }
}

/// Every column **anywhere in the database** declared as the type
/// `schema.name` (or an array of it).
///
/// Matched on the *type's* identity, which is not the same question as the
/// table's namespace and used to be conflated with it. `format_type` writes
/// `sales.mood` for a type off the `search_path` and a bare `mood` for one on
/// it, so: a **qualified** name must match both halves, and an **unqualified**
/// one matches only the default namespace, because that is the only place an
/// unqualified `format_type` result can have come from.
///
/// Scanning tables in every namespace is deliberate — a column declared with
/// this type can live anywhere, and skipping the ones that didn't share its
/// namespace meant the rebuild's final `DROP TYPE` failed on a dependent the
/// list never mentioned.
///
/// It remains a *lower* bound: a view, function or composite built on the type
/// can't be enumerated from `DbSchema` at all, which is why the recreate's risk
/// sentence says the server may still refuse rather than promising the list is
/// complete.
pub fn type_dependents(
    db: &crate::schema::DbSchema,
    schema: Option<&str>,
    name: &str,
) -> Vec<TypeDependent> {
    let target_ns = schema.unwrap_or(crate::schema::PG_DEFAULT_SCHEMA);
    let mut out = Vec::new();
    for t in &db.tables {
        // A view has no storage, so nothing in it is re-cast — and its *body* is
        // the dependency that makes the drop fail, which this can't fix anyway.
        // (A materialized view has storage but no `ALTER COLUMN … TYPE`, so it
        // can't be re-cast either; PostgreSQL refusing the `DROP TYPE` is the
        // documented lower bound.)
        if t.is_view {
            continue;
        }
        for c in &t.columns {
            let declared = c.type_name.trim().trim_end_matches("[]").trim();
            let matches = match declared.rsplit_once('.') {
                // Qualified: both halves, or it is a different type that merely
                // shares a name.
                Some((ns, bare)) => {
                    ns.eq_ignore_ascii_case(target_ns) && bare.eq_ignore_ascii_case(name)
                }
                // Unqualified ⇒ resolved through the `search_path`, so it is the
                // default namespace's type and nothing else.
                None => {
                    declared.eq_ignore_ascii_case(name)
                        && target_ns.eq_ignore_ascii_case(crate::schema::PG_DEFAULT_SCHEMA)
                }
            };
            if matches {
                out.push(TypeDependent {
                    schema: t.schema.clone(),
                    table: t.name.clone(),
                    column: c.name.clone(),
                    type_name: c.type_name.clone(),
                    default_value: c.default.clone(),
                });
            }
        }
    }
    out
}

/// The whole desired shape of one enum type.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnumDraft {
    /// The type's name on the server, or `None` for a new one. Identity, not a
    /// name: editing `info.name` is a rename.
    pub original: Option<String>,
    pub info: EnumInfo,
}

impl EnumDraft {
    pub fn from_info(e: &EnumInfo) -> EnumDraft {
        EnumDraft {
            original: Some(e.name.clone()),
            info: e.clone(),
        }
    }

    pub fn blank(name: impl Into<String>, schema: Option<String>) -> EnumDraft {
        EnumDraft {
            original: None,
            info: EnumInfo {
                name: name.into(),
                schema,
                ..Default::default()
            },
        }
    }

    /// Problems that would make the generated SQL nonsense.
    pub fn validate(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.info.name.trim().is_empty() {
            out.push("The type needs a name.".to_string());
        }
        // An empty enum is legal (`CREATE TYPE t AS ENUM ()`) and useless, so it
        // is allowed rather than rejected — but a *duplicate* label is an error
        // the server raises, and catching it here beats a failed apply.
        let mut seen: Vec<&str> = Vec::new();
        for v in &self.info.values {
            if seen.contains(&v.as_str()) {
                out.push(format!("The value {v:?} is listed more than once."));
            }
            // An **empty** label is legal SQL and almost certainly a blank row
            // left behind. It matters more than the usual blank-field slip:
            // PostgreSQL has no `DROP VALUE`, so the only way to take it back is
            // the full park-create-recast-drop rebuild.
            if v.is_empty() {
                out.push("A value can't be empty — PostgreSQL can never remove it.".to_string());
            }
            seen.push(v);
        }
        out
    }
}

/// The whole desired shape of one domain.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DomainDraft {
    pub original: Option<String>,
    pub info: DomainInfo,
}

impl DomainDraft {
    pub fn from_info(d: &DomainInfo) -> DomainDraft {
        DomainDraft {
            original: Some(d.name.clone()),
            info: d.clone(),
        }
    }

    pub fn blank(name: impl Into<String>, schema: Option<String>) -> DomainDraft {
        DomainDraft {
            original: None,
            info: DomainInfo {
                name: name.into(),
                schema,
                base_type: "text".to_string(),
                ..Default::default()
            },
        }
    }

    pub fn validate(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.info.name.trim().is_empty() {
            out.push("The domain needs a name.".to_string());
        }
        if self.info.base_type.trim().is_empty() {
            out.push("A domain needs a type to be based on.".to_string());
        }
        let mut seen: Vec<&str> = Vec::new();
        for ck in &self.info.checks {
            if ck.name.trim().is_empty() {
                out.push("Every constraint needs a name.".to_string());
            }
            if ck.expression.trim().is_empty() {
                out.push(format!("Constraint {} has no predicate.", ck.name));
            }
            // A duplicate name is a plan PostgreSQL always rejects
            // (`constraint "email_check2" for domain "email" already exists`),
            // and it is reachable in two clicks: the editor proposes
            // `{domain}_check{len+1}`, so removing one and adding another
            // re-proposes a name still in the list.
            if seen.contains(&ck.name.as_str()) {
                out.push(format!("Constraint {} is named twice.", ck.name));
            }
            seen.push(&ck.name);
        }
        out
    }
}

/// The whole desired shape of one sequence.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SequenceDraft {
    pub original: Option<String>,
    pub info: SequenceInfo,
    /// `RESTART WITH n`, when the user asked for one.
    ///
    /// Not part of `info`: restarting is an **action**, not a state. It doesn't
    /// show up in a re-introspection, so folding it into the model would make
    /// every re-opened editor diff dirty against a sequence nothing had changed.
    pub restart: Option<i64>,
}

impl SequenceDraft {
    pub fn from_info(s: &SequenceInfo) -> SequenceDraft {
        SequenceDraft {
            original: Some(s.name.clone()),
            info: s.clone(),
            restart: None,
        }
    }

    pub fn blank(name: impl Into<String>, schema: Option<String>) -> SequenceDraft {
        SequenceDraft {
            original: None,
            info: SequenceInfo {
                name: name.into(),
                schema,
                ..Default::default()
            },
            restart: None,
        }
    }

    /// Problems the server would raise, caught before the apply.
    ///
    /// These are checked here rather than left to PostgreSQL because each has a
    /// clear plain-language answer, and because the alternative is a rejected
    /// statement in the middle of a plan.
    pub fn validate(&self) -> Vec<String> {
        let s = &self.info;
        let mut out = Vec::new();
        if s.name.trim().is_empty() {
            out.push("The sequence needs a name.".to_string());
        }
        if s.increment == 0 {
            out.push("A sequence can't increment by 0.".to_string());
        }
        if s.min_value > s.max_value {
            out.push(format!(
                "The minimum ({}) is above the maximum ({}).",
                s.min_value, s.max_value
            ));
        }
        if s.start < s.min_value || s.start > s.max_value {
            out.push(format!(
                "The start value ({}) is outside {}..{}.",
                s.start, s.min_value, s.max_value
            ));
        }
        if s.cache < 1 {
            out.push("The cache has to be at least 1.".to_string());
        }
        let (tmin, tmax) = SequenceInfo::type_bounds(&s.data_type);
        if s.min_value < tmin || s.max_value > tmax {
            out.push(format!(
                "{}..{} doesn't fit in {}.",
                s.min_value, s.max_value, s.data_type
            ));
        }
        if let Some(r) = self.restart
            && (r < s.min_value || r > s.max_value)
        {
            out.push(format!(
                "Restarting at {r} is outside {}..{}.",
                s.min_value, s.max_value
            ));
        }
        // Where the counter *is* now. PostgreSQL checks the new bounds against
        // it — `ERROR: RESTART value (500) cannot be greater than MAXVALUE
        // (100)`, and the symmetric MINVALUE message, both measured on 16.14 —
        // and skips the check only when the same statement restarts the
        // sequence, which is why supplying a restart clears this. Without it the
        // user reaches Apply and gets the server's wording for an edit the
        // editor had already called valid.
        if self.restart.is_none()
            && let Some(last) = s.last_value
            && (last < s.min_value || last > s.max_value)
        {
            out.push(format!(
                "The sequence is at {last}, outside the new {}..{}. Give it a \
                 restart value inside the range.",
                s.min_value, s.max_value
            ));
        }
        out
    }
}

/// Whichever standalone object the editor is editing.
///
/// The counterpart to [`crate::schema::ObjectItem`] on the draft side: one modal
/// holds one of these, so its chrome, footer and preview path don't have to
/// three-way match. The forms below it differ per kind, because the objects do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectDraft {
    Enum(EnumDraft),
    Domain(DomainDraft),
    Sequence(SequenceDraft),
}

impl Default for ObjectDraft {
    fn default() -> Self {
        ObjectDraft::Enum(EnumDraft::default())
    }
}

impl ObjectDraft {
    /// The draft that describes an introspected object exactly — the editor's
    /// starting point, and the input to the round-trip gate.
    pub fn from_item(o: &crate::schema::ObjectItem) -> ObjectDraft {
        match o {
            crate::schema::ObjectItem::Enum(e) => ObjectDraft::Enum(EnumDraft::from_info(e)),
            crate::schema::ObjectItem::Domain(d) => ObjectDraft::Domain(DomainDraft::from_info(d)),
            crate::schema::ObjectItem::Sequence(s) => {
                ObjectDraft::Sequence(SequenceDraft::from_info(s))
            }
        }
    }

    pub fn blank(kind: ObjectKind, name: impl Into<String>, schema: Option<String>) -> ObjectDraft {
        match kind {
            ObjectKind::Enum => ObjectDraft::Enum(EnumDraft::blank(name, schema)),
            ObjectKind::Domain => ObjectDraft::Domain(DomainDraft::blank(name, schema)),
            ObjectKind::Sequence => ObjectDraft::Sequence(SequenceDraft::blank(name, schema)),
        }
    }

    pub fn kind(&self) -> ObjectKind {
        match self {
            ObjectDraft::Enum(_) => ObjectKind::Enum,
            ObjectDraft::Domain(_) => ObjectKind::Domain,
            ObjectDraft::Sequence(_) => ObjectKind::Sequence,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            ObjectDraft::Enum(d) => &d.info.name,
            ObjectDraft::Domain(d) => &d.info.name,
            ObjectDraft::Sequence(d) => &d.info.name,
        }
    }

    pub fn validate(&self) -> Vec<String> {
        match self {
            ObjectDraft::Enum(d) => d.validate(),
            ObjectDraft::Domain(d) => d.validate(),
            ObjectDraft::Sequence(d) => d.validate(),
        }
    }

    /// Everything that has to happen to turn `current` into this draft, or the
    /// `CREATE` when there is no `current`.
    ///
    /// The **one** call the editor's change count and its preview both go
    /// through, so the number in the footer can't disagree with the SQL — the
    /// rule every other editor here follows.
    ///
    /// A kind mismatch between `current` and the draft can't happen (the editor
    /// builds the draft from the object it opened on) and resolves to the
    /// `CREATE`, which is the reading that never destroys anything.
    pub fn change_set(
        &self,
        current: Option<&crate::schema::ObjectItem>,
        dependents: &[TypeDependent],
        dialect: SqlDialect,
    ) -> ChangeSet {
        use crate::schema::ObjectItem;
        match (current, self) {
            (Some(ObjectItem::Enum(c)), ObjectDraft::Enum(d)) => {
                diff_enum(c, d, dependents, dialect)
            }
            (Some(ObjectItem::Domain(c)), ObjectDraft::Domain(d)) => {
                diff_domain(c, d, dependents, dialect)
            }
            (Some(ObjectItem::Sequence(c)), ObjectDraft::Sequence(d)) => {
                diff_sequence(c, d, dialect)
            }
            (_, ObjectDraft::Enum(d)) => create_enum(d, dialect),
            (_, ObjectDraft::Domain(d)) => create_domain(d, dialect),
            (_, ObjectDraft::Sequence(d)) => create_sequence(d, dialect),
        }
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
    /// **SQLite only**: perform the rest of this set by rebuilding the table —
    /// see [`sqlite_rebuild_sql`], which this carries the inputs for.
    ///
    /// It sits *alongside* the changes it performs rather than replacing them,
    /// and that is the point: the preview still lists "change `a` to TEXT" in
    /// the user's terms, and this line says how it is going to happen. Folding
    /// them into one would trade a plan someone can check against what they drew
    /// for a plan they can only take on trust — and this is a procedure that
    /// drops the table in the middle.
    RebuildTable(Box<Rebuild>),
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
        /// A **MariaDB column-level** CHECK that has to be restated inside this
        /// clause or the server deletes it — see [`CheckInfo::column_level`].
        ///
        /// It rides on the change rather than being a change of its own because
        /// nothing about the constraint is changing: it is part of the column
        /// definition `MODIFY`/`CHANGE` replaces, in the same way the column's
        /// default is, and the preview would otherwise list an edit the user
        /// never made. The predicate is already re-pointed for a rename.
        inline_check: Option<Box<CheckInfo>>,
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
    /// Add a `CHECK` constraint. Both engines validate it against the rows
    /// already there, so this fails rather than half-applies.
    AddCheck(Box<CheckInfo>),
    /// Drop a `CHECK` constraint. Destructive in the sense the preview means it:
    /// no data is lost, but an invariant the table guaranteed stops being one.
    DropCheck {
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
    /// Create a trigger that doesn't exist yet.
    CreateTrigger(Box<TriggerDraft>),
    /// Redefine an existing trigger — always a `DROP` followed by a `CREATE`,
    /// because neither engine can alter one in place. One change, because it's
    /// one edit, with the cost stated in [`Change::risks`].
    ReplaceTrigger {
        draft: Box<TriggerDraft>,
    },
    DropTrigger {
        name: String,
    },
    /// Create a function that doesn't exist yet — plain `CREATE FUNCTION`, so a
    /// signature already taken fails instead of silently replacing someone
    /// else's routine.
    CreateFunction(Box<FunctionDraft>),
    /// Redefine an existing function in place, restating every option.
    ReplaceFunction(Box<FunctionDraft>),
    /// PostgreSQL renames a function in place, so unlike a trigger this is a
    /// change of its own and the triggers bound to it keep working.
    RenameFunction {
        from: Box<RoutineInfo>,
        to: String,
    },
    DropFunction(Box<RoutineInfo>),
    /// Create an enum type that doesn't exist yet.
    CreateEnum(Box<EnumInfo>),
    /// `ALTER TYPE … ADD VALUE`. One change per value, so the preview lists each
    /// one and its position rather than a count.
    ///
    /// `after`/`before` is how a value lands anywhere but the end. Exactly one is
    /// set: the anchor is the value the new one goes next to, which for a run of
    /// insertions is the previous *new* value, so they arrive in draft order.
    AddEnumValue {
        value: String,
        after: Option<String>,
        before: Option<String>,
    },
    /// `ALTER TYPE … RENAME VALUE … TO …`. Rewrites the label everywhere at once;
    /// no row is touched, because rows store the value's identity, not its text.
    RenameEnumValue {
        from: String,
        to: String,
    },
    /// Rebuild an enum from scratch, re-casting every column that uses it.
    ///
    /// The escape hatch for the two edits PostgreSQL has no `ALTER` for —
    /// **removing a value and reordering them**. It is a rename-create-recast-drop
    /// dance, and it is offered rather than refused because the alternative is an
    /// editor that can only ever append. The whole plan runs in one transaction on
    /// PostgreSQL, so it either lands or leaves nothing behind.
    RecreateEnum {
        info: Box<EnumInfo>,
        dependents: Vec<TypeDependent>,
    },
    CreateDomain(Box<DomainInfo>),
    /// `ALTER DOMAIN … SET/DROP DEFAULT`.
    SetDomainDefault {
        to: Option<String>,
    },
    /// `ALTER DOMAIN … SET/DROP NOT NULL`. Setting it is checked against every
    /// column of the domain's type, so it fails rather than half-applies.
    SetDomainNotNull {
        to: bool,
    },
    AddDomainCheck(Box<CheckInfo>),
    DropDomainCheck {
        name: String,
    },
    /// Rebuild a domain from scratch — the only way to change its base type,
    /// which `ALTER DOMAIN` has no action for.
    RecreateDomain {
        info: Box<DomainInfo>,
        /// The base type it had **before** the edit. Carried so `risks` can run
        /// the same narrowing analysis a column's type change gets: this is a
        /// retype of every value of every column of the domain, and it was the
        /// one such path in the emitter that disclosed nothing.
        from_type: String,
        dependents: Vec<TypeDependent>,
    },
    CreateSequence(Box<SequenceInfo>),
    /// `ALTER SEQUENCE`, restating **only** the clauses that changed — the same
    /// rule `TableOptions` follows, and for the same reason: a restated clause the
    /// user didn't edit is a change nobody reviewed.
    AlterSequence {
        from: Box<SequenceInfo>,
        to: Box<SequenceInfo>,
    },
    /// `ALTER SEQUENCE … RESTART WITH`. Its own change because it is an action
    /// rather than a state — see [`SequenceDraft::restart`].
    RestartSequence {
        to: i64,
    },
    /// `ALTER … RENAME TO` for any of the three standalone objects.
    RenameObject {
        kind: ObjectKind,
        to: String,
    },
    /// `DROP TYPE`/`DOMAIN`/`SEQUENCE`. Never `CASCADE`: cascading here drops the
    /// *columns* built on the type, which is a far larger act than the one the
    /// user asked for. Let the server refuse and name what still depends on it.
    DropObject {
        kind: ObjectKind,
    },
    /// `COMMENT ON …`. `None` clears it.
    SetObjectComment {
        kind: ObjectKind,
        comment: Option<String>,
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
            Change::RebuildTable(r) => format!(
                "Rebuild {} — SQLite can't alter a table in place, so the rows are \
                 copied into a new one and it takes the old one's place",
                r.current.name
            ),
            Change::DropTable => "Drop the table".to_string(),
            Change::TruncateTable => "Delete every row".to_string(),
            Change::RenameTable { to } => format!("Rename the table to {to}"),
            Change::AddColumn { column, .. } => {
                format!("Add column {} {}", column.name, column.type_name)
            }
            Change::DropColumn { name, .. } => format!("Drop column {name}"),
            Change::AlterColumn {
                from, to, position, ..
            } => {
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
            Change::AddCheck(ck) => format!("Add check {} ({})", ck.name, ck.expression),
            Change::DropCheck { name } => format!("Drop check {name}"),
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
            Change::CreateTrigger(d) => format!("Create trigger {}", d.info.name),
            Change::ReplaceTrigger { draft } => {
                let server = draft.original.as_deref().unwrap_or(&draft.info.name);
                if server != draft.info.name {
                    format!("Re-create trigger {server} as {}", draft.info.name)
                } else {
                    format!("Re-create trigger {}", draft.info.name)
                }
            }
            Change::DropTrigger { name } => format!("Drop trigger {name}"),
            Change::CreateFunction(d) => format!("Create function {}", d.info.name),
            Change::ReplaceFunction(d) => format!("Redefine function {}", d.info.name),
            Change::RenameFunction { from, to } => {
                format!("Rename function {} to {to}", from.name)
            }
            Change::DropFunction(f) => format!("Drop function {}", f.name),
            Change::CreateEnum(e) => format!(
                "Create type {} with {} value{}",
                e.name,
                e.values.len(),
                plural(e.values.len())
            ),
            Change::AddEnumValue {
                value,
                after,
                before,
            } => match (after, before) {
                (Some(a), _) => format!("Add value {value} after {a}"),
                (_, Some(b)) => format!("Add value {value} before {b}"),
                _ => format!("Add value {value}"),
            },
            Change::RenameEnumValue { from, to } => format!("Rename value {from} to {to}"),
            Change::RecreateEnum { info, dependents } => format!(
                "Re-create type {} ({} value{}, re-casting {} column{})",
                info.name,
                info.values.len(),
                plural(info.values.len()),
                dependents.len(),
                plural(dependents.len())
            ),
            Change::CreateDomain(d) => format!("Create domain {} as {}", d.name, d.base_type),
            Change::SetDomainDefault { to } => match to {
                Some(d) => format!("Set the domain's default to {d}"),
                None => "Drop the domain's default".to_string(),
            },
            Change::SetDomainNotNull { to } => {
                format!(
                    "Make the domain {}",
                    if *to { "NOT NULL" } else { "nullable" }
                )
            }
            Change::AddDomainCheck(ck) => format!("Add check {} ({})", ck.name, ck.expression),
            Change::DropDomainCheck { name } => format!("Drop check {name}"),
            Change::RecreateDomain {
                info,
                from_type,
                dependents,
            } => format!(
                "Re-create domain {} as {} (was {from_type}; re-casting {} column{})",
                info.name,
                info.base_type,
                dependents.len(),
                plural(dependents.len())
            ),
            Change::CreateSequence(s) => format!("Create sequence {}", s.name),
            // Name what changed, as `TableOptions` does — "alter the sequence"
            // couldn't say which of six clauses the statement carries.
            Change::AlterSequence { from, to } => {
                format!("Set the sequence's {}", sequence_edits(from, to).join(", "))
            }
            Change::RestartSequence { to } => format!("Restart the sequence at {to}"),
            Change::RenameObject { kind, to } => format!("Rename the {} to {to}", kind.label()),
            Change::DropObject { kind } => format!("Drop the {}", kind.label()),
            Change::SetObjectComment { kind, comment } => match comment {
                Some(_) => format!("Set the {}'s comment", kind.label()),
                None => format!("Clear the {}'s comment", kind.label()),
            },
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
            // The table really is dropped in the middle of this, so it belongs
            // in the destructive block even though the plan puts it back. What
            // protects the rows is that the whole procedure is one transaction.
            Change::RebuildTable(r) => vec![format!(
                "Drops {} and recreates it, copying every row across. \
                 Anything about the table Schemaic didn't read is not carried over.",
                r.current.name
            )],
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
            // No data is lost, which is exactly why it's worth a sentence: the
            // table stops guaranteeing something it guaranteed a moment ago, and
            // nothing about the statement or the grid afterwards shows it.
            Change::DropCheck { name } => vec![format!(
                "Drops check {name}. Rows the constraint refused are accepted from \
                 now on, and existing data is not re-examined if it's added back."
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
            // Like `DropCheck`, this destroys no data and still has to be said:
            // whatever the trigger maintained — an audit row, a denormalized
            // total, a guard — silently stops happening on the next write.
            Change::DropTrigger { name } => vec![format!(
                "Drops trigger {name}. Whatever it did on each write stops \
                 happening, and rows written from now on won't have it applied."
            )],
            // Neither engine can alter a trigger, so a redefinition is a drop and
            // a create. That is safe where DDL is transactional and *isn't* on
            // MySQL, which commits each statement as it runs — so a rejected new
            // definition there leaves the table with no trigger at all.
            Change::ReplaceTrigger { draft } => vec![format!(
                "Re-creating trigger {} drops it first. Where DDL isn't \
                 transactional (MySQL), a new definition the server rejects \
                 leaves the table with no trigger.",
                draft.info.name
            )],
            // No data is lost and nothing is dropped, and it still has to be
            // said: a function is shared, so every trigger bound to it starts
            // doing something else the moment this runs — including triggers on
            // tables this edit never mentioned.
            Change::ReplaceFunction(d) => vec![format!(
                "Redefines {}. Every trigger bound to it runs the new body from \
                 now on, including any on other tables.",
                d.info.name
            )],
            Change::DropFunction(f) => vec![format!(
                "Drops function {}. PostgreSQL refuses while a trigger still \
                 uses it, so any that do have to be dropped first.",
                f.name
            )],
            // Nothing is destroyed and it still has to be said, because it is the
            // one edit here that **can't be undone in place**: PostgreSQL has no
            // way to remove an enum value, so taking this one back means
            // re-creating the type and re-casting every column that uses it.
            Change::AddEnumValue { value, .. } => vec![format!(
                "Adding {value} can't be undone in place — PostgreSQL can't remove \
                 an enum value, so taking it back means re-creating the type. The \
                 value also can't be used until this plan is applied."
            )],
            // No row is rewritten and every row means something new, which is
            // exactly why it has to be said. A value list can't tell "I renamed
            // this one" from "I deleted it and typed another", so the plan takes
            // the reading that keeps the data — and this sentence is what makes
            // that reading visible before it runs.
            Change::RenameEnumValue { from, to } => vec![format!(
                "Every row holding {from} reads {to} from now on. If you meant to \
                 remove {from} rather than rename it, delete it and apply that on \
                 its own."
            )],
            Change::RecreateEnum { info, dependents } => {
                vec![recreate_risk(&info.name, "type", dependents)]
            }
            Change::RecreateDomain {
                info,
                from_type,
                dependents,
            } => {
                // The narrowing verdict goes **first**: `recreate_risk`'s
                // sentence ends "nothing is applied", which is the opposite of
                // what a truncating re-cast does, and a reader who stops after
                // the first sentence must not stop on that one.
                //
                // Normalized first, and with `Postgres` spelled in rather than
                // threaded through `risks()`: a domain exists on no other
                // engine, and without it the server's `character varying(255)`
                // and a typed `varchar(16)` read as two different type families
                // — which reports "rewrites every value" instead of naming the
                // truncation.
                let mut out = Vec::new();
                out.extend(type_change_risk(
                    &normalize_type(from_type, SqlDialect::Postgres),
                    &normalize_type(&info.base_type, SqlDialect::Postgres),
                    &format!("domain {}", info.name),
                ));
                out.push(recreate_risk(&info.name, "domain", dependents));
                out
            }
            // The same shape as `DropCheck` on a table: no data goes, but the
            // guarantee does, and every column of this type loses it at once.
            Change::DropDomainCheck { name } => vec![format!(
                "Drops check {name}. Every column of this domain stops being \
                 checked, and existing data is not re-examined if it's added back."
            )],
            Change::SetDomainNotNull { to: false } => vec![
                "Every column of this domain starts accepting NULL, including ones \
                 in tables this edit never mentioned."
                    .to_string(),
            ],
            Change::SetDomainNotNull { to: true } => vec![
                "The statement fails if any column of this domain already holds NULL.".to_string(),
            ],
            Change::DropObject { kind } => vec![match kind {
                ObjectKind::Sequence => "Drops the sequence and the position it had \
                     reached. A column defaulting to `nextval` on it stops working."
                    .to_string(),
                k => format!(
                    "Drops the {}. PostgreSQL refuses while a column still uses it, \
                     so those columns have to change type first.",
                    k.label()
                ),
            }],
            // Every row keeps its value, and the counter forgets where it was: the
            // next `nextval` can hand back a key that already exists.
            Change::RestartSequence { to } => vec![format!(
                "Restarts the counter at {to}. Values already handed out are not \
                 changed, so a key it reaches again collides."
            )],
            _ => Vec::new(),
        }
    }

    /// Whether this change destroys anything at all.
    pub fn is_destructive(&self) -> bool {
        !self.risks().is_empty()
    }
}

/// The sentence behind a rename-create-recast-drop rebuild.
///
/// It names the columns rather than counting them, because "3 columns" doesn't
/// tell anyone whether the one they care about is in the list. It is capped, for
/// the same reason: a type used by forty columns produces a paragraph nobody
/// reads, and past a handful the *count* is the useful part again.
///
/// It also says the list may be short. A view, function or composite type built
/// on this one can't be enumerated from the introspected schema at all, so the
/// honest claim is "these columns, and the server may still refuse" — never
/// "these columns, and that's all of them".
fn recreate_risk(name: &str, kind: &str, dependents: &[TypeDependent]) -> String {
    const NAMED: usize = 6;
    let mut out = format!(
        "Re-creating {kind} {name} drops and rebuilds it. Every column below is \
         re-cast through text; a value the new definition doesn't accept fails the \
         whole plan, and nothing is applied"
    );
    if dependents.is_empty() {
        out.push_str(
            ". Nothing uses it today, but a view or function built on it \
                      can't be listed here and would make the server refuse.",
        );
        return out;
    }
    let named: Vec<String> = dependents
        .iter()
        .take(NAMED)
        .map(|d| {
            format!(
                "{}.{}",
                crate::schema::display_name(d.schema.as_deref(), &d.table),
                d.column
            )
        })
        .collect();
    out.push_str(&format!(": {}", named.join(", ")));
    if dependents.len() > NAMED {
        out.push_str(&format!(" and {} more", dependents.len() - NAMED));
    }
    out.push_str(
        ". A view or function built on it can't be listed here and would make the \
         server refuse.",
    );
    out
}

/// Which of a sequence's clauses differ, named for the preview.
///
/// Shared by the summary and the emitter, so the line the user reads and the
/// statement that runs are built from the same comparison — the `TableOptions`
/// rule, which exists because the two once disagreed.
fn sequence_edits(from: &SequenceInfo, to: &SequenceInfo) -> Vec<String> {
    let mut out = Vec::new();
    if !from.data_type.eq_ignore_ascii_case(&to.data_type) {
        out.push(format!("type to {}", to.data_type));
    }
    if from.increment != to.increment {
        out.push(format!("increment to {}", to.increment));
    }
    if from.min_value != to.min_value {
        out.push(format!("minimum to {}", to.min_value));
    }
    if from.max_value != to.max_value {
        out.push(format!("maximum to {}", to.max_value));
    }
    if from.start != to.start {
        out.push(format!("start to {}", to.start));
    }
    if from.cache != to.cache {
        out.push(format!("cache to {}", to.cache));
    }
    if from.cycle != to.cycle {
        out.push(
            if to.cycle {
                "cycling on"
            } else {
                "cycling off"
            }
            .to_string(),
        );
    }
    if from.owned_by != to.owned_by {
        out.push(match &to.owned_by {
            Some(o) => format!("owner to {}.{}", o.table, o.column),
            None => "owner to none".to_string(),
        });
    }
    out
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
    out.extend(type_change_risk(&from.type_name, &to.type_name, &to.name));
    out
}

/// What changing a declared type from `from` to `to` costs the values already
/// stored — phrased about `subject`, which is a column name here and a domain's
/// name when [`Change::RecreateDomain`] asks.
///
/// Split out of [`alter_risks`] because a domain's base type is the same
/// question asked of a different object, and `RecreateDomain` was the one
/// narrowing path in the emitter that answered it with nothing at all.
fn type_change_risk(from: &str, to: &str, subject: &str) -> Option<String> {
    let (fb, fa) = {
        let p = split_type(from);
        (p.base, p.params)
    };
    let (tb, ta) = {
        let p = split_type(to);
        (p.base, p.params)
    };
    if fb.is_empty() || tb.is_empty() || from == to {
        return None;
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
        if lost.is_empty() {
            return None;
        }
        return Some(format!(
            "Narrowing {subject} from {from} to {to} {}.",
            lost.join(" and ")
        ));
    }
    Some(format!(
        "Changing {subject} from {from} to {to} rewrites every value; \
         it can fail or lose precision."
    ))
}

/// Where a plan is going: the dialect it must be spelled in, and — within the
/// MySQL family — *which* server, because the two diverge at the emitter.
///
/// A separate type rather than a second parameter everywhere so the ~40 call
/// sites that only ever had a dialect keep compiling: `diff(&t, &d, MySql)`
/// still works through [`From<SqlDialect>`], and means "MySQL family, flavour
/// not stated". Only the callers that actually know say so.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub dialect: SqlDialect,
    pub flavour: ServerFlavour,
}

impl From<SqlDialect> for Target {
    fn from(dialect: SqlDialect) -> Self {
        Target {
            dialect,
            flavour: ServerFlavour::Unknown,
        }
    }
}

impl Target {
    pub fn new(dialect: SqlDialect, flavour: ServerFlavour) -> Self {
        Target { dialect, flavour }
    }
}

/// A set of changes against one table, ready to review and emit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeSet {
    /// The table's name **on the server** — what an `ALTER` addresses. A rename
    /// is one of the changes, not a new identity here.
    pub table: String,
    pub schema: Option<String>,
    pub dialect: SqlDialect,
    /// The server flavour, when the caller knew it. Only the MySQL emitter's
    /// `ALTER TABLE` path reads it — see [`ChangeSet::destructive`] — so every
    /// other constructor leaves it `Unknown`.
    pub flavour: ServerFlavour,
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
        // A check that comes off and goes back on in the same plan is not a
        // dropped check, and saying "rows the constraint refused are accepted
        // from now on" about one is simply false. Neither engine can alter a
        // check in place, so *every* edit — and the compensating pair a MySQL
        // column rename needs — is a drop and an add; the `AddCheck` summary is
        // what states the new predicate.
        let re_added: HashSet<&str> = self
            .changes
            .iter()
            .filter_map(|c| match c {
                Change::AddCheck(ck) => Some(ck.name.as_str()),
                _ => None,
            })
            .collect();
        self.changes
            .iter()
            .filter(
                |c| !matches!(c, Change::DropCheck { name } if re_added.contains(name.as_str())),
            )
            .flat_map(Change::risks)
            .collect()
    }

    /// The changes in this set the dialect **can't express**, in plain language.
    ///
    /// Non-empty means [`ChangeSet::emit`] is writing less than the plan asks
    /// for, and the preview says so instead of applying the remainder. That is
    /// the same call [`Change::KeepLossyIndex`] makes: the user asked for
    /// something and it isn't in the script, and a preview that didn't mention
    /// it would be the dishonest half of a destructive operation.
    ///
    /// Only SQLite ever produces one today — see [`supports_change`].
    pub fn unsupported(&self) -> Vec<String> {
        // A rebuild performs the whole set — it writes the table the draft
        // describes — so nothing beside it is withheld, however little of it has
        // a statement of its own.
        //
        // **Except an index the model only partly read.** The rebuild drops the
        // table, so every index has to be recreated, and one recreated from a
        // partial reading is not the index that was there: a partial index comes
        // back covering every row, an expression index comes back missing its
        // key. That is the failure [`IndexInfo::lossy`] exists to name, and here
        // it is not a warning to read past — the plan is refused until the user
        // drops the index or leaves the table alone.
        if let Some(Change::RebuildTable(r)) = self.changes.iter().find(|c| is_rebuild(c)) {
            return r
                .draft
                .indexes
                .iter()
                .filter(|ix| ix.info.lossy)
                .map(|ix| {
                    format!(
                        "Index {} can't be rebuilt faithfully — SQLite keeps the part \
                         Schemaic couldn't read only in the index's own CREATE text, \
                         and recreating it from what was read would change what it \
                         indexes. Drop the index to go on without it.",
                        ix.info.name
                    )
                })
                .collect();
        }
        self.changes
            .iter()
            .filter(|c| !supports_change(self.dialect, c))
            .map(Change::summary)
            .collect()
    }

    /// The statements, in the order they must run. Ready to hand to the preview
    /// modal, the clipboard, or a query tab — they're the same text either way.
    pub fn emit(&self) -> Vec<String> {
        match self.dialect {
            SqlDialect::Postgres => self.emit_postgres(),
            // Not a fall-through any more: SQLite's `ALTER TABLE` takes one
            // operation and has no `DROP INDEX` form, so MySQL's shapes are not
            // merely unidiomatic there — they are statements the engine refuses.
            SqlDialect::Sqlite => self.emit_sqlite(),
            _ => self.emit_mysql(),
        }
    }

    /// The statements as one script, blank-line separated — what "Copy" and
    /// "Open in editor" hand over.
    pub fn script(&self) -> String {
        self.emit().join("\n\n")
    }

    /// The same script, but runnable by a **client** that splits on `;`.
    ///
    /// `run_ddl` hands each statement to the wire whole, so it never needs this;
    /// "Open in editor" drops the text into a query tab, where Schemaic's own
    /// splitter cuts on every top-level `;` — and a MySQL trigger's `BEGIN … END`
    /// body is full of them. Run Everything then ran `CREATE TRIGGER … SET NEW.a
    /// = 1;` as one statement and the rest as fragments: the application handing
    /// the user a script it cannot run itself, which is exactly what the escape
    /// hatch exists to avoid.
    ///
    /// So a statement carrying an internal `;` is wrapped in `DELIMITER $$` …
    /// `DELIMITER ;`, the form `mysqldump` writes and `sql::statement_bounds`
    /// now reads. Nothing is wrapped when nothing needs it, and PostgreSQL never
    /// does — a function body there is dollar-quoted, which `skip_noncode`
    /// already sees through.
    pub fn editor_script(&self) -> String {
        let stmts = self.emit();
        // `DELIMITER` is **MySQL's client directive**, so this is asked as
        // `!= MySql` rather than `!= Postgres`: SQLite would be handed a word it
        // has no idea about, at the top of the very script the escape hatch
        // exists to make runnable. It needs none of it — `statement_bounds`
        // knows a SQLite trigger's body runs to the `;` after its `END`.
        if self.dialect != SqlDialect::MySql
            || !stmts.iter().any(|s| needs_delimiter(s, self.dialect))
        {
            return stmts.join("\n\n");
        }
        let mut out = String::from("DELIMITER $$\n\n");
        out.push_str(
            &stmts
                .iter()
                .map(|s| format!("{}$$", s.trim_end().trim_end_matches(';')))
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
        out.push_str("\n\nDELIMITER ;");
        out
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
        // Functions before triggers: a trigger can't be created until the
        // function it executes exists. Today a change set only ever holds one
        // kind, so nothing depends on this yet — which is exactly why it's worth
        // getting right now rather than discovering it later.
        out.extend(self.function_statements());
        out.extend(self.trigger_statements());
        // The standalone objects are PostgreSQL's, so this contributes nothing on
        // a MySQL connection. It is still called from *both* emitters rather than
        // one, so that such a change set arriving here emits SQL the server can
        // reject instead of being silently dropped on the floor.
        out.extend(self.object_statements());
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
        // `DROP CONSTRAINT`, not MySQL 8's `DROP CHECK`: MariaDB only has the
        // former, and MySQL has had it since 8.0.19 — so one spelling covers
        // both current servers, where `DROP CHECK` covers only one. (A check
        // constraint needs a server new enough to have them at all: MySQL
        // 8.0.16, MariaDB 10.2.)
        for c in &self.changes {
            if let Change::DropCheck { name } = c {
                cl.push(format!("DROP CONSTRAINT {}", self.q(name)));
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
            if let Change::AlterColumn {
                from,
                to,
                position,
                inline_check,
            } = c
            {
                let pos = position.as_ref().map(|p| p.sql(d)).unwrap_or_default();
                let mut def = to.definition_sql(d);
                // A MariaDB column-level CHECK is part of the definition being
                // replaced, so it has to be restated here or the server deletes
                // it. It can't be re-added as a constraint afterwards instead:
                // `DROP CONSTRAINT`/`ADD CONSTRAINT` can't address one (1091 on
                // the drop), and the name is the column's, not the user's.
                if let Some(ck) = inline_check {
                    def.push(' ');
                    def.push_str(&ck.inline_sql());
                }
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
        for c in &self.changes {
            if let Change::AddCheck(ck) = c {
                cl.push(format!("ADD {}", ck.clause_sql(d)));
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
        // Functions before triggers: a trigger can't be created until the
        // function it executes exists. Today a change set only ever holds one
        // kind, so nothing depends on this yet — which is exactly why it's worth
        // getting right now rather than discovering it later.
        out.extend(self.function_statements());
        out.extend(self.trigger_statements());
        out.extend(self.object_statements());
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
                Change::DropForeignKey { name } | Change::DropCheck { name } => {
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
            if let Change::AddCheck(ck) = c {
                cl.push(format!("ADD {}", ck.clause_sql(d)));
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

    // ── SQLite ───────────────────────────────────────────────────────────────

    /// SQLite emits **one statement per change**, and only for the changes
    /// [`supports_change`] allows.
    ///
    /// Two things separate it from the MySQL emitter it used to fall through to.
    /// Its `ALTER TABLE` takes exactly one operation — there is no clause list —
    /// so two dropped columns are two statements. And an index is dropped by its
    /// own `DROP INDEX`, as on PostgreSQL, not by an `ALTER TABLE … DROP INDEX`
    /// that SQLite has no form of.
    ///
    /// Filtering on [`supports_change`] rather than trusting the caller is
    /// deliberate: a change this engine can't express emits **nothing**, so a
    /// gate that drifts open shows an empty preview instead of handing SQLite a
    /// statement written for MySQL.
    fn emit_sqlite(&self) -> Vec<String> {
        // A rebuild subsumes the set: the other entries describe what it
        // achieves, and emitting them beside it would apply the same edit twice.
        if let Some(Change::RebuildTable(r)) = self.changes.iter().find(|c| is_rebuild(c)) {
            let mut out = sqlite_rebuild_sql(&r.current, &r.draft);
            // The one thing the rebuild leaves out. It comes after, as a native
            // statement, so SQLite repoints the references other objects hold —
            // which is exactly what the rebuild's own rename must *not* do.
            for c in &self.changes {
                if let Change::RenameTable { to } = c {
                    out.push(format!(
                        "ALTER TABLE {} RENAME TO {};",
                        self.qname(),
                        self.q(to)
                    ));
                }
            }
            return out;
        }
        let supported = || {
            self.changes
                .iter()
                .filter(|c| supports_change(self.dialect, c))
        };
        // The view and trigger statements, from the same builders the other two
        // emitters use — including the view drop, which this emitter used to
        // spell out for itself. Each of those change sets holds nothing else, so
        // nothing below ever sees them.
        let mut out = self.view_statements();
        out.extend(self.trigger_statements());
        for c in supported() {
            if matches!(c, Change::DropTable) {
                out.push(format!("DROP TABLE {};", self.qname()));
            }
        }
        // Indexes before columns: SQLite refuses to drop a column an index still
        // names, so the two in one plan only work in this order.
        for c in supported() {
            if let Change::DropIndex { name, .. } = c {
                out.push(format!(
                    "DROP INDEX {};",
                    qualified(name, self.schema.as_deref(), self.dialect)
                ));
            }
        }
        for c in supported() {
            if let Change::DropColumn { name, .. } = c {
                out.push(format!(
                    "ALTER TABLE {} DROP COLUMN {};",
                    self.qname(),
                    self.q(name)
                ));
            }
        }
        // Adds after drops, so a column that replaces a dropped one of the same
        // name finds the name free. The definition comes from the one column
        // emitter the rebuild's `CREATE TABLE` also uses, so the two can't
        // disagree about what the column is — only about how it gets there.
        for c in supported() {
            if let Change::AddColumn { column, .. } = c {
                out.push(format!(
                    "ALTER TABLE {} ADD COLUMN {};",
                    self.qname(),
                    column.definition_sql(self.dialect)
                ));
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
                // renames a view with. Spelled out per engine rather than
                // `_ =>`, which silently handed SQLite MySQL's statement.
                Change::RenameView { to } => match d {
                    SqlDialect::Postgres => out.push(format!(
                        "ALTER VIEW {} RENAME TO {};",
                        self.qname(),
                        ddl_ident_in(to, d)
                    )),
                    SqlDialect::MySql => out.push(format!(
                        "RENAME TABLE {} TO {};",
                        self.qname(),
                        qualified(to, self.schema.as_deref(), d)
                    )),
                    // Unreachable rather than unimplemented: SQLite has no verb
                    // that renames a view ([`supports_view_rename`]), so
                    // `diff_view` turns a rename there into the drop-and-create
                    // above and this change never reaches the emitter.
                    SqlDialect::Sqlite => {
                        debug_assert!(false, "SQLite has no statement that renames a view")
                    }
                },
                _ => {}
            }
        }
        out
    }

    /// The trigger statements, in the order they must run.
    ///
    /// Separate from the `ALTER TABLE` builders for the same reason
    /// [`ChangeSet::view_statements`] is: these are whole statements that can
    /// never share an `ALTER`, and a trigger change set contains nothing else.
    ///
    /// A replace emits `DROP` then `CREATE` on **both** engines. PostgreSQL 14
    /// grew `CREATE OR REPLACE TRIGGER`, but using it would mean two apply paths
    /// for one edit and a version check to pick between them — and on PG the
    /// whole plan already runs in one transaction, so the drop-and-create is
    /// atomic anyway.
    fn trigger_statements(&self) -> Vec<String> {
        let d = self.dialect;
        let drop = |name: &str| -> String {
            match d {
                // PostgreSQL scopes a trigger to its table and needs it named;
                // MySQL and SQLite scope it to the database and take the bare
                // name — SQLite refuses an `ON` clause here just as MySQL does.
                SqlDialect::Postgres => format!(
                    "DROP TRIGGER {} ON {};",
                    ddl_ident_in(name, d),
                    self.qname()
                ),
                SqlDialect::MySql | SqlDialect::Sqlite => format!(
                    "DROP TRIGGER {};",
                    qualified(name, self.schema.as_deref(), d)
                ),
            }
        };
        // **All the drops, then all the creates** — not each replace's pair
        // together.
        //
        // Adjacent pairs collide the moment two triggers swap names (`a`→`b`,
        // `b`→`a`): statement 2 creates `b` while the original `b` is still
        // there, fails `ERROR 1359 (Trigger already exists)`, and on MySQL
        // statement 1 has **already committed** — trigger `a` is simply gone,
        // since MySQL DDL has no transaction to roll back. PostgreSQL rolls the
        // whole plan back, so the two engines disagreed about what a failed
        // apply leaves behind. `TriggerSetDraft::validate` can't catch it: it
        // compares only final names, and the final names are unique.
        //
        // This is the order `diff_triggers` already documents for standalone
        // drops, and the one `GridWrite::plan` uses in `core::model`.
        let mut drops = Vec::new();
        let mut creates = Vec::new();
        let mut push_create = |t: &TriggerInfo| {
            creates.extend(session_wrapped_create(t, d));
        };
        for c in &self.changes {
            match c {
                Change::CreateTrigger(draft) => push_create(&draft.info),
                Change::ReplaceTrigger { draft } => {
                    // The drop addresses the name the server knows; the create
                    // builds the draft's, which is how a rename comes for free.
                    drops.push(drop(draft.original.as_deref().unwrap_or(&draft.info.name)));
                    push_create(&draft.info);
                }
                Change::DropTrigger { name } => drops.push(drop(name)),
                _ => {}
            }
        }
        drops.extend(creates);
        drops
    }

    /// The function statements, in the order they must run.
    ///
    /// A rename goes **after** the redefinition, not before: `CREATE OR REPLACE`
    /// has to address the signature the server already has, and only then can
    /// `ALTER FUNCTION … RENAME` move it — the same ordering
    /// [`ChangeSet::view_statements`] uses, and for the same reason.
    fn function_statements(&self) -> Vec<String> {
        let d = self.dialect;
        let mut out = Vec::new();
        for c in &self.changes {
            match c {
                Change::CreateFunction(draft) => out.push(draft.info.create_sql(d, false)),
                Change::ReplaceFunction(draft) => {
                    // Address the name the server knows; the rename runs after.
                    let mut server = draft.info.clone();
                    if let Some(orig) = &draft.original {
                        server.name = orig.clone();
                    }
                    out.push(server.create_sql(d, true));
                }
                Change::RenameFunction { from, to } => out.push(format!(
                    "ALTER FUNCTION {} RENAME TO {};",
                    from.signature_sql(d),
                    ddl_ident_in(to, d)
                )),
                // By signature, not by name: PostgreSQL identifies a function by
                // its argument types, and a bare name is ambiguous the moment an
                // overload exists.
                Change::DropFunction(f) => {
                    out.push(format!("DROP FUNCTION {};", f.signature_sql(d)))
                }
                _ => {}
            }
        }
        out
    }

    /// The statements for a standalone object — an enum, a domain or a sequence.
    ///
    /// One builder for all three, on the same grounds as
    /// [`ChangeSet::view_statements`]: none of these can ever be a clause of an
    /// `ALTER TABLE`, so there is no coalescing to do and no engine split to make
    /// — every one of them is PostgreSQL-only, which is why nothing here consults
    /// the dialect beyond quoting.
    ///
    /// Order is dependency-first and then rename-last, the same shape the rest of
    /// the emitter uses: create before altering, alter under the name the server
    /// currently knows, and rename only once everything above has run.
    fn object_statements(&self) -> Vec<String> {
        let d = self.dialect;
        let qname = self.qname();
        let mut out = Vec::new();
        // A restart rides in the same `ALTER SEQUENCE` as the bound edits when
        // the plan has both. PostgreSQL cross-checks new bounds against the
        // sequence's **current** value unless the statement also restarts it —
        // measured on 16.14: `ALTER SEQUENCE s MAXVALUE 100` on a sequence
        // sitting at 500 is `ERROR: RESTART value (500) cannot be greater than
        // MAXVALUE (100)`, while the same clause with `RESTART WITH 50` after it
        // succeeds. Split across two statements, the narrowing-plus-restart pair
        // — the *only* form of that edit the server can accept — could never be
        // applied. The two stay separate `Change`s so the preview still says
        // both things; only the statement is shared.
        let folded_restart: Option<i64> = self
            .changes
            .iter()
            .any(|c| {
                matches!(c, Change::AlterSequence { from, to }
                if !sequence_alter_clauses(from, to, d).is_empty())
            })
            .then(|| {
                self.changes.iter().find_map(|c| match c {
                    Change::RestartSequence { to } => Some(*to),
                    _ => None,
                })
            })
            .flatten();
        for c in &self.changes {
            match c {
                Change::CreateEnum(e) => out.push(e.create_sql(d)),
                Change::CreateDomain(dom) => out.push(dom.create_sql(d)),
                Change::CreateSequence(s) => out.push(s.create_sql(d)),
                Change::AddEnumValue {
                    value,
                    after,
                    before,
                } => {
                    let anchor = match (after, before) {
                        (Some(a), _) => format!(" AFTER {}", ddl_string(a, d)),
                        (_, Some(b)) => format!(" BEFORE {}", ddl_string(b, d)),
                        _ => String::new(),
                    };
                    out.push(format!(
                        "ALTER TYPE {qname} ADD VALUE {}{anchor};",
                        ddl_string(value, d)
                    ));
                }
                Change::RenameEnumValue { from, to } => out.push(format!(
                    "ALTER TYPE {qname} RENAME VALUE {} TO {};",
                    ddl_string(from, d),
                    ddl_string(to, d)
                )),
                Change::RecreateEnum { info, dependents } => {
                    out.extend(recreate_type_sql(
                        ObjectKind::Enum,
                        &qname,
                        &self.table,
                        self.schema.as_deref(),
                        &info.create_sql(d),
                        dependents,
                        d,
                    ));
                }
                Change::RecreateDomain {
                    info, dependents, ..
                } => {
                    out.extend(recreate_type_sql(
                        ObjectKind::Domain,
                        &qname,
                        &self.table,
                        self.schema.as_deref(),
                        &info.create_sql(d),
                        dependents,
                        d,
                    ));
                }
                Change::SetDomainDefault { to } => out.push(match to {
                    Some(v) => format!("ALTER DOMAIN {qname} SET DEFAULT {v};"),
                    None => format!("ALTER DOMAIN {qname} DROP DEFAULT;"),
                }),
                Change::SetDomainNotNull { to } => out.push(format!(
                    "ALTER DOMAIN {qname} {} NOT NULL;",
                    if *to { "SET" } else { "DROP" }
                )),
                // Drops before adds, so a constraint can be redefined under the
                // name it already has within one plan.
                Change::DropDomainCheck { name } => out.push(format!(
                    "ALTER DOMAIN {qname} DROP CONSTRAINT {};",
                    ddl_ident_in(name, d)
                )),
                Change::AlterSequence { from, to } => {
                    let mut clauses = sequence_alter_clauses(from, to, d);
                    if !clauses.is_empty() {
                        if let Some(r) = folded_restart {
                            clauses.push(format!("RESTART WITH {r}"));
                        }
                        out.push(format!(
                            "ALTER SEQUENCE {qname}\n  {};",
                            clauses.join("\n  ")
                        ));
                    }
                }
                // Only when it hasn't already ridden along above.
                Change::RestartSequence { to } if folded_restart.is_none() => {
                    out.push(format!("ALTER SEQUENCE {qname} RESTART WITH {to};"))
                }
                _ => {}
            }
        }
        for c in &self.changes {
            if let Change::AddDomainCheck(ck) = c {
                out.push(format!("ALTER DOMAIN {qname} ADD {};", ck.clause_sql(d)));
            }
        }
        // The comment addresses the object under the name the server still knows,
        // so it goes before the rename — as the view and function renames do.
        for c in &self.changes {
            if let Change::SetObjectComment { kind, comment } = c {
                out.push(format!(
                    "COMMENT ON {} {qname} IS {};",
                    kind.sql_keyword(),
                    match comment {
                        Some(t) => ddl_string(t, d),
                        None => "NULL".to_string(),
                    }
                ));
            }
        }
        for c in &self.changes {
            match c {
                Change::RenameObject { kind, to } => out.push(format!(
                    "ALTER {} {qname} RENAME TO {};",
                    kind.sql_keyword(),
                    ddl_ident_in(to, d)
                )),
                Change::DropObject { kind } => {
                    out.push(format!("DROP {} {qname};", kind.sql_keyword()))
                }
                _ => {}
            }
        }
        out
    }
}

/// The rename-create-recast-drop dance behind [`Change::RecreateEnum`] and
/// [`Change::RecreateDomain`].
///
/// The old type is **renamed out of the way** rather than dropped first, so the
/// columns keep a valid type at every step and the new one can take the original
/// name immediately. Each dependent column then loses its default (a default is
/// stored against the old type and blocks the retype), is cast through **text**
/// — the one representation both an old and a new enum share — and has its
/// default restated. The old type goes last, when nothing points at it.
///
/// An array column casts through `text[]`: `mood[]` has no direct cast to the
/// rebuilt `mood[]`, but the element cast makes the array cast legal.
///
/// On PostgreSQL the whole plan is one transaction, so a value the new definition
/// rejects fails the cast and leaves the database exactly as it was.
fn recreate_type_sql(
    kind: ObjectKind,
    qname: &str,
    name: &str,
    schema: Option<&str>,
    create: &str,
    dependents: &[TypeDependent],
    d: SqlDialect,
) -> Vec<String> {
    // A name the user's own schema can't already hold, so the shuffle can't
    // collide with a real object.
    let parked = format!("{name}_schemaic_old");
    let qparked = qualified(&parked, schema, d);
    // A domain *is* a type and PostgreSQL will accept `ALTER TYPE`/`DROP TYPE`
    // on one, but the matching keyword is what the rest of the emitter uses and
    // what makes the script readable as the thing it edits.
    let kw = kind.sql_keyword();
    let mut out = vec![format!(
        "ALTER {kw} {qname} RENAME TO {};",
        ddl_ident_in(&parked, d)
    )];
    out.push(create.to_string());
    for dep in dependents {
        let table = qualified(&dep.table, dep.schema.as_deref(), d);
        let col = ddl_ident_in(&dep.column, d);
        if dep.default_value.is_some() {
            out.push(format!(
                "ALTER TABLE {table} ALTER COLUMN {col} DROP DEFAULT;"
            ));
        }
        let (target, via) = if dep.is_array() {
            (format!("{qname}[]"), "text[]")
        } else {
            (qname.to_string(), "text")
        };
        // **The second cast is the difference between refusing and destroying.**
        //
        // `USING col::text::varchar(16)` is an *explicit* cast, which truncates
        // and rounds; stopping at `::text` leaves PostgreSQL to apply the
        // *assignment* cast, which refuses ("value too long for type character
        // varying(16)"). Measured both ways on PG 16.14 against a 64-character
        // value: the explicit form committed a 16-character one.
        //
        // An enum cannot do without it — `USING m::text` alone is rejected with
        // "result of USING clause ... cannot be cast automatically to type
        // mood", because text has no assignment cast to an enum. So the two
        // kinds diverge here, and only the kind that *can* lose data silently
        // gives the cast up.
        let recast = match kind {
            ObjectKind::Domain => format!("{col}::{via}"),
            _ => format!("{col}::{via}::{target}"),
        };
        out.push(format!(
            "ALTER TABLE {table} ALTER COLUMN {col} TYPE {target} USING {recast};"
        ));
        if let Some(def) = &dep.default_value {
            out.push(format!(
                "ALTER TABLE {table} ALTER COLUMN {col} SET DEFAULT {def};"
            ));
        }
    }
    out.push(format!("DROP {kw} {qparked};"));
    out
}

/// The `ALTER SEQUENCE` clauses for the fields that changed — and only those.
///
/// Restating an unchanged clause would be a change nobody reviewed, which is the
/// same rule `TableOptions` follows. The clause list is kept in step with
/// [`sequence_edits`], which is what the preview's sentence is built from.
fn sequence_alter_clauses(from: &SequenceInfo, to: &SequenceInfo, d: SqlDialect) -> Vec<String> {
    let mut out = Vec::new();
    if !from.data_type.eq_ignore_ascii_case(&to.data_type) {
        out.push(format!("AS {}", to.data_type.trim()));
    }
    if from.increment != to.increment {
        out.push(format!("INCREMENT BY {}", to.increment));
    }
    // Bounds go out as explicit numbers rather than `NO MINVALUE`: the model holds
    // concrete values either way, and naming them says what the sequence will
    // actually enforce.
    if from.min_value != to.min_value {
        out.push(format!("MINVALUE {}", to.min_value));
    }
    if from.max_value != to.max_value {
        out.push(format!("MAXVALUE {}", to.max_value));
    }
    // `START WITH` alone changes only what a later `RESTART` would return to — it
    // does not move the counter. That's `RestartSequence`'s job, and keeping them
    // separate is why editing the start value doesn't silently rewind a live key.
    if from.start != to.start {
        out.push(format!("START WITH {}", to.start));
    }
    if from.cache != to.cache {
        out.push(format!("CACHE {}", to.cache));
    }
    if from.cycle != to.cycle {
        out.push(if to.cycle { "CYCLE" } else { "NO CYCLE" }.to_string());
    }
    if from.owned_by != to.owned_by {
        out.push(match &to.owned_by {
            Some(o) => format!(
                "OWNED BY {}.{}",
                qualified(&o.table, to.schema.as_deref(), d),
                ddl_ident_in(&o.column, d)
            ),
            None => "OWNED BY NONE".to_string(),
        });
    }
    out
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
    // Asked per engine, never as `!= Postgres`: SQLite shares none of MySQL's
    // view clauses, and sorting it onto whichever side it happens to fall would
    // emit `ALGORITHM`/`DEFINER`/`SQL SECURITY` at a server that has no idea
    // what they are.
    let my = dialect == SqlDialect::MySql;
    let pg = dialect == SqlDialect::Postgres;
    let o = &v.options;
    fn set(s: &Option<String>) -> Option<&str> {
        s.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }
    let mut sql = String::from("CREATE ");
    // Guarded on the capability as well as on the caller's answer: a `replace`
    // that reached SQLite would emit a statement the engine has no form of.
    if replace && supports_or_replace_view(dialect) {
        sql.push_str("OR REPLACE ");
    }
    if my {
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
    // SQLite's explicit column list, restated because the re-create would
    // otherwise rename the view's columns to whatever the body calls them.
    if let Some(cols) = set(&o.column_list).filter(|_| dialect == SqlDialect::Sqlite) {
        sql.push_str(&format!(" ({cols})"));
    }
    if pg && !o.storage.is_empty() {
        sql.push_str(&format!(" WITH ({})", o.storage.join(", ")));
    }
    sql.push_str(" AS\n");
    sql.push_str(&view_body(&v.select));
    // A materialized view has no check option — it isn't updatable at all — and
    // neither does SQLite, at any view.
    if (my || pg)
        && !o.materialized
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
    // SQLite sides with PostgreSQL on indexes — they are statements of their own,
    // there being no inline `KEY` — and has neither engine's table options.
    let sqlite = dialect == SqlDialect::Sqlite;
    let separate_indexes = pg || sqlite;
    let q = |s: &str| ddl_ident_in(s, dialect);
    let qname = qualified(&d.name, d.schema.as_deref(), dialect);
    // **SQLite's `AUTOINCREMENT` exists only inline**, as `INTEGER PRIMARY KEY
    // AUTOINCREMENT` on one column — there is no table-level spelling of it, and
    // a `PRIMARY KEY (…)` clause alongside would declare a second key. So the
    // one column it can apply to takes the whole declaration, and the table
    // constraint below stands down.
    let inline_key: Option<&str> = match d.primary_key.as_slice() {
        [only] if sqlite => d
            .columns
            .iter()
            .find(|c| &c.info.name == only && c.info.auto_increment)
            .map(|c| c.info.name.as_str()),
        _ => None,
    };
    let mut lines: Vec<String> = d
        .columns
        .iter()
        .map(|c| {
            if Some(c.info.name.as_str()) == inline_key {
                // Deliberately not `definition_sql` plus a suffix: `NOT NULL` is
                // implied by the rowid alias, and this is the form SQLite itself
                // writes back when asked for the table's DDL.
                return format!(
                    "  {} {} PRIMARY KEY AUTOINCREMENT",
                    q(&c.info.name),
                    c.info.type_name
                );
            }
            format!("  {}", c.info.definition_sql(dialect))
        })
        .collect();
    if !d.primary_key.is_empty() && inline_key.is_none() {
        lines.push(format!(
            "  PRIMARY KEY ({})",
            d.primary_key
                .iter()
                .map(|c| q(c))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !separate_indexes {
        // MySQL inlines its indexes; the other two can't and emit them after.
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
    // Inline on both engines — a check is a table constraint, not an index, so
    // PostgreSQL has nothing to split out here.
    for ck in &d.check_constraints {
        lines.push(format!("  {}", ck.info.clause_sql(dialect)));
    }
    let mut head = format!("CREATE TABLE {qname} (\n{}\n)", lines.join(",\n"));
    // None of the three exists in SQLite: no storage engine to name, no table
    // collation (it is per column there), and no comments anywhere in the
    // language.
    if !pg && !sqlite {
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
    if separate_indexes {
        for ix in &d.indexes {
            out.push(create_index_sql(&ix.info, &qname, dialect));
        }
    }
    if pg {
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

/// A trigger's `CREATE`, wrapped in the session state it was created under.
///
/// `CREATE TRIGGER` has **no clause** for `sql_mode`, `character_set_client` or
/// `collation_connection`, yet all three are part of what the trigger does: one
/// written under `sql_mode = ''` and recreated under a strict mode starts
/// failing every parent `INSERT`, and reversed it stops raising and silently
/// truncates. So the values are set on the session around the statement and
/// restored after — the shape `mysqldump` uses, and safe here because
/// `Db::run_ddl` runs a MySQL plan's statements in order on **one** connection.
///
/// Emitting nothing when nothing is known is deliberate: `None` means "not
/// fetched" (or PostgreSQL), and inventing a session state would be a change
/// nobody asked for. Restoring from a user variable rather than a literal keeps
/// the connection as it was found, whatever it was.
fn session_wrapped_create(t: &TriggerInfo, d: SqlDialect) -> Vec<String> {
    let create = t.create_sql(d);
    // MySQL's problem alone. PostgreSQL's trigger carries no session state, and
    // SQLite has no `SET SESSION` to carry it with — asked as `!= MySql` so a
    // third engine can't inherit `SET SESSION sql_mode = …` by falling through.
    if d != SqlDialect::MySql {
        return vec![create];
    }
    let settings: Vec<(&str, &str)> = [
        ("sql_mode", t.sql_mode.as_deref()),
        ("character_set_client", t.charset_client.as_deref()),
        ("collation_connection", t.collation_connection.as_deref()),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.map(|v| (k, v)))
    .collect();
    if settings.is_empty() {
        return vec![create];
    }
    let save = settings
        .iter()
        .map(|(k, _)| format!("@schemaic_{k} = @@SESSION.{k}"))
        .collect::<Vec<_>>()
        .join(", ");
    let set = settings
        .iter()
        .map(|(k, v)| format!("SESSION {k} = {}", ddl_string(v, d)))
        .collect::<Vec<_>>()
        .join(", ");
    let restore = settings
        .iter()
        .map(|(k, _)| format!("SESSION {k} = @schemaic_{k}"))
        .collect::<Vec<_>>()
        .join(", ");
    vec![
        format!("SET {save};"),
        format!("SET {set};"),
        create,
        format!("SET {restore};"),
    ]
}

/// Peel the parentheses that wrap a *whole* expression, leaving the predicate
/// itself.
///
/// Only a pair that opens at the start and closes at the end: `(a) AND (b)`
/// begins and ends with one, but they are not each other's match, and peeling
/// blindly leaves `a) AND (b`. The scan goes through [`sql::skip_noncode`],
/// because `name <> ')'` carries a close-paren inside a string literal and a raw
/// byte scan reads it as the end of the group.
fn peel_parens(s: &str, dialect: SqlDialect) -> &str {
    let mut t = s.trim();
    while t.len() >= 2 && t.starts_with('(') && t.ends_with(')') {
        // The pair only wraps the whole expression when the opener's *match* is
        // the final byte. `(a) AND (b)` starts and ends with one and peeling
        // blindly would leave `a) AND (b`.
        if sql::balanced_paren_span(t.as_bytes(), 0, dialect) != Some(t.len() - 1) {
            break;
        }
        t = t[1..t.len() - 1].trim();
    }
    t
}

/// The bare predicate of a `CHECK` constraint, as the model stores it — given
/// whatever the catalogue handed back.
///
/// Normalized **on the way in**, the way [`crate::schema::ColumnInfo::default`]'s
/// text is, so everything downstream holds one shape and the emitter owns the
/// wrapping. The two engines report three different things: PostgreSQL's
/// `pg_get_constraintdef` returns the whole clause, `CHECK ((total >= 0))`;
/// MySQL's `CHECK_CLAUSE` returns just the parenthesised predicate,
/// `` (`qty` > 0) ``; and a person types `qty > 0`. Storing them verbatim and
/// wrapping at emit produced `CHECK (((total >= 0)))` — valid, and read by the
/// user in the preview.
pub fn check_predicate(raw: &str, dialect: SqlDialect) -> String {
    let t = raw.trim();
    // `CHECK` only when it's the leading *word* — a predicate may legitimately
    // start with a column called `checked_at`.
    let t = t
        .strip_prefix("CHECK")
        .or_else(|| t.strip_prefix("check"))
        .filter(|rest| rest.starts_with(['(', ' ', '\t', '\n']))
        .unwrap_or(t);
    peel_parens(check_trailers(t).0, dialect).to_string()
}

/// Split PostgreSQL's clause trailers off the end of a `pg_get_constraintdef`
/// body: `(predicate, validated, inherited)`.
///
/// `pg_get_constraintdef` returns the **whole clause** — `CHECK ((qty > 0))
/// NOT VALID`, or `CHECK ((…)) NO INHERIT NOT VALID` — and `peel_parens` gates
/// on the text ending in `)`, which a trailer makes false. So the trailer was
/// stored *inside* `expression` and the emitter wrapped the lot:
/// `CHECK (((qty > 0)) NOT VALID)`, which every path that emits a check turned
/// into `ERROR: syntax error at or near "NOT"` — Copy DDL, `CREATE TABLE`, the
/// preview's script, and domain checks, which share this parser.
///
/// Order follows the server's own printing, so both are stripped in one pass
/// regardless of which are present.
fn check_trailers(s: &str) -> (&str, bool, bool) {
    let mut t = s.trim_end();
    let mut validated = true;
    let mut inherited = true;
    // Right to left, since that is the order they were appended in.
    for (kw, flag) in [
        ("NOT VALID", &mut validated),
        ("NO INHERIT", &mut inherited),
    ] {
        let end = t.len().saturating_sub(kw.len());
        if t.len() > kw.len() && t[end..].eq_ignore_ascii_case(kw) {
            // Only when it really is a trailing *clause*, not the tail of the
            // predicate's own last token.
            let before = t[..end].trim_end();
            if before.len() < t[..end].len() {
                *flag = false;
                t = before;
            }
        }
    }
    (t, validated, inherited)
}

/// The `NOT VALID` / `NO INHERIT` flags of a `pg_get_constraintdef` body, for
/// the introspection side to record beside the predicate that
/// [`check_predicate`] extracts from the same text.
pub fn check_clause_flags(raw: &str) -> (bool, bool) {
    let t = raw.trim();
    let t = t
        .strip_prefix("CHECK")
        .or_else(|| t.strip_prefix("check"))
        .filter(|rest| rest.starts_with(['(', ' ', '\t', '\n']))
        .unwrap_or(t);
    let (_, validated, inherited) = check_trailers(t);
    (validated, inherited)
}

/// The bare `WHEN` guard of a trigger, as the model stores it — the same
/// normalize-on-the-way-in rule as [`check_predicate`], for the same reason.
///
/// PostgreSQL's `pg_get_expr(tgqual, tgrelid)` re-prints the guard from its parse
/// tree and parenthesises it (`(new.total > 0)`), `pg_get_triggerdef` spells it
/// `WHEN ((new.total > 0))`, and a person types `new.total > 0`. Stored verbatim
/// and wrapped at emit, those become `WHEN (((new.total > 0)))` — valid, and read
/// by the user in the preview. So the model holds it bare and
/// [`crate::schema::TriggerInfo::create_sql`] is the only thing that wraps it.
pub fn trigger_condition(raw: &str, dialect: SqlDialect) -> String {
    let t = raw.trim();
    // `WHEN` only as a leading *word*: a guard may start with a column called
    // `when_due`, exactly as a check predicate may start with `checked_at`.
    let t = t
        .strip_prefix("WHEN")
        .or_else(|| t.strip_prefix("when"))
        .filter(|rest| rest.starts_with(['(', ' ', '\t', '\n']))
        .unwrap_or(t);
    peel_parens(t, dialect).to_string()
}

/// The comparison form: peeled, and with the whitespace runs the server re-prints
/// with squashed to one space.
///
/// Deliberately *not* token-aware — `qty>0` and `qty > 0` do not compare equal
/// here. Whitespace between tokens is all that's normalized, the same depth
/// [`defaults_equal`] goes to. Getting it wrong in this direction costs a
/// needless drop-and-add of an unchanged constraint, which re-validates and is
/// safe; a tokenizer that got it wrong the other way would silently keep a
/// predicate the user had edited.
fn norm_check_expr(s: &str, dialect: SqlDialect) -> String {
    let t = peel_parens(s, dialect);
    let b = t.as_bytes();
    let mut out = String::with_capacity(t.len());
    let mut i = 0usize;
    let mut pending_space = false;
    while i < b.len() {
        // A string, quoted identifier or comment passes through **byte for
        // byte**. Squashing inside one made `name <> 'a  b'` and
        // `name <> 'a b'` compare equal, so an edit to a `LIKE`/regex pattern
        // was silently discarded — the dangerous direction, and the one this
        // function's own doc says it avoids.
        if let Some(j) = sql::skip_noncode(b, i, dialect) {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push_str(&t[i..j]);
            i = j;
            continue;
        }
        if b[i].is_ascii_whitespace() {
            pending_space = true;
            i += 1;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        out.push(b[i] as char);
        i += 1;
    }
    out
}

/// Does this statement carry a `;` anywhere but at its very end?
///
/// The test for "a client splitting on `;` would cut this in half" — see
/// [`ChangeSet::editor_script`]. Through [`sql::statement_bounds`], so a `;`
/// inside a string or a comment doesn't count.
fn needs_delimiter(stmt: &str, dialect: SqlDialect) -> bool {
    let end = stmt.trim_end().len();
    sql::statement_bounds(stmt, dialect)
        .iter()
        .any(|&b| b > 0 && b < end)
}

/// Rewrite every reference to column `from` in a CHECK predicate as `to`,
/// returning `None` when the predicate never names it.
///
/// A **token walk**, not a substring replace: `qty` must not match inside
/// `qty_total`, inside the literal `'qty'`, or inside a comment — getting that
/// wrong re-points a constraint the user never touched, or corrupts a pattern.
/// The walk is [`sql::skip_noncode`]'s, so string, comment and quoted-identifier
/// boundaries are the ones the rest of the app agrees on. A quoted identifier is
/// a *non-code* run to that scanner, so it is matched here explicitly — it is
/// the form both servers print (`` `qty` `` / `"qty"`).
///
/// Case follows the engine: MySQL/MariaDB column names are case-insensitive,
/// PostgreSQL's are exactly as written once quoted — the same split
/// [`check_exprs_equal`] makes.
///
/// The replacement is always emitted quoted. This text goes into a statement
/// Schemaic runs, and the new name is whatever the user typed in the designer.
fn repoint_check_column(expr: &str, from: &str, to: &str, dialect: SqlDialect) -> Option<String> {
    let pg = dialect == SqlDialect::Postgres;
    let quote = if pg { b'"' } else { b'`' };
    let same = |s: &str| {
        if pg {
            s == from
        } else {
            s.eq_ignore_ascii_case(from)
        }
    };
    let b = expr.as_bytes();
    let mut out = String::with_capacity(expr.len());
    let mut hit = false;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(j) = sql::skip_noncode(b, i, dialect) {
            // Only a *quoted identifier* is a name; a string or comment that
            // reads like one isn't.
            let run = &expr[i..j];
            let q = quote as char;
            // A doubled quote inside the run is one literal quote.
            let inner = (b[i] == quote && j - i >= 2 && b[j - 1] == quote)
                .then(|| run[1..run.len() - 1].replace(&format!("{q}{q}"), &q.to_string()));
            match inner {
                Some(name) if same(&name) => {
                    out.push_str(&ddl_ident_in(to, dialect));
                    hit = true;
                }
                _ => out.push_str(run),
            }
            i = j;
            continue;
        }
        if sql::is_word_start(b[i]) {
            let start = i;
            while i < b.len() && sql::is_word_byte(b[i]) {
                i += 1;
            }
            let word = &expr[start..i];
            // A word immediately followed by `(` is a function call, not a
            // column — `qty(…)` names no column even in a table with a `qty`.
            let is_call = expr[i..].trim_start().starts_with('(');
            if same(word) && !is_call {
                out.push_str(&ddl_ident_in(to, dialect));
                hit = true;
            } else {
                out.push_str(word);
            }
            continue;
        }
        let start = i;
        i += 1;
        while i < b.len() && (b[i] & 0xC0) == 0x80 {
            i += 1;
        }
        out.push_str(&expr[start..i]);
    }
    hit.then_some(out)
}

/// Fold case only over the parts that aren't quoted.
///
/// `qty > 0` and `QTY > 0` name the same column, but `status = 'a'` and
/// `status = 'A'` are different predicates — and on PostgreSQL so are `"Qty"`
/// and `"qty"`, which is why the old "no `'` anywhere ⇒ fold the whole string"
/// rule was wrong in both directions at once.
fn check_exprs_equal(x: &str, y: &str, dialect: SqlDialect) -> bool {
    let (xb, yb) = (x.as_bytes(), y.as_bytes());
    let (mut i, mut j) = (0usize, 0usize);
    loop {
        if i >= xb.len() || j >= yb.len() {
            // Both exhausted together, or one ran out first and they differ.
            // `skip_noncode` indexes `b[i]` unguarded, so this has to come
            // before either call.
            return i >= xb.len() && j >= yb.len();
        }
        let xq = sql::skip_noncode(xb, i, dialect);
        let yq = sql::skip_noncode(yb, j, dialect);
        match (xq, yq) {
            // Both at a quoted run: it has to match exactly.
            (Some(xe), Some(ye)) => {
                if x[i..xe] != y[j..ye] {
                    return false;
                }
                i = xe;
                j = ye;
            }
            (None, None) => {
                if !xb[i].eq_ignore_ascii_case(&yb[j]) {
                    return false;
                }
                i += 1;
                j += 1;
            }
            // One side opened a quoted run where the other didn't.
            _ => return false,
        }
    }
}

/// Is this `CHECK` constraint the same as that one, as far as the server is
/// concerned?
///
/// A name difference counts: neither engine can alter a constraint in place, so a
/// rename is a drop and an add either way.
///
/// Case is folded only when neither side carries a quote, the same rule
/// [`defaults_equal`] follows — `qty > 0` and `QTY > 0` name the same column, but
/// `status = 'a'` and `status = 'A'` are different predicates.
pub fn checks_equal(a: &CheckInfo, b: &CheckInfo, dialect: SqlDialect) -> bool {
    if a.name != b.name || a.enforced != b.enforced {
        return false;
    }
    let (x, y) = (
        norm_check_expr(&a.expression, dialect),
        norm_check_expr(&b.expression, dialect),
    );
    check_exprs_equal(&x, &y, dialect)
}

// ── The diff ─────────────────────────────────────────────────────────────────

/// Everything that has to happen to turn `current` into `draft`.
///
/// Diffing a table against [`TableDraft::from_table`] of itself must produce
/// nothing — that's the round-trip gate, and it's what catches a model-fidelity
/// gap before a user ever sees a phantom change.
/// Can `dialect` have its **views** edited here?
///
/// All three, now — but they don't get there the same way, which is why the two
/// predicates below exist rather than a `dialect == Postgres` at each site.
/// SQLite has neither `CREATE OR REPLACE VIEW` nor a verb that renames a view,
/// so every edit is a drop and a create; the other two replace in place and
/// rename with a statement.
pub fn supports_view_editing(_dialect: SqlDialect) -> bool {
    true
}

/// Can `dialect` redefine a view **in place**, with `CREATE OR REPLACE VIEW`?
///
/// MySQL replaces anything and PostgreSQL replaces what it can append to
/// ([`pg_replaceable`]). SQLite has no form of the statement at all, so a
/// redefinition there is a `DROP` plus a `CREATE` — the same arm PostgreSQL
/// already takes when a replace won't do, reached unconditionally instead of on
/// a body test.
pub fn supports_or_replace_view(dialect: SqlDialect) -> bool {
    !matches!(dialect, SqlDialect::Sqlite)
}

/// Can `dialect` rename a view with a statement, leaving its body alone?
///
/// PostgreSQL has `ALTER VIEW … RENAME TO` and MySQL renames one with
/// `RENAME TABLE`. SQLite has neither: `ALTER VIEW` isn't a statement there, and
/// `ALTER TABLE v RENAME TO …` refuses a view outright — *"view v may not be
/// altered"*. A rename there rides along with the re-create every edit already
/// performs, which is why [`diff_view`] treats a bare rename as a redefinition.
pub fn supports_view_rename(dialect: SqlDialect) -> bool {
    !matches!(dialect, SqlDialect::Sqlite)
}

/// Can `dialect` have its **triggers** edited here?
///
/// All three. SQLite was the holdout, and the thing that had to come first was
/// the *reader*, not the emitter: it keeps no catalogue of a trigger's parts, so
/// until [`sqlite_trigger_info`] could parse `sqlite_master`'s `CREATE TRIGGER`
/// text into [`crate::schema::TriggerInfo`], the list was empty — and an editor
/// over an empty list shows a table's triggers as gone and offers to "add" one
/// that already exists.
///
/// [`crate::schema::TableInfo::dependent_ddl`] still holds the same statements
/// verbatim, and still is what a rebuild replays. The two are not redundant: the
/// model is what the *editor* diffs, and the text is what a table rebuild puts
/// back without depending on the parse being perfect.
pub fn supports_trigger_editing(_dialect: SqlDialect) -> bool {
    true
}

/// Is this the change that performs a whole set by rebuilding the table?
fn is_rebuild(c: &Change) -> bool {
    matches!(c, Change::RebuildTable(_))
}

/// Can SQLite add this column with its own `ALTER TABLE … ADD COLUMN`, instead
/// of by rebuilding the table around it?
///
/// Appending a column is the most common designer edit there is, and SQLite
/// performs it instantly — copying the whole table to achieve it was correct and
/// absurd. But the engine's restrictions are **narrow and unforgiving**, and
/// each one is a way to write the plan that half-applies: the fast path is
/// taken, the engine refuses the statement, and the edit the preview promised is
/// simply gone. So this answers `false` for anything it isn't sure of — a
/// needless rebuild is slow, a wrong fast path is a lie.
///
/// The rules, each measured against SQLite 3.46 rather than read off the
/// grammar, with the engine's own wording where it has some:
///
/// * **`position` must be empty.** `ADD COLUMN` always appends; a column the
///   user dropped into the middle carries one, and taking the fast path there
///   leaves the designer showing one order and the table having another. This is
///   the rule with no error message behind it — the statement *succeeds*, in the
///   wrong place.
/// * **No primary key** — *"Cannot add a PRIMARY KEY column"*.
/// * **No counter.** `AUTOINCREMENT` is legal only spelled inline as `INTEGER
///   PRIMARY KEY AUTOINCREMENT`, so [`ColumnInfo::definition_sql`] drops it for
///   SQLite; a native add would silently lose it where the rebuild's table
///   builder can place it.
/// * **A constant default**, if any — *"Cannot add a column with non-constant
///   default"*.
/// * **`NOT NULL` needs a non-null default** — *"Cannot add a NOT NULL column
///   with default value NULL"*.
///
/// Two things deliberately absent. **Uniqueness** isn't on [`ColumnInfo`] at
/// all: it arrives as an index, which has no native arm in [`supports_change`]
/// and so takes the set back to a rebuild on its own. That is a fact about this
/// gate, *not* about SQLite — what the engine refuses is an inline `UNIQUE` in
/// the column definition, and there is none to emit, so a native add followed by
/// a `CREATE UNIQUE INDEX` would be two legal statements the day that arm
/// exists. And a **generated** column is addable — the
/// emitter writes no `VIRTUAL`/`STORED` keyword, so SQLite's own default
/// (`VIRTUAL`) applies, and it is `STORED` that the engine refuses. A generated
/// column also carries its expression *instead of* a default, so the null-default
/// rule has nothing to reach.
fn sqlite_native_add(column: &ColumnInfo, position: Option<&Position>) -> bool {
    if position.is_some() || column.primary_key || column.auto_increment {
        return false;
    }
    if column.generated.is_some() {
        return true;
    }
    let default = column.default.as_deref().map(str::trim);
    if let Some(d) = default
        && !sqlite_constant_default(d)
    {
        return false;
    }
    if !column.nullable && !default.is_some_and(|d| !d.eq_ignore_ascii_case("NULL")) {
        return false;
    }
    true
}

/// Is `default` a *constant* in SQLite's sense — something `ADD COLUMN` will
/// take?
///
/// Two forms are not: the `CURRENT_*` keywords, and anything parenthesised
/// (which covers a bare `now()` too — not a legal `DEFAULT` there at all).
/// Everything else a column can carry is a literal and is accepted, which the
/// engine confirms for signed numbers, blob literals, `TRUE`/`FALSE` and even a
/// bare word.
///
/// The parenthesis is looked for at a **code** position through the shared
/// lexer, not with a `contains('(')`: a default of `'a (b)'` is a string whose
/// parens are data, and rejecting it would send an ordinary edit the long way
/// round.
fn sqlite_constant_default(default: &str) -> bool {
    let d = default.trim();
    if ["CURRENT_TIME", "CURRENT_DATE", "CURRENT_TIMESTAMP"]
        .iter()
        .any(|k| d.eq_ignore_ascii_case(k))
    {
        return false;
    }
    let b = d.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(j) = sql::skip_noncode(b, i, SqlDialect::Sqlite) {
            i = j.max(i + 1);
            continue;
        }
        if b[i] == b'(' {
            return false;
        }
        i += 1;
    }
    true
}

/// What a rebuild needs: the table as it is, and as it should be.
///
/// Both sides, because the copy is the whole point — the new table comes from
/// the draft, and which of its columns takes which of the old one's data can
/// only be answered by looking at both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rebuild {
    pub current: TableInfo,
    pub draft: TableDraft,
}

/// The suffix the shadow table carries while a rebuild is in flight. It exists
/// only between the `CREATE` and the `RENAME`, both inside one transaction, so
/// nothing ever sees it — but a name that collided with a real table would fail
/// the whole plan, so it is deliberately not the `new_X` the SQLite manual uses
/// in its example.
const REBUILD_SUFFIX: &str = "_schemaic_rebuild";

/// The twelve-step rebuild, as statements: the only way to change most of a
/// SQLite table.
///
/// Its `ALTER TABLE` does `RENAME TABLE`, `RENAME COLUMN`, `ADD COLUMN` and
/// `DROP COLUMN`. Everything else — a retype, a reorder, a key, a constraint —
/// has to be done by building the table you wanted, moving the rows into it, and
/// putting it where the old one was. The order is not negotiable and each step
/// is destructive on its own, which is why this is one function with one test
/// suite rather than a shape assembled at each call site:
///
/// 1. **create** the shadow table from `draft`, under a name nothing else holds;
/// 2. **copy** the rows, column by column, mapping each new column to the old
///    one it came from — that mapping is what makes a rename a rename rather
///    than a drop and an add;
/// 3. **drop** the original, which takes its indexes and triggers with it;
/// 4. **rename** the shadow into its place;
/// 5. **recreate** the indexes, *after* the rename — an index name is unique per
///    schema in SQLite, so creating one before the old table is gone collides
///    with the index it is replacing;
/// 6. **replay** [`TableInfo::dependent_ddl`], the `CREATE` text of the triggers
///    that hung off the table and went down with it.
///
/// The rows move with `INSERT … SELECT`, not `CREATE TABLE … AS SELECT`, because
/// the latter takes its column types from the query rather than from the
/// declaration and would quietly discard every constraint on the new table.
///
/// **A rename of the table itself is not part of this.** The rebuild always ends
/// under the original name, and `ALTER TABLE … RENAME TO` is emitted after it —
/// that statement is native, and letting SQLite perform it is what keeps the
/// references in other tables' foreign keys pointing at the right place.
/// One SQLite trigger, read out of the `CREATE TRIGGER` text `sqlite_master`
/// stores. `None` for anything that isn't a readable `CREATE TRIGGER`.
///
/// SQLite keeps **no catalogue of a trigger's parts** — there is no
/// `information_schema.TRIGGERS` and no pragma, only the statement — so this is
/// the one engine where introspection is a parse, and it is the thing that had
/// to exist before a trigger editor could be offered at all
/// ([`supports_trigger_editing`]). It is also why the rebuild keeps replaying
/// [`TableInfo::dependent_ddl`] verbatim rather than re-emitting from this
/// model: a table rebuild must not depend on the parse being perfect.
///
/// **Structure from the AST, body from the text.** The per-dialect parser
/// ([`crate::intel`]) answers what the timing, events, `UPDATE OF` columns and
/// `WHEN` guard are — questions a scanner gets wrong on a trigger whose body
/// contains the same words. The **body** is then taken verbatim from the
/// original text, because re-printing it from the AST normalises away the
/// user's comments, casing and line breaks, and a body that comes back
/// different is a phantom change on every open and a rewritten trigger on every
/// apply.
pub fn sqlite_trigger_info(create_sql: &str) -> Option<TriggerInfo> {
    use sqlparser::ast::{Statement, TriggerEvent as PEvent, TriggerPeriod};

    let d = SqlDialect::Sqlite;
    let mut stmts = sqlparser::parser::Parser::parse_sql(&*d.parser(), create_sql).ok()?;
    let Statement::CreateTrigger(t) = stmts.pop()? else {
        return None;
    };

    // `Ident::value` is the name with its quoting already removed, which is the
    // bare form the model holds and the emitter quotes again.
    let name = t.name.0.last()?.as_ident()?.value.clone();
    let table = t.table_name.0.last()?.as_ident()?.value.clone();

    // SQLite's timing is optional and defaults to `BEFORE`. Falling back to the
    // enum's own default would say `Before` too, but only by coincidence — this
    // is the engine's rule, written down.
    let timing = match t.period {
        Some(TriggerPeriod::After) => TriggerTiming::After,
        Some(TriggerPeriod::InsteadOf) => TriggerTiming::InsteadOf,
        Some(TriggerPeriod::Before) | None => TriggerTiming::Before,
        // `FOR` is a period no SQLite trigger has; refuse rather than file it
        // under a timing the statement didn't say.
        Some(TriggerPeriod::For) => return None,
    };

    let mut events = Vec::new();
    let mut update_columns = Vec::new();
    for e in &t.events {
        events.push(match e {
            PEvent::Insert => TriggerEvent::Insert,
            PEvent::Delete => TriggerEvent::Delete,
            PEvent::Update(cols) => {
                update_columns = cols.iter().map(|c| c.value.clone()).collect();
                TriggerEvent::Update
            }
            // SQLite has no TRUNCATE trigger; a statement claiming one isn't one
            // of its own.
            PEvent::Truncate => return None,
        });
    }
    if events.is_empty() {
        return None;
    }

    Some(TriggerInfo {
        name,
        schema: None,
        table,
        timing,
        events,
        update_columns,
        // SQLite has only row-level triggers — `FOR EACH STATEMENT` is a syntax
        // error there, and `FOR EACH ROW` is accepted but says nothing.
        level: TriggerLevel::Row,
        condition: t
            .condition
            .as_ref()
            .map(|c| peel_parens(&c.to_string(), d).to_string()),
        action: TriggerAction::Body(sqlite_trigger_body(create_sql)?),
        // Everything below belongs to one of the other two engines. Left `None`
        // rather than guessed, so the round-trip gate stays meaningful.
        definer: None,
        order: None,
        sql_mode: None,
        charset_client: None,
        collation_connection: None,
        old_table: None,
        new_table: None,
        enabled: crate::schema::TriggerEnabled::default(),
        constraint: false,
    })
}

/// A SQLite trigger's body — the `BEGIN … END` block — taken verbatim from the
/// statement that declares it.
///
/// The body starts at the first `BEGIN` at a **code** position and paren depth
/// zero, both qualifications carrying the weight they do in `db::sqlite`'s
/// `view_body_of`: the header's `WHEN` guard may hold a string or a quoted
/// identifier spelling `begin`, and the shared lexer is what sees through them.
/// Everything from there to the end of the statement is the block, `END`
/// included — SQLite stores one statement per row, so its last token is the
/// block's own terminator.
fn sqlite_trigger_body(create_sql: &str) -> Option<String> {
    use crate::sql::{is_word_byte, is_word_start, skip_noncode};

    let b = create_sql.as_bytes();
    let mut i = 0usize;
    let mut depth = 0i32;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, SqlDialect::Sqlite) {
            i = j.max(i + 1);
            continue;
        }
        match b[i] {
            b'(' => {
                depth += 1;
                i += 1;
                continue;
            }
            b')' => {
                depth -= 1;
                i += 1;
                continue;
            }
            _ => {}
        }
        if !is_word_start(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 1;
        while end < b.len() && is_word_byte(b[end]) {
            end += 1;
        }
        if depth == 0 && create_sql[start..end].eq_ignore_ascii_case("BEGIN") {
            let body = create_sql[start..].trim().trim_end_matches(';').trim_end();
            return (!body.is_empty()).then(|| body.to_string());
        }
        i = end;
    }
    None
}

pub fn sqlite_rebuild_sql(current: &TableInfo, draft: &TableDraft) -> Vec<String> {
    let d = SqlDialect::Sqlite;
    let q = |s: &str| ddl_ident_in(s, d);
    let original = qualified(&current.name, current.schema.as_deref(), d);
    let shadow_name = format!("{}{REBUILD_SUFFIX}", current.name);
    let shadow = qualified(&shadow_name, current.schema.as_deref(), d);

    // The table to build: the draft, under the shadow name and without its
    // indexes, which are created against the real table further down.
    let mut build = draft.clone();
    build.name = shadow_name;
    build.indexes.clear();
    let mut out = create_table_sql(&build, d);

    // Which new column takes which old one's data. A column the user added has
    // no `original` and is left out of both lists so its default applies, and a
    // generated column is left out because it cannot be inserted into — it is
    // computed from the rows this statement moves.
    let live: HashSet<&str> = current.columns.iter().map(|c| c.name.as_str()).collect();
    let (into, from): (Vec<String>, Vec<String>) = draft
        .columns
        .iter()
        .filter(|c| c.info.generated.is_none())
        .filter_map(|c| {
            let was = c.original.as_deref().filter(|o| live.contains(o))?;
            Some((q(&c.info.name), q(was)))
        })
        .unzip();
    if !into.is_empty() {
        out.push(format!(
            "INSERT INTO {shadow} ({}) SELECT {} FROM {original};",
            into.join(", "),
            from.join(", ")
        ));
    }

    // **Without this the rename fails on any view over the table.** From 3.25
    // SQLite re-parses every view and trigger during `ALTER TABLE … RENAME` so
    // it can update their references — and by this point the original table is
    // already gone, so a view selecting from it resolves to nothing and the
    // whole rebuild dies on `error in view v: no such table: main.t`. The legacy
    // behaviour is the right one here precisely *because* nothing should be
    // rewritten: the table is coming back under the name it had, so every
    // reference to it is already correct.
    //
    // It rides in the plan rather than in the backend because it is a property
    // of these statements and not of the connection — and because the preview
    // then shows the whole procedure, which is the honest thing to put in front
    // of someone about to approve it.
    out.push("PRAGMA legacy_alter_table = ON;".to_string());
    out.push(format!("DROP TABLE {original};"));
    out.push(format!(
        "ALTER TABLE {shadow} RENAME TO {};",
        q(&current.name)
    ));
    out.push("PRAGMA legacy_alter_table = OFF;".to_string());
    for ix in &draft.indexes {
        out.push(create_index_sql(&ix.info, &original, d));
    }
    out.extend(current.dependent_ddl.iter().cloned());
    out
}

/// Can `dialect` express this one change as SQL [`ChangeSet::emit`] writes?
///
/// It asks about a change **on its own** — a context-menu shortcut, which has a
/// change and no draft to build a table from. `DROP TABLE` and `DROP VIEW` are
/// the same statement on every engine, `DROP INDEX` is a standalone statement
/// SQLite has, and dropping a column is one of the four things its `ALTER TABLE`
/// does; those shortcuts work there and are worth keeping, because hiding them
/// would take away something the engine genuinely performs.
///
/// A *plan* is a different question. [`diff`] has both sides of the edit, so it
/// can answer anything by rebuilding the table — which is why
/// [`Change::RebuildTable`] is on this list, and why a designer edit is not
/// limited to what one shortcut could raise.
///
/// Everything else is `false` on SQLite, and each `false` is the twelve-step
/// rebuild in disguise: a foreign key or a constraint-backed index can only
/// come off by recreating the table around it.
///
/// **This is the gate now that `run_ddl` executes a SQLite plan instead of
/// refusing every one.** The menus consult it so an entry that can't work is
/// absent, and the emitter honours it so a change that slipped through emits
/// nothing rather than MySQL's spelling of it.
pub fn supports_change(dialect: SqlDialect, change: &Change) -> bool {
    if dialect != SqlDialect::Sqlite {
        return true;
    }
    // The one change whose answer depends on what it *contains* rather than on
    // what kind it is: SQLite adds a column natively, but only a column that
    // meets every one of its restrictions — see [`sqlite_native_add`].
    if let Change::AddColumn { column, position } = change {
        return sqlite_native_add(column, position.as_ref());
    }
    matches!(
        change,
        Change::DropTable
            | Change::DropView {
                materialized: false
            }
            | Change::DropColumn { .. }
            | Change::DropIndex {
                constraint: None,
                ..
            }
            // The rebuild, which performs a whole set of table changes that have
            // no statement of their own. `diff` is what puts one in a set; a
            // context-menu shortcut has no draft to build from, which is why the
            // changes it can raise on their own are still the four above.
            | Change::RebuildTable(_)
            // Views. SQLite creates and drops them like anyone else; what it has
            // no form of is replacing one in place or renaming it, and
            // `diff_view` resolves both into a drop plus a create before they
            // reach here ([`supports_or_replace_view`], [`supports_view_rename`]).
            | Change::CreateView(_)
            | Change::ReplaceView { .. }
            // Triggers. SQLite has `CREATE TRIGGER` and `DROP TRIGGER` and no
            // form that alters one, which is the same drop-and-create every
            // engine here already performs — see [`supports_trigger_editing`]
            // for what had to exist first.
            | Change::CreateTrigger(_)
            | Change::ReplaceTrigger { .. }
            | Change::DropTrigger { .. }
    )
}

pub fn diff(current: &TableInfo, draft: &TableDraft, target: impl Into<Target>) -> ChangeSet {
    let Target { dialect, flavour } = target.into();
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
                        // Filled in below, once the check diff is known.
                        inline_check: None,
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

    // CHECK constraints, on the same drop-and-recreate rule — neither engine can
    // alter one in place.
    //
    // Deliberately *not* re-pointed through `renamed` the way a foreign key's
    // column list is. A check's predicate is an expression, not a list of names,
    // and each engine already answers the rename question itself: PostgreSQL
    // stores the parse tree, so `RENAME COLUMN` rewrites every check that
    // references it and the next introspection reads the new name; MySQL stores
    // the text and *refuses* to rename a column a check depends on. So a rewrite
    // here would be either redundant or a way to emit SQL the server rejects.
    let ck_claimed: HashSet<&str> = draft
        .check_constraints
        .iter()
        .filter_map(|c| c.original.as_deref())
        .collect();
    let mut dropped_ck: Vec<String> = Vec::new();
    let mut added_ck: Vec<CheckInfo> = Vec::new();
    for ck in &current.check_constraints {
        if !ck_claimed.contains(ck.name.as_str()) {
            dropped_ck.push(ck.name.clone());
        }
    }
    for d in &draft.check_constraints {
        let cur = d
            .original
            .as_deref()
            .and_then(|n| current.check_constraints.iter().find(|c| c.name == n));
        match cur {
            Some(ck) if checks_equal(ck, &d.info, dialect) => {}
            Some(ck) => {
                dropped_ck.push(ck.name.clone());
                added_ck.push(d.info.clone());
            }
            None => added_ck.push(d.info.clone()),
        }
    }
    for name in dropped_ck {
        changes.push(Change::DropCheck { name });
    }
    for ck in added_ck {
        changes.push(Change::AddCheck(Box::new(ck)));
    }

    // What a column clause does to the checks standing on that column — and the
    // two servers Schemaic calls one dialect do opposite things, so this is the
    // one place in the emitter driven by `flavour` rather than `dialect`.
    // PostgreSQL needs neither arm: it rewrites its own stored parse tree.
    //
    // A constraint the draft already changed is left alone in both arms. The
    // user's edit is the authority there, and touching it twice would either
    // duplicate the statement or overwrite what they typed.
    if dialect != SqlDialect::Postgres {
        let touched: HashSet<String> = changes
            .iter()
            .filter_map(|c| match c {
                Change::DropCheck { name } => Some(name.clone()),
                Change::AddCheck(ck) => Some(ck.name.clone()),
                _ => None,
            })
            .collect();
        if flavour.is_mariadb() {
            // **MariaDB.** A column-level check is part of the column
            // definition, so `MODIFY`/`CHANGE COLUMN` deletes it unless the
            // clause restates it — measured on 10.11.14, including a bare
            // rename. It has no name of its own (MariaDB names it after the
            // column and renames it along with the column), so a rename
            // re-points the predicate too: `CHANGE COLUMN q qty bigint
            // CHECK (q > 0)` is `ERROR 1054 Unknown column 'q'`.
            for c in changes.iter_mut() {
                let Change::AlterColumn {
                    from,
                    to,
                    inline_check,
                    ..
                } = c
                else {
                    continue;
                };
                let Some(ck) = current.check_constraints.iter().find(|ck| {
                    ck.column_level
                        && ck.name.eq_ignore_ascii_case(&from.name)
                        && !touched.contains(&ck.name)
                }) else {
                    continue;
                };
                let mut ck = ck.clone();
                if from.name != to.name {
                    ck.name = to.name.clone();
                    if let Some(e) =
                        repoint_check_column(&ck.expression, &from.name, &to.name, dialect)
                    {
                        ck.expression = e;
                    }
                }
                *inline_check = Some(Box::new(ck));
            }
            // The other direction: a column-level check the draft **removed or
            // edited** can't come off with `DROP CONSTRAINT` either — MariaDB
            // can't address one by name (`ERROR 1091 … check that it exists`).
            // The only way to take it off is to restate the column without the
            // clause, so the plan swaps that drop for a column clause. An edit
            // then lands as an ordinary `ADD CONSTRAINT`, i.e. as a table-level
            // constraint, which is the only kind `ALTER TABLE` can add.
            let dropped_inline: Vec<String> = changes
                .iter()
                .filter_map(|c| match c {
                    Change::DropCheck { name } => current
                        .check_constraints
                        .iter()
                        .find(|ck| ck.column_level && ck.name == *name)
                        .map(|ck| ck.name.clone()),
                    _ => None,
                })
                .collect();
            changes.retain(
                |c| !matches!(c, Change::DropCheck { name } if dropped_inline.contains(name)),
            );
            for name in &dropped_inline {
                // MariaDB names a column-level check after its column, and the
                // syntax gives no way to name it anything else.
                let Some(cur) = current
                    .columns
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(name))
                else {
                    continue;
                };
                // A column on its way out takes its check with it.
                if changes
                    .iter()
                    .any(|c| matches!(c, Change::DropColumn { name, .. } if *name == cur.name))
                {
                    continue;
                }
                if changes
                    .iter()
                    .any(|c| matches!(c, Change::AlterColumn { from, .. } if from.name == cur.name))
                {
                    continue;
                }
                let to = draft
                    .columns
                    .iter()
                    .find(|c| c.original.as_deref() == Some(cur.name.as_str()))
                    .map(|c| c.info.clone())
                    .unwrap_or_else(|| cur.clone());
                changes.push(Change::AlterColumn {
                    from: Box::new(cur.clone()),
                    to: Box::new(to),
                    position: None,
                    inline_check: None,
                });
            }
        } else {
            // **MySQL 8.** It refuses to rename a column any check names at all
            // — `ERROR 3959 … hence column cannot be dropped or renamed` — so
            // the rename only goes through if the constraint comes off first and
            // back on after, which the emitter already orders that way. Measured
            // live: the drop, the `CHANGE COLUMN` and the add run in one
            // `ALTER TABLE`.
            let renames: Vec<(String, String)> = changes
                .iter()
                .filter_map(|c| match c {
                    Change::AlterColumn { from, to, .. } if from.name != to.name => {
                        Some((from.name.clone(), to.name.clone()))
                    }
                    _ => None,
                })
                .collect();
            let mut pairs: Vec<CheckInfo> = Vec::new();
            for ck in &current.check_constraints {
                if touched.contains(&ck.name) {
                    continue;
                }
                let mut expr = ck.expression.clone();
                let mut hit = false;
                for (from, to) in &renames {
                    if let Some(e) = repoint_check_column(&expr, from, to, dialect) {
                        expr = e;
                        hit = true;
                    }
                }
                if hit {
                    pairs.push(CheckInfo {
                        expression: expr,
                        ..ck.clone()
                    });
                }
            }
            for ck in pairs {
                changes.push(Change::DropCheck {
                    name: ck.name.clone(),
                });
                changes.push(Change::AddCheck(Box::new(ck)));
            }
        }
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

    // **On SQLite, anything the engine can't do with a statement of its own is
    // done by rebuilding the table**, and the rebuild performs the whole set at
    // once — it writes the table the draft describes, so every change in the
    // list is already in it.
    //
    // The test is "is there something here SQLite has no statement for", not
    // "is this an alter": a set of nothing but the statements it does have keeps
    // its direct path and pays nothing.
    //
    // **This stays a question about the whole set, not about each change.** An
    // `ADD COLUMN` that `sqlite_native_add` calls native still rides the rebuild
    // when anything beside it needs one — the rebuild writes the table the draft
    // describes, so the column is already in it, and emitting the `ADD COLUMN`
    // as well would add it twice.
    if dialect == SqlDialect::Sqlite
        && !changes.is_empty()
        && changes.iter().any(|c| !supports_change(dialect, c))
    {
        changes.insert(
            0,
            Change::RebuildTable(Box::new(Rebuild {
                current: current.clone(),
                draft: draft.clone(),
            })),
        );
    }

    ChangeSet {
        table: current.name.clone(),
        schema: current.schema.clone(),
        dialect,
        flavour,
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
                // A move is a `MODIFY COLUMN` too, so it destroys a MariaDB
                // column-level check exactly as a retype does. `diff` fills this
                // in for every `AlterColumn` it ends up with, this one included.
                inline_check: None,
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

    // A rename is a redefinition on an engine with no verb for it: SQLite gets
    // the new name out of the `CREATE` half of the re-create, so a bare rename
    // has to take that path rather than fall through to `RenameView` below.
    let redefined = view_body(&draft.select) != old_body || draft.options != old_options;
    if redefined || (renamed && !supports_view_rename(dialect)) {
        // MySQL's `CREATE OR REPLACE VIEW` redefines anything, so among the
        // engines that *have* the statement the question — and the override —
        // is PostgreSQL's. SQLite doesn't have it and always re-creates.
        let recreate = !supports_or_replace_view(dialect)
            || (dialect == SqlDialect::Postgres
                && (draft.force_recreate || {
                    let cols: Vec<String> =
                        current.columns.iter().map(|c| c.name.clone()).collect();
                    pg_replaceable(&cols, &draft.select, dialect) == Some(false)
                }));
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
        flavour: ServerFlavour::Unknown,
        changes,
    }
}

/// Everything that has to happen to turn the trigger `current` into `draft`.
///
/// Same round-trip gate as [`diff`] and [`diff_view`]: a trigger diffed against
/// its own draft must produce nothing.
///
/// There is no partial edit to detect here, and that is the whole design.
/// Neither engine can alter a trigger, so *any* difference — the name included —
/// costs the same drop-and-create, and splitting a rename out into its own
/// change would be a distinction with no consequence.
pub fn diff_trigger(current: &TriggerInfo, draft: &TriggerDraft, dialect: SqlDialect) -> ChangeSet {
    let changes = if draft.info == *current {
        Vec::new()
    } else {
        vec![Change::ReplaceTrigger {
            draft: Box::new(draft.clone()),
        }]
    };
    ChangeSet {
        table: current.table.clone(),
        schema: current.schema.clone(),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes,
    }
}

/// Everything that has to happen to turn a table's triggers `current` into
/// `draft` — the whole set in one plan.
///
/// Same round-trip gate as [`diff`]: a set diffed against its own draft must
/// produce nothing.
///
/// **Drops are emitted first**, before any create or re-create. A user who
/// deletes one trigger and names a new one after it is doing something the
/// server would otherwise refuse — the name is still taken when the create runs
/// — and there is no reason to make them apply it in two passes.
pub fn diff_triggers(
    current: &[TriggerInfo],
    draft: &TriggerSetDraft,
    dialect: SqlDialect,
) -> ChangeSet {
    let mut changes: Vec<Change> = Vec::new();
    for cur in current {
        let kept = draft
            .triggers
            .iter()
            .any(|d| d.original.as_deref() == Some(cur.name.as_str()));
        if !kept {
            changes.push(Change::DropTrigger {
                name: cur.name.clone(),
            });
        }
    }
    for d in &draft.triggers {
        match d
            .original
            .as_deref()
            .and_then(|n| current.iter().find(|c| c.name == n))
        {
            // Unchanged: no statement. This is what the gate rests on.
            Some(cur) if d.info == *cur => {}
            Some(_) => changes.push(Change::ReplaceTrigger {
                draft: Box::new(d.clone()),
            }),
            // Either genuinely new, or naming a server trigger that has since
            // gone. Emitting a create either way lets the server be the one to
            // say so, rather than guessing from a stale schema.
            None => changes.push(Change::CreateTrigger(Box::new(d.clone()))),
        }
    }
    ChangeSet {
        table: draft.table.clone(),
        schema: draft.schema.clone(),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes,
    }
}

/// The `CREATE TRIGGER` for a brand-new trigger.
pub fn create_trigger(draft: &TriggerDraft, dialect: SqlDialect) -> ChangeSet {
    ChangeSet {
        table: draft.info.table.clone(),
        schema: draft.info.schema.clone(),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes: vec![Change::CreateTrigger(Box::new(draft.clone()))],
    }
}

/// The `DROP TRIGGER` for one trigger.
pub fn drop_trigger(t: &TriggerInfo, dialect: SqlDialect) -> ChangeSet {
    ChangeSet {
        table: t.table.clone(),
        schema: t.schema.clone(),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes: vec![Change::DropTrigger {
            name: t.name.clone(),
        }],
    }
}

/// Everything that has to happen to turn the function `current` into `draft`.
///
/// Same round-trip gate as [`diff`]: a function diffed against its own draft
/// must produce nothing.
///
/// Unlike a trigger, a rename is its own change — PostgreSQL renames a function
/// in place with `ALTER FUNCTION … RENAME TO`, and every trigger bound to it
/// keeps working, so there is no reason to pay for a drop-and-create.
pub fn diff_function(
    current: &RoutineInfo,
    draft: &FunctionDraft,
    dialect: SqlDialect,
) -> ChangeSet {
    let mut changes: Vec<Change> = Vec::new();
    let renamed = draft.info.name != current.name && !draft.info.name.trim().is_empty();
    // Compare everything *except* the name, which the rename below owns.
    let mut same_name = draft.info.clone();
    same_name.name = current.name.clone();
    if same_name != *current {
        let mut d = draft.clone();
        // The replace addresses the server's signature; the rename runs after.
        d.info.name = current.name.clone();
        d.original = Some(current.name.clone());
        changes.push(Change::ReplaceFunction(Box::new(d)));
    }
    if renamed {
        changes.push(Change::RenameFunction {
            from: Box::new(current.clone()),
            to: draft.info.name.clone(),
        });
    }
    ChangeSet {
        table: current.name.clone(),
        schema: current.schema.clone(),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes,
    }
}

/// The `CREATE FUNCTION` for a brand-new function.
pub fn create_function(draft: &FunctionDraft, dialect: SqlDialect) -> ChangeSet {
    ChangeSet {
        table: draft.info.name.clone(),
        schema: draft.info.schema.clone(),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes: vec![Change::CreateFunction(Box::new(draft.clone()))],
    }
}

/// The `DROP FUNCTION` for one function.
pub fn drop_function(f: &RoutineInfo, dialect: SqlDialect) -> ChangeSet {
    ChangeSet {
        table: f.name.clone(),
        schema: f.schema.clone(),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes: vec![Change::DropFunction(Box::new(f.clone()))],
    }
}

/// The `CREATE VIEW` for a brand-new view.
pub fn create_view(draft: &ViewDraft, dialect: SqlDialect) -> ChangeSet {
    ChangeSet {
        table: draft.name.clone(),
        schema: draft.schema.clone(),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes: vec![Change::CreateView(Box::new(draft.clone()))],
    }
}

/// The `CREATE TABLE` for a brand-new table.
pub fn create(draft: &TableDraft, dialect: SqlDialect) -> ChangeSet {
    ChangeSet {
        table: draft.name.clone(),
        schema: draft.schema.clone(),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes: vec![Change::CreateTable(Box::new(draft.clone()))],
    }
}

// ── Standalone objects ───────────────────────────────────────────────────────

/// Everything that has to happen to turn the enum `current` into `draft`.
///
/// The shape of this diff is dictated by what PostgreSQL can and can't do.
/// Appending or inserting a value is `ADD VALUE`, and renaming one is `RENAME
/// VALUE` — both in place, both cheap. **Removing or reordering is neither**:
/// there is no `DROP VALUE` and no way to move one, so the moment the draft
/// implies either, the whole edit collapses into a single
/// [`Change::RecreateEnum`] rather than a mixture. A plan that added a value and
/// then rebuilt the type around it would do the first half twice.
///
/// Same round-trip gate as [`diff`]: an enum diffed against its own draft must
/// produce nothing.
pub fn diff_enum(
    current: &EnumInfo,
    draft: &EnumDraft,
    dependents: &[TypeDependent],
    dialect: SqlDialect,
) -> ChangeSet {
    let mut changes = Vec::new();
    let new = &draft.info;
    match enum_value_plan(&current.values, &new.values) {
        Some(steps) => changes.extend(steps),
        // Removed or reordered: no `ALTER` reaches it, so rebuild.
        None => changes.push(Change::RecreateEnum {
            info: Box::new(EnumInfo {
                // The rebuild creates the type under the name the server knows;
                // any rename is the separate change below, so the two don't have
                // to agree about which name the recast columns point at.
                name: current.name.clone(),
                schema: current.schema.clone(),
                values: new.values.clone(),
                comment: new.comment.clone(),
            }),
            dependents: dependents.to_vec(),
        }),
    }
    // A rebuild restates the comment itself, so setting it again would be a
    // second statement saying the same thing.
    let rebuilt = matches!(changes.first(), Some(Change::RecreateEnum { .. }));
    if !rebuilt && current.comment != new.comment {
        changes.push(Change::SetObjectComment {
            kind: ObjectKind::Enum,
            comment: new.comment.clone(),
        });
    }
    if new.name != current.name && !new.name.trim().is_empty() {
        changes.push(Change::RenameObject {
            kind: ObjectKind::Enum,
            to: new.name.clone(),
        });
    }
    ChangeSet {
        table: current.name.clone(),
        schema: current.schema.clone(),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes,
    }
}

/// How to get from the value list `current` to `want` using only `ADD VALUE` and
/// `RENAME VALUE`, or `None` when no sequence of those can do it.
///
/// `None` is the answer whenever a value is **removed or reordered**, because
/// PostgreSQL offers neither operation — the caller's cue to rebuild the type.
///
/// A rename is recognised positionally: with the surviving values lined up in
/// order, a slot whose text changed is the same value under a new label, and one
/// PostgreSQL rewrites without touching a row. Each insertion anchors on the
/// value **before it in the draft**, which by the time the statement runs already
/// exists — so a run of new values inserted together arrives in the order the
/// list shows, rather than all landing on the same anchor in reverse.
fn enum_value_plan(current: &[String], want: &[String]) -> Option<Vec<Change>> {
    // Every original value has to survive somewhere, in its original order. The
    // ones that don't move are matched by position among the *kept* values, so a
    // rename is a slot that changed text rather than a drop plus an add.
    if want.len() < current.len() {
        return None;
    }
    // Which draft slots stand for the values already on the server: the first
    // `current.len()` slots that aren't brand-new insertions. Rather than guess,
    // walk the draft and greedily match each original value by name; whatever is
    // left over in order is a rename candidate.
    let mut kept: Vec<Option<usize>> = Vec::with_capacity(current.len());
    let mut next = 0usize;
    for c in current {
        // Look for this exact value at or after the cursor — anything before it
        // would mean the order changed.
        match want[next..].iter().position(|w| w == c) {
            Some(off) => {
                kept.push(Some(next + off));
                next += off + 1;
            }
            None => kept.push(None),
        }
    }
    // An unmatched original is either a rename or a removal. It is a rename only
    // if a draft slot is free at the position the value held relative to its
    // matched neighbours; anything else is a removal, and rebuilds.
    let mut renames = Vec::new();
    let mut taken: Vec<usize> = kept.iter().flatten().copied().collect();
    for (i, slot) in kept.iter().enumerate() {
        if slot.is_some() {
            continue;
        }
        let lower = kept[..i].iter().flatten().max().map(|x| x + 1).unwrap_or(0);
        let upper = kept[i + 1..]
            .iter()
            .flatten()
            .min()
            .copied()
            .unwrap_or(want.len());
        // No free slot where this value sat ⇒ it was removed, not renamed, and
        // no `ALTER` can express that.
        let s = (lower..upper).find(|s| !taken.contains(s))?;
        taken.push(s);
        renames.push(Change::RenameEnumValue {
            from: current[i].clone(),
            to: want[s].clone(),
        });
    }
    // Everything the draft holds that no original claimed is an insertion.
    //
    // The slots that already exist on the server — kept values and rename
    // targets — before any of these statements run. A head insertion has to
    // anchor on one of *these*, not on whatever happens to sit next to it.
    let surviving = taken.clone();
    let mut out = renames;
    for (s, v) in want.iter().enumerate() {
        if taken.contains(&s) {
            continue;
        }
        out.push(Change::AddEnumValue {
            value: v.clone(),
            // Anchor on the value before it, which by then exists: it is either
            // original or was added by an earlier statement in this same loop,
            // which walks the slots in order.
            after: (s > 0).then(|| want[s - 1].clone()),
            // The head has no predecessor, so it anchors ahead — on the first
            // slot that survives from the server. Taking `want[1]` instead
            // named a label that doesn't exist yet whenever slot 1 was itself
            // new, and PostgreSQL rejected the whole plan.
            before: (s == 0)
                .then(|| {
                    surviving
                        .iter()
                        .filter(|&&x| x > s)
                        .min()
                        .map(|&x| want[x].clone())
                })
                .flatten(),
        });
        taken.push(s);
    }
    Some(out)
}

/// Everything that has to happen to turn the domain `current` into `draft`.
///
/// A domain's default, nullability and constraints are all alterable in place.
/// Its **base type is not** — `ALTER DOMAIN` has no action for it — so changing
/// that collapses the whole edit into a [`Change::RecreateDomain`], on the same
/// grounds as the enum's rebuild.
pub fn diff_domain(
    current: &DomainInfo,
    draft: &DomainDraft,
    dependents: &[TypeDependent],
    dialect: SqlDialect,
) -> ChangeSet {
    let new = &draft.info;
    let mut changes = Vec::new();
    // `types_equal` so `varchar(45)` and `character varying(45)` are the same
    // domain, which is what keeps an editor from opening already-changed.
    let retyped = !types_equal(&current.base_type, &new.base_type, dialect)
        || current.collation != new.collation;
    if retyped {
        changes.push(Change::RecreateDomain {
            info: Box::new(DomainInfo {
                name: current.name.clone(),
                schema: current.schema.clone(),
                ..new.clone()
            }),
            from_type: current.base_type.clone(),
            dependents: dependents.to_vec(),
        });
    } else {
        if !defaults_equal(
            current.default_value.as_deref(),
            new.default_value.as_deref(),
        ) {
            changes.push(Change::SetDomainDefault {
                to: new.default_value.clone(),
            });
        }
        if current.not_null != new.not_null {
            changes.push(Change::SetDomainNotNull { to: new.not_null });
        }
        // Drops first, so a constraint can be redefined under the name it has.
        for ck in &current.checks {
            if !new
                .checks
                .iter()
                .any(|n| n.name == ck.name && checks_equal(ck, n, dialect))
            {
                changes.push(Change::DropDomainCheck {
                    name: ck.name.clone(),
                });
            }
        }
        for ck in &new.checks {
            if !current
                .checks
                .iter()
                .any(|c| c.name == ck.name && checks_equal(c, ck, dialect))
            {
                changes.push(Change::AddDomainCheck(Box::new(ck.clone())));
            }
        }
    }
    if !retyped && current.comment != new.comment {
        changes.push(Change::SetObjectComment {
            kind: ObjectKind::Domain,
            comment: new.comment.clone(),
        });
    }
    if new.name != current.name && !new.name.trim().is_empty() {
        changes.push(Change::RenameObject {
            kind: ObjectKind::Domain,
            to: new.name.clone(),
        });
    }
    ChangeSet {
        table: current.name.clone(),
        schema: current.schema.clone(),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes,
    }
}

/// Everything that has to happen to turn the sequence `current` into `draft`.
///
/// Every field of a sequence is alterable in place, so this never rebuilds. The
/// restart is deliberately its own change and not part of the `ALTER`: moving the
/// counter and changing the definition are different acts with different
/// consequences, and the preview has to be able to say so separately.
pub fn diff_sequence(
    current: &SequenceInfo,
    draft: &SequenceDraft,
    dialect: SqlDialect,
) -> ChangeSet {
    let new = &draft.info;
    let mut changes = Vec::new();
    if !sequence_edits(current, new).is_empty() {
        changes.push(Change::AlterSequence {
            from: Box::new(current.clone()),
            to: Box::new(SequenceInfo {
                name: current.name.clone(),
                schema: current.schema.clone(),
                ..new.clone()
            }),
        });
    }
    if let Some(r) = draft.restart {
        changes.push(Change::RestartSequence { to: r });
    }
    if current.comment != new.comment {
        changes.push(Change::SetObjectComment {
            kind: ObjectKind::Sequence,
            comment: new.comment.clone(),
        });
    }
    if new.name != current.name && !new.name.trim().is_empty() {
        changes.push(Change::RenameObject {
            kind: ObjectKind::Sequence,
            to: new.name.clone(),
        });
    }
    ChangeSet {
        table: current.name.clone(),
        schema: current.schema.clone(),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes,
    }
}

/// The `CREATE TYPE` for a brand-new enum.
pub fn create_enum(draft: &EnumDraft, dialect: SqlDialect) -> ChangeSet {
    object_set(
        &draft.info.name,
        draft.info.schema.as_deref(),
        dialect,
        Change::CreateEnum(Box::new(draft.info.clone())),
    )
}

/// The `CREATE DOMAIN` for a brand-new domain.
pub fn create_domain(draft: &DomainDraft, dialect: SqlDialect) -> ChangeSet {
    object_set(
        &draft.info.name,
        draft.info.schema.as_deref(),
        dialect,
        Change::CreateDomain(Box::new(draft.info.clone())),
    )
}

/// The `CREATE SEQUENCE` for a brand-new sequence.
pub fn create_sequence(draft: &SequenceDraft, dialect: SqlDialect) -> ChangeSet {
    object_set(
        &draft.info.name,
        draft.info.schema.as_deref(),
        dialect,
        Change::CreateSequence(Box::new(draft.info.clone())),
    )
}

/// The `DROP` for one standalone object — the context menu's shortcut.
pub fn drop_object(
    kind: ObjectKind,
    name: &str,
    schema: Option<&str>,
    dialect: SqlDialect,
) -> ChangeSet {
    object_set(name, schema, dialect, Change::DropObject { kind })
}

/// A one-change set against a standalone object. [`single`]'s counterpart, kept
/// apart only because the field is called `table` and these aren't one.
fn object_set(name: &str, schema: Option<&str>, dialect: SqlDialect, change: Change) -> ChangeSet {
    ChangeSet {
        table: name.to_string(),
        schema: schema.map(str::to_string),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes: vec![change],
    }
}

/// A one-change set against a table — how every context-menu shortcut reaches
/// the preview without opening the designer.
pub fn single(table: &str, schema: Option<&str>, dialect: SqlDialect, change: Change) -> ChangeSet {
    ChangeSet {
        table: table.to_string(),
        schema: schema.map(str::to_string),
        dialect,
        flavour: ServerFlavour::Unknown,
        changes: vec![change],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intel::SqlDialect::{MySql, Postgres};
    use crate::schema::{IndexColumn, TriggerEnabled};

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
    fn taking_a_composite_key_column_out_and_back_leaves_the_key_alone() {
        // A double-write of the toggle — the app had one, an Enter on the
        // focused switch flipping it off and on inside a single keypress — used
        // to move the column to the *end* of the key: a keypress whose visible
        // result is nothing emitted `DROP PRIMARY KEY` + a reordered `ADD`.
        let mut d = TableDraft::from_table(&ab_table());
        d.columns[1].info.nullable = false;
        d.primary_key = vec!["a".into(), "b".into()];
        d.set_in_primary_key(0, false);
        d.set_in_primary_key(0, true);
        assert_eq!(d.primary_key, vec!["a", "b"]);
        // The **last** column of a two-column key: the re-add appends, so this
        // one passes whether the scan finds the insertion point or falls back to
        // `push`. It is here as the control, not as the interesting case.
        d.primary_key = vec!["a".into(), "b".into()];
        d.set_in_primary_key(1, false);
        d.set_in_primary_key(1, true);
        assert_eq!(d.primary_key, vec!["a", "b"]);

        // **The middle of a three-column key**, which is what the comment here
        // used to claim while running on a two-column fixture — so index 1 was
        // the last column and the insertion scan's interior branch was never
        // exercised at all. A regression to `push` passed both assertions above.
        let mut t = ab_table();
        t.columns.push(col("c", "int"));
        t.indexes[0] = IndexInfo::plain("PRIMARY", vec!["a", "b", "c"], true);
        let mut d = TableDraft::from_table(&t);
        d.columns[1].info.nullable = false;
        assert_eq!(d.primary_key, vec!["a", "b", "c"]);
        d.set_in_primary_key(1, false);
        assert_eq!(d.primary_key, vec!["a", "c"]);
        d.set_in_primary_key(1, true);
        assert_eq!(
            d.primary_key,
            vec!["a", "b", "c"],
            "b goes back between a and c, not on the end"
        );
    }

    #[test]
    fn a_new_key_member_lands_at_its_column_ordinal() {
        let mut d = TableDraft::from_table(&ab_table());
        d.primary_key = vec!["b".into()];
        d.set_in_primary_key(0, true);
        assert_eq!(d.primary_key, vec!["a", "b"], "a is column 0, so it leads");
        d.primary_key.clear();
        d.set_in_primary_key(1, true);
        d.set_in_primary_key(0, true);
        assert_eq!(d.primary_key, vec!["a", "b"], "click order doesn't decide");
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
    /// `CHECK` constraints: the invariant half of a table, and the one a
    /// `CREATE TABLE` built from a draft used to drop without a word.
    mod checks {
        use super::*;

        fn ck(name: &str, expr: &str) -> CheckInfo {
            CheckInfo {
                name: name.into(),
                expression: expr.into(),
                ..Default::default()
            }
        }

        fn table_with(checks: Vec<CheckInfo>) -> TableInfo {
            TableInfo {
                name: "t".into(),
                columns: vec![col("qty", "int")],
                check_constraints: checks,
                ..Default::default()
            }
        }

        /// Three sources, one stored shape — so the emitter wraps exactly once
        /// and the preview doesn't read `CHECK (((total >= 0)))`.
        #[test]
        fn the_stored_predicate_is_bare_whatever_the_catalogue_said() {
            // PostgreSQL hands back the whole clause.
            assert_eq!(
                check_predicate("CHECK ((total >= (0)::numeric))", Postgres),
                "total >= (0)::numeric"
            );
            // MySQL hands back the parenthesised predicate.
            assert_eq!(check_predicate("(`qty` > 0)", MySql), "`qty` > 0");
            // A person types the predicate.
            assert_eq!(check_predicate("qty > 0", MySql), "qty > 0");
            // Round-trips: what's stored, re-wrapped, is what the server said.
            let ck = CheckInfo {
                name: "c".into(),
                expression: check_predicate("CHECK ((total >= 0))", Postgres),
                ..Default::default()
            };
            assert_eq!(
                ck.clause_sql(Postgres),
                "CONSTRAINT \"c\" CHECK (total >= 0)"
            );
        }

        /// Normalization must stop at a string boundary.
        ///
        /// `norm_check_expr` squashed whitespace runs across the *whole*
        /// predicate, so `name <> 'a  b'` and `name <> 'a b'` compared equal and
        /// an edit to a `LIKE` or regex pattern was **silently discarded** —
        /// the dangerous direction, and the one this function's own doc claims
        /// to avoid. Case folding had the mirror flaw: it applied to the whole
        /// string whenever neither side contained a `'`, which folds
        /// PostgreSQL's case-*sensitive* `"Qty"` into `"qty"`.
        #[test]
        fn a_checks_string_literals_are_compared_byte_for_byte() {
            let ck = |e: &str| CheckInfo {
                name: "c".into(),
                expression: e.into(),
                ..Default::default()
            };
            // The reported case: only the spacing *inside* the literal differs.
            assert!(!checks_equal(
                &ck("name <> 'a  b'"),
                &ck("name <> 'a b'"),
                MySql
            ));
            // Different letter case inside a literal is a different predicate.
            assert!(!checks_equal(
                &ck("status = 'a'"),
                &ck("status = 'A'"),
                MySql
            ));
            // A quoted identifier is case-sensitive on PostgreSQL.
            assert!(!checks_equal(
                &ck("\"Qty\" > 0"),
                &ck("\"qty\" > 0"),
                Postgres
            ));

            // …while everything outside a literal still normalizes: spacing
            // between tokens, and the case of bare identifiers and keywords.
            assert!(checks_equal(&ck("qty   >  0"), &ck("qty > 0"), MySql));
            assert!(checks_equal(&ck("QTY > 0"), &ck("qty > 0"), MySql));
            assert!(checks_equal(
                &ck("name <> 'a  b'  AND qty > 0"),
                &ck("name <> 'a  b' and qty > 0"),
                MySql
            ));
        }

        /// MariaDB destroys a column's CHECK when the column is altered, so the
        /// clause has to restate it.
        ///
        /// Measured on MariaDB 10.11.14: a column declared
        /// `qty INT CHECK (qty > 0)`, widened to `BIGINT`, comes back with **no
        /// constraints at all** and accepts `-5` on the next insert — a row the
        /// table refused a moment earlier. MySQL 8.4 keeps the constraint, so
        /// the same plan has opposite outcomes on the two servers Schemaic
        /// calls one dialect. Only *column-level* checks are affected; a
        /// table-level `CONSTRAINT … CHECK` survives (also measured).
        ///
        /// The flavour is not a `SqlDialect` arm — the two agree on everything
        /// `sql`/`intel`/`filter` care about — so it rides on the plan.
        ///
        /// It can't be compensated with `DROP CONSTRAINT` + `ADD CONSTRAINT`
        /// the way MySQL's rename is: MariaDB can't address a column-level
        /// check by name at all (`ERROR 1091 … check that it exists`), and the
        /// syntax refuses to give one a name in the first place. Restating the
        /// clause inside the `MODIFY` is the only form that works, and it was
        /// measured to keep the constraint at `LEVEL = 'Column'`.
        fn checked_column_table() -> TableInfo {
            TableInfo {
                name: "t".into(),
                columns: vec![ColumnInfo {
                    name: "qty".into(),
                    type_name: "int".into(),
                    nullable: true,
                    ..Default::default()
                }],
                check_constraints: vec![CheckInfo {
                    // MariaDB names a column-level check after its column.
                    name: "qty".into(),
                    expression: "`qty` > 0".into(),
                    column_level: true,
                    ..Default::default()
                }],
                ..Default::default()
            }
        }

        #[test]
        fn a_mariadb_column_alter_restates_the_columns_own_check() {
            let before = checked_column_table();
            let mut draft = TableDraft::from_table(&before);
            draft.columns[0].info.type_name = "bigint".into();

            let sql = diff(&before, &draft, Target::new(MySql, ServerFlavour::MariaDb))
                .emit()
                .join("\n");
            assert!(
                sql.contains("MODIFY COLUMN `qty` bigint CHECK (`qty` > 0)"),
                "{sql}"
            );
            // The constraint is not *changed*, so it is not a change: it rides
            // on the column clause rather than appearing as an edit nobody made.
            let cs = diff(&before, &draft, Target::new(MySql, ServerFlavour::MariaDb));
            assert_eq!(cs.len(), 1, "{:?}", cs.changes);

            // Real MySQL keeps the constraint through a `MODIFY`, so restating
            // it there would create a second one.
            let sql = diff(&before, &draft, Target::new(MySql, ServerFlavour::MySql))
                .emit()
                .join("\n");
            assert!(!sql.contains("CHECK"), "{sql}");
        }

        /// The round-trip gate, for the new field: a table carrying a
        /// column-level check must diff to nothing against its own draft, or
        /// every MariaDB designer would open on a phantom change.
        #[test]
        fn a_column_level_check_round_trips_to_no_changes() {
            let before = checked_column_table();
            let draft = TableDraft::from_table(&before);
            for flavour in [ServerFlavour::MariaDb, ServerFlavour::MySql] {
                let cs = diff(&before, &draft, Target::new(MySql, flavour));
                assert!(cs.is_empty(), "{flavour:?}: {:?}", cs.changes);
            }
        }

        #[test]
        fn re_pointing_matches_names_the_way_the_engine_does() {
            // MySQL/MariaDB: case-insensitive, and both spellings re-point.
            assert_eq!(
                repoint_check_column("QTY > 0", "qty", "quantity", MySql).as_deref(),
                Some("`quantity` > 0")
            );
            // PostgreSQL: a quoted name is exactly what was written.
            assert_eq!(
                repoint_check_column("\"Qty\" > 0", "qty", "quantity", Postgres),
                None
            );
            assert_eq!(
                repoint_check_column("\"Qty\" > 0", "Qty", "Quantity", Postgres).as_deref(),
                Some("\"Quantity\" > 0")
            );
            // A doubled quote inside a name is one quote, not a boundary.
            assert_eq!(
                repoint_check_column("`a``b` > 0", "a`b", "c", MySql).as_deref(),
                Some("`c` > 0")
            );
            // Nothing to re-point is `None`, not an unchanged copy — that is
            // what tells the caller no constraint is involved.
            assert_eq!(repoint_check_column("a > 0", "qty", "q", MySql), None);
        }

        /// A bare `CHANGE COLUMN` destroys it too, and the restated predicate
        /// has to name the *new* column — `CHANGE COLUMN q qty bigint
        /// CHECK (q > 0)` is `ERROR 1054 Unknown column 'q'`. MariaDB renames
        /// the constraint with the column, so the plan does the same.
        #[test]
        fn a_mariadb_rename_re_points_the_restated_check() {
            let before = checked_column_table();
            let mut draft = TableDraft::from_table(&before);
            draft.columns[0].info.name = "quantity".into();

            let cs = diff(&before, &draft, Target::new(MySql, ServerFlavour::MariaDb));
            let sql = cs.emit().join("\n");
            assert!(
                sql.contains("CHANGE COLUMN `qty` `quantity` int CHECK (`quantity` > 0)"),
                "{sql}"
            );
            // And no drop-and-add pair: MariaDB would refuse the drop.
            assert!(
                !sql.contains("DROP CONSTRAINT") && !sql.contains("ADD CONSTRAINT"),
                "{sql}"
            );
        }

        /// A move is a `MODIFY COLUMN` as much as a retype is, so it loses the
        /// check the same way.
        #[test]
        fn a_mariadb_column_move_restates_the_check_too() {
            let mut before = checked_column_table();
            // `qty` last on the server, first in the draft — so it is the one
            // column the plan moves.
            before.columns.insert(
                0,
                ColumnInfo {
                    name: "a".into(),
                    type_name: "int".into(),
                    nullable: true,
                    ..Default::default()
                },
            );
            let mut draft = TableDraft::from_table(&before);
            draft.columns.swap(0, 1);

            let sql = diff(&before, &draft, Target::new(MySql, ServerFlavour::MariaDb))
                .emit()
                .join("\n");
            assert!(sql.contains("CHECK (`qty` > 0)"), "{sql}");
        }

        /// The user's own edit stays the authority: a check the draft changed is
        /// applied as the drop-and-add the designer asked for, not restated
        /// inline from the version on the server.
        #[test]
        fn an_edited_column_check_is_not_also_restated_inline() {
            let before = checked_column_table();
            let mut draft = TableDraft::from_table(&before);
            draft.columns[0].info.type_name = "bigint".into();
            draft.check_constraints[0].info.expression = "`qty` > 10".into();

            let sql = diff(&before, &draft, Target::new(MySql, ServerFlavour::MariaDb))
                .emit()
                .join("\n");
            assert!(!sql.contains("MODIFY COLUMN `qty` bigint CHECK"), "{sql}");
            assert_eq!(sql.matches("CHECK (`qty` > 10)").count(), 1, "{sql}");
            // And the old one comes off by restating the column, not by a
            // `DROP CONSTRAINT` MariaDB answers with 1091.
            assert!(!sql.contains("DROP CONSTRAINT"), "{sql}");
        }

        /// Deleting one is the same shape with nothing added back — and it is
        /// the case that had no working statement at all before: the designer
        /// listed a constraint whose removal the server refused.
        #[test]
        fn a_deleted_column_check_comes_off_by_restating_the_column() {
            let before = checked_column_table();
            let mut draft = TableDraft::from_table(&before);
            draft.check_constraints.clear();

            let cs = diff(&before, &draft, Target::new(MySql, ServerFlavour::MariaDb));
            let sql = cs.emit().join("\n");
            assert!(!sql.contains("DROP CONSTRAINT"), "{sql}");
            assert!(sql.contains("MODIFY COLUMN `qty` int"), "{sql}");
            assert!(!sql.contains("CHECK"), "{sql}");

            // MySQL 8 has no column-level checks, so nothing changes there.
            let sql = diff(&before, &draft, Target::new(MySql, ServerFlavour::MySql))
                .emit()
                .join("\n");
            assert!(sql.contains("DROP CONSTRAINT `qty`"), "{sql}");
        }

        /// A column on its way out takes its check with it — the plan must not
        /// grow a clause restating a column it is dropping.
        #[test]
        fn dropping_the_column_does_not_resurrect_it_to_drop_its_check() {
            let before = checked_column_table();
            let mut draft = TableDraft::from_table(&before);
            draft.columns.clear();
            draft.check_constraints.clear();

            let sql = diff(&before, &draft, Target::new(MySql, ServerFlavour::MariaDb))
                .emit()
                .join("\n");
            assert!(sql.contains("DROP COLUMN `qty`"), "{sql}");
            assert!(!sql.contains("MODIFY COLUMN"), "{sql}");
        }

        /// A PostgreSQL clause trailer belongs to the *constraint*, not to the
        /// predicate — and putting it in the predicate made every statement
        /// Schemaic emitted for that table a syntax error.
        ///
        /// `pg_get_constraintdef` returns `CHECK ((qty > 0)) NOT VALID`;
        /// `peel_parens` gates on the text ending in `)`, which the trailer
        /// makes false, so the whole thing was stored as the expression and
        /// `clause_sql` wrapped it: `CHECK (((qty > 0)) NOT VALID)` →
        /// `ERROR: syntax error at or near "NOT"`. Copy DDL, `CREATE TABLE`,
        /// the preview's script and every domain check share the path. All four
        /// inputs below are verbatim PG 16.14 output.
        #[test]
        fn a_postgres_clause_trailer_is_not_part_of_the_predicate() {
            assert_eq!(
                check_predicate("CHECK ((qty > 0)) NOT VALID", Postgres),
                "qty > 0"
            );
            assert_eq!(
                check_predicate("CHECK ((qty < 100)) NO INHERIT", Postgres),
                "qty < 100"
            );
            // Both, in the order the server prints them — and a close-paren
            // inside a literal, which is why the peel goes through the lexer.
            assert_eq!(
                check_predicate("CHECK ((name <> ')'::text)) NO INHERIT NOT VALID", Postgres),
                "name <> ')'::text"
            );
            assert_eq!(check_predicate("CHECK ((qty <> 5))", Postgres), "qty <> 5");
        }

        /// The trailers are *carried*, not dropped: restating the constraint
        /// without them changes what the table promises, and would turn a
        /// working Copy DDL script into one that fails on data the server
        /// itself accepts.
        #[test]
        fn a_carried_trailer_round_trips_through_the_emitter() {
            for (raw, tail) in [
                ("CHECK ((qty > 0)) NOT VALID", " NOT VALID"),
                ("CHECK ((qty > 0)) NO INHERIT", " NO INHERIT"),
                (
                    "CHECK ((qty > 0)) NO INHERIT NOT VALID",
                    " NO INHERIT NOT VALID",
                ),
                ("CHECK ((qty > 0))", ""),
            ] {
                let (validated, inherited) = check_clause_flags(raw);
                let ck = CheckInfo {
                    name: "c".into(),
                    expression: check_predicate(raw, Postgres),
                    validated,
                    inherited,
                    ..Default::default()
                };
                assert_eq!(
                    ck.clause_sql(Postgres),
                    format!("CONSTRAINT \"c\" CHECK (qty > 0){tail}"),
                    "{raw}"
                );
            }
            // MySQL has neither clause and must never grow one.
            let ck = CheckInfo {
                name: "c".into(),
                expression: "qty > 0".into(),
                validated: false,
                inherited: false,
                enforced: true,
                column_level: false,
            };
            assert_eq!(ck.clause_sql(MySql), "CONSTRAINT `c` CHECK (qty > 0)");
        }

        /// `CHECK` is stripped only as a leading *word* — a predicate may open
        /// with a column whose name merely starts with those five letters.
        #[test]
        fn a_column_named_like_the_keyword_survives() {
            assert_eq!(
                check_predicate("checked_at IS NOT NULL", MySql),
                "checked_at IS NOT NULL"
            );
            assert_eq!(check_predicate("(checkout > 0)", MySql), "checkout > 0");
        }

        fn fnc() -> RoutineInfo {
            RoutineInfo {
                name: "audit".into(),
                schema: Some("public".into()),
                arguments: String::new(),
                returns: "trigger".into(),
                language: "plpgsql".into(),
                body: "BEGIN RETURN NEW; END;".into(),
                ..Default::default()
            }
        }

        /// The gate: a draft taken straight off a function has nothing to say.
        #[test]
        fn an_untouched_function_is_not_a_change() {
            let f = fnc();
            let d = FunctionDraft::from_info(&f);
            assert!(diff_function(&f, &d, Postgres).changes.is_empty());
        }

        /// PostgreSQL renames a function in place and the triggers bound to it
        /// keep working — so unlike a trigger, this costs no re-creation.
        #[test]
        fn renaming_a_function_is_a_rename_not_a_recreate() {
            let f = fnc();
            let mut d = FunctionDraft::from_info(&f);
            d.info.name = "audit2".into();
            let cs = diff_function(&f, &d, Postgres);
            assert_eq!(cs.changes.len(), 1);
            assert_eq!(
                cs.emit(),
                vec!["ALTER FUNCTION \"audit\"() RENAME TO \"audit2\";"]
            );
        }

        /// A rename *and* a body edit: the replace has to address the signature
        /// the server still has, and the rename runs after it.
        #[test]
        fn a_rename_with_a_body_edit_replaces_under_the_old_name_first() {
            let f = fnc();
            let mut d = FunctionDraft::from_info(&f);
            d.info.name = "audit2".into();
            d.info.body = "BEGIN RETURN OLD; END;".into();
            let sql = diff_function(&f, &d, Postgres).emit();
            assert_eq!(sql.len(), 2);
            assert!(
                sql[0].contains("CREATE OR REPLACE FUNCTION \"audit\"()"),
                "{sql:?}"
            );
            assert!(
                sql[1].starts_with("ALTER FUNCTION \"audit\"() RENAME TO"),
                "{sql:?}"
            );
        }

        /// Everything a replace would otherwise reset has to be restated — the
        /// `SET search_path` most of all, since dropping it from a SECURITY
        /// DEFINER function is a privilege-escalation hole.
        #[test]
        fn create_function_restates_every_option() {
            let mut f = fnc();
            f.security_definer = true;
            f.strict = true;
            f.volatility = crate::schema::Volatility::Stable;
            f.settings = vec!["search_path=public".into()];
            let sql = f.create_sql(Postgres, true);
            assert!(
                sql.starts_with("CREATE OR REPLACE FUNCTION \"audit\"()"),
                "{sql}"
            );
            assert!(sql.contains("RETURNS trigger"), "{sql}");
            assert!(sql.contains("LANGUAGE plpgsql"), "{sql}");
            assert!(sql.contains("STABLE"), "{sql}");
            assert!(sql.contains("STRICT"), "{sql}");
            assert!(sql.contains("SECURITY DEFINER"), "{sql}");
            assert!(sql.contains("SET search_path=public"), "{sql}");
        }

        /// VOLATILE is the default and says nothing, so restating it would be
        /// noise — the same call `create_view_sql` makes about UNDEFINED.
        #[test]
        fn the_default_volatility_is_not_restated() {
            assert!(!fnc().create_sql(Postgres, false).contains("VOLATILE"));
        }

        /// A body containing the delimiter would terminate the statement in the
        /// middle of itself, and PostgreSQL has no escape inside a dollar quote.
        #[test]
        fn the_dollar_tag_avoids_whatever_the_body_already_uses() {
            use crate::schema::dollar_tag;
            assert_eq!(dollar_tag("BEGIN END;"), "$$");
            assert_eq!(dollar_tag("x := $$a$$;"), "$fn$");
            assert_eq!(dollar_tag("$$ and $fn$"), "$body$");

            let mut f = fnc();
            f.body = "sql := $$SELECT 1$$;".into();
            let sql = f.create_sql(Postgres, false);
            assert!(sql.contains("AS $fn$"), "{sql}");
            assert!(sql.trim_end().ends_with("$fn$;"), "{sql}");
        }

        /// By signature, not by name: an overload makes a bare name ambiguous.
        #[test]
        fn drop_function_names_the_signature() {
            let mut f = fnc();
            f.arguments = "a integer, b text".into();
            assert_eq!(
                drop_function(&f, Postgres).emit(),
                vec!["DROP FUNCTION \"audit\"(a integer, b text);"]
            );
        }

        /// Redefining a shared function changes what every trigger bound to it
        /// does, including on tables this edit never mentioned.
        #[test]
        fn replacing_a_function_states_the_reach_of_the_change() {
            let f = fnc();
            let mut d = FunctionDraft::from_info(&f);
            d.info.body = "BEGIN RETURN OLD; END;".into();
            let risks = diff_function(&f, &d, Postgres).destructive();
            assert_eq!(risks.len(), 1);
            assert!(risks[0].contains("other tables"), "{risks:?}");
        }

        #[test]
        fn a_blank_trigger_function_starts_valid_and_returns() {
            let d = FunctionDraft::blank_trigger("f", None);
            assert!(d.validate().is_empty());
            // The most common first-run failure is a body that never returns.
            assert!(d.info.body.contains("RETURN NEW"));
            assert!(d.info.is_trigger_function());
        }

        #[test]
        fn validate_catches_the_empty_draft_and_a_declared_argument() {
            let d = FunctionDraft::default();
            let msgs = d.validate().join(" | ");
            assert!(msgs.contains("needs a name"), "{msgs}");
            assert!(msgs.contains("needs a language"), "{msgs}");
            assert!(msgs.contains("needs a body"), "{msgs}");

            let mut d = FunctionDraft::blank_trigger("f", None);
            d.info.arguments = "a integer".into();
            assert!(d.validate().join(" | ").contains("TG_ARGV"));
        }

        fn my_trigger() -> TriggerInfo {
            TriggerInfo {
                name: "t_ins".into(),
                table: "orders".into(),
                timing: TriggerTiming::Before,
                events: vec![TriggerEvent::Insert],
                action: TriggerAction::Body("SET NEW.x = 1".into()),
                ..Default::default()
            }
        }

        fn pg_trigger() -> TriggerInfo {
            TriggerInfo {
                name: "t_upd".into(),
                schema: Some("public".into()),
                table: "orders".into(),
                timing: TriggerTiming::After,
                events: vec![TriggerEvent::Update],
                action: TriggerAction::Function {
                    name: "audit".into(),
                    args: vec![],
                },
                ..Default::default()
            }
        }

        fn table_with_triggers(ts: Vec<TriggerInfo>) -> TableInfo {
            TableInfo {
                name: "orders".into(),
                triggers: ts,
                ..Default::default()
            }
        }

        /// The escape hatch has to hand over something the app itself can run.
        /// A MySQL compound body holds its own semicolons, and the query tab
        /// splits on every top-level one — so the script it gets is wrapped in
        /// `DELIMITER $$`, and the two renderings must stay statement-for-
        /// statement equal.
        #[test]
        fn a_compound_mysql_body_is_handed_over_as_one_statement() {
            let mut t = my_trigger();
            t.action = TriggerAction::Body("BEGIN\n  SET NEW.a = 1;\n  SET NEW.b = 2;\nEND".into());
            let cs = create_trigger(&TriggerDraft::from_info(&t), MySql);

            // On the wire it is one statement and `DELIMITER` never appears.
            assert_eq!(cs.emit().len(), 1);
            assert!(!cs.emit()[0].contains("DELIMITER"));

            // In a query tab it survives the splitter as one statement.
            let script = cs.editor_script();
            assert!(script.starts_with("DELIMITER $$"), "{script}");
            let ranges = sql::statement_ranges(&script, MySql);
            assert_eq!(ranges.len(), 1, "{script}");
            assert!(script[ranges[0].0..ranges[0].1].starts_with("CREATE TRIGGER"));

            // A statement with no internal `;` is handed over untouched.
            let plain = create_trigger(&TriggerDraft::from_info(&my_trigger()), MySql);
            assert_eq!(plain.editor_script(), plain.script());
            // …and PostgreSQL never wraps: its bodies are dollar-quoted.
            let pg = create_trigger(&TriggerDraft::from_info(&pg_trigger()), Postgres);
            assert_eq!(pg.editor_script(), pg.script());
        }

        /// The gate, for the whole set: a draft off a table says nothing.
        #[test]
        fn an_untouched_trigger_set_is_not_a_change() {
            let t = table_with_triggers(vec![my_trigger(), {
                let mut b = my_trigger();
                b.name = "t_two".into();
                b
            }]);
            let d = TriggerSetDraft::from_table(&t);
            assert!(diff_triggers(&t.triggers, &d, MySql).changes.is_empty());
        }

        /// A MySQL trigger carries the session state it was created under, and
        /// `CREATE TRIGGER` has no clause for any of it.
        ///
        /// Recreated under whatever the applying session happens to have, a
        /// trigger written under `sql_mode = ''` starts failing every parent
        /// `INSERT` — or, reversed, stops raising and silently truncates.
        /// Nothing in the preview named it, because the three values were never
        /// read at all.
        #[test]
        fn a_mysql_trigger_is_recreated_under_the_session_state_it_was_written_in() {
            let mut t = my_trigger();
            t.sql_mode = Some("NO_ENGINE_SUBSTITUTION".into());
            t.charset_client = Some("latin1".into());
            t.collation_connection = Some("latin1_swedish_ci".into());
            let table = table_with_triggers(vec![t.clone()]);
            let mut d = TriggerSetDraft::from_table(&table);
            d.triggers[0].info.action = TriggerAction::Body("SET NEW.x = 2".into());
            let sql = diff_triggers(&table.triggers, &d, MySql).emit();

            let create = sql
                .iter()
                .position(|s| s.contains("CREATE"))
                .expect("a create");
            // Saved before, set before, restored after — in that order.
            assert!(sql[create - 2].starts_with("SET @schemaic_sql_mode = @@SESSION.sql_mode"));
            assert!(
                sql[create - 1].contains("SESSION sql_mode = 'NO_ENGINE_SUBSTITUTION'"),
                "{:?}",
                sql[create - 1]
            );
            assert!(sql[create - 1].contains("SESSION character_set_client = 'latin1'"));
            assert!(sql[create + 1].contains("SESSION sql_mode = @schemaic_sql_mode"));

            // Nothing known ⇒ nothing emitted: `None` means "not fetched", and
            // inventing a session state is a change nobody asked for.
            let plain = table_with_triggers(vec![my_trigger()]);
            let mut d = TriggerSetDraft::from_table(&plain);
            d.triggers[0].info.action = TriggerAction::Body("SET NEW.x = 2".into());
            let sql = diff_triggers(&plain.triggers, &d, MySql).emit();
            assert!(!sql.iter().any(|s| s.contains("@@SESSION")), "{sql:?}");
        }

        /// Swapping two triggers' names must not destroy one of them.
        ///
        /// `TriggerSetDraft::validate` compares only *final* names, and after an
        /// `a`→`b`, `b`→`a` swap those are unique — so the plan is accepted.
        /// Emitted as adjacent `DROP`+`CREATE` pairs it then failed `ERROR 1359`
        /// on statement 2, with statement 1 already committed on MySQL: trigger
        /// `a` gone, and no transaction to roll back.
        #[test]
        fn swapping_two_trigger_names_drops_both_before_creating_either() {
            let a = my_trigger();
            let mut b = my_trigger();
            b.name = "t_two".into();
            let t = table_with_triggers(vec![a, b]);
            let mut d = TriggerSetDraft::from_table(&t);
            d.triggers[0].info.name = "t_two".into();
            d.triggers[1].info.name = "t_ins".into();
            let sql = diff_triggers(&t.triggers, &d, MySql).emit();
            let first_create = sql
                .iter()
                .position(|s| s.contains("CREATE"))
                .expect("a create");
            let last_drop = sql
                .iter()
                .rposition(|s| s.starts_with("DROP TRIGGER"))
                .expect("a drop");
            assert!(
                last_drop < first_create,
                "every drop must precede every create: {sql:?}"
            );
        }

        /// The round-trip gate for the state PG 16.14 reports and the emitter
        /// has to restate: transition tables and a non-default `tgenabled`.
        ///
        /// Both were unmodelled, so a trigger carrying either came back
        /// *stripped* — and because the clause is dropped silently, the failure
        /// lands on the next write to the table, not in the preview.
        #[test]
        fn a_transition_table_trigger_round_trips_and_restates_its_clauses() {
            let mut pg = pg_trigger();
            pg.level = TriggerLevel::Statement;
            pg.old_table = Some("o".into());
            pg.new_table = Some("n".into());
            pg.enabled = TriggerEnabled::Always;
            let t = table_with_triggers(vec![pg.clone()]);
            let d = TriggerSetDraft::from_table(&t);
            assert!(
                diff_triggers(&t.triggers, &d, Postgres).changes.is_empty(),
                "phantom change on an untouched trigger"
            );
            // And a real recreate carries both clauses through.
            let sql = pg.create_sql(Postgres);
            assert!(
                sql.contains("REFERENCING OLD TABLE AS \"o\" NEW TABLE AS \"n\""),
                "{sql}"
            );
            assert!(sql.contains("ENABLE ALWAYS TRIGGER"), "{sql}");
        }

        /// The other half of the gate: the UI sorts a trigger's events whenever
        /// a checkbox is ticked, so a *sorted* copy of an introspected trigger
        /// must still diff to nothing.
        ///
        /// It did not. PostgreSQL prints `AFTER DELETE OR UPDATE` in `tgtype`
        /// bit order, so introspection produced `[Delete, Update]`, while
        /// `TriggerEvent`'s derived `Ord` followed a declaration order of
        /// `Insert, Update, Delete` and re-sorted it to `[Update, Delete]`.
        /// Ticking any event on and straight back off left the designer
        /// reporting one change and offering to drop and recreate a trigger
        /// nothing had touched — and that phantom recreate is how a trigger's
        /// unmodelled state gets destroyed with the user having asked for
        /// nothing at all.
        #[test]
        fn sorting_an_introspected_triggers_events_is_not_a_change() {
            let mut pg = pg_trigger();
            pg.events = vec![TriggerEvent::Delete, TriggerEvent::Update];
            let t = table_with_triggers(vec![pg]);
            let mut d = TriggerSetDraft::from_table(&t);
            // Exactly what `ui::trigger_editor` does on every event toggle.
            d.triggers[0].info.events.sort();
            assert!(
                diff_triggers(&t.triggers, &d, Postgres).changes.is_empty(),
                "phantom change: {:?}",
                diff_triggers(&t.triggers, &d, Postgres).emit()
            );
        }

        #[test]
        fn removing_a_trigger_from_the_set_drops_it() {
            let t = table_with_triggers(vec![my_trigger()]);
            let mut d = TriggerSetDraft::from_table(&t);
            d.triggers.clear();
            let cs = diff_triggers(&t.triggers, &d, MySql);
            assert_eq!(cs.emit(), vec!["DROP TRIGGER `t_ins`;"]);
        }

        #[test]
        fn adding_a_trigger_to_the_set_creates_it() {
            let t = table_with_triggers(vec![my_trigger()]);
            let mut d = TriggerSetDraft::from_table(&t);
            let mut fresh = TriggerDraft::from_info(&my_trigger());
            fresh.original = None; // new: never on the server
            fresh.info.name = "t_new".into();
            d.triggers.push(fresh);
            let cs = diff_triggers(&t.triggers, &d, MySql);
            assert_eq!(cs.len(), 1);
            assert!(
                cs.emit()[0].contains("CREATE TRIGGER `t_new`"),
                "{:?}",
                cs.emit()
            );
        }

        /// Deleting one and naming its replacement after it is a thing people do,
        /// and it only works if the drop runs first.
        #[test]
        fn a_drop_is_emitted_before_a_create_that_reuses_the_name() {
            let t = table_with_triggers(vec![my_trigger()]);
            let mut d = TriggerSetDraft::from_table(&t);
            d.triggers.clear();
            let mut fresh = TriggerDraft::from_info(&my_trigger());
            fresh.original = None;
            d.triggers.push(fresh); // same name, `t_ins`
            let sql = diff_triggers(&t.triggers, &d, MySql).emit();
            assert_eq!(sql.len(), 2);
            assert_eq!(sql[0], "DROP TRIGGER `t_ins`;");
            assert!(sql[1].starts_with("CREATE TRIGGER `t_ins`"), "{sql:?}");
        }

        /// One plan, three kinds of change — the point of editing the set at once.
        #[test]
        fn a_set_carries_a_drop_an_edit_and_an_add_together() {
            let (a, mut b) = (my_trigger(), my_trigger());
            b.name = "t_two".into();
            let t = table_with_triggers(vec![a, b]);
            let mut d = TriggerSetDraft::from_table(&t);
            d.triggers.remove(0); // drop `t_ins`
            d.triggers[0].info.action = TriggerAction::Body("SET NEW.x = 9".into()); // edit
            let mut fresh = TriggerDraft::from_info(&my_trigger());
            fresh.original = None;
            fresh.info.name = "t_three".into();
            d.triggers.push(fresh); // add
            let cs = diff_triggers(&t.triggers, &d, MySql);
            assert_eq!(cs.len(), 3);
            assert!(matches!(cs.changes[0], Change::DropTrigger { .. }));
            assert!(matches!(cs.changes[1], Change::ReplaceTrigger { .. }));
            assert!(matches!(cs.changes[2], Change::CreateTrigger(_)));
        }

        /// Only a set can produce this one, and a list-plus-form makes it easy
        /// to do by accident.
        #[test]
        fn validate_catches_two_triggers_sharing_a_name() {
            let (a, mut b) = (my_trigger(), my_trigger());
            b.name = "T_INS".into(); // both engines fold case here
            let t = table_with_triggers(vec![a, b]);
            let msgs = TriggerSetDraft::from_table(&t)
                .validate(&t.triggers, MySql, TriggerHost::Table)
                .join(" | ");
            assert!(msgs.contains("both called"), "{msgs}");
        }

        /// One constraint trigger must not make every *other* trigger on the
        /// table uneditable.
        ///
        /// `validate` folded every member's messages into the set's, and the
        /// modal renders `errs.first()` and gates Preview SQL on the set being
        /// empty — so a table carrying one constraint trigger could not have any
        /// of its triggers edited, and the only way out was to select that one
        /// and drop it. The rule is "you may not *change* this one", not "this
        /// one exists".
        #[test]
        fn a_constraint_trigger_only_blocks_edits_to_itself() {
            let mut ct = pg_trigger();
            ct.name = "ct".into();
            ct.constraint = true;
            let t = table_with_triggers(vec![pg_trigger(), ct]);

            // Untouched: the set is clean and the other trigger is editable.
            let mut d = TriggerSetDraft::from_table(&t);
            assert!(
                d.validate(&t.triggers, Postgres, TriggerHost::Table)
                    .is_empty(),
                "{:?}",
                d.validate(&t.triggers, Postgres, TriggerHost::Table)
            );

            // Editing the *ordinary* one stays clean.
            d.triggers[0].info.condition = Some("new.total > 5".into());
            assert!(
                d.validate(&t.triggers, Postgres, TriggerHost::Table)
                    .is_empty()
            );

            // Editing the constraint trigger is what is refused, by name.
            d.triggers[1].info.condition = Some("new.total > 5".into());
            let msgs = d
                .validate(&t.triggers, Postgres, TriggerHost::Table)
                .join(" | ");
            assert!(msgs.contains("constraint trigger ct"), "{msgs}");
        }

        /// The gate: a draft taken straight off a trigger has nothing to say.
        #[test]
        fn an_untouched_trigger_is_not_a_change() {
            for (t, d) in [(my_trigger(), MySql), (pg_trigger(), Postgres)] {
                let draft = TriggerDraft::from_info(&t);
                assert!(
                    diff_trigger(&t, &draft, d).changes.is_empty(),
                    "{d:?} reported a phantom change"
                );
            }
        }

        /// Any difference costs the same drop-and-create, so a rename is not a
        /// change of its own — it rides the replace.
        #[test]
        fn a_rename_is_one_replace_not_a_rename_change() {
            let t = my_trigger();
            let mut draft = TriggerDraft::from_info(&t);
            draft.info.name = "t_ins2".into();
            let cs = diff_trigger(&t, &draft, MySql);
            assert_eq!(cs.changes.len(), 1);
            assert!(matches!(cs.changes[0], Change::ReplaceTrigger { .. }));
            // The drop names what the server has; the create names the new one.
            let sql = cs.emit();
            assert_eq!(sql[0], "DROP TRIGGER `t_ins`;");
            assert!(sql[1].contains("TRIGGER `t_ins2`"), "{sql:?}");
        }

        /// PostgreSQL scopes a trigger to its table and needs it named; MySQL
        /// scopes it to the database and refuses the `ON` clause.
        #[test]
        fn drop_trigger_is_spelled_per_engine() {
            assert_eq!(
                drop_trigger(&my_trigger(), MySql).emit(),
                vec!["DROP TRIGGER `t_ins`;"]
            );
            assert_eq!(
                drop_trigger(&pg_trigger(), Postgres).emit(),
                vec!["DROP TRIGGER \"t_upd\" ON \"orders\";"]
            );
        }

        #[test]
        fn create_trigger_emits_one_statement() {
            let cs = create_trigger(&TriggerDraft::from_info(&my_trigger()), MySql);
            let sql = cs.emit();
            assert_eq!(sql.len(), 1);
            assert!(sql[0].starts_with("CREATE TRIGGER `t_ins` BEFORE INSERT ON `orders`"));
        }

        /// A replace destroys nothing on PostgreSQL, where the plan is one
        /// transaction — but MySQL commits each DDL statement as it runs, so the
        /// preview has to say the table can end up with no trigger at all.
        #[test]
        fn replacing_a_trigger_states_the_drop_first_cost() {
            let t = my_trigger();
            let mut draft = TriggerDraft::from_info(&t);
            draft.info.action = TriggerAction::Body("SET NEW.x = 2".into());
            let risks = diff_trigger(&t, &draft, MySql).destructive();
            assert_eq!(risks.len(), 1);
            assert!(risks[0].contains("drops it first"), "{risks:?}");
        }

        #[test]
        fn dropping_a_trigger_says_what_stops_happening() {
            let risks = drop_trigger(&my_trigger(), MySql).destructive();
            assert_eq!(risks.len(), 1);
            assert!(risks[0].contains("stops"), "{risks:?}");
        }

        /// The model holds both engines' shapes so introspection never lies about
        /// what a server reported; refusing the impossible one is `validate`'s job.
        #[test]
        fn validate_refuses_each_engine_the_other_s_shape() {
            let mut t = my_trigger();
            t.events = vec![TriggerEvent::Insert, TriggerEvent::Update];
            t.condition = Some("NEW.x > 0".into());
            let msgs = TriggerDraft::from_info(&t)
                .validate(MySql, TriggerHost::Table)
                .join(" | ");
            assert!(msgs.contains("one event"), "{msgs}");
            assert!(msgs.contains("no WHEN"), "{msgs}");

            let mut p = pg_trigger();
            p.action = TriggerAction::Body("SET x = 1".into());
            let msgs = TriggerDraft::from_info(&p)
                .validate(Postgres, TriggerHost::Table)
                .join(" | ");
            assert!(msgs.contains("runs a function"), "{msgs}");
        }

        #[test]
        fn validate_catches_the_level_rules_postgresql_enforces() {
            let mut p = pg_trigger();
            p.events = vec![TriggerEvent::Truncate];
            p.level = TriggerLevel::Row;
            let msgs = TriggerDraft::from_info(&p)
                .validate(Postgres, TriggerHost::Table)
                .join(" | ");
            assert!(msgs.contains("FOR EACH STATEMENT"), "{msgs}");

            let mut p = pg_trigger();
            p.timing = TriggerTiming::InsteadOf;
            p.level = TriggerLevel::Statement;
            let msgs = TriggerDraft::from_info(&p)
                .validate(Postgres, TriggerHost::View)
                .join(" | ");
            assert!(msgs.contains("FOR EACH ROW"), "{msgs}");
        }

        /// The timing rules are exact opposites on a table and a view, and the
        /// modal had them inverted — `INSTEAD OF` offered only where the server
        /// always refuses it, while a view's triggers were unreachable. Both
        /// messages measured verbatim on PG 16.14.
        #[test]
        fn validate_knows_which_timings_a_table_and_a_view_can_take() {
            // `"t" is a table … Tables cannot have INSTEAD OF triggers.`
            let mut p = pg_trigger();
            p.timing = TriggerTiming::InsteadOf;
            p.level = TriggerLevel::Row;
            let msgs = TriggerDraft::from_info(&p)
                .validate(Postgres, TriggerHost::Table)
                .join(" | ");
            assert!(msgs.contains("Only a view"), "{msgs}");
            assert!(
                TriggerDraft::from_info(&p)
                    .validate(Postgres, TriggerHost::View)
                    .is_empty()
            );

            // `"v" is a view … Views cannot have row-level BEFORE or AFTER
            // triggers.` — but statement-level is legal there, which is why the
            // rule can't be "a view only takes INSTEAD OF".
            let mut p = pg_trigger();
            p.timing = TriggerTiming::Before;
            p.level = TriggerLevel::Row;
            let msgs = TriggerDraft::from_info(&p)
                .validate(Postgres, TriggerHost::View)
                .join(" | ");
            assert!(msgs.contains("FOR EACH STATEMENT"), "{msgs}");
            assert!(
                TriggerDraft::from_info(&p)
                    .validate(Postgres, TriggerHost::Table)
                    .is_empty()
            );
            p.level = TriggerLevel::Statement;
            assert!(
                TriggerDraft::from_info(&p)
                    .validate(Postgres, TriggerHost::View)
                    .is_empty()
            );
        }

        #[test]
        fn validate_refuses_a_constraint_trigger_and_the_empty_draft() {
            // The constraint-trigger refusal lives on the *set*, not on the
            // member, because it is about a change rather than a state — see
            // `a_constraint_trigger_only_blocks_edits_to_itself`.
            let mut p = pg_trigger();
            p.constraint = true;
            let t = table_with_triggers(vec![p]);
            let mut d = TriggerSetDraft::from_table(&t);
            d.triggers[0].info.condition = Some("new.total > 5".into());
            let msgs = d
                .validate(&t.triggers, Postgres, TriggerHost::Table)
                .join(" | ");
            assert!(msgs.contains("constraint trigger"), "{msgs}");

            let msgs = TriggerDraft::blank("", "", None)
                .validate(MySql, TriggerHost::Table)
                .join(" | ");
            assert!(msgs.contains("needs a name"), "{msgs}");
            assert!(msgs.contains("needs a table"), "{msgs}");
            assert!(msgs.contains("at least one event"), "{msgs}");
        }

        /// A trigger's `WHEN` normalizes by the same rule, and has to survive the
        /// same three input shapes: the server's `pg_get_expr`, the whole clause
        /// out of `pg_get_triggerdef`, and what a person types.
        #[test]
        fn trigger_condition_reduces_every_shape_to_the_bare_guard() {
            assert_eq!(
                trigger_condition("(new.total > 0)", Postgres),
                "new.total > 0"
            );
            assert_eq!(
                trigger_condition("WHEN ((new.total > 0))", Postgres),
                "new.total > 0"
            );
            assert_eq!(
                trigger_condition("new.total > 0", Postgres),
                "new.total > 0"
            );
        }

        #[test]
        fn trigger_condition_keeps_a_column_named_like_the_keyword() {
            assert_eq!(
                trigger_condition("when_due IS NOT NULL", Postgres),
                "when_due IS NOT NULL"
            );
            // Two groups that are not each other's match must not be peeled.
            assert_eq!(trigger_condition("(a) AND (b)", Postgres), "(a) AND (b)");
        }

        /// The gate: a draft taken straight off a table has nothing to say.
        #[test]
        fn an_untouched_check_is_not_a_change() {
            let t = table_with(vec![ck("qty_pos", "(`qty` > 0)")]);
            let d = TableDraft::from_table(&t);
            assert!(diff(&t, &d, MySql).changes.is_empty());
        }

        /// The server re-prints a predicate from its parse tree, so the text that
        /// comes back is never quite the text that went in. Comparing it raw
        /// meant re-typing what the server itself would print counted as an edit.
        #[test]
        fn re_parenthesised_and_re_spaced_predicates_are_the_same_check() {
            let a = ck("c", "((qty > 0))");
            for same in ["(qty > 0)", "qty > 0", "  qty   >   0  "] {
                assert!(
                    checks_equal(&a, &ck("c", same), MySql),
                    "{same:?} should match {:?}",
                    a.expression
                );
            }
            // The stated limit: whitespace *between* tokens is normalized, not
            // tokenisation itself. This costs a re-validating drop-and-add, and
            // is the safe side of the trade.
            assert!(!checks_equal(&a, &ck("c", "qty>0"), MySql));
            // Case folds for identifiers…
            assert!(checks_equal(&a, &ck("c", "QTY > 0"), MySql));
            // …but not inside a literal, where the two really differ.
            let s = ck("c", "status = 'a'");
            assert!(!checks_equal(&s, &ck("c", "status = 'A'"), MySql));
        }

        /// The paren peeler has to know where a group actually closes. `(a) AND
        /// (b)` opens and ends with a paren that aren't each other's match, and a
        /// literal can carry an unbalanced one of its own.
        #[test]
        fn only_parens_wrapping_the_whole_predicate_are_peeled() {
            assert!(checks_equal(
                &ck("c", "((a > 0) AND (b > 0))"),
                &ck("c", "(a > 0) AND (b > 0)"),
                MySql
            ));
            // Not equal: peeling blindly turns the first into `a > 0) AND (b > 0`
            // and would match anything that normalises to the same wreck.
            assert!(!checks_equal(
                &ck("c", "(a > 0) AND (b > 0)"),
                &ck("c", "a > 0) AND (b > 0"),
                MySql
            ));
            // A close-paren inside a string is not the end of the group.
            assert!(checks_equal(
                &ck("c", "(name <> ')')"),
                &ck("c", "name <> ')'"),
                MySql
            ));
        }

        /// A real edit is a drop and an add, in that order — neither engine can
        /// alter a check in place.
        #[test]
        fn an_edited_predicate_drops_and_re_adds() {
            let t = table_with(vec![ck("qty_pos", "qty > 0")]);
            let mut d = TableDraft::from_table(&t);
            d.check_constraints[0].info.expression = "qty > 10".into();
            let cs = diff(&t, &d, MySql);
            assert!(matches!(
                cs.changes.as_slice(),
                [Change::DropCheck { .. }, Change::AddCheck(_)]
            ));
            let sql = cs.emit().join("\n");
            assert!(sql.contains("DROP CONSTRAINT `qty_pos`"), "{sql}");
            assert!(
                sql.contains("ADD CONSTRAINT `qty_pos` CHECK (qty > 10)"),
                "{sql}"
            );
        }

        /// Turning enforcement back on is a change of what the table *accepts*,
        /// so it can't be waved through as cosmetic.
        #[test]
        fn enforcement_is_part_of_the_comparison() {
            let t = table_with(vec![CheckInfo {
                enforced: false,
                ..ck("soft", "(qty > 0)")
            }]);
            let mut d = TableDraft::from_table(&t);
            d.check_constraints[0].info.enforced = true;
            assert_eq!(diff(&t, &d, MySql).changes.len(), 2);
        }

        /// Removing one is the case worth a sentence: nothing is deleted, but the
        /// table stops guaranteeing something, and only the preview can say so.
        #[test]
        fn dropping_a_check_is_reported_as_a_loss_of_the_guarantee() {
            let t = table_with(vec![ck("qty_pos", "(qty > 0)")]);
            let mut d = TableDraft::from_table(&t);
            d.check_constraints.clear();
            let cs = diff(&t, &d, MySql);
            assert!(matches!(cs.changes.as_slice(), [Change::DropCheck { .. }]));
            let risks = cs.changes[0].risks();
            assert_eq!(risks.len(), 1, "{risks:?}");
            assert!(risks[0].contains("qty_pos"), "{risks:?}");
            assert!(cs.changes[0].is_destructive());
        }

        /// PostgreSQL drops every constraint by name through one spelling, and
        /// adds a check inline in the same `ALTER TABLE` as a foreign key —
        /// unlike an index, which has to be a statement of its own.
        #[test]
        fn postgres_adds_and_drops_in_the_one_alter() {
            let t = TableInfo {
                name: "t".into(),
                schema: Some("public".into()),
                columns: vec![col("qty", "integer")],
                check_constraints: vec![ck("old", "((qty > 0))")],
                ..Default::default()
            };
            let mut d = TableDraft::from_table(&t);
            d.check_constraints = vec![CheckDraft::new(ck("fresh", "qty > 5"))];
            let sql = diff(&t, &d, Postgres).emit().join("\n");
            assert_eq!(sql.matches("ALTER TABLE").count(), 1, "{sql}");
            assert!(sql.contains("DROP CONSTRAINT \"old\""), "{sql}");
            assert!(
                sql.contains("ADD CONSTRAINT \"fresh\" CHECK (qty > 5)"),
                "{sql}"
            );
            // `NOT ENFORCED` is MySQL's alone and would be a syntax error here.
            assert!(!sql.contains("NOT ENFORCED"), "{sql}");
        }

        /// **MySQL 8 refuses to rename a column any check names** — `ERROR 3959
        /// … hence column cannot be dropped or renamed` — so the constraint has
        /// to come off before the rename and go back on after, re-pointed.
        /// Measured live: the three clauses run in one `ALTER TABLE`.
        #[test]
        fn a_mysql_rename_drops_and_re_adds_the_checks_that_name_the_column() {
            let t = table_with(vec![ck("qty_pos", "(`qty` > 0)")]);
            let mut d = TableDraft::from_table(&t);
            d.columns[0].info.name = "quantity".into();
            let cs = diff(&t, &d, MySql);
            let sql = cs.emit().join("\n");
            assert!(sql.contains("DROP CONSTRAINT `qty_pos`"), "{sql}");
            assert!(
                sql.contains("ADD CONSTRAINT `qty_pos` CHECK ((`quantity` > 0))"),
                "{sql}"
            );
            // Dropped and immediately re-added is not a lost guarantee, and
            // saying it is would be the preview's only sentence about this plan.
            assert!(
                !cs.destructive().iter().any(|r| r.contains("qty_pos")),
                "{:?}",
                cs.destructive()
            );
        }

        /// Both the servers that *rewrite the predicate themselves* are left
        /// alone: PostgreSQL rewrites its stored parse tree, MariaDB rewrites a
        /// table-level check's text. An unnecessary drop-and-add on PostgreSQL
        /// costs a full validating scan.
        #[test]
        fn a_rename_is_not_re_pointed_where_the_server_does_it() {
            let t = table_with(vec![ck("qty_pos", "(`qty` > 0)")]);
            let mut d = TableDraft::from_table(&t);
            d.columns[0].info.name = "quantity".into();
            for target in [
                Target::from(Postgres),
                Target::new(MySql, ServerFlavour::MariaDb),
            ] {
                let cs = diff(&t, &d, target);
                assert!(
                    !cs.changes
                        .iter()
                        .any(|c| matches!(c, Change::DropCheck { .. } | Change::AddCheck(_))),
                    "{:?}",
                    cs.changes
                );
            }
        }

        /// The walk is over tokens, not bytes: a rename must not reach into a
        /// longer name, a string literal, or a function call that happens to
        /// share the name.
        #[test]
        fn re_pointing_a_rename_does_not_reach_into_look_alikes() {
            let t = table_with(vec![ck(
                "c",
                "`qty_total` > 0 AND note <> 'qty' AND qty(1) > 0",
            )]);
            let mut d = TableDraft::from_table(&t);
            d.columns[0].info.name = "quantity".into();
            let cs = diff(&t, &d, MySql);
            assert!(
                !cs.changes
                    .iter()
                    .any(|c| matches!(c, Change::DropCheck { .. })),
                "{:?}",
                cs.changes
            );
        }
    }

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
                // Two checks, one of them `NOT ENFORCED` — the state a redefine
                // would silently turn back on.
                check_constraints: vec![
                    CheckInfo {
                        name: "articles_chk_1".into(),
                        expression: "`body_len` >= 0".into(),
                        ..Default::default()
                    },
                    CheckInfo {
                        name: "slug_shape".into(),
                        expression: "`slug` <> _utf8mb4''".into(),
                        enforced: false,
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

        /// `world.city` on PostgreSQL — a `serial` (auto-increment, and *not*
        /// also the `nextval` default the catalogue renders it as), a
        /// `character varying`, a defaulted integer, and a primary key that can
        /// only be dropped by its constraint name.
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
                // The bare predicate, as `check_predicate` stores it — the cast is
                // PostgreSQL's own re-printing, the wrapping parens are not.
                check_constraints: vec![CheckInfo {
                    name: "invoice_total_nonneg".into(),
                    expression: "total >= (0)::numeric".into(),
                    ..Default::default()
                }],
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
                // A recreated table that drops its checks accepts data the
                // original refused — and the statement says nothing about it.
                for ck in &t.check_constraints {
                    assert!(
                        sql.contains(&ck.name) && sql.contains(&ck.expression),
                        "{}: check {} missing from\n{sql}",
                        t.name,
                        ck.name
                    );
                    // `NOT ENFORCED` is the half that changes what a write does.
                    assert_eq!(
                        sql.contains(&format!("CHECK ({}) NOT ENFORCED", ck.expression)),
                        !ck.enforced,
                        "{}: enforcement of {} did not survive\n{sql}",
                        t.name,
                        ck.name
                    );
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

        /// A SQLite view. It carries no definer, security type or algorithm —
        /// SQLite has none of them — and the one thing it *does* carry is the
        /// explicit column list, which is part of the object rather than of its
        /// body and would otherwise be dropped by the re-create every edit
        /// there is.
        pub(super) fn sqlite_view() -> TableInfo {
            TableInfo {
                name: "recent".into(),
                columns: vec![col("who"), col("what")],
                is_view: true,
                view_definition: Some("SELECT name, action FROM audit WHERE at > 0".into()),
                view_options: Some(ViewOptions {
                    column_list: Some("who, what".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }
        }

        pub(super) fn view_fixtures() -> Vec<(SqlDialect, TableInfo)> {
            vec![
                (MySql, my_view()),
                (Postgres, pg_view()),
                (SqlDialect::Sqlite, sqlite_view()),
            ]
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
            inline_check: None,
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
            inline_check: None,
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
            inline_check: None,
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

/// The three standalone PostgreSQL objects: enums, domains and sequences.
///
/// Their own module because what they're testing is a different thing from the
/// table differ above — the same reason `lossy_index_tests` is separate.
#[cfg(test)]
mod object_tests {
    use super::*;
    use crate::intel::SqlDialect::Postgres;

    // ── Enums ───────────────────────────────────────────────────────────────

    fn mood() -> EnumInfo {
        EnumInfo {
            name: "mood".into(),
            schema: Some("public".into()),
            values: vec!["sad".into(), "ok".into(), "happy".into()],
            comment: Some("how it went".into()),
        }
    }

    fn enum_cs(current: &EnumInfo, mutate: impl FnOnce(&mut EnumDraft)) -> ChangeSet {
        let mut d = EnumDraft::from_info(current);
        mutate(&mut d);
        diff_enum(current, &d, &[], Postgres)
    }

    #[test]
    fn an_enum_diffed_against_itself_has_no_changes() {
        let e = mood();
        let cs = diff_enum(&e, &EnumDraft::from_info(&e), &[], Postgres);
        assert!(cs.is_empty(), "phantom changes: {:?}", cs.changes);
        assert!(cs.emit().is_empty());
    }

    #[test]
    fn appending_a_value_anchors_on_the_one_before_it() {
        let cs = enum_cs(&mood(), |d| d.info.values.push("elated".into()));
        assert_eq!(cs.len(), 1);
        assert_eq!(
            cs.emit(),
            vec!["ALTER TYPE \"mood\" ADD VALUE 'elated' AFTER 'happy';"]
        );
    }

    #[test]
    fn inserting_a_value_in_the_middle_anchors_on_its_predecessor() {
        let cs = enum_cs(&mood(), |d| d.info.values.insert(1, "meh".into()));
        assert_eq!(
            cs.emit(),
            vec!["ALTER TYPE \"mood\" ADD VALUE 'meh' AFTER 'sad';"]
        );
    }

    #[test]
    fn inserting_at_the_head_anchors_ahead_instead() {
        // There is no predecessor to anchor on, so the only correct clause names
        // what now follows it.
        let cs = enum_cs(&mood(), |d| d.info.values.insert(0, "awful".into()));
        assert_eq!(
            cs.emit(),
            vec!["ALTER TYPE \"mood\" ADD VALUE 'awful' BEFORE 'sad';"]
        );
    }

    /// A run of insertions must arrive in list order. Anchoring each on an
    /// *existing* value instead of on its predecessor would land them all at the
    /// same spot, in reverse.
    #[test]
    fn consecutive_insertions_chain_onto_each_other() {
        let cs = enum_cs(&mood(), |d| {
            d.info.values.insert(1, "a".into());
            d.info.values.insert(2, "b".into());
        });
        assert_eq!(
            cs.emit(),
            vec![
                "ALTER TYPE \"mood\" ADD VALUE 'a' AFTER 'sad';",
                "ALTER TYPE \"mood\" ADD VALUE 'b' AFTER 'a';",
            ]
        );
    }

    #[test]
    fn renaming_a_value_is_an_alter_not_a_rebuild() {
        // No row is touched: a row stores the value's identity, not its label.
        let cs = enum_cs(&mood(), |d| d.info.values[1] = "fine".into());
        assert_eq!(
            cs.emit(),
            vec!["ALTER TYPE \"mood\" RENAME VALUE 'ok' TO 'fine';"]
        );
    }

    /// A value list can't distinguish "I renamed this" from "I deleted it and
    /// typed another", so the plan takes the reading that keeps the data — and
    /// says so, because the two mean very different things about existing rows.
    #[test]
    fn a_rename_discloses_that_it_relabels_every_row() {
        let cs = enum_cs(&mood(), |d| d.info.values[1] = "fine".into());
        let risk = cs.destructive().join(" ");
        assert!(risk.contains("Every row holding ok reads fine"), "{risk}");
        assert!(
            risk.contains("delete it and apply that on its own"),
            "{risk}"
        );
    }

    /// Renames are emitted first, so an insertion may anchor on a value's **new**
    /// label — which is the only name that exists by the time the `ADD` runs.
    #[test]
    fn an_insertion_can_anchor_on_a_value_renamed_in_the_same_plan() {
        let cs = enum_cs(&mood(), |d| {
            d.info.values[1] = "fine".into();
            d.info.values.insert(2, "good".into());
        });
        assert_eq!(
            cs.emit(),
            vec![
                "ALTER TYPE \"mood\" RENAME VALUE 'ok' TO 'fine';",
                "ALTER TYPE \"mood\" ADD VALUE 'good' AFTER 'fine';",
            ]
        );
    }

    /// A swap keeps every value and still can't be done in place — there is no
    /// way to move one — so it rebuilds rather than being read as two renames.
    #[test]
    fn swapping_two_values_rebuilds() {
        let cs = enum_cs(&mood(), |d| d.info.values.swap(0, 2));
        assert!(
            matches!(cs.changes[0], Change::RecreateEnum { .. }),
            "{:?}",
            cs.changes
        );
    }

    #[test]
    fn removing_a_value_rebuilds_because_postgres_cannot_drop_one() {
        let cs = enum_cs(&mood(), |d| {
            d.info.values.retain(|v| v != "ok");
        });
        assert_eq!(cs.len(), 1, "one rebuild, not a mixture: {:?}", cs.changes);
        assert!(matches!(cs.changes[0], Change::RecreateEnum { .. }));
    }

    #[test]
    fn reordering_values_rebuilds_too() {
        // The order *is* the comparison order, so it can't be left alone — and
        // PostgreSQL has no way to move a value.
        let cs = enum_cs(&mood(), |d| d.info.values.reverse());
        assert!(matches!(cs.changes[0], Change::RecreateEnum { .. }));
    }

    /// The rebuild is the dangerous one, so its script is pinned whole.
    #[test]
    fn a_rebuild_parks_the_old_type_and_recasts_every_column() {
        let cur = mood();
        let mut d = EnumDraft::from_info(&cur);
        d.info.values = vec!["ok".into(), "happy".into()];
        let deps = vec![
            TypeDependent {
                schema: Some("public".into()),
                table: "people".into(),
                column: "m".into(),
                type_name: "mood".into(),
                default_value: Some("'ok'::mood".into()),
            },
            TypeDependent {
                schema: Some("public".into()),
                table: "people".into(),
                column: "tags".into(),
                type_name: "mood[]".into(),
                default_value: None,
            },
        ];
        assert_eq!(
            diff_enum(&cur, &d, &deps, Postgres).emit(),
            vec![
                "ALTER TYPE \"mood\" RENAME TO \"mood_schemaic_old\";",
                "CREATE TYPE \"mood\" AS ENUM ('ok', 'happy');\n\
                 COMMENT ON TYPE \"mood\" IS 'how it went';",
                "ALTER TABLE \"people\" ALTER COLUMN \"m\" DROP DEFAULT;",
                "ALTER TABLE \"people\" ALTER COLUMN \"m\" TYPE \"mood\" \
                 USING \"m\"::text::\"mood\";",
                "ALTER TABLE \"people\" ALTER COLUMN \"m\" SET DEFAULT 'ok'::mood;",
                // An array casts through `text[]`; there is no direct cast.
                "ALTER TABLE \"people\" ALTER COLUMN \"tags\" TYPE \"mood\"[] \
                 USING \"tags\"::text[]::\"mood\"[];",
                "DROP TYPE \"mood_schemaic_old\";",
            ]
        );
    }

    #[test]
    fn a_rebuild_names_the_columns_it_recasts() {
        let cur = mood();
        let mut d = EnumDraft::from_info(&cur);
        d.info.values.remove(0);
        let deps = vec![TypeDependent {
            schema: Some("sales".into()),
            table: "people".into(),
            column: "m".into(),
            type_name: "mood".into(),
            default_value: None,
        }];
        let risk = diff_enum(&cur, &d, &deps, Postgres).destructive().join(" ");
        assert!(risk.contains("sales.people.m"), "{risk}");
        // And admits the list can't be complete.
        assert!(risk.contains("view or function"), "{risk}");
    }

    /// Adding a value destroys nothing and is still disclosed: it is the one edit
    /// here PostgreSQL offers no way to undo.
    #[test]
    fn adding_a_value_warns_that_it_cannot_be_taken_back() {
        let cs = enum_cs(&mood(), |d| d.info.values.push("elated".into()));
        let risk = cs.destructive().join(" ");
        assert!(risk.contains("can't remove an enum value"), "{risk}");
        assert!(risk.contains("until this plan is applied"), "{risk}");
    }

    #[test]
    fn renaming_the_type_runs_after_everything_addressing_it() {
        let cs = enum_cs(&mood(), |d| {
            d.info.values.push("elated".into());
            d.info.name = "feeling".into();
        });
        let sql = cs.emit();
        assert_eq!(sql.len(), 2);
        assert!(
            sql[0].starts_with("ALTER TYPE \"mood\" ADD VALUE"),
            "{sql:?}"
        );
        assert_eq!(sql[1], "ALTER TYPE \"mood\" RENAME TO \"feeling\";");
    }

    #[test]
    fn a_rebuild_does_not_also_restate_the_comment() {
        // `CREATE TYPE` carries it, so a second `COMMENT ON` would be one
        // statement saying what the one above it already said.
        let cs = enum_cs(&mood(), |d| {
            d.info.values.remove(0);
            d.info.comment = Some("different".into());
        });
        assert_eq!(cs.len(), 1);
        assert!(cs.emit().iter().any(|s| s.contains("IS 'different'")));
    }

    #[test]
    fn clearing_a_comment_emits_null_rather_than_an_empty_string() {
        let cs = enum_cs(&mood(), |d| d.info.comment = None);
        assert_eq!(cs.emit(), vec!["COMMENT ON TYPE \"mood\" IS NULL;"]);
    }

    #[test]
    fn a_duplicate_enum_value_is_caught_before_the_apply() {
        let mut d = EnumDraft::from_info(&mood());
        d.info.values.push("ok".into());
        assert!(d.validate().iter().any(|m| m.contains("more than once")));
        // An empty enum is legal, if useless, so it isn't rejected.
        let mut empty = EnumDraft::blank("t", Some("public".into()));
        assert!(empty.validate().is_empty());
        empty.info.name = String::new();
        assert!(!empty.validate().is_empty());
    }

    #[test]
    fn a_new_enum_creates_rather_than_alters() {
        let d = EnumDraft {
            original: None,
            info: EnumInfo {
                name: "mood".into(),
                schema: Some("sales".into()),
                values: vec!["ok".into()],
                comment: None,
            },
        };
        assert_eq!(
            create_enum(&d, Postgres).emit(),
            vec!["CREATE TYPE \"sales\".\"mood\" AS ENUM ('ok');"]
        );
    }

    // ── Standalone objects: domains ─────────────────────────────────────────

    fn email() -> DomainInfo {
        DomainInfo {
            name: "email".into(),
            schema: Some("public".into()),
            base_type: "character varying(255)".into(),
            collation: None,
            collation_schema: None,
            default_value: Some("''::character varying".into()),
            not_null: true,
            checks: vec![CheckInfo {
                name: "email_shaped".into(),
                expression: "(VALUE)::text ~ '@'::text".into(),
                ..Default::default()
            }],
            comment: Some("an address".into()),
        }
    }

    fn domain_cs(current: &DomainInfo, mutate: impl FnOnce(&mut DomainDraft)) -> ChangeSet {
        let mut d = DomainDraft::from_info(current);
        mutate(&mut d);
        diff_domain(current, &d, &[], Postgres)
    }

    #[test]
    fn a_domain_diffed_against_itself_has_no_changes() {
        let d = email();
        let cs = diff_domain(&d, &DomainDraft::from_info(&d), &[], Postgres);
        assert!(cs.is_empty(), "phantom changes: {:?}", cs.changes);
    }

    /// A collation is an object like any other, and the `COLLATE` clause
    /// resolves through `search_path`. Emitted bare, a collation in another
    /// namespace either doesn't exist (`ERROR: collation "mycoll" for encoding
    /// "UTF8" does not exist`) or — measured on 16.14 — binds a *different*,
    /// same-named one that is on the path, so the rebuilt domain compares under
    /// another locale and every index over it is rebuilt with another ordering.
    #[test]
    fn a_domains_collation_is_emitted_with_its_namespace() {
        let mut d = email();
        d.base_type = "text".into();
        d.collation = Some("mycoll".into());
        d.collation_schema = Some("s31b".into());
        assert!(
            d.create_sql(Postgres)
                .contains("COLLATE \"s31b\".\"mycoll\""),
            "{}",
            d.create_sql(Postgres)
        );
        // A built-in carries no namespace — `pg_catalog` is searched first and
        // can't be shadowed — and `public` follows `qualified_ident`'s rule.
        d.collation = Some("C".into());
        d.collation_schema = None;
        assert!(d.create_sql(Postgres).contains("COLLATE \"C\""), "{d:?}");
        // …and it round-trips, so no designer opens on a phantom change.
        let cs = diff_domain(&d, &DomainDraft::from_info(&d), &[], Postgres);
        assert!(cs.is_empty(), "phantom changes: {:?}", cs.changes);
        let mut ns = d.clone();
        ns.collation_schema = Some("s31b".into());
        let cs = diff_domain(&ns, &DomainDraft::from_info(&ns), &[], Postgres);
        assert!(cs.is_empty(), "phantom changes: {:?}", cs.changes);
    }

    /// The same normalization a column's type gets, for the same reason: the
    /// server says `character varying(255)` and a person types `varchar(255)`.
    #[test]
    fn an_equivalent_base_type_is_not_a_rebuild() {
        let cs = domain_cs(&email(), |d| d.info.base_type = "varchar(255)".into());
        assert!(cs.is_empty(), "{:?}", cs.changes);
    }

    #[test]
    fn changing_the_base_type_rebuilds_because_alter_domain_cannot() {
        let cs = domain_cs(&email(), |d| d.info.base_type = "text".into());
        assert_eq!(cs.len(), 1);
        assert!(matches!(cs.changes[0], Change::RecreateDomain { .. }));
        assert!(cs.emit()[0].starts_with("ALTER DOMAIN \"email\" RENAME TO"));
        assert!(cs.emit().last().unwrap().starts_with("DROP DOMAIN"));
    }

    /// Narrowing a domain's base type **destroys data and commits**, and the
    /// only sentence the preview showed promised the opposite: `recreate_risk`
    /// says a value the new definition doesn't accept "fails the whole plan,
    /// and nothing is applied".
    ///
    /// It doesn't. `recreate_type_sql` re-cast every dependent column with
    /// `USING col::text::domain`, and an **explicit** cast to `varchar(n)` /
    /// `numeric(p,s)` truncates and rounds where the assignment cast a bare
    /// `ALTER COLUMN … TYPE` uses *refuses*. Measured on PG 16.14: 64 → 16
    /// characters destroyed and committed; `numeric(10,4)` 1.2345 → 1.23.
    ///
    /// `column_risks`' narrowing analysis is the existing, tested answer to
    /// exactly this question — `RecreateDomain` was the one narrowing path that
    /// skipped it.
    #[test]
    fn narrowing_a_domains_base_type_says_what_it_costs() {
        let dep = TypeDependent {
            schema: Some("public".into()),
            table: "people".into(),
            column: "addr".into(),
            type_name: "email".into(),
            default_value: None,
        };
        let mut d = DomainDraft::from_info(&email());
        d.info.base_type = "varchar(16)".into();
        let cs = diff_domain(&email(), &d, std::slice::from_ref(&dep), Postgres);
        let risks = cs.destructive().join(" ");
        assert!(risks.contains("truncates"), "{risks}");

        // The scale half — the direction that loses data without the statement
        // complaining at all.
        let mut num = email();
        num.base_type = "numeric(10,4)".into();
        let mut d = DomainDraft::from_info(&num);
        d.info.base_type = "numeric(10,2)".into();
        let cs = diff_domain(&num, &d, std::slice::from_ref(&dep), Postgres);
        let risks = cs.destructive().join(" ");
        assert!(risks.contains("rounds"), "{risks}");

        // Widening costs nothing and must not manufacture a warning.
        let mut d = DomainDraft::from_info(&email());
        d.info.base_type = "varchar(512)".into();
        let cs = diff_domain(&email(), &d, std::slice::from_ref(&dep), Postgres);
        let risks = cs.destructive().join(" ");
        assert!(!risks.contains("truncates"), "{risks}");
    }

    /// The other half: stop the truncation happening at all.
    ///
    /// A domain rebuild re-casts through `text` and then **explicitly** to the
    /// new domain, which is what silently truncates. Dropping that second cast
    /// leaves an assignment cast, which PostgreSQL refuses — verified live: the
    /// bare form and `USING a::text` both error `value too long for type
    /// character varying(16)` where `USING a::text::d16` committed a 16-char
    /// value over a 64-char one.
    ///
    /// An **enum** rebuild still needs both casts — `USING m::text` alone is
    /// rejected with "cannot be cast automatically to type mood" — so the two
    /// object kinds diverge here on purpose.
    #[test]
    fn a_domain_rebuild_recasts_without_the_truncating_explicit_cast() {
        let dep = TypeDependent {
            schema: Some("public".into()),
            table: "people".into(),
            column: "addr".into(),
            type_name: "email".into(),
            default_value: None,
        };
        let mut d = DomainDraft::from_info(&email());
        d.info.base_type = "varchar(16)".into();
        let sql = diff_domain(&email(), &d, std::slice::from_ref(&dep), Postgres)
            .emit()
            .join("\n");
        assert!(
            sql.contains("USING \"addr\"::text;"),
            "domain recast should stop at text: {sql}"
        );
        assert!(
            !sql.contains("::text::"),
            "the second, explicit cast is what truncates: {sql}"
        );
    }

    /// The counterpart to the domain rule, pinned because it looks like the
    /// same code and must not be "simplified" to match.
    ///
    /// An enum rebuild keeps **both** casts. PostgreSQL 16.14 rejects
    /// `USING m::text` on its own — "result of USING clause for column m cannot
    /// be cast automatically to type mood" — because there is no assignment
    /// cast from text to an enum. A domain gives the second cast up precisely
    /// because it *does* have one, and that cast truncates.
    #[test]
    fn an_enum_rebuild_keeps_the_explicit_cast_a_domain_gives_up() {
        let mut e = mood();
        e.values = vec!["ok".into(), "bad".into()];
        let mut d = EnumDraft::from_info(&e);
        // Removing a value is what collapses an enum edit into a full rebuild.
        d.info.values = vec!["ok".into()];
        let dep = TypeDependent {
            schema: Some("public".into()),
            table: "t".into(),
            column: "m".into(),
            type_name: "mood".into(),
            default_value: None,
        };
        let sql = diff_enum(&e, &d, std::slice::from_ref(&dep), Postgres)
            .emit()
            .join("\n");
        assert!(
            sql.contains("USING \"m\"::text::\"mood\";"),
            "an enum needs the explicit cast: {sql}"
        );
    }

    #[test]
    fn a_domains_default_nullability_and_checks_alter_in_place() {
        let cs = domain_cs(&email(), |d| {
            d.info.default_value = None;
            d.info.not_null = false;
            d.info.checks = vec![CheckInfo {
                name: "email_shaped".into(),
                expression: "(VALUE)::text ~ '@example'::text".into(),
                ..Default::default()
            }];
        });
        assert_eq!(
            cs.emit(),
            vec![
                "ALTER DOMAIN \"email\" DROP DEFAULT;",
                "ALTER DOMAIN \"email\" DROP NOT NULL;",
                // Drop before add, so the name can be reused within one plan.
                "ALTER DOMAIN \"email\" DROP CONSTRAINT \"email_shaped\";",
                "ALTER DOMAIN \"email\" ADD CONSTRAINT \"email_shaped\" \
                 CHECK ((VALUE)::text ~ '@example'::text);",
            ]
        );
    }

    /// Both directions are disclosed: dropping it changes what every column of
    /// the domain accepts, and setting it can fail against rows already there.
    #[test]
    fn both_nullability_directions_are_disclosed() {
        let off = domain_cs(&email(), |d| d.info.not_null = false)
            .destructive()
            .join(" ");
        assert!(off.contains("starts accepting NULL"), "{off}");
        let mut nullable = email();
        nullable.not_null = false;
        let on = diff_domain(&nullable, &DomainDraft::from_info(&email()), &[], Postgres)
            .destructive()
            .join(" ");
        assert!(on.contains("fails if any column"), "{on}");
    }

    #[test]
    fn a_retyped_predicate_that_means_the_same_is_not_a_change() {
        // `checks_equal` governs a domain's constraints exactly as it does a
        // table's — modulo wrapping parens and whitespace.
        let cs = domain_cs(&email(), |d| {
            d.info.checks[0].expression = "((VALUE)::text  ~  '@'::text)".into();
        });
        assert!(cs.is_empty(), "{:?}", cs.changes);
    }

    // ── Standalone objects: sequences ───────────────────────────────────────

    fn counter() -> SequenceInfo {
        SequenceInfo {
            name: "counter".into(),
            schema: Some("public".into()),
            last_value: Some(41),
            ..Default::default()
        }
    }

    #[test]
    fn a_sequence_diffed_against_itself_has_no_changes() {
        let s = counter();
        let cs = diff_sequence(&s, &SequenceDraft::from_info(&s), Postgres);
        assert!(cs.is_empty(), "phantom changes: {:?}", cs.changes);
    }

    #[test]
    fn a_sequence_alter_restates_only_what_changed() {
        let s = counter();
        let mut d = SequenceDraft::from_info(&s);
        d.info.increment = 5;
        d.info.cycle = true;
        assert_eq!(
            diff_sequence(&s, &d, Postgres).emit(),
            vec!["ALTER SEQUENCE \"counter\"\n  INCREMENT BY 5\n  CYCLE;"]
        );
    }

    /// Moving the counter is a different act from changing where a later restart
    /// would return to, so the two stay separate **changes** with separate
    /// sentences — but they share the statement, because PostgreSQL cross-checks
    /// new bounds against the sequence's current value in any `ALTER SEQUENCE`
    /// that doesn't also restart it.
    #[test]
    fn restarting_is_its_own_change_but_rides_in_the_same_statement() {
        let s = counter();
        let mut d = SequenceDraft::from_info(&s);
        d.info.start = 100;
        d.restart = Some(500);
        let cs = diff_sequence(&s, &d, Postgres);
        assert_eq!(cs.len(), 2, "{:?}", cs.changes);
        assert_eq!(
            cs.emit(),
            vec!["ALTER SEQUENCE \"counter\"\n  START WITH 100\n  RESTART WITH 500;"]
        );
        assert!(cs.destructive().iter().any(|r| r.contains("collides")));
    }

    /// A restart on its own is still a statement of its own — there is no
    /// `ALTER SEQUENCE` for it to ride in.
    #[test]
    fn a_lone_restart_is_its_own_statement() {
        let s = counter();
        let mut d = SequenceDraft::from_info(&s);
        d.restart = Some(500);
        assert_eq!(
            diff_sequence(&s, &d, Postgres).emit(),
            vec!["ALTER SEQUENCE \"counter\" RESTART WITH 500;"]
        );
    }

    /// The edit the split made impossible: narrowing the range below where the
    /// counter sits is legal only when the same statement restarts it.
    /// Measured on 16.14 — `ALTER SEQUENCE s MAXVALUE 100` on a sequence at 500
    /// is `ERROR: RESTART value (500) cannot be greater than MAXVALUE (100)`.
    #[test]
    fn narrowing_below_the_counter_is_emitted_with_its_restart() {
        let mut s = counter();
        s.last_value = Some(500);
        let mut d = SequenceDraft::from_info(&s);
        d.info.max_value = 100;
        d.restart = Some(50);
        assert!(d.validate().is_empty(), "{:?}", d.validate());
        let sql = diff_sequence(&s, &d, Postgres).emit().join("\n");
        assert_eq!(sql.matches("ALTER SEQUENCE").count(), 1, "{sql}");
        assert!(sql.contains("MAXVALUE 100"), "{sql}");
        assert!(sql.contains("RESTART WITH 50"), "{sql}");
    }

    /// …and without the restart it is the editor that says so, not the server
    /// halfway through a plan.
    #[test]
    fn narrowing_below_the_counter_without_a_restart_is_rejected_up_front() {
        let mut s = counter();
        s.last_value = Some(500);
        let mut d = SequenceDraft::from_info(&s);
        d.info.max_value = 100;
        let msgs = d.validate().join(" ");
        assert!(msgs.contains("at 500"), "{msgs}");
        // The same edit with a restart is fine, so the message must not fire.
        d.restart = Some(50);
        assert!(d.validate().is_empty(), "{:?}", d.validate());
    }

    /// A restart doesn't survive a re-introspection, so keeping it in the model
    /// would make every re-opened editor dirty against a sequence nothing changed.
    #[test]
    fn a_restart_is_not_part_of_the_sequence_model() {
        let s = counter();
        let d = SequenceDraft::from_info(&s);
        assert_eq!(d.restart, None);
        assert!(diff_sequence(&s, &d, Postgres).is_empty());
    }

    #[test]
    fn detaching_a_sequence_from_its_column_says_owned_by_none() {
        let mut s = counter();
        s.owned_by = Some(crate::schema::SequenceOwner {
            table: "orders".into(),
            column: "id".into(),
            internal: false,
        });
        let mut d = SequenceDraft::from_info(&s);
        d.info.owned_by = None;
        assert_eq!(
            diff_sequence(&s, &d, Postgres).emit(),
            vec!["ALTER SEQUENCE \"counter\"\n  OWNED BY NONE;"]
        );
    }

    #[test]
    fn a_nonsensical_sequence_is_caught_before_the_apply() {
        let mut d = SequenceDraft::from_info(&counter());
        d.info.increment = 0;
        assert!(d.validate().iter().any(|m| m.contains("increment by 0")));

        let mut d = SequenceDraft::from_info(&counter());
        d.info.start = 900;
        d.info.max_value = 100;
        let msgs = d.validate().join(" ");
        assert!(msgs.contains("outside"), "{msgs}");

        // Bounds have to fit the storage type, or the server refuses.
        let mut d = SequenceDraft::from_info(&counter());
        d.info.data_type = "smallint".into();
        assert!(d.validate().iter().any(|m| m.contains("doesn't fit")));

        // And a restart outside the range is caught with the rest.
        let mut d = SequenceDraft::from_info(&counter());
        d.restart = Some(-5);
        assert!(d.validate().iter().any(|m| m.contains("Restarting at -5")));
    }

    // ── Shared: rename, drop, comment ───────────────────────────────────────

    #[test]
    fn each_kind_is_dropped_with_its_own_keyword() {
        for (kind, kw) in [
            (ObjectKind::Enum, "TYPE"),
            (ObjectKind::Domain, "DOMAIN"),
            (ObjectKind::Sequence, "SEQUENCE"),
        ] {
            assert_eq!(
                drop_object(kind, "x", Some("sales"), Postgres).emit(),
                vec![format!("DROP {kw} \"sales\".\"x\";")]
            );
        }
    }

    /// Never `CASCADE`: cascading drops the *columns* built on the type, which is
    /// a far larger act than the one asked for.
    #[test]
    fn dropping_an_object_never_cascades() {
        let cs = drop_object(ObjectKind::Enum, "mood", None, Postgres);
        assert!(!cs.emit()[0].contains("CASCADE"));
        assert!(cs.destructive()[0].contains("refuses while a column still uses it"));
    }

    #[test]
    fn dropping_a_sequence_says_what_stops_working() {
        let cs = drop_object(ObjectKind::Sequence, "counter", None, Postgres);
        assert!(cs.destructive()[0].contains("nextval"));
    }

    // ── Which columns a rebuild has to touch ────────────────────────────────

    fn schema_using_mood() -> crate::schema::DbSchema {
        let colt = |name: &str, ty: &str| ColumnInfo {
            name: name.into(),
            type_name: ty.into(),
            ..Default::default()
        };
        crate::schema::DbSchema {
            tables: vec![
                TableInfo {
                    name: "people".into(),
                    schema: Some("public".into()),
                    columns: vec![
                        colt("m", "mood"),
                        colt("tags", "mood[]"),
                        colt("qualified", "public.mood"),
                        colt("other", "text"),
                    ],
                    ..Default::default()
                },
                // Another namespace's same-named type is a different type — and
                // saying so takes the **qualifier**, not the table's namespace.
                // This column used to be declared a bare `mood`, which
                // `format_type` only writes for a type on the `search_path`,
                // i.e. `public.mood`: the fixture asserted the table's namespace
                // decided the type's identity, which is exactly the defect.
                TableInfo {
                    name: "elsewhere".into(),
                    schema: Some("sales".into()),
                    columns: vec![colt("m", "sales.mood")],
                    ..Default::default()
                },
                // A view has no storage to re-cast.
                TableInfo {
                    name: "v".into(),
                    schema: Some("public".into()),
                    is_view: true,
                    columns: vec![colt("m", "mood")],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn dependents_cover_arrays_and_qualified_names_but_not_other_namespaces() {
        let deps = type_dependents(&schema_using_mood(), Some("public"), "mood");
        let names: Vec<&str> = deps.iter().map(|d| d.column.as_str()).collect();
        assert_eq!(names, vec!["m", "tags", "qualified"]);
        assert!(deps[1].is_array());
        assert!(!deps[0].is_array());
        // The other namespace's `mood` is a different type entirely.
        assert_eq!(
            type_dependents(&schema_using_mood(), Some("sales"), "mood").len(),
            1
        );
    }

    /// Three draft validators had the same omission: a section with no arm,
    /// where every sibling guarded its equivalent.
    ///
    /// Each reaches the server as a statement it always rejects, and the enum
    /// one is worse than a failed apply — PostgreSQL has no `DROP VALUE`, so an
    /// empty label can only be taken back by a full type rebuild.
    #[test]
    fn the_draft_validators_catch_what_the_server_would_reject() {
        // A blank check on a table: two clicks in the designer, emitted as
        // `ADD CONSTRAINT `t_chk` CHECK ()`.
        let mut t = TableDraft::blank("t", None);
        t.columns.push(ColumnDraft::new(ColumnInfo {
            name: "id".into(),
            type_name: "int".into(),
            ..Default::default()
        }));
        t.check_constraints.push(CheckDraft::new(CheckInfo {
            name: "t_chk".into(),
            expression: "  ".into(),
            ..Default::default()
        }));
        let msgs = t.validate().join(" | ");
        assert!(msgs.contains("no predicate"), "{msgs}");

        // Two checks sharing a name — the shape the domain editor's suffix
        // used to propose after a remove-then-add.
        let mut t2 = TableDraft::blank("t", None);
        t2.columns.push(ColumnDraft::new(ColumnInfo {
            name: "id".into(),
            type_name: "int".into(),
            ..Default::default()
        }));
        for _ in 0..2 {
            t2.check_constraints.push(CheckDraft::new(CheckInfo {
                name: "dup".into(),
                expression: "id > 0".into(),
                ..Default::default()
            }));
        }
        assert!(t2.validate().join(" | ").contains("both called dup"));

        // An empty enum label.
        let mut e = EnumDraft::from_info(&mood());
        e.info.values.push(String::new());
        assert!(
            e.validate().join(" | ").contains("can never remove it"),
            "{:?}",
            e.validate()
        );

        // A domain naming two constraints the same.
        let mut d = DomainDraft::from_info(&email());
        d.info.checks = vec![
            CheckInfo {
                name: "email_check1".into(),
                expression: "VALUE IS NOT NULL".into(),
                ..Default::default()
            },
            CheckInfo {
                name: "email_check1".into(),
                expression: "VALUE <> ''".into(),
                ..Default::default()
            },
        ];
        assert!(d.validate().join(" | ").contains("named twice"));
    }

    /// Two values added at the head must not anchor on each other.
    ///
    /// The head insertion has no predecessor, so it anchors `BEFORE` the *next*
    /// slot — which was taken as `want[1]` unconditionally. When slot 1 is
    /// itself a new value, that names a label PostgreSQL doesn't have yet:
    /// `ERROR: "bad" is not an existing enum label`, and since the plan runs in
    /// one transaction the whole edit rolls back. Two values could not be added
    /// at the head of an enum at all.
    #[test]
    fn two_values_added_at_the_head_anchor_on_one_that_exists() {
        let cur = EnumInfo {
            name: "mood".into(),
            schema: Some("public".into()),
            values: vec!["dire".into(), "ok".into()],
            comment: None,
        };
        let mut d = EnumDraft::from_info(&cur);
        d.info.values = vec!["awful".into(), "bad".into(), "dire".into(), "ok".into()];
        let sql = diff_enum(&cur, &d, &[], Postgres).emit().join("\n");

        // Whatever each `ADD VALUE` anchors on has to exist when it runs: either
        // an original value, or one an earlier statement in this plan added.
        assert!(
            sql.contains("ADD VALUE 'awful' BEFORE 'dire'"),
            "head insertion must anchor on a surviving value: {sql}"
        );
        assert!(
            sql.contains("ADD VALUE 'bad' AFTER 'awful'"),
            "the second may anchor on the first, which now exists: {sql}"
        );
    }

    /// The dependent is decided by the **type's** identity, not the table's.
    ///
    /// `type_dependents` filtered on `t.schema != schema` and then stripped the
    /// qualifier before comparing names, so it answered a question nobody
    /// asked — "which columns of tables in this namespace are declared with
    /// something *called* this". Two opposite faults, and the function's own
    /// doc claimed neither happened:
    ///
    /// - a `public.orders.state` column declared `sales.status` was re-cast to
    ///   `public.status`, silently retyping a column the user never edited;
    /// - a `sales.audit.state` column declared `public.status` was **skipped**,
    ///   so the final `DROP TYPE` failed and the rebuild became impossible with
    ///   no way to see why.
    #[test]
    fn dependents_are_matched_on_the_types_identity_not_the_tables_namespace() {
        let colt = |name: &str, ty: &str| ColumnInfo {
            name: name.into(),
            type_name: ty.into(),
            ..Default::default()
        };
        let db = crate::schema::DbSchema {
            tables: vec![
                TableInfo {
                    name: "orders".into(),
                    schema: Some("public".into()),
                    // Another namespace's type, in a table in *this* one.
                    columns: vec![colt("state", "sales.status"), colt("ok", "status")],
                    ..Default::default()
                },
                TableInfo {
                    name: "audit".into(),
                    schema: Some("sales".into()),
                    // *This* namespace's type, in a table somewhere else.
                    columns: vec![colt("state", "public.status")],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let deps = type_dependents(&db, Some("public"), "status");
        let got: Vec<(&str, &str)> = deps
            .iter()
            .map(|d| (d.table.as_str(), d.column.as_str()))
            .collect();
        // `public.orders.ok` (unqualified ⇒ on the search_path ⇒ public) and
        // `sales.audit.state` (explicitly public). Not `orders.state`, which is
        // `sales.status`.
        assert_eq!(got, vec![("orders", "ok"), ("audit", "state")], "{got:?}");

        // And the mirror: `sales.status`'s one dependent lives in `public`.
        let deps = type_dependents(&db, Some("sales"), "status");
        let got: Vec<(&str, &str)> = deps
            .iter()
            .map(|d| (d.table.as_str(), d.column.as_str()))
            .collect();
        assert_eq!(got, vec![("orders", "state")], "{got:?}");
    }

    #[test]
    fn a_type_nothing_uses_still_warns_about_what_cannot_be_listed() {
        let cur = mood();
        let mut d = EnumDraft::from_info(&cur);
        d.info.values.remove(0);
        let risk = diff_enum(&cur, &d, &[], Postgres).destructive().join(" ");
        assert!(risk.contains("Nothing uses it today"), "{risk}");
    }
}

#[cfg(test)]
mod sqlite_drop_tests {
    use super::*;
    use crate::intel::SqlDialect::{MySql, Postgres, Sqlite};

    fn drop_index(name: &str, constraint: Option<&str>) -> Change {
        Change::DropIndex {
            name: name.into(),
            constraint: constraint.map(str::to_string),
        }
    }

    fn drop_column(name: &str) -> Change {
        Change::DropColumn {
            name: name.into(),
            type_name: "TEXT".into(),
        }
    }

    /// The four SQLite has a statement for. Each is a whole-object drop or the
    /// one `ALTER TABLE` form it does have — none needs the rebuild.
    #[test]
    fn sqlite_expresses_the_drops_it_has_statements_for() {
        for c in [
            Change::DropTable,
            Change::DropView {
                materialized: false,
            },
            drop_column("email"),
            drop_index("ix_email", None),
        ] {
            assert!(supports_change(Sqlite, &c), "{c:?}");
        }
    }

    /// No `ALTER TABLE … DROP CONSTRAINT` exists in SQLite, so this one really
    /// does need the twelve-step rebuild — the menu hides it.
    #[test]
    fn sqlite_cannot_express_a_dropped_foreign_key() {
        assert!(!supports_change(
            Sqlite,
            &Change::DropForeignKey { name: "fk".into() }
        ));
    }

    /// A UNIQUE constraint's backing index is part of the table definition in
    /// SQLite; `DROP INDEX` refuses it, so it is the rebuild again.
    #[test]
    fn sqlite_cannot_express_a_constraint_backed_index() {
        assert!(!supports_change(Sqlite, &drop_index("uq", Some("uq"))));
    }

    /// Anything that reaches the designer stays out on SQLite, whatever else
    /// this predicate lets through.
    #[test]
    fn sqlite_expresses_no_designer_change() {
        for c in [
            Change::RenameTable { to: "t2".into() },
            Change::TruncateTable,
            Change::PrimaryKey {
                from: vec!["id".into()],
                to: vec![],
                drop_constraint: None,
            },
        ] {
            assert!(!supports_change(Sqlite, &c), "{c:?}");
        }
    }

    /// The predicate is about SQLite. The two engines with a full emitter
    /// express everything it is ever asked about.
    #[test]
    fn the_full_engines_express_every_drop() {
        for d in [MySql, Postgres] {
            for c in [
                Change::DropTable,
                Change::DropForeignKey { name: "fk".into() },
                drop_index("uq", Some("uq")),
                Change::RenameTable { to: "t2".into() },
            ] {
                assert!(supports_change(d, &c), "{d:?} {c:?}");
            }
        }
    }

    /// SQLite has no `ALTER TABLE … DROP INDEX` — the index is dropped by its
    /// own statement, as it is on PostgreSQL.
    #[test]
    fn sqlite_drops_an_index_by_its_own_statement() {
        let cs = single("t", None, Sqlite, drop_index("ix_email", None));
        assert_eq!(cs.emit(), vec!["DROP INDEX \"ix_email\";"]);
    }

    /// **One operation per `ALTER TABLE`** — SQLite refuses a clause list, so
    /// two dropped columns are two statements, not MySQL's single coalesced
    /// `ALTER`.
    #[test]
    fn sqlite_drops_each_column_in_its_own_alter() {
        let cs = ChangeSet {
            table: "users".into(),
            schema: None,
            dialect: Sqlite,
            flavour: ServerFlavour::Unknown,
            changes: vec![drop_column("email"), drop_column("phone")],
        };
        assert_eq!(
            cs.emit(),
            vec![
                "ALTER TABLE \"users\" DROP COLUMN \"email\";",
                "ALTER TABLE \"users\" DROP COLUMN \"phone\";",
            ]
        );
    }

    #[test]
    fn sqlite_drops_a_table_and_a_view_like_anyone_else() {
        assert_eq!(
            single("t", None, Sqlite, Change::DropTable).emit(),
            vec!["DROP TABLE \"t\";"]
        );
        assert_eq!(
            single(
                "v",
                None,
                Sqlite,
                Change::DropView {
                    materialized: false
                }
            )
            .emit(),
            vec!["DROP VIEW \"v\";"]
        );
    }

    /// The emitter is the backstop, not a second gate: a change SQLite can't
    /// express must not come out as MySQL's spelling of it, which is what the
    /// missing arm used to do.
    #[test]
    fn sqlite_emits_nothing_for_a_change_it_cannot_express() {
        let cs = single(
            "t",
            None,
            Sqlite,
            Change::DropForeignKey { name: "fk".into() },
        );
        assert!(cs.emit().is_empty(), "{:?}", cs.emit());
    }

    /// Emitting less than the plan asks for is only honest if the plan says so
    /// — dropping a column a foreign key stands on is the case that reaches
    /// here, because the draft differ takes the constraint off first.
    #[test]
    fn a_withheld_change_is_reported_rather_than_dropped() {
        let cs = ChangeSet {
            table: "orders".into(),
            schema: None,
            dialect: Sqlite,
            flavour: ServerFlavour::Unknown,
            changes: vec![
                Change::DropForeignKey {
                    name: "fk_customer".into(),
                },
                drop_column("customer_id"),
            ],
        };
        assert_eq!(cs.emit().len(), 1, "only the column drop is expressible");
        let withheld = cs.unsupported();
        assert_eq!(withheld.len(), 1);
        assert!(withheld[0].contains("fk_customer"), "{withheld:?}");
    }

    /// Nothing is withheld when everything is expressible, on any engine.
    #[test]
    fn a_plan_the_engine_can_express_withholds_nothing() {
        assert!(
            single("t", None, Sqlite, Change::DropTable)
                .unsupported()
                .is_empty()
        );
        for d in [MySql, Postgres] {
            assert!(
                single("t", None, d, Change::DropForeignKey { name: "fk".into() })
                    .unsupported()
                    .is_empty(),
                "{d:?}"
            );
        }
    }
}

#[cfg(test)]
mod sqlite_create_tests {
    use super::*;
    use crate::intel::SqlDialect::Sqlite;
    use crate::schema::{CheckInfo, ForeignKeyInfo, IndexColumn, IndexInfo};

    fn col(name: &str, ty: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: ty.into(),
            nullable: true,
            ..Default::default()
        }
    }

    fn emit(t: &TableInfo) -> Vec<String> {
        create_table_sql(&TableDraft::from_table(t), Sqlite)
    }

    fn one(t: &TableInfo) -> String {
        emit(t).join("\n")
    }

    /// A rowid table with an `INTEGER PRIMARY KEY AUTOINCREMENT`. The keyword is
    /// **inline or nothing** in SQLite — there is no table-level form of it, and
    /// `AUTO_INCREMENT` is MySQL's spelling of a different thing.
    fn autoinc() -> TableInfo {
        TableInfo {
            name: "users".into(),
            columns: vec![
                ColumnInfo {
                    name: "id".into(),
                    type_name: "INTEGER".into(),
                    nullable: false,
                    primary_key: true,
                    auto_increment: true,
                    ..Default::default()
                },
                col("email", "TEXT"),
            ],
            indexes: vec![IndexInfo {
                name: "PRIMARY".into(),
                columns: vec![IndexColumn::plain("id")],
                unique: true,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn an_autoincrement_key_is_written_inline() {
        let sql = one(&autoinc());
        assert!(
            sql.contains(r#""id" INTEGER PRIMARY KEY AUTOINCREMENT"#),
            "{sql}"
        );
        assert!(!sql.contains("AUTO_INCREMENT"), "MySQL's spelling: {sql}");
        assert!(
            !sql.contains("PRIMARY KEY (\"id\")"),
            "the table-level clause would be a second key: {sql}"
        );
    }

    /// A composite key has no autoincrement to inline, so it takes the ordinary
    /// table-level clause.
    #[test]
    fn a_composite_key_is_written_as_a_table_constraint() {
        let t = TableInfo {
            name: "memberships".into(),
            columns: vec![col("team", "TEXT"), col("person", "TEXT")],
            indexes: vec![IndexInfo {
                name: "PRIMARY".into(),
                columns: vec![IndexColumn::plain("team"), IndexColumn::plain("person")],
                unique: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(
            one(&t).contains(r#"PRIMARY KEY ("team", "person")"#),
            "{}",
            one(&t)
        );
    }

    /// SQLite has no inline `KEY`/`UNIQUE KEY` — that is MySQL-only syntax, and
    /// emitting it is a syntax error rather than an infelicity. Indexes come
    /// after the table, as they do on PostgreSQL.
    #[test]
    fn indexes_come_after_the_table_not_inside_it() {
        let mut t = autoinc();
        t.indexes.push(IndexInfo {
            name: "ix_email".into(),
            columns: vec![IndexColumn::plain("email")],
            ..Default::default()
        });
        t.indexes.push(IndexInfo {
            name: "uq_email".into(),
            columns: vec![IndexColumn::plain("email")],
            unique: true,
            ..Default::default()
        });
        let stmts = emit(&t);
        assert!(!stmts[0].contains(" KEY \""), "no inline key: {}", stmts[0]);
        let rest = stmts[1..].join("\n");
        assert!(rest.contains(r#"CREATE INDEX "ix_email""#), "{rest}");
        assert!(rest.contains(r#"CREATE UNIQUE INDEX "uq_email""#), "{rest}");
    }

    /// None of the three table options exists in SQLite, and `COMMENT` doesn't
    /// exist at all — not on a table, not on a column.
    #[test]
    fn no_table_options_and_no_comments_survive() {
        let mut t = autoinc();
        t.engine = Some("InnoDB".into());
        t.collation = Some("utf8mb4_bin".into());
        t.comment = Some("people".into());
        t.columns[1].comment = Some("login".into());
        let sql = one(&t);
        for banned in ["ENGINE=", "COLLATE=", "COMMENT", "InnoDB"] {
            assert!(!sql.contains(banned), "{banned} in:\n{sql}");
        }
    }

    /// `ON UPDATE CURRENT_TIMESTAMP` is a MySQL column attribute. SQLite has no
    /// such clause, and a stray one makes the whole statement unparseable.
    #[test]
    fn a_mysql_on_update_clause_is_not_carried_over() {
        let mut t = autoinc();
        t.columns[1].on_update = Some("CURRENT_TIMESTAMP".into());
        assert!(!one(&t).contains("ON UPDATE"), "{}", one(&t));
    }

    /// What SQLite *does* have stays: a column collation, a default, NOT NULL,
    /// a generated expression, and inline FK and CHECK constraints.
    #[test]
    fn what_sqlite_does_have_is_kept() {
        let mut t = autoinc();
        t.columns[1].collation = Some("NOCASE".into());
        t.columns[1].nullable = false;
        t.columns[1].default = Some("''".into());
        t.columns.push(ColumnInfo {
            name: "domain".into(),
            type_name: "TEXT".into(),
            nullable: true,
            generated: Some("substr(email, instr(email, '@'))".into()),
            ..Default::default()
        });
        t.foreign_keys.push(ForeignKeyInfo {
            name: "fk_team".into(),
            columns: vec!["email".into()],
            ref_table: "teams".into(),
            ref_columns: vec!["email".into()],
            on_delete: Some("CASCADE".into()),
            ..Default::default()
        });
        t.check_constraints.push(CheckInfo {
            name: "ck_email".into(),
            expression: "length(email) > 3".into(),
            enforced: true,
            ..Default::default()
        });
        let sql = one(&t);
        assert!(sql.contains("COLLATE NOCASE"), "{sql}");
        assert!(sql.contains("NOT NULL"), "{sql}");
        assert!(sql.contains("DEFAULT ''"), "{sql}");
        assert!(sql.contains("GENERATED ALWAYS AS ("), "{sql}");
        assert!(sql.contains(r#"REFERENCES "teams""#), "{sql}");
        assert!(sql.contains("ON DELETE CASCADE"), "{sql}");
        assert!(sql.contains("CHECK (length(email) > 3)"), "{sql}");
    }

    /// PostgreSQL's identity spelling must not leak either.
    #[test]
    fn no_identity_syntax_leaks_in() {
        assert!(!one(&autoinc()).contains("AS IDENTITY"));
    }
}

#[cfg(test)]
mod sqlite_rebuild_tests {
    use super::*;
    use crate::schema::{IndexColumn, IndexInfo};

    fn col(name: &str, ty: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: ty.into(),
            nullable: true,
            ..Default::default()
        }
    }

    /// `t (a INTEGER, b TEXT)`, no key, no indexes.
    fn table() -> TableInfo {
        TableInfo {
            name: "t".into(),
            columns: vec![col("a", "INTEGER"), col("b", "TEXT")],
            ..Default::default()
        }
    }

    fn plan(current: &TableInfo, draft: &TableDraft) -> Vec<String> {
        sqlite_rebuild_sql(current, draft)
    }

    /// The shape of the whole thing, on the simplest edit there is: a retype,
    /// which SQLite's `ALTER TABLE` cannot do at all.
    #[test]
    fn a_retype_becomes_create_copy_drop_rename() {
        let t = table();
        let mut d = TableDraft::from_table(&t);
        d.columns[0].info.type_name = "TEXT".into();
        assert_eq!(
            plan(&t, &d),
            vec![
                "CREATE TABLE \"t_schemaic_rebuild\" (\n  \"a\" TEXT,\n  \"b\" TEXT\n);",
                r#"INSERT INTO "t_schemaic_rebuild" ("a", "b") SELECT "a", "b" FROM "t";"#,
                "PRAGMA legacy_alter_table = ON;",
                r#"DROP TABLE "t";"#,
                r#"ALTER TABLE "t_schemaic_rebuild" RENAME TO "t";"#,
                "PRAGMA legacy_alter_table = OFF;",
            ]
        );
    }

    /// The copy is what carries a rename across: the new table's column takes
    /// the old one's data, which is the whole difference between a rename and a
    /// drop-plus-add.
    #[test]
    fn a_renamed_column_copies_from_its_old_name() {
        let t = table();
        let mut d = TableDraft::from_table(&t);
        d.columns[0].info.name = "z".into();
        assert!(
            plan(&t, &d)[1].contains(r#"("z", "b") SELECT "a", "b""#),
            "{:#?}",
            plan(&t, &d)
        );
    }

    /// A column the user added has nothing to copy from — leaving it out of both
    /// lists is what lets its DEFAULT (or NULL) apply.
    #[test]
    fn an_added_column_is_left_out_of_the_copy() {
        let t = table();
        let mut d = TableDraft::from_table(&t);
        d.columns.push(ColumnDraft::new(col("c", "TEXT")));
        let got = plan(&t, &d);
        assert!(got[0].contains(r#""c" TEXT"#), "still created: {}", got[0]);
        assert_eq!(
            got[1],
            r#"INSERT INTO "t_schemaic_rebuild" ("a", "b") SELECT "a", "b" FROM "t";"#
        );
    }

    #[test]
    fn a_dropped_column_is_in_neither_list() {
        let t = table();
        let mut d = TableDraft::from_table(&t);
        d.columns.remove(0);
        let got = plan(&t, &d);
        assert!(!got[0].contains(r#""a""#), "{}", got[0]);
        assert_eq!(
            got[1],
            r#"INSERT INTO "t_schemaic_rebuild" ("b") SELECT "b" FROM "t";"#
        );
    }

    /// **A generated column cannot be inserted into.** It is created with the
    /// table and computed from the copied rows; naming it in the `INSERT` makes
    /// the statement fail outright.
    #[test]
    fn a_generated_column_is_created_but_never_copied() {
        let mut t = table();
        t.columns.push(ColumnInfo {
            generated: Some("upper(b)".into()),
            ..col("shout", "TEXT")
        });
        let d = TableDraft::from_table(&t);
        let got = plan(&t, &d);
        assert!(
            got[0].contains("GENERATED ALWAYS AS (upper(b))"),
            "{}",
            got[0]
        );
        assert!(!got[1].contains("shout"), "{}", got[1]);
    }

    /// Indexes are recreated **after** the rename, against the real table. Doing
    /// it earlier would collide with the old table's index of the same name,
    /// which is still there until the drop.
    #[test]
    fn indexes_are_recreated_after_the_rename() {
        let mut t = table();
        t.indexes.push(IndexInfo {
            name: "ix_b".into(),
            columns: vec![IndexColumn::plain("b")],
            ..Default::default()
        });
        let d = TableDraft::from_table(&t);
        let got = plan(&t, &d);
        assert!(
            !got[0].contains("CREATE INDEX"),
            "not against the shadow table: {}",
            got[0]
        );
        let rename = got.iter().position(|s| s.contains("RENAME TO")).unwrap();
        let index = got.iter().position(|s| s.contains("CREATE INDEX")).unwrap();
        assert!(index > rename, "{got:#?}");
        assert_eq!(got[index], r#"CREATE INDEX "ix_b" ON "t" ("b");"#);
    }

    /// A trigger is dropped with the table it hangs off, so a rebuild that
    /// didn't put it back would quietly disarm it.
    #[test]
    fn dependents_are_replayed_last() {
        let trg = r#"CREATE TRIGGER "t_ai" AFTER INSERT ON "t" BEGIN SELECT 1; END;"#;
        let mut t = table();
        t.dependent_ddl = vec![trg.to_string()];
        let d = TableDraft::from_table(&t);
        let got = sqlite_rebuild_sql(&t, &d);
        assert_eq!(got.last().map(String::as_str), Some(trg), "{got:#?}");
    }

    /// Every column is new, so there is nothing to carry over — and
    /// `INSERT INTO t () SELECT FROM …` is not a statement.
    #[test]
    fn nothing_to_copy_emits_no_insert() {
        let t = table();
        let mut d = TableDraft::from_table(&t);
        d.columns.clear();
        d.columns.push(ColumnDraft::new(col("fresh", "TEXT")));
        let got = plan(&t, &d);
        assert!(!got.iter().any(|s| s.starts_with("INSERT")), "{got:#?}");
        assert!(
            got.iter().any(|s| s.starts_with("CREATE TABLE")),
            "{got:#?}"
        );
        assert!(got.iter().any(|s| s.starts_with("DROP TABLE")), "{got:#?}");
    }
}

/// Reading a SQLite trigger back into the model. SQLite keeps no catalogue of a
/// trigger's parts — `sqlite_master` holds the `CREATE TRIGGER` text and nothing
/// else — so this is the one engine where introspection is a *parse*, and the
/// editor is only as honest as it is.
#[cfg(test)]
mod sqlite_trigger_read_tests {
    use super::*;
    use crate::intel::SqlDialect::Sqlite;

    fn read(sql: &str) -> TriggerInfo {
        sqlite_trigger_info(sql).unwrap_or_else(|| panic!("should read: {sql}"))
    }

    #[test]
    fn a_plain_trigger_reads_back() {
        let t = read("CREATE TRIGGER t AFTER INSERT ON emp BEGIN UPDATE log SET n = n + 1; END");
        assert_eq!(t.name, "t");
        assert_eq!(t.table, "emp");
        assert_eq!(t.timing, TriggerTiming::After);
        assert_eq!(t.events, vec![TriggerEvent::Insert]);
        // SQLite has only row-level triggers — `FOR EACH STATEMENT` is a syntax
        // error there — so the level is never in doubt.
        assert_eq!(t.level, TriggerLevel::Row);
        assert_eq!(t.condition, None);
        assert!(t.update_columns.is_empty());
        assert_eq!(
            t.action,
            TriggerAction::Body("BEGIN UPDATE log SET n = n + 1; END".into())
        );
        // None of the other engines' fields are invented.
        assert_eq!(t.schema, None);
        assert_eq!(t.definer, None);
        assert_eq!(t.sql_mode, None);
        assert!(!t.constraint);
    }

    /// SQLite's timing is optional, and omitting it means `BEFORE`. Read as
    /// `After` — the enum's own default — the editor would show the wrong answer
    /// and a re-create would move the trigger.
    #[test]
    fn an_omitted_timing_is_before() {
        let t = read("CREATE TRIGGER t DELETE ON emp BEGIN SELECT 1; END");
        assert_eq!(t.timing, TriggerTiming::Before);
        assert_eq!(t.events, vec![TriggerEvent::Delete]);
    }

    /// Two things SQLite has that MySQL doesn't, and that the model documented as
    /// PostgreSQL's alone: `UPDATE OF` and `WHEN`.
    #[test]
    fn update_of_columns_and_a_when_guard_are_read() {
        let t = read(
            "CREATE TRIGGER t BEFORE UPDATE OF a, b ON emp \
             FOR EACH ROW WHEN NEW.a > OLD.a \
             BEGIN SELECT RAISE(ABORT, 'nope'); END",
        );
        assert_eq!(t.timing, TriggerTiming::Before);
        assert_eq!(t.events, vec![TriggerEvent::Update]);
        assert_eq!(t.update_columns, vec!["a".to_string(), "b".to_string()]);
        // Held bare, without the parens the emitter puts back — the same rule
        // `check_predicate` follows, so a round trip doesn't grow a layer.
        assert_eq!(t.condition.as_deref(), Some("NEW.a > OLD.a"));
    }

    #[test]
    fn an_instead_of_trigger_on_a_view_reads_back() {
        let t = read(
            "CREATE TRIGGER t INSTEAD OF DELETE ON v BEGIN DELETE FROM emp WHERE a = OLD.a; END",
        );
        assert_eq!(t.timing, TriggerTiming::InsteadOf);
        assert_eq!(t.table, "v");
    }

    /// The body is the user's own SQL and is kept **verbatim** — comments,
    /// newlines and indentation included. Re-printing it from the AST would hand
    /// back a trigger nobody wrote, and the round trip below is what would then
    /// report a phantom change on every open.
    #[test]
    fn the_body_is_kept_exactly_as_written() {
        let sql = "CREATE TRIGGER t AFTER INSERT ON emp\n  BEGIN\n    -- keep me\n    \
                   SELECT 1;\n  END";
        let TriggerAction::Body(b) = read(sql).action else {
            panic!("a SQLite trigger runs a body")
        };
        assert_eq!(b, "BEGIN\n    -- keep me\n    SELECT 1;\n  END");
    }

    /// Quoted identifiers arrive unquoted in the model, as they do from every
    /// other introspection path — the emitter is what quotes them again.
    #[test]
    fn quoted_names_are_unquoted_in_the_model() {
        let t = read(r#"CREATE TRIGGER "odd ;name" AFTER DELETE ON "my emp" BEGIN SELECT 1; END"#);
        assert_eq!(t.name, "odd ;name");
        assert_eq!(t.table, "my emp");
    }

    /// Anything it can't read is `None`, and the caller drops that trigger rather
    /// than showing a guess. The safe direction: a trigger read wrong would be
    /// *emitted* wrong, and a trigger not read at all is simply never touched by
    /// [`diff_triggers`], which only drops what the server copy lists.
    #[test]
    fn an_unreadable_statement_reads_back_as_nothing() {
        for sql in [
            "SELECT 1",
            "CREATE TABLE t (a INT)",
            "CREATE TRIGGER t AFTER INSERT ON emp",
            "not sql at all",
            "",
        ] {
            assert!(sqlite_trigger_info(sql).is_none(), "{sql:?}");
        }
    }

    /// **The round-trip gate**, and the reason the reader can be trusted at all:
    /// what SQLite stores, read into the model and emitted again, must diff to
    /// nothing against itself. A field the reader drops shows up here as a
    /// phantom change on a trigger nobody touched.
    #[test]
    fn every_shape_diffs_to_nothing_against_itself() {
        for sql in [
            "CREATE TRIGGER t AFTER INSERT ON emp BEGIN SELECT 1; END",
            "CREATE TRIGGER t DELETE ON emp BEGIN SELECT 1; END",
            "CREATE TRIGGER t BEFORE UPDATE OF a, b ON emp FOR EACH ROW \
             WHEN NEW.a > OLD.a BEGIN SELECT 1; END",
            "CREATE TRIGGER t INSTEAD OF UPDATE ON v BEGIN SELECT 1; END",
            r#"CREATE TRIGGER "odd ;name" AFTER DELETE ON "my emp" BEGIN SELECT 1; END"#,
            // A guard the user already parenthesised. The model holds it bare
            // and the emitter wraps it exactly once, so this must not grow a
            // layer of parens per edit.
            "CREATE TRIGGER t AFTER INSERT ON emp WHEN (NEW.a > 1) BEGIN SELECT 1; END",
            // A body whose statements contain the words the header uses.
            "CREATE TRIGGER t AFTER INSERT ON emp BEGIN \
             UPDATE log SET n = CASE WHEN NEW.a > 1 THEN 1 ELSE 2 END; END",
        ] {
            let info = read(sql);
            let draft = TriggerDraft::from_info(&info);
            let cs = diff_triggers(std::slice::from_ref(&info), &set_of(&draft), Sqlite);
            assert!(cs.is_empty(), "phantom change on {sql}: {:#?}", cs.changes);
            // …and the statement it emits reads back as the same trigger, which
            // is the half a self-diff can't see.
            let again = read(&info.create_sql(Sqlite));
            assert_eq!(again, info, "emitting and re-reading changed it: {sql}");
        }
    }

    fn set_of(d: &TriggerDraft) -> TriggerSetDraft {
        TriggerSetDraft {
            table: d.info.table.clone(),
            schema: d.info.schema.clone(),
            triggers: vec![d.clone()],
        }
    }
}

/// What SQLite refuses a trigger for. Each rule is the engine's own, measured
/// against 3.45 rather than inferred, and each is refused in the modal because
/// by the time a `CREATE` fails at Apply the matching `DROP` has already run.
#[cfg(test)]
mod sqlite_trigger_rule_tests {
    use super::*;
    use crate::intel::SqlDialect::Sqlite;

    fn draft() -> TriggerDraft {
        let mut d = TriggerDraft::blank("t", "emp", None);
        d.info.timing = TriggerTiming::After;
        d.info.events = vec![TriggerEvent::Insert];
        d.info.action = TriggerAction::Body("BEGIN SELECT 1; END".into());
        d
    }

    fn errs(d: &TriggerDraft, host: TriggerHost) -> Vec<String> {
        d.validate(Sqlite, host)
    }

    #[test]
    fn a_well_formed_trigger_validates_clean() {
        assert!(errs(&draft(), TriggerHost::Table).is_empty());
    }

    /// `cannot create INSTEAD OF trigger on table: emp`, and its exact opposite
    /// on a view — `cannot create BEFORE trigger on view: v`. SQLite is stricter
    /// than PostgreSQL here: a view takes *only* INSTEAD OF.
    #[test]
    fn instead_of_belongs_to_views_and_nothing_else_does() {
        let mut d = draft();
        d.info.timing = TriggerTiming::InsteadOf;
        assert!(!errs(&d, TriggerHost::Table).is_empty(), "table");
        assert!(errs(&d, TriggerHost::View).is_empty(), "view");

        let d = draft(); // AFTER
        assert!(!errs(&d, TriggerHost::View).is_empty(), "AFTER on a view");
    }

    #[test]
    fn there_is_no_statement_level_trigger() {
        let mut d = draft();
        d.info.level = TriggerLevel::Statement;
        assert!(!errs(&d, TriggerHost::Table).is_empty());
    }

    #[test]
    fn one_event_per_trigger_and_never_truncate() {
        let mut d = draft();
        d.info.events = vec![TriggerEvent::Insert, TriggerEvent::Update];
        assert!(!errs(&d, TriggerHost::Table).is_empty(), "two events");

        let mut d = draft();
        d.info.events = vec![TriggerEvent::Truncate];
        assert!(!errs(&d, TriggerHost::Table).is_empty(), "truncate");
    }

    /// SQLite's grammar has no bare-statement body, and an empty block is a
    /// syntax error too — both refused before Apply rather than after the drop.
    #[test]
    fn the_body_has_to_be_a_begin_end_block() {
        for body in ["", "SELECT 1;", "BEGIN END", "BEGIN  END", "-- nothing"] {
            let mut d = draft();
            d.info.action = TriggerAction::Body(body.into());
            assert!(!errs(&d, TriggerHost::Table).is_empty(), "{body:?}");
        }
        for body in [
            "BEGIN SELECT 1; END",
            "  begin\n select 'END'; \nend  ",
            "BEGIN /* c */ SELECT 1; END",
        ] {
            let mut d = draft();
            d.info.action = TriggerAction::Body(body.into());
            assert!(errs(&d, TriggerHost::Table).is_empty(), "{body:?}");
        }
    }

    /// "Open in editor" must hand back a script Schemaic can run itself. On
    /// MySQL that means `DELIMITER $$` around a body full of `;`; SQLite has no
    /// such directive, and emitting one would put a word the engine has never
    /// heard of at the top of the script. It needs none: the splitter knows a
    /// trigger body runs to the `;` after its `END`.
    #[test]
    fn the_editor_script_never_hands_sqlite_a_delimiter_directive() {
        let mut d = draft();
        d.info.action = TriggerAction::Body("BEGIN UPDATE log SET n = 1; SELECT 2; END".into());
        let script = create_trigger(&d, Sqlite).editor_script();
        assert!(
            !script.to_ascii_uppercase().contains("DELIMITER"),
            "{script}"
        );
        // …and it really is one statement to the splitter.
        assert_eq!(
            crate::sql::statement_ranges(&script, Sqlite).len(),
            1,
            "{script}"
        );
    }

    #[test]
    fn a_function_action_is_not_a_sqlite_trigger() {
        let mut d = draft();
        d.info.action = TriggerAction::Function {
            name: "f".into(),
            args: vec![],
        };
        assert!(!errs(&d, TriggerHost::Table).is_empty());
    }

    #[test]
    fn update_of_needs_an_update_event() {
        let mut d = draft(); // INSERT
        d.info.update_columns = vec!["a".into()];
        assert!(!errs(&d, TriggerHost::Table).is_empty());
        d.info.events = vec![TriggerEvent::Update];
        assert!(errs(&d, TriggerHost::Table).is_empty());
    }

    /// The MySQL messages must not reach a SQLite user: it *has* a `WHEN` guard
    /// and it *has* `UPDATE OF`, both of which MySQL's arm rejects outright.
    #[test]
    fn mysqls_refusals_do_not_apply_here() {
        let mut d = draft();
        d.info.events = vec![TriggerEvent::Update];
        d.info.update_columns = vec!["a".into(), "b".into()];
        d.info.condition = Some("NEW.a > OLD.a".into());
        assert!(
            errs(&d, TriggerHost::Table).is_empty(),
            "{:?}",
            errs(&d, TriggerHost::Table)
        );
    }
}

/// SQLite's view editing, which is a different shape from the other two engines
/// rather than a subset of them: it has no `CREATE OR REPLACE VIEW` and no verb
/// that renames a view, so *every* edit is a drop and a create.
#[cfg(test)]
mod sqlite_view_tests {
    use super::*;
    use crate::intel::SqlDialect::Sqlite;

    fn view() -> TableInfo {
        TableInfo {
            name: "v".into(),
            columns: vec![
                ColumnInfo {
                    name: "a".into(),
                    type_name: "INTEGER".into(),
                    nullable: true,
                    ..Default::default()
                },
                ColumnInfo {
                    name: "b".into(),
                    type_name: "TEXT".into(),
                    nullable: true,
                    ..Default::default()
                },
            ],
            is_view: true,
            view_definition: Some("SELECT a, b FROM t".into()),
            view_options: Some(ViewOptions::default()),
            ..Default::default()
        }
    }

    #[test]
    fn sqlite_edits_views() {
        assert!(supports_view_editing(Sqlite));
    }

    #[test]
    fn a_body_change_drops_and_creates_rather_than_replacing() {
        let cur = view();
        let mut d = ViewDraft::from_table(&cur).unwrap();
        d.select = "SELECT a, b FROM t WHERE a > 1".into();
        let cs = diff_view(&cur, &d, Sqlite);
        assert!(
            matches!(
                cs.changes.as_slice(),
                [Change::ReplaceView { recreate: true, .. }]
            ),
            "{:#?}",
            cs.changes
        );
        let sql = cs.script();
        assert!(
            !sql.to_ascii_uppercase().contains("OR REPLACE"),
            "SQLite has no CREATE OR REPLACE VIEW: {sql}"
        );
        let drop_at = sql.find("DROP VIEW").expect("drops the old view");
        let create_at = sql.find("CREATE VIEW").expect("creates the new one");
        assert!(drop_at < create_at, "the drop has to come first: {sql}");
    }

    #[test]
    fn a_rename_drops_and_creates_too() {
        // SQLite refuses `ALTER TABLE v RENAME TO …` on a view outright ("view v
        // may not be altered") and has no `ALTER VIEW` at all, so the rename verb
        // the other two engines use isn't available to fall back on.
        let cur = view();
        let mut d = ViewDraft::from_table(&cur).unwrap();
        d.name = "v2".into();
        let cs = diff_view(&cur, &d, Sqlite);
        assert!(
            !cs.changes
                .iter()
                .any(|c| matches!(c, Change::RenameView { .. })),
            "SQLite can't rename a view: {:#?}",
            cs.changes
        );
        let sql = cs.script();
        assert!(sql.contains("DROP VIEW \"v\""), "{sql}");
        assert!(sql.contains("CREATE VIEW \"v2\""), "{sql}");
        assert!(!sql.to_ascii_uppercase().contains("RENAME"), "{sql}");
    }

    #[test]
    fn the_column_list_survives_the_re_create() {
        // `CREATE VIEW v (x, y) AS SELECT a, b …` names the view's columns
        // independently of the body. Dropping it on the way through would
        // silently rename every column of the view to whatever the SELECT calls
        // them — the reason the list is carried in the model at all.
        let mut cur = view();
        cur.view_options = Some(ViewOptions {
            column_list: Some("x, y".into()),
            ..Default::default()
        });
        let mut d = ViewDraft::from_table(&cur).unwrap();
        d.select = "SELECT a, b FROM t WHERE a > 1".into();
        let sql = diff_view(&cur, &d, Sqlite).script();
        assert!(sql.contains("CREATE VIEW \"v\" (x, y) AS"), "{sql}");
    }

    #[test]
    fn no_column_list_emits_no_parentheses() {
        let cur = view();
        let d = ViewDraft::from_table(&cur).unwrap();
        let sql = create_view(&d, Sqlite).script();
        assert!(sql.contains("CREATE VIEW \"v\" AS"), "{sql}");
    }

    #[test]
    fn the_other_engines_options_are_never_emitted() {
        // Nothing in the SQLite form can set these, but a model that picked them
        // up anywhere (a draft carried across a reconnect, a future importer)
        // must not emit MySQL's clauses at a SQLite server, which would fail at
        // Apply having looked fine in the preview.
        let mut cur = view();
        cur.view_options = Some(ViewOptions {
            check_option: Some("CASCADED".into()),
            definer: Some("root@localhost".into()),
            security: Some("DEFINER".into()),
            algorithm: Some("MERGE".into()),
            storage: vec!["security_barrier=true".into()],
            ..Default::default()
        });
        let d = ViewDraft::from_table(&cur).unwrap();
        let sql = create_view(&d, Sqlite).script().to_ascii_uppercase();
        for clause in [
            "ALGORITHM",
            "DEFINER",
            "SQL SECURITY",
            "CHECK OPTION",
            "WITH (",
        ] {
            assert!(!sql.contains(clause), "{clause} reached SQLite: {sql}");
        }
    }

    #[test]
    fn a_materialized_flag_cannot_reach_sqlite() {
        // `MATERIALIZED` is PostgreSQL's word; SQLite has no such object, and the
        // draft validator is what stops the modal before the emitter has to.
        let mut cur = view();
        cur.view_options = Some(ViewOptions {
            materialized: true,
            ..Default::default()
        });
        let d = ViewDraft::from_table(&cur).unwrap();
        assert!(!d.validate().is_empty());
        let sql = create_view(&d, Sqlite).script().to_ascii_uppercase();
        assert!(!sql.contains("MATERIALIZED"), "{sql}");
    }
}

#[cfg(test)]
mod sqlite_designer_tests {
    use super::*;
    use crate::intel::SqlDialect::Sqlite;
    use crate::schema::{IndexColumn, IndexInfo};

    fn col(name: &str, ty: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: ty.into(),
            nullable: true,
            ..Default::default()
        }
    }

    fn table() -> TableInfo {
        TableInfo {
            name: "t".into(),
            columns: vec![col("a", "INTEGER"), col("b", "TEXT")],
            ..Default::default()
        }
    }

    fn retyped(t: &TableInfo) -> ChangeSet {
        let mut d = TableDraft::from_table(t);
        d.columns[0].info.type_name = "TEXT".into();
        diff(t, &d, Sqlite)
    }

    /// A retype has no statement of its own in SQLite, so the set grows the
    /// change that performs it — **beside** the retype, not instead of it, so
    /// the preview still says what the user asked for.
    #[test]
    fn a_change_with_no_statement_gains_a_rebuild() {
        let cs = retyped(&table());
        assert!(
            matches!(cs.changes.first(), Some(Change::RebuildTable(_))),
            "{:?}",
            cs.changes
        );
        assert!(
            cs.changes
                .iter()
                .any(|c| matches!(c, Change::AlterColumn { .. })),
            "the retype is still listed: {:?}",
            cs.changes
        );
        assert!(
            cs.emit().iter().any(|s| s.contains("INSERT INTO")),
            "{:?}",
            cs.emit()
        );
        assert!(
            cs.unsupported().is_empty(),
            "the rebuild performs all of it"
        );
    }

    // ── the native ADD COLUMN fast path ──────────────────────────────────────
    //
    // A test per condition, deliberately, rather than one happy path: each of
    // SQLite's restrictions is a way to write the plan that **half-applies** —
    // the fast path is taken, the engine refuses the statement, and the edit the
    // preview promised is gone. Every rule below was measured against SQLite
    // 3.46 rather than read off the grammar; the engine's own wording is quoted
    // where it has some.

    /// The added column, with everything else about it left alone.
    fn added(t: &TableInfo, c: ColumnInfo) -> ChangeSet {
        let mut d = TableDraft::from_table(t);
        d.columns.push(ColumnDraft::new(c));
        diff(t, &d, Sqlite)
    }

    fn new_col() -> ColumnInfo {
        ColumnInfo {
            name: "c".into(),
            type_name: "TEXT".into(),
            nullable: true,
            ..Default::default()
        }
    }

    /// **The case this exists for.** Appending an ordinary column is the most
    /// common designer edit there is, and SQLite does it instantly — copying the
    /// whole table to achieve it was correct and absurd.
    #[test]
    fn appending_a_plain_column_is_native() {
        let cs = added(&table(), new_col());
        assert!(!cs.changes.iter().any(is_rebuild), "{:?}", cs.changes);
        assert_eq!(cs.emit(), vec![r#"ALTER TABLE "t" ADD COLUMN "c" TEXT;"#]);
    }

    /// Everything the column may legally carry, all at once — so the rule can't
    /// pass by being timid. A generated column is addable (SQLite's default is
    /// `VIRTUAL`, which is what the emitter writes by omitting the keyword), and
    /// so is one that is `NOT NULL` *because* it is generated.
    #[test]
    fn the_things_a_column_may_carry_stay_native() {
        for c in [
            ColumnInfo {
                default: Some("'x'".into()),
                nullable: false,
                ..new_col()
            },
            ColumnInfo {
                collation: Some("NOCASE".into()),
                default: Some("''".into()),
                ..new_col()
            },
            ColumnInfo {
                default: Some("-1".into()),
                type_name: "INTEGER".into(),
                ..new_col()
            },
            ColumnInfo {
                generated: Some("a * 2".into()),
                type_name: "INTEGER".into(),
                ..new_col()
            },
            // Generated *and* NOT NULL: the null-default rule doesn't reach a
            // column that has no default to speak of. Verified against 3.46.
            ColumnInfo {
                generated: Some("'x'".into()),
                nullable: false,
                ..new_col()
            },
        ] {
            let cs = added(&table(), c.clone());
            assert!(
                !cs.changes.iter().any(is_rebuild),
                "{} should be native: {:?}",
                c.name,
                cs.changes
            );
        }
    }

    /// *"Cannot add a PRIMARY KEY column"*.
    #[test]
    fn a_key_column_still_rebuilds() {
        let c = ColumnInfo {
            primary_key: true,
            type_name: "INTEGER".into(),
            ..new_col()
        };
        assert!(added(&table(), c).changes.iter().any(is_rebuild));
    }

    /// `AUTOINCREMENT` is legal only spelled inline as `INTEGER PRIMARY KEY
    /// AUTOINCREMENT`, so `column_sql` drops it for SQLite — a native add would
    /// silently lose the counter, where the rebuild's table builder can place it.
    #[test]
    fn a_counter_column_still_rebuilds() {
        let c = ColumnInfo {
            auto_increment: true,
            type_name: "INTEGER".into(),
            ..new_col()
        };
        assert!(added(&table(), c).changes.iter().any(is_rebuild));
    }

    /// *"Cannot add a NOT NULL column with default value NULL"* — which covers
    /// both no default at all and an explicit `DEFAULT NULL`.
    #[test]
    fn not_null_without_a_usable_default_still_rebuilds() {
        for default in [None, Some("NULL".to_string()), Some("null".to_string())] {
            let c = ColumnInfo {
                nullable: false,
                default,
                ..new_col()
            };
            let cs = added(&table(), c.clone());
            assert!(
                cs.changes.iter().any(is_rebuild),
                "{:?} should rebuild: {:?}",
                c.default,
                cs.changes
            );
        }
    }

    /// *"Cannot add a column with non-constant default"* — the `CURRENT_*`
    /// keywords and anything parenthesised. A bare function call isn't a legal
    /// `DEFAULT` at all, and is caught by the same paren test.
    #[test]
    fn a_non_constant_default_still_rebuilds() {
        for default in [
            "CURRENT_TIMESTAMP",
            "current_timestamp",
            "CURRENT_DATE",
            "CURRENT_TIME",
            "(1 + 1)",
            "(SELECT max(a) FROM t)",
            "now()",
        ] {
            let c = ColumnInfo {
                default: Some(default.into()),
                ..new_col()
            };
            let cs = added(&table(), c);
            assert!(
                cs.changes.iter().any(is_rebuild),
                "{default} should rebuild: {:?}",
                cs.changes
            );
        }
    }

    /// A paren *inside a string literal* is data, not an expression — asked
    /// through the shared lexer rather than with a `contains('(')`.
    #[test]
    fn a_paren_inside_a_literal_default_is_not_an_expression() {
        let c = ColumnInfo {
            default: Some("'a (b)'".into()),
            ..new_col()
        };
        assert!(!added(&table(), c).changes.iter().any(is_rebuild));
    }

    /// **`ADD COLUMN` always appends.** A column the user dropped into the
    /// middle carries a `Position`, and taking the fast path there would put it
    /// at the end instead — the designer showing one order and the table having
    /// another.
    #[test]
    fn a_column_added_in_the_middle_still_rebuilds() {
        let t = table();
        let mut d = TableDraft::from_table(&t);
        d.columns.insert(1, ColumnDraft::new(new_col()));
        let cs = diff(&t, &d, Sqlite);
        assert!(
            cs.changes.iter().any(is_rebuild),
            "an ADD COLUMN would land at the end: {:?}",
            cs.changes
        );
    }

    /// Uniqueness isn't on `ColumnInfo` — it arrives as an index, which SQLite
    /// has no native `supports_change` arm for and which therefore takes the set
    /// back to a rebuild. Pinned because the fast path would otherwise be one
    /// `AddIndex` arm away from adding *"Cannot add a UNIQUE column"* to a plan.
    #[test]
    fn a_column_that_brings_an_index_still_rebuilds() {
        let t = table();
        let mut d = TableDraft::from_table(&t);
        d.columns.push(ColumnDraft::new(new_col()));
        d.indexes.push(IndexDraft {
            original: None,
            info: IndexInfo {
                name: "ix_c".into(),
                columns: vec![IndexColumn {
                    name: "c".into(),
                    ..Default::default()
                }],
                unique: true,
                ..Default::default()
            },
        });
        assert!(diff(&t, &d, Sqlite).changes.iter().any(is_rebuild));
    }

    /// The fast path is one column's answer, not the set's: an add that could
    /// have gone native rides the rebuild when anything else in the same edit
    /// needs one, because the rebuild already writes the column.
    #[test]
    fn a_native_add_beside_a_retype_is_subsumed_by_the_rebuild() {
        let t = table();
        let mut d = TableDraft::from_table(&t);
        d.columns[0].info.type_name = "TEXT".into();
        d.columns.push(ColumnDraft::new(new_col()));
        let cs = diff(&t, &d, Sqlite);
        assert!(cs.changes.iter().any(is_rebuild), "{:?}", cs.changes);
        assert!(
            !cs.emit().iter().any(|s| s.contains("ADD COLUMN")),
            "the rebuild writes the column; adding it again would be twice: {:?}",
            cs.emit()
        );
    }

    /// A set SQLite can do directly keeps its direct path and pays nothing.
    #[test]
    fn a_plain_drop_needs_no_rebuild() {
        let t = table();
        let mut d = TableDraft::from_table(&t);
        d.columns.remove(0);
        let cs = diff(&t, &d, Sqlite);
        assert!(!cs.changes.iter().any(is_rebuild), "{:?}", cs.changes);
        assert_eq!(cs.emit(), vec![r#"ALTER TABLE "t" DROP COLUMN "a";"#]);
    }

    /// The other engines alter in place and never grow one.
    #[test]
    fn the_full_engines_never_rebuild() {
        for d in [SqlDialect::MySql, SqlDialect::Postgres] {
            let t = table();
            let mut draft = TableDraft::from_table(&t);
            draft.columns[0].info.type_name = "TEXT".into();
            assert!(!diff(&t, &draft, d).changes.iter().any(is_rebuild), "{d:?}");
        }
    }

    /// A rename is left to SQLite's own statement, after the rebuild, so the
    /// engine repoints the references other objects hold.
    #[test]
    fn a_rename_rides_after_the_rebuild_as_a_native_statement() {
        let t = table();
        let mut d = TableDraft::from_table(&t);
        d.columns[0].info.type_name = "TEXT".into();
        d.name = "t2".into();
        let sql = diff(&t, &d, Sqlite).emit();
        assert_eq!(
            sql.last().map(String::as_str),
            Some(r#"ALTER TABLE "t" RENAME TO "t2";"#),
            "{sql:#?}"
        );
    }

    /// **A partial index would come back covering every row.** SQLite keeps a
    /// partial index's predicate only in its own `CREATE` text, so the model
    /// carries `lossy` instead — and a rebuild recreates its indexes from the
    /// model. Recreating this one silently widens it, which is the plan refusing
    /// to be applied rather than a warning to read past.
    #[test]
    fn a_lossy_index_stops_the_rebuild() {
        let mut t = table();
        t.indexes.push(IndexInfo {
            name: "ix_partial".into(),
            columns: vec![IndexColumn::plain("a")],
            lossy: true,
            ..Default::default()
        });
        let withheld = retyped(&t).unsupported();
        assert_eq!(withheld.len(), 1, "{withheld:?}");
        assert!(withheld[0].contains("ix_partial"), "{withheld:?}");
    }

    /// Dropping the lossy index is a way to proceed — there is then nothing to
    /// recreate unfaithfully.
    #[test]
    fn dropping_the_lossy_index_clears_the_way() {
        let mut t = table();
        t.indexes.push(IndexInfo {
            name: "ix_partial".into(),
            columns: vec![IndexColumn::plain("a")],
            lossy: true,
            ..Default::default()
        });
        let mut d = TableDraft::from_table(&t);
        d.columns[0].info.type_name = "TEXT".into();
        d.indexes.clear();
        assert!(diff(&t, &d, Sqlite).unsupported().is_empty());
    }
}
