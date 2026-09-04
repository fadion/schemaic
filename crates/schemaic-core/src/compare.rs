//! Compare two databases object by object, and turn the comparison into one
//! migration plan.
//!
//! **The differ is the comparator.** An object is *differing* precisely when
//! [`ddl::diff`] (or its per-kind sibling) yields a non-empty [`ChangeSet`] for
//! it — there is no second field-by-field comparison anywhere here. That is
//! what keeps the tree's verdict and the plan's contents the same fact: a
//! release that teaches `diff` about a column attribute teaches this module
//! about it in the same commit, and an object the tree calls *differing* can
//! never produce an empty plan — nor the reverse, which is the worse half, a
//! compare that says "identical" over a real difference and silently leaves a
//! migration out.
//!
//! **Right is the source of truth.** Every change set here is the work that
//! makes the *left* database look like the right one, so the left is where the
//! DDL runs: the plan takes the left side's [`ServerFlavour`], an object only
//! the left side has is a `DROP`, and one only the right side has is a
//! `CREATE`. Swapping the pair is the caller's job and reverses all of it.
//!
//! **One dialect, so one engine.** [`SchemaComparison::of`] takes a single
//! [`SqlDialect`] because a [`ChangeSet`] carries one, and that is the honest
//! encoding of a real limit rather than a missing feature: type names, defaults
//! and index shapes do not map between engines, so a MySQL-to-PostgreSQL plan
//! would be confidently wrong exactly where it mattered. A caller pairing two
//! connections refuses the mismatch before it reaches here.
//!
//! ## What a comparison does not know
//!
//! Three limits are inherited from the models being compared. All three are
//! *emitting* problems rather than comparing ones — the comparison lands on the
//! right verdict and it is the generated SQL that suffers, which is why each
//! one is something an entry says about itself instead of a silent caveat:
//!
//! - **MySQL bodies arrive escape-mangled.** `TriggerInfo::action`,
//!   `RoutineInfo::body` and `EventInfo::body` from an eager `Db::fetch_schema`
//!   come from `information_schema`, whose escapes are already resolved. Two
//!   mangled bodies still compare equal to each other, so the *status* is
//!   right; the `CREATE` emitted for a differing routine is not, until the
//!   caller has replaced the body with the lazy
//!   `Db::{trigger,routine,event}_source` text. [`CompareEntry::needs_source`]
//!   is which entries that applies to.
//! - **A lossy index cannot be compared faithfully.** [`IndexInfo::lossy`]
//!   marks a PostgreSQL index whose expression keys or opclasses the model
//!   never read, so two of them compare equal whatever the server holds.
//!   [`CompareEntry::uncertain`] is how an entry says so, rather than the tree
//!   quietly claiming a match it cannot support.
//! - **A foreign-key cycle has no create order.** [`SchemaComparison::cycles`]
//!   reports one the same way [`crate::dump::DumpPlan::cycles`] does: the plan
//!   still holds every object, and the flag is what tells the reader the order
//!   alone can't be trusted.
//!
//! [`ddl::diff`]: crate::ddl::diff
//! [`ChangeSet`]: crate::ddl::ChangeSet
//! [`ServerFlavour`]: crate::schema::ServerFlavour
//! [`IndexInfo::lossy`]: crate::schema::IndexInfo::lossy

use std::collections::BTreeMap;

use crate::ddl::{
    self, Change, ChangeSet, DomainDraft, EnumDraft, EventDraft, ObjectKind, RoutineDraft,
    SequenceDraft, TableDraft, Target, TriggerDraft, ViewDraft,
};
use crate::intel::SqlDialect;
use crate::schema::{
    DbSchema, DomainInfo, EnumInfo, EventInfo, RoutineInfo, RoutineKind, SequenceInfo, TableInfo,
    TriggerInfo, display_name,
};

/// What kind of object a [`CompareEntry`] is about.
///
/// **The declaration order is the creation order**, and it is what
/// [`SchemaComparison::of`] sorts on: a standalone type before the table whose
/// column names it, a table before the view selecting from it, and a routine or
/// trigger after both because its body names everything above it.
/// `DbSchema::create_ddl_script` states the same order for the same reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompareKind {
    Enum,
    Domain,
    Sequence,
    Table,
    View,
    Function,
    Procedure,
    Trigger,
    Event,
}

impl CompareKind {
    /// The singular noun the UI puts in a heading or a preview subject, and the
    /// prefix of [`CompareEntry::key`].
    pub fn label(self) -> &'static str {
        match self {
            CompareKind::Enum => "enum",
            CompareKind::Domain => "domain",
            CompareKind::Sequence => "sequence",
            CompareKind::Table => "table",
            CompareKind::View => "view",
            CompareKind::Function => "function",
            CompareKind::Procedure => "procedure",
            CompareKind::Trigger => "trigger",
            CompareKind::Event => "event",
        }
    }

    /// The routine kind this is, when it is one.
    pub fn routine_kind(self) -> Option<RoutineKind> {
        match self {
            CompareKind::Function => Some(RoutineKind::Function),
            CompareKind::Procedure => Some(RoutineKind::Procedure),
            _ => None,
        }
    }

    /// Does this kind need tables to exist? Those are dropped *before* the
    /// tables they hang off, and created *after* them.
    fn depends_on_tables(self) -> bool {
        matches!(
            self,
            CompareKind::View
                | CompareKind::Function
                | CompareKind::Procedure
                | CompareKind::Trigger
                | CompareKind::Event
        )
    }

    /// Is this a standalone type — created before any table that names it, and
    /// dropped only after every table that did?
    fn is_type(self) -> bool {
        matches!(
            self,
            CompareKind::Enum | CompareKind::Domain | CompareKind::Sequence
        )
    }

    /// Does an object of this kind carry a body that MySQL's eager
    /// `information_schema` read mangles? See the module doc.
    fn carries_body(self) -> bool {
        matches!(
            self,
            CompareKind::Function
                | CompareKind::Procedure
                | CompareKind::Trigger
                | CompareKind::Event
        )
    }
}

/// Where an object exists, and whether the two sides agree about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectStatus {
    /// On the left only. The plan drops it.
    OnlyLeft,
    /// On the right only. The plan creates it.
    OnlyRight,
    /// On both, and the differ found work to do. The plan alters it.
    Differing,
    /// On both, and the differ found nothing. Contributes no statements.
    Same,
}

impl ObjectStatus {
    /// Would this status put anything in a plan?
    pub fn is_difference(self) -> bool {
        self != ObjectStatus::Same
    }
}

/// One object, as the two sides see it.
#[derive(Clone, Debug)]
pub struct CompareEntry {
    pub kind: CompareKind,
    /// The namespace, when the engine has them. `None` on MySQL and SQLite.
    pub schema: Option<String>,
    /// The object's own name.
    pub name: String,
    /// The table a trigger hangs off — part of a trigger's identity, and `None`
    /// for every other kind.
    pub table: Option<String>,
    /// A routine's identity arguments, which is the rest of its identity where
    /// the engine overloads on them. `None` for every other kind.
    pub signature: Option<String>,
    /// Derived from `changes` and which sides hold the object; never set
    /// independently of them.
    pub status: ObjectStatus,
    /// The work that makes the left side match the right. Empty exactly when
    /// `status` is [`ObjectStatus::Same`].
    pub changes: ChangeSet,
    /// The comparison could not be trusted to see a real difference — a
    /// PostgreSQL index whose expression the model never read. The verdict
    /// stands, being the best the model can do; a tree that drew it the same as
    /// a fully-read match would be overclaiming.
    pub uncertain: bool,
}

impl CompareEntry {
    /// Identity, stable across a refetch — what a selection set stores and what
    /// an expansion set keys on.
    pub fn key(&self) -> String {
        let mut k = String::from(self.kind.label());
        k.push(':');
        match &self.table {
            // A trigger's namespace is its *table's*, so the table carries the
            // qualifier and the trigger name stays bare — `trigger:app.city.t`,
            // which is the order a caller composing a key by hand would write.
            // Qualifying the name instead read `trigger:city.app.t`.
            Some(t) => {
                k.push_str(&display_name(self.schema.as_deref(), t));
                k.push('.');
                k.push_str(&self.name);
            }
            None => k.push_str(&display_name(self.schema.as_deref(), &self.name)),
        }
        if let Some(sig) = &self.signature {
            k.push('(');
            k.push_str(sig);
            k.push(')');
        }
        k
    }

    /// What the tree shows: the qualified name, with a trigger's table in front
    /// of it because a bare trigger name says nothing about where it lives.
    pub fn label(&self) -> String {
        match &self.table {
            Some(t) => format!("{t}.{}", self.name),
            None => display_name(self.schema.as_deref(), &self.name),
        }
    }

    /// Must the caller refresh this object's body from the lazy
    /// `Db::{trigger,routine,event}_source` before the emitted SQL can be
    /// trusted? See the module doc — MySQL's eager read resolves the escapes.
    ///
    /// False for a drop, which needs no body, and for every engine but MySQL.
    pub fn needs_source(&self) -> bool {
        self.kind.carries_body()
            && self.changes.dialect == SqlDialect::MySql
            && matches!(
                self.status,
                ObjectStatus::OnlyRight | ObjectStatus::Differing
            )
    }
}

/// How many objects fell into each status — the compare header's summary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompareCounts {
    pub same: usize,
    pub differing: usize,
    pub only_left: usize,
    pub only_right: usize,
}

impl CompareCounts {
    /// Everything that is not [`ObjectStatus::Same`].
    pub fn differences(&self) -> usize {
        self.differing + self.only_left + self.only_right
    }
}

/// Two databases, paired object by object.
#[derive(Clone, Debug)]
pub struct SchemaComparison {
    /// Every object either side holds, **in plan order** — types before the
    /// tables naming them, dependents dropped before their tables and created
    /// after, drops of a kind after the creates of it. Group by
    /// [`CompareEntry::kind`] for display rather than re-sorting in place.
    pub entries: Vec<CompareEntry>,
    /// Both sides' engine.
    pub dialect: SqlDialect,
    /// A foreign-key cycle meant no create order satisfies every reference, so
    /// one edge was broken. The plan still holds every object; this is what
    /// says the order alone can't be trusted.
    pub cycles: bool,
}

impl SchemaComparison {
    /// Pair every object in `left` with its namesake in `right`, and ask the
    /// differ what it would take to make `left` match.
    ///
    /// `dialect` is both sides' engine; see the module doc for why there is only
    /// one of it.
    pub fn of(left: &DbSchema, right: &DbSchema, dialect: SqlDialect) -> SchemaComparison {
        // The flavour of the side the DDL runs on. MariaDB's `ALTER TABLE`
        // diverges from MySQL's in ways that lose a column's own CHECK, and it
        // is the *left* server that will read the statements.
        let target = Target::new(dialect, left.flavour);
        let mut entries: Vec<CompareEntry> = Vec::new();

        // ── tables and views ────────────────────────────────────────────────
        //
        // `is_view` is part of the **key**, not something to diff: no `ALTER`
        // turns one into the other, so a name that swapped kinds is a drop and
        // a create rather than a migration nobody can express.
        let table_key = |t: &TableInfo| {
            format!(
                "{}{}",
                if t.is_view { "v:" } else { "t:" },
                display_name(t.schema.as_deref(), &t.name)
            )
        };
        for (l, r) in pair(
            left.tables.iter().map(|t| (table_key(t), t)),
            right.tables.iter().map(|t| (table_key(t), t)),
        ) {
            entries.push(table_entry(l, r, target, dialect));
        }

        // ── triggers ────────────────────────────────────────────────────────
        //
        // Flattened out of their tables, because a trigger is an object a user
        // reads and migrates on its own. Keyed by table *and* name: MySQL scopes
        // a trigger name to the schema and PostgreSQL to the table, and the
        // wider key is correct under both.
        let trigger_key =
            |t: &TriggerInfo| format!("{}.{}", display_name(t.schema.as_deref(), &t.table), t.name);
        for (l, r) in pair(
            left.tables
                .iter()
                .flat_map(|t| t.triggers.iter())
                .map(|tr| (trigger_key(tr), tr)),
            right
                .tables
                .iter()
                .flat_map(|t| t.triggers.iter())
                .map(|tr| (trigger_key(tr), tr)),
        ) {
            entries.push(trigger_entry(l, r, dialect));
        }

        // ── routines ────────────────────────────────────────────────────────
        //
        // The identity arguments are in the key: PostgreSQL overloads on them,
        // so two functions of one name are two objects, and a key without them
        // would silently pair the wrong pair.
        let routine_key = |r: &RoutineInfo| {
            format!(
                "{:?}:{}({})",
                r.kind,
                display_name(r.schema.as_deref(), &r.name),
                r.identity_arguments
            )
        };
        for (l, r) in pair(
            left.routines.iter().map(|r| (routine_key(r), r.as_ref())),
            right.routines.iter().map(|r| (routine_key(r), r.as_ref())),
        ) {
            entries.push(routine_entry(l, r, dialect));
        }

        // ── events ──────────────────────────────────────────────────────────
        let event_key = |e: &EventInfo| display_name(e.schema.as_deref(), &e.name);
        for (l, r) in pair(
            left.events.iter().map(|e| (event_key(e), e.as_ref())),
            right.events.iter().map(|e| (event_key(e), e.as_ref())),
        ) {
            entries.push(event_entry(l, r, dialect));
        }

        // ── standalone types ────────────────────────────────────────────────
        let enum_key = |e: &EnumInfo| display_name(e.schema.as_deref(), &e.name);
        for (l, r) in pair(
            left.enums.iter().map(|e| (enum_key(e), e)),
            right.enums.iter().map(|e| (enum_key(e), e)),
        ) {
            entries.push(enum_entry(l, r, left, dialect));
        }

        let domain_key = |d: &DomainInfo| display_name(d.schema.as_deref(), &d.name);
        for (l, r) in pair(
            left.domains.iter().map(|d| (domain_key(d), d)),
            right.domains.iter().map(|d| (domain_key(d), d)),
        ) {
            entries.push(domain_entry(l, r, left, dialect));
        }

        // A sequence a column owns is created by that column's `serial` or
        // identity declaration. Comparing it as an object of its own proposes a
        // `CREATE SEQUENCE` the `CREATE TABLE` already makes, and a drop the
        // table's own drop already performs.
        let seq_key = |s: &SequenceInfo| display_name(s.schema.as_deref(), &s.name);
        let standalone = |s: &&SequenceInfo| s.owned_by.is_none();
        for (l, r) in pair(
            left.sequences
                .iter()
                .filter(standalone)
                .map(|s| (seq_key(s), s)),
            right
                .sequences
                .iter()
                .filter(standalone)
                .map(|s| (seq_key(s), s)),
        ) {
            entries.push(sequence_entry(l, r, dialect));
        }

        // ── order ───────────────────────────────────────────────────────────
        let (creates, c1) = fk_rank(&right.tables, dialect, false);
        let (drops, c2) = fk_rank(&left.tables, dialect, true);
        entries.sort_by_cached_key(|e| {
            let ph = phase(e.kind, e.status);
            let name = display_name(e.schema.as_deref(), &e.name);
            // Only the two table phases carry a foreign-key rank; everything
            // else ties at zero and falls through to the name.
            let rank = match (e.kind, e.status) {
                (CompareKind::Table, ObjectStatus::OnlyRight) => {
                    creates.get(&name).copied().unwrap_or(0)
                }
                (CompareKind::Table, ObjectStatus::OnlyLeft) => {
                    drops.get(&name).copied().unwrap_or(0)
                }
                _ => 0,
            };
            (ph, kind_rank(e.kind, e.status), rank, e.key())
        });

        SchemaComparison {
            entries,
            dialect,
            cycles: c1 || c2,
        }
    }

    /// Just the entries that would contribute statements, in plan order.
    pub fn differences(&self) -> impl Iterator<Item = &CompareEntry> {
        self.entries.iter().filter(|e| e.status.is_difference())
    }

    /// The per-status tally.
    pub fn counts(&self) -> CompareCounts {
        let mut c = CompareCounts::default();
        for e in &self.entries {
            match e.status {
                ObjectStatus::Same => c.same += 1,
                ObjectStatus::Differing => c.differing += 1,
                ObjectStatus::OnlyLeft => c.only_left += 1,
                ObjectStatus::OnlyRight => c.only_right += 1,
            }
        }
        c
    }

    /// Does any difference carry a body the caller must refresh from the lazy
    /// source before the SQL is trustworthy? See [`CompareEntry::needs_source`].
    pub fn needs_source(&self) -> bool {
        self.differences().any(CompareEntry::needs_source)
    }

    /// One plan over the entries `include` accepts, in the order they must run.
    ///
    /// [`ObjectStatus::Same`] entries are never included whatever `include`
    /// says — they hold an empty change set, and a plan listing them would
    /// claim work that isn't there.
    pub fn plan(&self, include: impl Fn(&CompareEntry) -> bool) -> SchemaPlan {
        SchemaPlan {
            sets: self
                .differences()
                .filter(|e| include(e))
                .map(|e| e.changes.clone())
                .collect(),
            dialect: self.dialect,
        }
    }
}

/// A migration as several objects' change sets, ordered.
///
/// The aggregate is a list rather than one wide [`ChangeSet`] because a set
/// carries a single `table`/`schema`/`dialect` and most [`Change`] variants are
/// addressed at that one name instead of carrying their own — so a single set
/// *cannot* hold edits to two objects. Widening `Change` would put a second
/// notion of "which object" beside the one every emitter already reads, so a
/// multi-object plan stays a list of single-object sets, and everything the
/// preview modal asks for is the concatenation of what each set answers.
#[derive(Clone, Debug, Default)]
pub struct SchemaPlan {
    /// One set per object, in the order they must run.
    pub sets: Vec<ChangeSet>,
    /// The engine every set in it shares — carried here rather than read off
    /// the first set, so an empty plan still answers
    /// [`SchemaPlan::editor_script`] as the engine it was built for.
    pub dialect: SqlDialect,
}

impl SchemaPlan {
    pub fn is_empty(&self) -> bool {
        self.sets.iter().all(ChangeSet::is_empty)
    }

    /// How many objects this plan touches.
    pub fn len(&self) -> usize {
        self.sets.len()
    }

    /// Every statement, in the order they must run.
    pub fn emit(&self) -> Vec<String> {
        self.sets.iter().flat_map(ChangeSet::emit).collect()
    }

    /// The statements as one script, blank-line separated, under the same
    /// "INCOMPLETE" preamble a single set's [`ChangeSet::script`] carries.
    ///
    /// **A copied script must not look complete.** [`SchemaPlan::emit`] leaves
    /// out whatever the engine can't express faithfully, exactly as one set's
    /// does, so the omission has to travel with the text — an aggregate that
    /// concatenated only the statements would drop the one sentence saying the
    /// script is partial.
    pub fn script(&self) -> String {
        format!(
            "{}{}",
            ddl::withheld_header(&self.unsupported()),
            self.emit().join("\n\n")
        )
    }

    /// The script as it may **leave** a preview — for the clipboard and for the
    /// editor tab, split on `;` by the app's own splitter.
    ///
    /// [`ddl::client_script`] is what makes a MySQL routine or trigger body
    /// survive that split, and a compare plan is the most likely thing to carry
    /// several of them. There is no password to scrub here the way
    /// [`ChangeSet::export_script`] must: no builder this module calls produces
    /// an account change.
    pub fn editor_script(&self) -> String {
        format!(
            "{}{}",
            ddl::withheld_header(&self.unsupported()),
            ddl::client_script(&self.emit(), self.dialect)
        )
    }

    /// What a preview's risk block calls itself — the stronger of its sets'
    /// answers, since one irreversible statement makes the whole plan one.
    pub fn risk_heading(&self) -> &'static str {
        if self.sets.iter().all(ChangeSet::risk_reversible) {
            "Before you apply"
        } else {
            "This can't be undone"
        }
    }

    /// Every destructive consequence, in plan order.
    pub fn destructive(&self) -> Vec<String> {
        self.sets.iter().flat_map(ChangeSet::destructive).collect()
    }

    /// Everything the engine can't express, in plan order. Non-empty means
    /// [`SchemaPlan::emit`] is writing less than the plan asks for.
    pub fn unsupported(&self) -> Vec<String> {
        self.sets.iter().flat_map(ChangeSet::unsupported).collect()
    }

    /// One line per change, for the preview modal's summary list.
    pub fn summaries(&self) -> Vec<String> {
        self.sets
            .iter()
            .flat_map(|s| s.changes.iter().map(Change::summary))
            .collect()
    }
}

// ── pairing ─────────────────────────────────────────────────────────────────

/// Pair two sides by key: every key either side holds, once, in key order so
/// two runs of one comparison read the same.
fn pair<'a, T>(
    left: impl Iterator<Item = (String, &'a T)>,
    right: impl Iterator<Item = (String, &'a T)>,
) -> Vec<(Option<&'a T>, Option<&'a T>)> {
    let mut by_key: BTreeMap<String, (Option<&'a T>, Option<&'a T>)> = BTreeMap::new();
    for (k, v) in left {
        by_key.entry(k).or_default().0 = Some(v);
    }
    for (k, v) in right {
        by_key.entry(k).or_default().1 = Some(v);
    }
    by_key.into_values().collect()
}

/// The status a pair implies. **Read off the change set**, never beside it —
/// see the module doc.
fn status_of(on_left: bool, on_right: bool, changes: &ChangeSet) -> ObjectStatus {
    match (on_left, on_right) {
        (true, false) => ObjectStatus::OnlyLeft,
        (false, true) => ObjectStatus::OnlyRight,
        _ if changes.is_empty() => ObjectStatus::Same,
        _ => ObjectStatus::Differing,
    }
}

/// Where an entry sits in the plan. Dependents come off before their tables and
/// go on after them; a type is created before any table naming it and dropped
/// only once every table that did is gone.
fn phase(kind: CompareKind, status: ObjectStatus) -> u8 {
    match status {
        // Never planned. Sorted past everything so the difference entries keep
        // their order regardless of how many untouched objects sit between them.
        ObjectStatus::Same => 9,
        ObjectStatus::OnlyLeft if kind.depends_on_tables() => 0,
        ObjectStatus::OnlyRight | ObjectStatus::Differing if kind.is_type() => 1,
        ObjectStatus::OnlyRight if kind == CompareKind::Table => 2,
        ObjectStatus::Differing if kind == CompareKind::Table => 3,
        ObjectStatus::OnlyLeft if kind == CompareKind::Table => 4,
        ObjectStatus::OnlyRight | ObjectStatus::Differing => 5,
        ObjectStatus::OnlyLeft => 6,
    }
}

/// Where a kind sits **within** its phase, which is what settles the order
/// between two kinds a single phase holds — the created types, and the created
/// or altered table dependents.
///
/// [`CompareKind`]'s declaration order is the creation order, so this is just
/// that ordinal, **negated for a drop** because dropping runs it backwards: a
/// domain built on an enum is created after it and dropped before it. Sorting
/// those on [`CompareEntry::key`] instead put `domain:` ahead of `enum:` and
/// `trigger:` ahead of `view:` — alphabetical, which is not a dependency order
/// and emitted a `CREATE DOMAIN` naming a type the next statement created.
fn kind_rank(kind: CompareKind, status: ObjectStatus) -> i16 {
    let ordinal = kind as i16;
    match status {
        ObjectStatus::OnlyLeft => -ordinal,
        _ => ordinal,
    }
}

/// Each table's position in foreign-key order, by qualified name, plus whether
/// a cycle had to be broken. `reverse` is for the drop phase: a referencing
/// table has to go before the table it references.
///
/// [`crate::dump::order_tables`] is the sort — a second topological order over
/// the same edges is a second answer to one question.
fn fk_rank(
    tables: &[TableInfo],
    dialect: SqlDialect,
    reverse: bool,
) -> (BTreeMap<String, usize>, bool) {
    let chosen: Vec<String> = tables
        .iter()
        .filter(|t| !t.is_view)
        .map(|t| display_name(t.schema.as_deref(), &t.name))
        .collect();
    let (order, cycles) = crate::dump::order_tables(tables, &chosen, dialect);
    let n = order.len();
    let rank = order
        .into_iter()
        .enumerate()
        .map(|(pos, i)| {
            let name = display_name(tables[i].schema.as_deref(), &tables[i].name);
            (name, if reverse { n - pos } else { pos })
        })
        .collect();
    (rank, cycles)
}

// ── per-kind entries ────────────────────────────────────────────────────────

/// An empty set addressed at an object, for the one case a builder can't answer
/// (a view whose model says it isn't one). It reads as [`ObjectStatus::Same`],
/// which is the truthful answer: nothing here can state a change.
fn empty_set(name: &str, schema: Option<&str>, dialect: SqlDialect) -> ChangeSet {
    ChangeSet {
        table: name.to_string(),
        schema: schema.map(str::to_string),
        dialect,
        flavour: crate::schema::ServerFlavour::Unknown,
        changes: Vec::new(),
    }
}

fn table_entry(
    l: Option<&TableInfo>,
    r: Option<&TableInfo>,
    target: Target,
    dialect: SqlDialect,
) -> CompareEntry {
    let any = l.or(r).expect("a pair holds at least one side");
    let is_view = any.is_view;
    let kind = if is_view {
        CompareKind::View
    } else {
        CompareKind::Table
    };
    let changes = match (l, r) {
        (Some(l), Some(r)) if is_view => match ViewDraft::from_table(r) {
            Some(d) => ddl::diff_view(l, &d, dialect),
            None => empty_set(&any.name, any.schema.as_deref(), dialect),
        },
        (Some(l), Some(r)) => ddl::diff(l, &TableDraft::from_table(r), target),
        (None, Some(r)) if is_view => match ViewDraft::from_table(r) {
            Some(d) => ddl::create_view(&d, dialect),
            None => empty_set(&any.name, any.schema.as_deref(), dialect),
        },
        (None, Some(r)) => ddl::create(&TableDraft::from_table(r), dialect),
        (Some(l), None) if is_view => ddl::single(
            &l.name,
            l.schema.as_deref(),
            dialect,
            Change::DropView {
                materialized: l.view_options.as_ref().is_some_and(|o| o.materialized),
            },
        ),
        (Some(l), None) => ddl::single(&l.name, l.schema.as_deref(), dialect, Change::DropTable),
        (None, None) => unreachable!("a pair holds at least one side"),
    };
    // An index the model only partly read compares equal whatever the server
    // holds, so a match over one is a match this cannot vouch for.
    let uncertain = match (l, r) {
        (Some(l), Some(r)) => l.indexes.iter().chain(&r.indexes).any(|ix| ix.lossy),
        _ => false,
    };
    CompareEntry {
        kind,
        schema: any.schema.clone(),
        name: any.name.clone(),
        table: None,
        signature: None,
        status: status_of(l.is_some(), r.is_some(), &changes),
        changes,
        uncertain,
    }
}

fn trigger_entry(
    l: Option<&TriggerInfo>,
    r: Option<&TriggerInfo>,
    dialect: SqlDialect,
) -> CompareEntry {
    let any = l.or(r).expect("a pair holds at least one side");
    let changes = match (l, r) {
        (Some(l), Some(r)) => ddl::diff_trigger(l, &TriggerDraft::from_info(r), dialect),
        (None, Some(r)) => ddl::create_trigger(&TriggerDraft::from_info(r), dialect),
        (Some(l), None) => ddl::drop_trigger(l, dialect),
        (None, None) => unreachable!("a pair holds at least one side"),
    };
    CompareEntry {
        kind: CompareKind::Trigger,
        schema: any.schema.clone(),
        name: any.name.clone(),
        table: Some(any.table.clone()),
        signature: None,
        status: status_of(l.is_some(), r.is_some(), &changes),
        changes,
        uncertain: false,
    }
}

fn routine_entry(
    l: Option<&RoutineInfo>,
    r: Option<&RoutineInfo>,
    dialect: SqlDialect,
) -> CompareEntry {
    let any = l.or(r).expect("a pair holds at least one side");
    let changes = match (l, r) {
        (Some(l), Some(r)) => ddl::diff_routine(l, &RoutineDraft::from_info(r), dialect),
        (None, Some(r)) => ddl::create_routine(&RoutineDraft::from_info(r), dialect),
        (Some(l), None) => ddl::drop_routine(l, dialect),
        (None, None) => unreachable!("a pair holds at least one side"),
    };
    CompareEntry {
        kind: match any.kind {
            RoutineKind::Function => CompareKind::Function,
            RoutineKind::Procedure => CompareKind::Procedure,
        },
        schema: any.schema.clone(),
        name: any.name.clone(),
        table: None,
        signature: Some(any.identity_arguments.clone()),
        status: status_of(l.is_some(), r.is_some(), &changes),
        changes,
        uncertain: false,
    }
}

fn event_entry(l: Option<&EventInfo>, r: Option<&EventInfo>, dialect: SqlDialect) -> CompareEntry {
    let any = l.or(r).expect("a pair holds at least one side");
    let changes = match (l, r) {
        (Some(l), Some(r)) => ddl::diff_event(l, &EventDraft::from_info(r), dialect),
        (None, Some(r)) => ddl::create_event(&EventDraft::from_info(r), dialect),
        (Some(l), None) => ddl::drop_event(l, dialect),
        (None, None) => unreachable!("a pair holds at least one side"),
    };
    CompareEntry {
        kind: CompareKind::Event,
        schema: any.schema.clone(),
        name: any.name.clone(),
        table: None,
        signature: None,
        status: status_of(l.is_some(), r.is_some(), &changes),
        changes,
        uncertain: false,
    }
}

fn enum_entry(
    l: Option<&EnumInfo>,
    r: Option<&EnumInfo>,
    left: &DbSchema,
    dialect: SqlDialect,
) -> CompareEntry {
    let any = l.or(r).expect("a pair holds at least one side");
    let changes = match (l, r) {
        (Some(l), Some(r)) => {
            // The dependents are read off the **left** schema: they are the
            // columns this change has to re-cast, and they live where the DDL
            // runs. Asking the right side would list columns that aren't there.
            let deps = ddl::type_dependents(left, l.schema.as_deref(), &l.name);
            ddl::diff_enum(l, &EnumDraft::from_info(r), &deps, dialect)
        }
        (None, Some(r)) => ddl::create_enum(&EnumDraft::from_info(r), dialect),
        (Some(l), None) => {
            ddl::drop_object(ObjectKind::Enum, &l.name, l.schema.as_deref(), dialect)
        }
        (None, None) => unreachable!("a pair holds at least one side"),
    };
    CompareEntry {
        kind: CompareKind::Enum,
        schema: any.schema.clone(),
        name: any.name.clone(),
        table: None,
        signature: None,
        status: status_of(l.is_some(), r.is_some(), &changes),
        changes,
        uncertain: false,
    }
}

fn domain_entry(
    l: Option<&DomainInfo>,
    r: Option<&DomainInfo>,
    left: &DbSchema,
    dialect: SqlDialect,
) -> CompareEntry {
    let any = l.or(r).expect("a pair holds at least one side");
    let changes = match (l, r) {
        (Some(l), Some(r)) => {
            let deps = ddl::type_dependents(left, l.schema.as_deref(), &l.name);
            ddl::diff_domain(l, &DomainDraft::from_info(r), &deps, dialect)
        }
        (None, Some(r)) => ddl::create_domain(&DomainDraft::from_info(r), dialect),
        (Some(l), None) => {
            ddl::drop_object(ObjectKind::Domain, &l.name, l.schema.as_deref(), dialect)
        }
        (None, None) => unreachable!("a pair holds at least one side"),
    };
    CompareEntry {
        kind: CompareKind::Domain,
        schema: any.schema.clone(),
        name: any.name.clone(),
        table: None,
        signature: None,
        status: status_of(l.is_some(), r.is_some(), &changes),
        changes,
        uncertain: false,
    }
}

fn sequence_entry(
    l: Option<&SequenceInfo>,
    r: Option<&SequenceInfo>,
    dialect: SqlDialect,
) -> CompareEntry {
    let any = l.or(r).expect("a pair holds at least one side");
    let changes = match (l, r) {
        (Some(l), Some(r)) => ddl::diff_sequence(l, &SequenceDraft::from_info(r), dialect),
        (None, Some(r)) => ddl::create_sequence(&SequenceDraft::from_info(r), dialect),
        (Some(l), None) => {
            ddl::drop_object(ObjectKind::Sequence, &l.name, l.schema.as_deref(), dialect)
        }
        (None, None) => unreachable!("a pair holds at least one side"),
    };
    CompareEntry {
        kind: CompareKind::Sequence,
        schema: any.schema.clone(),
        name: any.name.clone(),
        table: None,
        signature: None,
        status: status_of(l.is_some(), r.is_some(), &changes),
        changes,
        uncertain: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ColumnInfo, ForeignKeyInfo, SequenceOwner, TriggerAction, ViewOptions};

    fn col(name: &str, ty: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            type_name: ty.to_string(),
            ..Default::default()
        }
    }

    fn table(name: &str, cols: &[(&str, &str)]) -> TableInfo {
        TableInfo {
            name: name.to_string(),
            columns: cols.iter().map(|(n, t)| col(n, t)).collect(),
            ..Default::default()
        }
    }

    fn view(name: &str, body: &str) -> TableInfo {
        TableInfo {
            name: name.to_string(),
            is_view: true,
            view_definition: Some(body.to_string()),
            view_options: Some(ViewOptions::default()),
            ..Default::default()
        }
    }

    fn trigger(name: &str, on: &str, body: &str) -> TriggerInfo {
        TriggerInfo {
            name: name.to_string(),
            table: on.to_string(),
            action: TriggerAction::Body(body.to_string()),
            ..Default::default()
        }
    }

    fn schema_of(tables: Vec<TableInfo>) -> DbSchema {
        DbSchema {
            tables,
            ..Default::default()
        }
    }

    fn find<'a>(c: &'a SchemaComparison, key: &str) -> &'a CompareEntry {
        c.entries
            .iter()
            .find(|e| e.key() == key)
            .unwrap_or_else(|| panic!("no entry {key}; have {:?}", keys(c)))
    }

    fn keys(c: &SchemaComparison) -> Vec<String> {
        c.entries.iter().map(|e| e.key()).collect()
    }

    fn mysql(left: DbSchema, right: DbSchema) -> SchemaComparison {
        SchemaComparison::of(&left, &right, SqlDialect::MySql)
    }

    // ── pairing and status ───────────────────────────────────────────────────

    #[test]
    fn two_identical_schemas_are_all_same() {
        let t = || schema_of(vec![table("city", &[("id", "int")])]);
        let c = mysql(t(), t());
        assert_eq!(c.counts().same, 1);
        assert_eq!(c.counts().differences(), 0);
        assert_eq!(find(&c, "table:city").status, ObjectStatus::Same);
    }

    #[test]
    fn a_table_only_the_left_side_has_is_only_left() {
        let c = mysql(
            schema_of(vec![table("gone", &[("id", "int")])]),
            schema_of(vec![]),
        );
        assert_eq!(find(&c, "table:gone").status, ObjectStatus::OnlyLeft);
        assert_eq!(c.counts().only_left, 1);
    }

    #[test]
    fn a_table_only_the_right_side_has_is_only_right() {
        let c = mysql(
            schema_of(vec![]),
            schema_of(vec![table("fresh", &[("id", "int")])]),
        );
        assert_eq!(find(&c, "table:fresh").status, ObjectStatus::OnlyRight);
        assert_eq!(c.counts().only_right, 1);
    }

    #[test]
    fn a_table_with_an_extra_column_on_the_right_is_differing() {
        let c = mysql(
            schema_of(vec![table("city", &[("id", "int")])]),
            schema_of(vec![table(
                "city",
                &[("id", "int"), ("name", "varchar(80)")],
            )]),
        );
        assert_eq!(find(&c, "table:city").status, ObjectStatus::Differing);
    }

    #[test]
    fn the_status_is_the_differs_verdict_and_nothing_else() {
        // The property the module doc claims: differing iff the change set is
        // non-empty, in both directions. Asserted over every entry rather than
        // one, because the failure this guards is a status computed beside the
        // differ instead of from it.
        let c = mysql(
            schema_of(vec![
                table("same", &[("id", "int")]),
                table("changed", &[("id", "int")]),
                table("gone", &[("id", "int")]),
            ]),
            schema_of(vec![
                table("same", &[("id", "int")]),
                table("changed", &[("id", "int"), ("extra", "int")]),
                table("fresh", &[("id", "int")]),
            ]),
        );
        assert_eq!(c.entries.len(), 4);
        for e in &c.entries {
            assert_eq!(
                e.changes.is_empty(),
                e.status == ObjectStatus::Same,
                "{} claims {:?} with {} changes",
                e.key(),
                e.status,
                e.changes.len()
            );
        }
    }

    #[test]
    fn the_left_sides_flavour_is_what_the_plan_targets() {
        // MariaDB's ALTER TABLE loses a column's own CHECK where MySQL's does
        // not, and it is the left server that reads the statements.
        let left = DbSchema {
            tables: vec![table("city", &[("id", "int")])],
            flavour: crate::schema::ServerFlavour::MariaDb,
            ..Default::default()
        };
        let right = DbSchema {
            tables: vec![table("city", &[("id", "int"), ("extra", "int")])],
            flavour: crate::schema::ServerFlavour::MySql,
            ..Default::default()
        };
        let c = SchemaComparison::of(&left, &right, SqlDialect::MySql);
        assert_eq!(
            find(&c, "table:city").changes.flavour,
            crate::schema::ServerFlavour::MariaDb
        );
    }

    // ── kinds ────────────────────────────────────────────────────────────────

    #[test]
    fn a_view_is_compared_as_a_view_not_a_table() {
        let c = mysql(
            schema_of(vec![view("v", "select 1")]),
            schema_of(vec![view("v", "select 2")]),
        );
        let e = find(&c, "view:v");
        assert_eq!(e.kind, CompareKind::View);
        assert_eq!(e.status, ObjectStatus::Differing);
    }

    #[test]
    fn a_name_that_is_a_table_here_and_a_view_there_is_a_drop_and_a_create() {
        // Not a "differing table": no ALTER turns one into the other, so the
        // honest reading is two objects that happen to share a name.
        let c = mysql(
            schema_of(vec![table("thing", &[("id", "int")])]),
            schema_of(vec![view("thing", "select 1")]),
        );
        assert_eq!(find(&c, "table:thing").status, ObjectStatus::OnlyLeft);
        assert_eq!(find(&c, "view:thing").status, ObjectStatus::OnlyRight);
    }

    #[test]
    fn a_trigger_is_its_own_entry_keyed_by_its_table() {
        let mut l = table("city", &[("id", "int")]);
        l.triggers = vec![trigger("t_ins", "city", "SET @a = 1")];
        let mut r = table("city", &[("id", "int")]);
        r.triggers = vec![trigger("t_ins", "city", "SET @a = 2")];
        let c = mysql(schema_of(vec![l]), schema_of(vec![r]));
        let e = find(&c, "trigger:city.t_ins");
        assert_eq!(e.kind, CompareKind::Trigger);
        assert_eq!(e.status, ObjectStatus::Differing);
        assert_eq!(e.label(), "city.t_ins");
    }

    #[test]
    fn a_trigger_the_right_side_added_is_only_right() {
        let l = table("city", &[("id", "int")]);
        let mut r = table("city", &[("id", "int")]);
        r.triggers = vec![trigger("t_ins", "city", "SET @a = 1")];
        let c = mysql(schema_of(vec![l]), schema_of(vec![r]));
        assert_eq!(
            find(&c, "trigger:city.t_ins").status,
            ObjectStatus::OnlyRight
        );
        // The table itself is untouched — a trigger is not a table difference.
        assert_eq!(find(&c, "table:city").status, ObjectStatus::Same);
    }

    #[test]
    fn two_routines_of_the_same_name_but_different_kinds_are_separate_objects() {
        let f = |kind: RoutineKind| RoutineInfo {
            name: "thing".to_string(),
            kind,
            body: "BEGIN END".to_string(),
            ..Default::default()
        };
        let left = DbSchema {
            routines: vec![std::sync::Arc::new(f(RoutineKind::Function))],
            ..Default::default()
        };
        let right = DbSchema {
            routines: vec![std::sync::Arc::new(f(RoutineKind::Procedure))],
            ..Default::default()
        };
        let c = SchemaComparison::of(&left, &right, SqlDialect::MySql);
        assert_eq!(c.counts().only_left, 1);
        assert_eq!(c.counts().only_right, 1);
        // And they are two keys, not one — a function and a procedure of one
        // name would otherwise collapse into a single tree row.
        assert_eq!(keys(&c).len(), 2);
    }

    #[test]
    fn a_sqlite_table_difference_plans_the_rebuild_not_an_alter() {
        // The three engines are asked the same question and answer it their own
        // way: SQLite cannot add a column with an `ALTER` the designer's other
        // changes need, so `diff` folds the whole table into a rebuild. Pinned
        // because the plan reaches `emit` through the dialect on each set, and a
        // compare that handed SQLite MySQL's shapes would emit statements the
        // engine refuses.
        let c = SchemaComparison::of(
            &schema_of(vec![table("city", &[("id", "int")])]),
            &schema_of(vec![table("city", &[("id", "int"), ("name", "text")])]),
            SqlDialect::Sqlite,
        );
        let e = find(&c, "table:city");
        assert_eq!(e.status, ObjectStatus::Differing);
        assert_eq!(e.changes.dialect, SqlDialect::Sqlite);
        let sql = c.plan(|_| true).script();
        assert!(!sql.contains("MODIFY COLUMN"), "{sql}");
        assert!(sql.contains("\"city\""), "sqlite quotes with \": {sql}");
    }

    #[test]
    fn two_overloads_of_one_function_are_separate_objects() {
        // PostgreSQL overloads on the argument types, so the signature is part
        // of the identity. A key without it pairs the wrong two functions.
        let f = |args: &str| RoutineInfo {
            name: "area".to_string(),
            schema: Some("public".to_string()),
            kind: RoutineKind::Function,
            identity_arguments: args.to_string(),
            body: "SELECT 1".to_string(),
            ..Default::default()
        };
        let left = DbSchema {
            routines: vec![std::sync::Arc::new(f("integer"))],
            ..Default::default()
        };
        let right = DbSchema {
            routines: vec![std::sync::Arc::new(f("text"))],
            ..Default::default()
        };
        let c = SchemaComparison::of(&left, &right, SqlDialect::Postgres);
        assert_eq!(c.counts().only_left, 1);
        assert_eq!(c.counts().only_right, 1);
        assert_eq!(keys(&c).len(), 2);
    }

    #[test]
    fn an_event_only_the_right_side_has_is_only_right() {
        let right = DbSchema {
            events: vec![std::sync::Arc::new(EventInfo {
                name: "nightly".to_string(),
                body: "DO SET @a = 1".to_string(),
                ..Default::default()
            })],
            ..Default::default()
        };
        let c = SchemaComparison::of(&DbSchema::default(), &right, SqlDialect::MySql);
        assert_eq!(find(&c, "event:nightly").status, ObjectStatus::OnlyRight);
    }

    #[test]
    fn an_enum_with_an_added_value_is_differing() {
        let e = |vals: &[&str]| EnumInfo {
            name: "mood".to_string(),
            schema: Some("app".to_string()),
            values: vals.iter().map(|v| v.to_string()).collect(),
            ..Default::default()
        };
        let left = DbSchema {
            enums: vec![e(&["ok"])],
            ..Default::default()
        };
        let right = DbSchema {
            enums: vec![e(&["ok", "sad"])],
            ..Default::default()
        };
        let c = SchemaComparison::of(&left, &right, SqlDialect::Postgres);
        assert_eq!(find(&c, "enum:app.mood").status, ObjectStatus::Differing);
    }

    #[test]
    fn the_default_namespace_is_not_in_a_key() {
        // `display_name` leaves PostgreSQL's `public` off, the same way the
        // schema tree and every tab title do — so a key reads `enum:mood`, not
        // `enum:public.mood`, and a caller matching on one won't miss it.
        let e = EnumInfo {
            name: "mood".to_string(),
            schema: Some("public".to_string()),
            values: vec!["ok".to_string()],
            ..Default::default()
        };
        let right = DbSchema {
            enums: vec![e],
            ..Default::default()
        };
        let c = SchemaComparison::of(&DbSchema::default(), &right, SqlDialect::Postgres);
        assert_eq!(keys(&c), vec!["enum:mood".to_string()]);
    }

    #[test]
    fn a_sequence_a_column_owns_is_not_an_object_of_its_own() {
        // A `serial` column's sequence is created by the column. Comparing it
        // separately proposes a CREATE SEQUENCE the CREATE TABLE already makes.
        // The standalone one beside it is what keeps this from passing by
        // simply finding no sequences at all.
        let seq = |name: &str, owner: Option<SequenceOwner>| SequenceInfo {
            name: name.to_string(),
            schema: Some("app".to_string()),
            owned_by: owner,
            ..Default::default()
        };
        let right = DbSchema {
            sequences: vec![
                seq(
                    "city_id_seq",
                    Some(SequenceOwner {
                        table: "city".to_string(),
                        column: "id".to_string(),
                        internal: true,
                    }),
                ),
                seq("order_no", None),
            ],
            ..Default::default()
        };
        let c = SchemaComparison::of(&DbSchema::default(), &right, SqlDialect::Postgres);
        assert_eq!(
            keys(&c),
            vec!["sequence:app.order_no".to_string()],
            "only the standalone sequence is an object of its own"
        );
    }

    #[test]
    fn a_namespace_is_part_of_an_objects_identity() {
        let t = |ns: &str| TableInfo {
            name: "city".to_string(),
            schema: Some(ns.to_string()),
            columns: vec![col("id", "int")],
            ..Default::default()
        };
        let left = schema_of(vec![t("app")]);
        let right = schema_of(vec![t("other")]);
        let c = SchemaComparison::of(&left, &right, SqlDialect::Postgres);
        assert_eq!(find(&c, "table:app.city").status, ObjectStatus::OnlyLeft);
        assert_eq!(find(&c, "table:other.city").status, ObjectStatus::OnlyRight);
    }

    // ── the plan ─────────────────────────────────────────────────────────────

    #[test]
    fn an_all_same_comparison_plans_nothing() {
        let t = || schema_of(vec![table("city", &[("id", "int")])]);
        let plan = mysql(t(), t()).plan(|_| true);
        assert!(plan.is_empty());
        assert!(plan.emit().is_empty());
    }

    #[test]
    fn a_same_entry_is_never_planned_even_when_included() {
        // `include` says yes to everything; the plan still has to leave the
        // untouched table out, because its set holds no changes to run. The
        // changed table beside it is what proves the filter ran at all.
        let c = mysql(
            schema_of(vec![
                table("city", &[("id", "int")]),
                table("town", &[("id", "int")]),
            ]),
            schema_of(vec![
                table("city", &[("id", "int")]),
                table("town", &[("id", "int"), ("extra", "int")]),
            ]),
        );
        let plan = c.plan(|_| true);
        assert_eq!(plan.len(), 1);
        assert!(plan.script().contains("`town`"), "{}", plan.script());
    }

    #[test]
    fn the_plan_only_holds_the_entries_include_accepted() {
        let c = mysql(
            schema_of(vec![]),
            schema_of(vec![
                table("a", &[("id", "int")]),
                table("b", &[("id", "int")]),
            ]),
        );
        let plan = c.plan(|e| e.name == "a");
        assert_eq!(plan.len(), 1);
        assert!(plan.script().contains("`a`"), "{}", plan.script());
        assert!(!plan.script().contains("`b`"), "{}", plan.script());
    }

    #[test]
    fn only_left_plans_a_drop_and_only_right_plans_a_create() {
        let c = mysql(
            schema_of(vec![table("gone", &[("id", "int")])]),
            schema_of(vec![table("fresh", &[("id", "int")])]),
        );
        let sql = c.plan(|_| true).script().to_uppercase();
        assert!(sql.contains("DROP TABLE"), "{sql}");
        assert!(sql.contains("CREATE TABLE"), "{sql}");
    }

    #[test]
    fn a_drop_is_reported_as_destructive() {
        let c = mysql(
            schema_of(vec![table("gone", &[("id", "int")])]),
            schema_of(vec![]),
        );
        assert!(!c.plan(|_| true).destructive().is_empty());
    }

    #[test]
    fn a_pure_create_is_not_destructive() {
        let c = mysql(
            schema_of(vec![]),
            schema_of(vec![table("fresh", &[("id", "int")])]),
        );
        let plan = c.plan(|_| true);
        assert_eq!(plan.len(), 1, "the create has to be in the plan at all");
        assert!(plan.destructive().is_empty());
    }

    #[test]
    fn the_summaries_are_one_line_per_change() {
        let c = mysql(
            schema_of(vec![table("city", &[("id", "int")])]),
            schema_of(vec![table("city", &[("id", "int"), ("extra", "int")])]),
        );
        let plan = c.plan(|_| true);
        assert_eq!(plan.summaries().len(), plan.sets[0].len());
        assert!(!plan.summaries().is_empty());
    }

    // ── ordering ─────────────────────────────────────────────────────────────

    #[test]
    fn a_referenced_table_is_created_before_the_table_referencing_it() {
        // Foreign keys are emitted inline in CREATE TABLE, so the order between
        // two new tables is the difference between a plan that runs and one that
        // fails on an unknown reference.
        let parent = table("parent", &[("id", "int")]);
        let mut child = table("child", &[("id", "int"), ("parent_id", "int")]);
        child.foreign_keys = vec![ForeignKeyInfo {
            name: "fk_parent".to_string(),
            columns: vec!["parent_id".to_string()],
            ref_table: "parent".to_string(),
            ref_columns: vec!["id".to_string()],
            ..Default::default()
        }];
        // Child listed *first*, so insertion order can't accidentally be right,
        // and it sorts before "parent" by name too.
        let c = mysql(schema_of(vec![]), schema_of(vec![child, parent]));
        let sql = c.plan(|_| true).script();
        let at = |n: &str| sql.find(n).unwrap_or_else(|| panic!("no {n} in {sql}"));
        assert!(at("`parent`") < at("`child`"), "{sql}");
        assert!(!c.cycles);
    }

    #[test]
    fn a_referencing_table_is_dropped_before_the_table_it_references() {
        let parent = table("parent", &[("id", "int")]);
        let mut child = table("child", &[("id", "int"), ("parent_id", "int")]);
        child.foreign_keys = vec![ForeignKeyInfo {
            name: "fk_parent".to_string(),
            columns: vec!["parent_id".to_string()],
            ref_table: "parent".to_string(),
            ref_columns: vec!["id".to_string()],
            ..Default::default()
        }];
        let c = mysql(schema_of(vec![parent, child]), schema_of(vec![]));
        let names: Vec<&str> = c.differences().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["child", "parent"]);
    }

    #[test]
    fn a_foreign_key_cycle_is_reported_rather_than_hidden() {
        // No creation order satisfies a cycle. The plan still holds both tables;
        // `cycles` is what says the order alone can't be trusted.
        let fk = |to: &str| ForeignKeyInfo {
            name: format!("fk_{to}"),
            columns: vec!["other".to_string()],
            ref_table: to.to_string(),
            ref_columns: vec!["id".to_string()],
            ..Default::default()
        };
        let mut a = table("a", &[("id", "int"), ("other", "int")]);
        a.foreign_keys = vec![fk("b")];
        let mut b = table("b", &[("id", "int"), ("other", "int")]);
        b.foreign_keys = vec![fk("a")];
        let c = mysql(schema_of(vec![]), schema_of(vec![a, b]));
        assert!(c.cycles);
        assert_eq!(c.counts().only_right, 2);
    }

    #[test]
    fn a_type_is_created_before_the_tables_and_dropped_after_them() {
        let e = EnumInfo {
            name: "mood".to_string(),
            schema: Some("public".to_string()),
            values: vec!["ok".to_string()],
            ..Default::default()
        };
        let t = TableInfo {
            name: "person".to_string(),
            schema: Some("public".to_string()),
            columns: vec![col("id", "int")],
            ..Default::default()
        };
        let right = DbSchema {
            tables: vec![t.clone()],
            enums: vec![e.clone()],
            ..Default::default()
        };
        let c = SchemaComparison::of(&DbSchema::default(), &right, SqlDialect::Postgres);
        let kinds: Vec<CompareKind> = c.differences().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![CompareKind::Enum, CompareKind::Table]);

        // And the other way round: dropping runs the table first, type last.
        let left = DbSchema {
            tables: vec![t],
            enums: vec![e],
            ..Default::default()
        };
        let c = SchemaComparison::of(&left, &DbSchema::default(), SqlDialect::Postgres);
        let kinds: Vec<CompareKind> = c.differences().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![CompareKind::Table, CompareKind::Enum]);
    }

    #[test]
    fn a_new_domain_is_created_after_the_enum_it_names() {
        // `CREATE DOMAIN d AS mood` needs the enum first, and both land in the
        // same phase — so the order between two *kinds* of type has to come from
        // the dependency order, not from whatever their labels sort as.
        let right = DbSchema {
            enums: vec![EnumInfo {
                name: "mood".to_string(),
                schema: Some("app".to_string()),
                values: vec!["ok".to_string()],
                ..Default::default()
            }],
            domains: vec![DomainInfo {
                name: "feeling".to_string(),
                schema: Some("app".to_string()),
                base_type: "app.mood".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let c = SchemaComparison::of(&DbSchema::default(), &right, SqlDialect::Postgres);
        let kinds: Vec<CompareKind> = c.differences().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![CompareKind::Enum, CompareKind::Domain]);
    }

    #[test]
    fn a_dropped_enum_goes_after_the_domain_that_names_it() {
        // Dropping runs the creation order backwards: the enum cannot go while
        // a domain still names it.
        let left = DbSchema {
            enums: vec![EnumInfo {
                name: "mood".to_string(),
                schema: Some("app".to_string()),
                values: vec!["ok".to_string()],
                ..Default::default()
            }],
            domains: vec![DomainInfo {
                name: "feeling".to_string(),
                schema: Some("app".to_string()),
                base_type: "app.mood".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let c = SchemaComparison::of(&left, &DbSchema::default(), SqlDialect::Postgres);
        let kinds: Vec<CompareKind> = c.differences().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![CompareKind::Domain, CompareKind::Enum]);
    }

    #[test]
    fn a_new_view_is_created_before_the_trigger_that_names_it() {
        // Both are table dependents in one phase. A trigger body may select from
        // the view, so the view goes first — "trigger" sorting before "view"
        // alphabetically is not a dependency order.
        let mut t = table("city", &[("id", "int")]);
        t.triggers = vec![trigger("t_ins", "city", "SET @a = 1")];
        let c = mysql(
            schema_of(vec![table("city", &[("id", "int")])]),
            schema_of(vec![t, view("v", "select 1")]),
        );
        let kinds: Vec<CompareKind> = c.differences().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![CompareKind::View, CompareKind::Trigger]);
    }

    #[test]
    fn a_plan_that_withholds_a_statement_says_so_above_its_script() {
        // The rule `ChangeSet::script` follows: emit leaves out what the engine
        // can't express faithfully, and a copied script must not look complete.
        // An aggregate that concatenated only the statements would drop exactly
        // the sentence that says the text is partial.
        let mut left = table("city", &[("id", "int")]);
        left.indexes = vec![crate::schema::IndexInfo {
            name: "ix_expr".to_string(),
            lossy: true,
            ..Default::default()
        }];
        let mut right = table("city", &[("id", "int"), ("name", "text")]);
        right.indexes = vec![crate::schema::IndexInfo {
            name: "ix_expr".to_string(),
            lossy: true,
            ..Default::default()
        }];
        let c = SchemaComparison::of(
            &schema_of(vec![left]),
            &schema_of(vec![right]),
            SqlDialect::Sqlite,
        );
        let plan = c.plan(|_| true);
        assert!(
            !plan.unsupported().is_empty(),
            "the fixture has to withhold something for this to mean anything"
        );
        assert!(
            plan.script().starts_with("-- INCOMPLETE"),
            "the script has to admit it is partial: {}",
            plan.script()
        );
    }

    #[test]
    fn a_plan_that_withholds_nothing_has_no_header() {
        let c = mysql(
            schema_of(vec![]),
            schema_of(vec![table("fresh", &[("id", "int")])]),
        );
        let plan = c.plan(|_| true);
        assert!(plan.unsupported().is_empty());
        assert!(
            plan.script().starts_with("CREATE TABLE"),
            "{}",
            plan.script()
        );
    }

    #[test]
    fn a_view_is_dropped_before_the_table_it_selects_from() {
        let c = mysql(
            schema_of(vec![table("city", &[("id", "int")]), view("v", "select 1")]),
            schema_of(vec![]),
        );
        let kinds: Vec<CompareKind> = c.differences().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![CompareKind::View, CompareKind::Table]);
    }

    #[test]
    fn a_view_is_created_after_the_table_it_selects_from() {
        let c = mysql(
            schema_of(vec![]),
            schema_of(vec![view("v", "select 1"), table("city", &[("id", "int")])]),
        );
        let kinds: Vec<CompareKind> = c.differences().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![CompareKind::Table, CompareKind::View]);
    }

    #[test]
    fn a_trigger_is_created_after_the_table_it_hangs_off() {
        let mut t = table("city", &[("id", "int")]);
        t.triggers = vec![trigger("t_ins", "city", "SET @a = 1")];
        let c = mysql(schema_of(vec![]), schema_of(vec![t]));
        let kinds: Vec<CompareKind> = c.differences().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![CompareKind::Table, CompareKind::Trigger]);
    }

    #[test]
    fn untouched_objects_do_not_disturb_the_order_of_the_differences() {
        // `Same` entries sort past every difference, so a schema with a hundred
        // identical tables in it still plans in dependency order.
        let mut left = schema_of(vec![
            table("aaa_same", &[("id", "int")]),
            table("zzz_same", &[("id", "int")]),
            table("city", &[("id", "int")]),
        ]);
        let right = schema_of(vec![
            table("aaa_same", &[("id", "int")]),
            table("zzz_same", &[("id", "int")]),
            view("v", "select 1"),
        ]);
        left.tables.push(view("v", "select 1"));
        let c = SchemaComparison::of(&left, &right, SqlDialect::MySql);
        let kinds: Vec<CompareKind> = c.differences().map(|e| e.kind).collect();
        // The view is identical on both sides, so the only difference is the
        // dropped table — and it is not preceded by anything.
        assert_eq!(kinds, vec![CompareKind::Table]);
    }

    // ── honesty about what it can't see ──────────────────────────────────────

    #[test]
    fn a_match_over_a_lossy_index_is_marked_uncertain() {
        let t = || {
            let mut t = table("city", &[("id", "int")]);
            t.schema = Some("app".to_string());
            t.indexes = vec![crate::schema::IndexInfo {
                name: "ix_lower".to_string(),
                lossy: true,
                ..Default::default()
            }];
            t
        };
        let c = SchemaComparison::of(
            &schema_of(vec![t()]),
            &schema_of(vec![t()]),
            SqlDialect::Postgres,
        );
        let e = find(&c, "table:app.city");
        assert_eq!(e.status, ObjectStatus::Same);
        assert!(e.uncertain, "a partly-read index cannot vouch for a match");
    }

    #[test]
    fn a_fully_read_match_is_not_uncertain() {
        let t = || table("city", &[("id", "int")]);
        let c = mysql(schema_of(vec![t()]), schema_of(vec![t()]));
        assert!(!find(&c, "table:city").uncertain);
    }

    #[test]
    fn a_mysql_routine_being_created_needs_its_real_source() {
        let r = RoutineInfo {
            name: "thing".to_string(),
            kind: RoutineKind::Procedure,
            body: "BEGIN SET @a = 'it\\'s'; END".to_string(),
            ..Default::default()
        };
        let right = DbSchema {
            routines: vec![std::sync::Arc::new(r)],
            ..Default::default()
        };
        let c = SchemaComparison::of(&DbSchema::default(), &right, SqlDialect::MySql);
        assert!(c.needs_source());
        assert!(find(&c, "procedure:thing()").needs_source());
    }

    #[test]
    fn a_dropped_routine_needs_no_source_and_nor_does_a_table() {
        // A DROP names the object and nothing else, so the mangled body never
        // reaches the SQL — claiming otherwise would send the caller off to
        // fetch a body it has no use for.
        let r = RoutineInfo {
            name: "thing".to_string(),
            kind: RoutineKind::Procedure,
            body: "BEGIN END".to_string(),
            ..Default::default()
        };
        let left = DbSchema {
            tables: vec![table("gone", &[("id", "int")])],
            routines: vec![std::sync::Arc::new(r)],
            ..Default::default()
        };
        let c = SchemaComparison::of(&left, &DbSchema::default(), SqlDialect::MySql);
        assert_eq!(c.counts().only_left, 2);
        assert!(!c.needs_source());
    }

    #[test]
    fn a_postgres_function_needs_no_source_fetch() {
        // The mangling is MySQL's `information_schema`; PostgreSQL's body
        // arrives verbatim, so there is nothing to go back for.
        let f = RoutineInfo {
            name: "thing".to_string(),
            schema: Some("public".to_string()),
            kind: RoutineKind::Function,
            body: "SELECT 1".to_string(),
            ..Default::default()
        };
        let right = DbSchema {
            routines: vec![std::sync::Arc::new(f)],
            ..Default::default()
        };
        let c = SchemaComparison::of(&DbSchema::default(), &right, SqlDialect::Postgres);
        assert_eq!(c.counts().only_right, 1);
        assert!(!c.needs_source());
    }

    // ── the aggregate delegates, and doesn't invent ──────────────────────────

    #[test]
    fn the_plans_statements_are_exactly_its_sets_statements_in_order() {
        // The aggregate is a concatenation, not a second emitter. If this ever
        // diverges, some statement is being built here instead of in `ddl`.
        let c = mysql(
            schema_of(vec![table("gone", &[("id", "int")])]),
            schema_of(vec![table("fresh", &[("id", "int")])]),
        );
        let plan = c.plan(|_| true);
        let expected: Vec<String> = plan.sets.iter().flat_map(|s| s.emit()).collect();
        assert_eq!(plan.emit(), expected);
        assert!(!expected.is_empty());
    }

    #[test]
    fn a_namespaced_triggers_key_qualifies_its_table() {
        let mut t = TableInfo {
            name: "city".to_string(),
            schema: Some("app".to_string()),
            columns: vec![col("id", "int")],
            ..Default::default()
        };
        t.triggers = vec![TriggerInfo {
            name: "t_ins".to_string(),
            schema: Some("app".to_string()),
            table: "city".to_string(),
            action: TriggerAction::Body("SET @a = 1".to_string()),
            ..Default::default()
        }];
        let c = SchemaComparison::of(
            &schema_of(vec![t]),
            &DbSchema::default(),
            SqlDialect::Postgres,
        );
        assert!(
            keys(&c).contains(&"trigger:app.city.t_ins".to_string()),
            "{:?}",
            keys(&c)
        );
    }

    #[test]
    fn a_plan_with_a_drop_in_it_calls_itself_irreversible() {
        // The heading titles the *destructive* list, so it only means anything
        // where there is one — a create-only plan has nothing for it to head,
        // which `a_pure_create_is_not_destructive` is what pins.
        let c = mysql(
            schema_of(vec![table("gone", &[("id", "int")])]),
            schema_of(vec![]),
        );
        let plan = c.plan(|_| true);
        assert!(!plan.destructive().is_empty());
        assert_eq!(plan.risk_heading(), "This can't be undone");
    }

    #[test]
    fn the_editor_script_keeps_a_trigger_body_runnable() {
        // `client_script` wraps a MySQL body in DELIMITER so the app's own
        // splitter doesn't cut it at the semicolons inside BEGIN … END.
        let mut t = table("city", &[("id", "int")]);
        t.triggers = vec![trigger(
            "t_ins",
            "city",
            "BEGIN SET @a = 1; SET @b = 2; END",
        )];
        let c = mysql(
            schema_of(vec![table("city", &[("id", "int")])]),
            schema_of(vec![t]),
        );
        let script = c.plan(|_| true).editor_script();
        assert!(script.contains("DELIMITER"), "{script}");
    }

    #[test]
    fn an_empty_plan_carries_the_dialect_it_was_built_for() {
        let c = SchemaComparison::of(
            &DbSchema::default(),
            &DbSchema::default(),
            SqlDialect::Sqlite,
        );
        let plan = c.plan(|_| true);
        assert!(plan.is_empty());
        assert_eq!(plan.dialect, SqlDialect::Sqlite);
    }

    #[test]
    fn an_empty_plan_answers_every_question_emptily() {
        let plan = SchemaPlan::default();
        assert!(plan.is_empty());
        assert_eq!(plan.len(), 0);
        assert!(plan.emit().is_empty());
        assert!(plan.script().is_empty());
        assert!(plan.destructive().is_empty());
        assert!(plan.unsupported().is_empty());
        assert!(plan.summaries().is_empty());
    }

    #[test]
    fn comparing_two_empty_schemas_yields_no_entries() {
        let c = mysql(DbSchema::default(), DbSchema::default());
        assert!(c.entries.is_empty());
        assert_eq!(c.counts(), CompareCounts::default());
        assert!(!c.cycles);
    }

    #[test]
    fn a_key_is_unique_per_object() {
        let mut a = table("city", &[("id", "int")]);
        a.triggers = vec![trigger("t", "city", "SET @a = 1")];
        let mut b = table("town", &[("id", "int")]);
        b.triggers = vec![trigger("t", "town", "SET @a = 1")];
        let c = mysql(schema_of(vec![a, b]), DbSchema::default());
        let mut ks = keys(&c);
        let before = ks.len();
        assert_eq!(before, 4, "two tables and their two triggers");
        ks.sort();
        ks.dedup();
        assert_eq!(ks.len(), before, "duplicate keys in {ks:?}");
    }
}
