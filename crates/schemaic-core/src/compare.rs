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

use std::collections::{BTreeMap, HashSet};

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
    /// The object's `CREATE` as the **left** side has it, and empty when only
    /// the right side does.
    ///
    /// Captured here rather than derived on demand so a view showing the two
    /// sides side by side is [`crate::diff::line_diff`] over two strings and
    /// nothing else. The alternative — handing the view the two `TableInfo`s
    /// and letting it ask — is how a second opinion about what an object *is*
    /// ends up in a renderer, and this text is only ever read, never emitted:
    /// the statements come from `changes`.
    pub left_ddl: String,
    /// The same, as the **right** side has it. Empty when only the left does.
    pub right_ddl: String,
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
    /// of it because a bare trigger name says nothing about where it lives, and
    /// a routine's argument types after it because they are the rest of its
    /// name where the engine overloads on them.
    ///
    /// **The signature is not optional decoration.** A comparison is the first
    /// surface in this app to list two overloads of one PostgreSQL function
    /// side by side, and without it they draw as two identical rows: same text
    /// in the tree, same heading over the diff pane, and a filter that cannot
    /// tell them apart. Their keys differ, so ticking one and reading the other
    /// is a plan the user cannot see is not the one they meant.
    pub fn label(&self) -> String {
        let mut out = match &self.table {
            Some(t) => format!("{t}.{}", self.name),
            None => display_name(self.schema.as_deref(), &self.name),
        };
        if let Some(sig) = &self.signature {
            out.push('(');
            out.push_str(sig);
            out.push(')');
        }
        out
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

    /// Every object counted, agreed or not.
    pub fn total(&self) -> usize {
        self.same + self.differences()
    }
}

/// Can two sources be compared at all, and what to say when they can't.
///
/// **The dialect is the test, not the engine.** MySQL and MariaDB speak one
/// dialect and compare perfectly well — the difference between them rides on
/// the schema's [`ServerFlavour`] and reaches the emitter from there — while the
/// three dialects do not map onto one another at all: a type name, a default and
/// an index shape each mean something different across them, so the plan would
/// be wrong precisely where someone trusted it. A [`ChangeSet`] carries one
/// dialect for the same reason, which is why [`SchemaComparison::of`] takes one
/// and this refusal happens before it is called.
///
/// [`ServerFlavour`]: crate::schema::ServerFlavour
pub fn comparable(left: SqlDialect, right: SqlDialect) -> Result<(), String> {
    if left == right {
        return Ok(());
    }
    Err(format!(
        "{} and {} can't be compared. Type names, defaults and index shapes \
         don't carry across engines, so any migration generated from the \
         difference would be wrong.",
        left.engine_label(),
        right.engine_label()
    ))
}

/// One row of the compare tree, in display order.
#[derive(Clone, Debug)]
pub enum CompareRow<'a> {
    /// A kind's heading, with the tally of what is visible beneath it. A kind
    /// showing nothing has no heading at all.
    Group {
        kind: CompareKind,
        counts: CompareCounts,
        expanded: bool,
    },
    /// One object, belonging to the heading above it. Present only while that
    /// heading is expanded.
    Object(&'a CompareEntry),
}

/// What the tree is currently showing.
#[derive(Clone, Copy, Debug, Default)]
pub struct RowFilter<'a> {
    /// A name fragment. Matched through [`schema::object_name_matches`] — the
    /// one predicate every schema-search surface in this app matches on — over
    /// [`CompareEntry::label`], so typing a table's name finds the triggers
    /// hanging off it and not just the table. Empty shows everything.
    ///
    /// [`schema::object_name_matches`]: crate::schema::object_name_matches
    pub query: &'a str,
    /// Show the objects the two sides agree about.
    ///
    /// Off by default, and that is a reading decision rather than a performance
    /// one: a comparison is opened to find what differs, and two hundred
    /// identical tables put the four that matter below the fold.
    pub show_same: bool,
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

    /// The tree's rows, grouped by kind and filtered.
    ///
    /// **Display order, which is deliberately not plan order.** Groups run in
    /// [`CompareKind`]'s own order and objects within one run **alphabetically
    /// by label**, because a tree is read by looking a name up. The order the
    /// statements must run in lives in [`SchemaComparison::entries`] and reaches
    /// SQL through [`SchemaComparison::plan`]; nothing should derive one from the
    /// other. A group whose entries are all filtered out is omitted rather than
    /// shown empty.
    ///
    /// `expanded` holds the [`CompareKind::label`] of each open group — see
    /// [`SchemaComparison::default_expanded`] for what to seed it with.
    pub fn rows<'a>(
        &'a self,
        filter: RowFilter<'_>,
        expanded: &HashSet<String>,
    ) -> Vec<CompareRow<'a>> {
        let needle = filter.query.trim().to_lowercase();
        let mut visible: Vec<&CompareEntry> = self
            .entries
            .iter()
            .filter(|e| filter.show_same || e.status.is_difference())
            .filter(|e| {
                needle.is_empty() || crate::schema::object_name_matches(&e.label(), &needle)
            })
            .collect();
        // Kind first so the groups come out in order, then the label a reader is
        // scanning for.
        visible.sort_by_cached_key(|e| (e.kind, e.label()));

        let mut out: Vec<CompareRow<'a>> = Vec::new();
        let mut i = 0;
        while i < visible.len() {
            let kind = visible[i].kind;
            let end = visible[i..]
                .iter()
                .position(|e| e.kind != kind)
                .map_or(visible.len(), |n| i + n);
            let mut counts = CompareCounts::default();
            for e in &visible[i..end] {
                match e.status {
                    ObjectStatus::Same => counts.same += 1,
                    ObjectStatus::Differing => counts.differing += 1,
                    ObjectStatus::OnlyLeft => counts.only_left += 1,
                    ObjectStatus::OnlyRight => counts.only_right += 1,
                }
            }
            let open = expanded.contains(kind.label());
            out.push(CompareRow::Group {
                kind,
                counts,
                expanded: open,
            });
            if open {
                out.extend(visible[i..end].iter().copied().map(CompareRow::Object));
            }
            i = end;
        }
        out
    }

    /// The keys of every object the filter is **showing** that a plan could
    /// also include — what "Select all" means while a filter is narrowing the
    /// list.
    ///
    /// **Filtered, deliberately.** A "Select all" that reached past the filter
    /// ticks objects the user has not seen, and it sits in the same bar as the
    /// filter box: narrow four hundred objects to three, press it, and the
    /// footer jumps to three hundred. The two controls have to agree about what
    /// "all" is.
    ///
    /// A body [`CompareEntry::needs_source`] flags is left out for the reason it
    /// has no tick-box at all.
    pub fn selectable_keys(&self, filter: RowFilter<'_>) -> Vec<String> {
        let needle = filter.query.trim().to_lowercase();
        self.differences()
            .filter(|e| !e.needs_source())
            .filter(|e| {
                needle.is_empty() || crate::schema::object_name_matches(&e.label(), &needle)
            })
            .map(|e| e.key())
            .collect()
    }

    /// The groups to open when a comparison is first shown: every kind that has
    /// a difference in it.
    ///
    /// A kind holding nothing but agreement stays shut — it is the answer
    /// "nothing to see here", and opening it buries the kinds that do differ.
    pub fn default_expanded(&self) -> HashSet<String> {
        self.differences()
            .map(|e| e.kind.label().to_string())
            .collect()
    }

    /// One plan over the entries `include` accepts, in the order they must run.
    ///
    /// [`ObjectStatus::Same`] entries are never included whatever `include`
    /// says — they hold an empty change set, and a plan listing them would
    /// claim work that isn't there.
    pub fn plan(&self, include: impl Fn(&CompareEntry) -> bool) -> SchemaPlan {
        let sets: Vec<ChangeSet> = self
            .differences()
            .filter(|e| include(e))
            .map(|e| e.changes.clone())
            .collect();
        // A cycle only breaks a plan that **creates** a table: that is the
        // statement carrying the inline foreign key with nothing to point at
        // yet. A plan of pure alters or drops is unaffected however tangled the
        // schema is.
        //
        // It errs toward warning: the cycle is a fact about the whole
        // comparison's foreign keys, and whether the *selected* tables are the
        // ones in it can't be answered from a set (`dump::order_tables` reports
        // a cycle, not which edge). Over-reporting costs a sentence in the risk
        // block, which is the cheap side — the flag is a warning and never a
        // refusal.
        let creates_a_table = sets
            .iter()
            .flat_map(|s| s.changes.iter())
            .any(|c| matches!(c, Change::CreateTable(_)));
        SchemaPlan {
            sets,
            dialect: self.dialect,
            cycles: self.cycles && creates_a_table,
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
    /// A foreign-key cycle in the comparison this plan came from, meaning no
    /// creation order satisfies every reference. Reported through
    /// [`SchemaPlan::destructive`] rather than [`SchemaPlan::unsupported`],
    /// which is the same call [`crate::dump::DumpPlan`] makes: the statements
    /// are all there and one of them will be refused, so the honest thing is to
    /// say so above the Apply button rather than to withhold a plan the user
    /// may still want to copy and reorder.
    pub cycles: bool,
}

impl SchemaPlan {
    pub fn is_empty(&self) -> bool {
        self.sets.iter().all(ChangeSet::is_empty)
    }

    /// `schema.object` for one of this plan's sets — what puts the object's
    /// name on a line that would otherwise be about no object in particular.
    fn subject_of(set: &ChangeSet) -> String {
        display_name(set.schema.as_deref(), &set.table)
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
    /// several of them.
    pub fn editor_script(&self) -> String {
        format!(
            "{}{}",
            ddl::withheld_header(&self.unsupported()),
            ddl::client_script(&self.emit(), self.dialect)
        )
    }

    /// The script as it leaves the preview through **Copy** or **Open in
    /// editor** — [`ChangeSet::export_script`]'s counterpart, and what
    /// `DdlPreview::script` must be given.
    ///
    /// Both of those exits put the text somewhere durable: the clipboard, and a
    /// query tab whose text `tabs.json` writes in the clear. A comparison
    /// produces no account change today, so this is byte-for-byte
    /// [`SchemaPlan::editor_script`] — which is exactly why it exists as a
    /// function rather than as a sentence in a comment saying so. The property
    /// that must hold is "no plaintext password leaves this modal", and a
    /// builder that one day puts an account change in a plan should inherit it
    /// instead of having to notice the prose.
    pub fn export_script(&self) -> String {
        if !self
            .sets
            .iter()
            .flat_map(|s| s.changes.iter())
            .any(ddl::is_account_change)
        {
            return self.editor_script();
        }
        // Per set, so each one's own redaction notice travels with its
        // statements — the aggregate has no scrubber of its own to add.
        format!(
            "{}{}",
            ddl::withheld_header(&self.unsupported()),
            self.sets
                .iter()
                .map(ChangeSet::export_script)
                .collect::<Vec<_>>()
                .join("\n\n")
        )
    }

    /// What a preview's risk block calls itself — the stronger of its sets'
    /// answers, since one irreversible statement makes the whole plan one.
    pub fn risk_heading(&self) -> &'static str {
        if self.sets.iter().all(ChangeSet::risk_reversible) && !self.cycles {
            "Before you apply"
        } else {
            "This can't be undone"
        }
    }

    /// Every destructive consequence, in plan order, **each named for the
    /// object it happens to**.
    ///
    /// A single set's risks are read under a title naming that one table, so
    /// they say "Drops the table and every row in it" and leave the *which* to
    /// the heading. A plan has no such heading: eight dropped tables produced
    /// that same sentence eight times over a title reading "12 objects", on the
    /// one surface standing between someone and an irreversible `DROP`.
    ///
    /// A foreign-key cycle is reported here too — see [`SchemaPlan::cycles`].
    pub fn destructive(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .sets
            .iter()
            .flat_map(|s| {
                let subject = Self::subject_of(s);
                s.destructive()
                    .into_iter()
                    .map(move |r| format!("{subject} — {r}"))
            })
            .collect();
        if self.cycles {
            out.push(
                "The foreign keys between these tables form a cycle, so no creation \
                 order satisfies all of them. One statement will be refused for \
                 referencing a table that does not exist yet, and neither MySQL nor \
                 MariaDB rolls DDL back."
                    .to_string(),
            );
        }
        out
    }

    /// Everything the engine can't express, in plan order, each named for its
    /// object. Non-empty means [`SchemaPlan::emit`] is writing less than the
    /// plan asks for — and Apply is refused while it does, which for a plan
    /// over many objects is why the line has to say *which* tick to clear.
    pub fn unsupported(&self) -> Vec<String> {
        self.sets
            .iter()
            .flat_map(|s| {
                let subject = Self::subject_of(s);
                s.unsupported()
                    .into_iter()
                    .map(move |w| format!("{subject} — {w}"))
            })
            .collect()
    }

    /// One line per change, for the preview modal's summary list, each named
    /// for its object — see [`SchemaPlan::destructive`] for why.
    pub fn summaries(&self) -> Vec<String> {
        self.sets
            .iter()
            .flat_map(|s| {
                let subject = Self::subject_of(s);
                s.changes
                    .iter()
                    .map(Change::summary)
                    .map(move |c| format!("{subject} — {c}"))
            })
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
        left_ddl: side_ddl(l, |t| t.create_ddl(dialect)),
        right_ddl: side_ddl(r, |t| t.create_ddl(dialect)),
    }
}

/// One side's `CREATE` text, or empty when that side doesn't hold the object.
fn side_ddl<T>(side: Option<&T>, ddl: impl Fn(&T) -> String) -> String {
    side.map(ddl).unwrap_or_default()
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
        left_ddl: side_ddl(l, |t| t.create_sql(dialect)),
        right_ddl: side_ddl(r, |t| t.create_sql(dialect)),
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
        // `replace: false` — this text is read, never run, and a reader wants to
        // see the object as it stands rather than as a statement that would
        // overwrite it.
        left_ddl: side_ddl(l, |f| f.create_sql(dialect, false)),
        right_ddl: side_ddl(r, |f| f.create_sql(dialect, false)),
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
        left_ddl: side_ddl(l, |e| e.create_sql(dialect)),
        right_ddl: side_ddl(r, |e| e.create_sql(dialect)),
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
        left_ddl: side_ddl(l, |e| e.create_sql(dialect)),
        right_ddl: side_ddl(r, |e| e.create_sql(dialect)),
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
        left_ddl: side_ddl(l, |d| d.create_sql(dialect)),
        right_ddl: side_ddl(r, |d| d.create_sql(dialect)),
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
        left_ddl: side_ddl(l, |s| s.create_sql(dialect)),
        right_ddl: side_ddl(r, |s| s.create_sql(dialect)),
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

    #[test]
    fn every_line_a_plan_shows_names_the_object_it_is_about() {
        // A single set's lines are read under a title naming that one table. A
        // plan has no such title — its subject is a count — so eight dropped
        // tables produced one sentence eight times over "12 objects", on the
        // surface standing between someone and an irreversible DROP.
        let c = mysql(
            schema_of(vec![
                table("gone_a", &[("id", "int")]),
                table("gone_b", &[("id", "int")]),
            ]),
            schema_of(vec![]),
        );
        let plan = c.plan(|_| true);
        let summaries = plan.summaries();
        assert!(
            summaries.iter().any(|s| s.starts_with("gone_a — ")),
            "{summaries:?}"
        );
        assert!(
            summaries.iter().any(|s| s.starts_with("gone_b — ")),
            "{summaries:?}"
        );
        // And the risks, which are the half that matters most.
        let risks = plan.destructive();
        assert_eq!(risks.len(), 2, "{risks:?}");
        assert!(
            risks.iter().any(|r| r.starts_with("gone_a — ")),
            "{risks:?}"
        );
        assert!(
            risks.iter().any(|r| r.starts_with("gone_b — ")),
            "{risks:?}"
        );
        // Two objects, two *distinguishable* lines — the failure was that they
        // were byte-identical.
        assert_ne!(risks[0], risks[1]);
    }

    #[test]
    fn a_namespaced_object_is_named_with_its_namespace_in_a_plans_lines() {
        let t = |ns: &str| TableInfo {
            name: "city".to_string(),
            schema: Some(ns.to_string()),
            columns: vec![col("id", "int")],
            ..Default::default()
        };
        let c = SchemaComparison::of(
            &schema_of(vec![t("app")]),
            &DbSchema::default(),
            SqlDialect::Postgres,
        );
        let risks = c.plan(|_| true).destructive();
        assert!(
            risks.iter().all(|r| r.starts_with("app.city — ")),
            "{risks:?}"
        );
    }

    #[test]
    fn a_withheld_line_names_the_object_whose_tick_has_to_be_cleared() {
        // Apply is refused while anything is withheld. Over one object that is
        // "don't apply half an edit"; over two hundred it is "one of these is
        // blocking the rest", and a bare summary doesn't say which.
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
        let withheld = c.plan(|_| true).unsupported();
        assert!(!withheld.is_empty());
        assert!(
            withheld.iter().all(|w| w.starts_with("city — ")),
            "{withheld:?}"
        );
    }

    #[test]
    fn a_cycle_is_reported_above_apply_when_the_plan_creates_a_table() {
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
        let plan = c.plan(|_| true);
        assert!(plan.cycles);
        assert!(
            plan.destructive().iter().any(|r| r.contains("cycle")),
            "{:?}",
            plan.destructive()
        );
        // A cycle is a statement the server will refuse, so the plan cannot
        // call itself reversible.
        assert_eq!(plan.risk_heading(), "This can't be undone");
        // But it is a warning, not a refusal: the statements are all there.
        assert!(plan.unsupported().is_empty());
        assert!(!plan.emit().is_empty());
    }

    #[test]
    fn a_cycle_is_not_reported_for_a_plan_that_creates_no_table() {
        // The inline foreign key in a CREATE is what a cycle breaks. A plan of
        // pure alters is unaffected however tangled the schema is.
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
        let mut a2 = a.clone();
        a2.columns.push(col("extra", "int"));
        let c = mysql(schema_of(vec![a, b.clone()]), schema_of(vec![a2, b]));
        assert!(c.cycles, "the schema still has the cycle");
        let plan = c.plan(|_| true);
        assert!(!plan.cycles, "but this plan only alters");
        assert!(!plan.destructive().iter().any(|r| r.contains("cycle")));
    }

    #[test]
    fn a_plan_with_no_account_change_exports_exactly_what_the_editor_gets() {
        // The property is "no plaintext password leaves this modal". A
        // comparison produces no account change, so the two are byte-identical
        // — which is why `export_script` exists as a function rather than as a
        // comment claiming the two are interchangeable here.
        let c = mysql(
            schema_of(vec![]),
            schema_of(vec![table("fresh", &[("id", "int")])]),
        );
        let plan = c.plan(|_| true);
        assert_eq!(plan.export_script(), plan.editor_script());
        assert!(!plan.export_script().is_empty());
    }

    // ── what "select all" means ──────────────────────────────────────────────

    #[test]
    fn select_all_covers_only_what_the_filter_is_showing() {
        let c = mysql(
            schema_of(vec![]),
            schema_of(vec![
                table("user_role", &[("id", "int")]),
                table("user_group", &[("id", "int")]),
                table("invoice", &[("id", "int")]),
            ]),
        );
        let all = c.selectable_keys(RowFilter::default());
        assert_eq!(all.len(), 3);
        let narrowed = c.selectable_keys(RowFilter {
            query: "user_",
            show_same: false,
        });
        assert_eq!(narrowed.len(), 2, "{narrowed:?}");
        assert!(narrowed.iter().all(|k| k.contains("user_")), "{narrowed:?}");
    }

    #[test]
    fn select_all_never_covers_a_body_that_has_to_be_re_read() {
        let mut t = table("city", &[("id", "int")]);
        t.triggers = vec![trigger("t_ins", "city", "SET @a = 1")];
        let c = mysql(schema_of(vec![]), schema_of(vec![t]));
        let keys = c.selectable_keys(RowFilter::default());
        assert!(keys.iter().any(|k| k.starts_with("table:")), "{keys:?}");
        assert!(
            !keys.iter().any(|k| k.starts_with("trigger:")),
            "a blocked body has no tick to select: {keys:?}"
        );
    }

    #[test]
    fn a_routines_label_carries_its_signature_so_two_overloads_read_apart() {
        // The tree draws `label()`, sorts on it and filters on it. Without the
        // signature two overloads are two identical rows, and ticking one while
        // reading the other is a plan the user cannot see is wrong.
        let f = |args: &str| RoutineInfo {
            name: "area".to_string(),
            schema: Some("app".to_string()),
            kind: RoutineKind::Function,
            identity_arguments: args.to_string(),
            body: "SELECT 1".to_string(),
            ..Default::default()
        };
        let right = DbSchema {
            routines: vec![
                std::sync::Arc::new(f("integer")),
                std::sync::Arc::new(f("text")),
            ],
            ..Default::default()
        };
        let c = SchemaComparison::of(&DbSchema::default(), &right, SqlDialect::Postgres);
        let labels: Vec<String> = c.differences().map(|e| e.label()).collect();
        assert_eq!(labels.len(), 2);
        assert_ne!(labels[0], labels[1], "{labels:?}");
        assert!(
            labels.contains(&"app.area(integer)".to_string()),
            "{labels:?}"
        );
        // And the filter can now separate them, since it matches on the label.
        let narrowed = c.selectable_keys(RowFilter {
            query: "(text)",
            show_same: false,
        });
        assert_eq!(narrowed.len(), 1, "{narrowed:?}");
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

    // ── the side-by-side text ────────────────────────────────────────────────

    #[test]
    fn a_differing_object_carries_both_sides_ddl() {
        let c = mysql(
            schema_of(vec![table("city", &[("id", "int")])]),
            schema_of(vec![table("city", &[("id", "int"), ("name", "text")])]),
        );
        let e = find(&c, "table:city");
        assert!(e.left_ddl.contains("`id`"), "{}", e.left_ddl);
        assert!(!e.left_ddl.contains("`name`"), "{}", e.left_ddl);
        assert!(e.right_ddl.contains("`name`"), "{}", e.right_ddl);
    }

    #[test]
    fn a_one_sided_object_leaves_the_other_sides_ddl_empty() {
        // What makes the diff pane read as a whole-object add or remove without
        // the view needing to know which case it is looking at.
        let c = mysql(
            schema_of(vec![table("gone", &[("id", "int")])]),
            schema_of(vec![table("fresh", &[("id", "int")])]),
        );
        let gone = find(&c, "table:gone");
        assert!(!gone.left_ddl.is_empty());
        assert!(gone.right_ddl.is_empty());
        let fresh = find(&c, "table:fresh");
        assert!(fresh.left_ddl.is_empty());
        assert!(!fresh.right_ddl.is_empty());
    }

    #[test]
    fn every_kind_captures_a_ddl_for_the_side_that_holds_it() {
        // The pane is one `line_diff` over these two strings, so a kind whose
        // builder was never wired would show an empty diff and read as
        // "identical" — the failure this covers, over every kind at once.
        let mut t = table("city", &[("id", "int")]);
        t.triggers = vec![trigger("t_ins", "city", "SET @a = 1")];
        let right = DbSchema {
            tables: vec![t, view("v", "select 1")],
            enums: vec![EnumInfo {
                name: "mood".to_string(),
                values: vec!["ok".to_string()],
                ..Default::default()
            }],
            domains: vec![DomainInfo {
                name: "feeling".to_string(),
                base_type: "text".to_string(),
                ..Default::default()
            }],
            sequences: vec![SequenceInfo {
                name: "counter".to_string(),
                ..Default::default()
            }],
            routines: vec![std::sync::Arc::new(RoutineInfo {
                name: "fn_thing".to_string(),
                kind: RoutineKind::Function,
                body: "SELECT 1".to_string(),
                ..Default::default()
            })],
            events: vec![std::sync::Arc::new(EventInfo {
                name: "nightly".to_string(),
                body: "DO SET @a = 1".to_string(),
                ..Default::default()
            })],
            ..Default::default()
        };
        let c = SchemaComparison::of(&DbSchema::default(), &right, SqlDialect::Postgres);
        assert!(c.entries.len() >= 7, "{:?}", keys(&c));
        for e in &c.entries {
            assert!(
                !e.right_ddl.trim().is_empty(),
                "{} ({:?}) captured no DDL for the side that holds it",
                e.key(),
                e.kind
            );
        }
    }

    // ── comparability ────────────────────────────────────────────────────────

    #[test]
    fn one_dialect_compares_with_itself() {
        assert!(comparable(SqlDialect::MySql, SqlDialect::MySql).is_ok());
        assert!(comparable(SqlDialect::Postgres, SqlDialect::Postgres).is_ok());
        assert!(comparable(SqlDialect::Sqlite, SqlDialect::Sqlite).is_ok());
    }

    #[test]
    fn two_dialects_are_refused_by_name() {
        let e = comparable(SqlDialect::MySql, SqlDialect::Postgres).unwrap_err();
        assert!(e.contains("MySQL/MariaDB"), "{e}");
        assert!(e.contains("PostgreSQL"), "{e}");
        // Refused every way round, not just the one the picker happens to build.
        assert!(comparable(SqlDialect::Postgres, SqlDialect::MySql).is_err());
        assert!(comparable(SqlDialect::Sqlite, SqlDialect::MySql).is_err());
        assert!(comparable(SqlDialect::Postgres, SqlDialect::Sqlite).is_err());
    }

    // ── the tree's rows ──────────────────────────────────────────────────────

    fn open(kinds: &[CompareKind]) -> HashSet<String> {
        kinds.iter().map(|k| k.label().to_string()).collect()
    }

    fn mixed() -> SchemaComparison {
        mysql(
            schema_of(vec![
                table("agreed", &[("id", "int")]),
                table("changed", &[("id", "int")]),
                table("gone", &[("id", "int")]),
                view("v_gone", "select 1"),
            ]),
            schema_of(vec![
                table("agreed", &[("id", "int")]),
                table("changed", &[("id", "int"), ("extra", "int")]),
                table("fresh", &[("id", "int")]),
            ]),
        )
    }

    #[test]
    fn a_collapsed_group_shows_its_heading_and_none_of_its_objects() {
        let c = mixed();
        let rows = c.rows(RowFilter::default(), &HashSet::new());
        assert!(rows.iter().all(|r| matches!(r, CompareRow::Group { .. })));
        // Tables and the one view — two headings, no objects.
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn an_expanded_group_lists_its_objects_under_it() {
        let c = mixed();
        let rows = c.rows(RowFilter::default(), &open(&[CompareKind::Table]));
        let mut it = rows.iter();
        assert!(matches!(
            it.next(),
            Some(CompareRow::Group {
                kind: CompareKind::Table,
                ..
            })
        ));
        let names: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                CompareRow::Object(e) => Some(e.name.as_str()),
                _ => None,
            })
            .collect();
        // Alphabetical, and without the table both sides agree about.
        assert_eq!(names, vec!["changed", "fresh", "gone"]);
    }

    #[test]
    fn the_display_order_is_alphabetical_while_the_plan_stays_in_dependency_order() {
        // The two orders are different answers to different questions, and this
        // is the test that keeps anyone from deriving one from the other.
        let parent = table("parent", &[("id", "int")]);
        let mut child = table("child", &[("id", "int"), ("parent_id", "int")]);
        child.foreign_keys = vec![ForeignKeyInfo {
            name: "fk_parent".to_string(),
            columns: vec!["parent_id".to_string()],
            ref_table: "parent".to_string(),
            ref_columns: vec!["id".to_string()],
            ..Default::default()
        }];
        let c = mysql(schema_of(vec![]), schema_of(vec![child, parent]));

        let shown: Vec<&str> = c
            .rows(RowFilter::default(), &open(&[CompareKind::Table]))
            .iter()
            .filter_map(|r| match r {
                CompareRow::Object(e) => Some(e.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            shown,
            vec!["child", "parent"],
            "the tree reads alphabetically"
        );

        let planned: Vec<&str> = c.differences().map(|e| e.name.as_str()).collect();
        assert_eq!(
            planned,
            vec!["parent", "child"],
            "the plan still creates the referenced table first"
        );
    }

    #[test]
    fn show_same_is_what_brings_the_agreed_objects_in() {
        let c = mixed();
        let with = c.rows(
            RowFilter {
                query: "",
                show_same: true,
            },
            &open(&[CompareKind::Table]),
        );
        let names: Vec<&str> = with
            .iter()
            .filter_map(|r| match r {
                CompareRow::Object(e) => Some(e.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["agreed", "changed", "fresh", "gone"]);
    }

    #[test]
    fn a_groups_counts_tally_only_what_is_visible_beneath_it() {
        let c = mixed();
        let rows = c.rows(RowFilter::default(), &HashSet::new());
        let tables = rows
            .iter()
            .find_map(|r| match r {
                CompareRow::Group {
                    kind: CompareKind::Table,
                    counts,
                    ..
                } => Some(*counts),
                _ => None,
            })
            .expect("a table group");
        // The agreed table is filtered out, so it is not in the heading either —
        // a count that included it would contradict the rows below it.
        assert_eq!(
            tables,
            CompareCounts {
                same: 0,
                differing: 1,
                only_left: 1,
                only_right: 1,
            }
        );
    }

    #[test]
    fn a_query_matches_a_triggers_table_as_well_as_its_own_name() {
        // The label is what is matched, so looking up a table finds what hangs
        // off it. Searching the bare object name would hide the trigger.
        let mut r = table("city", &[("id", "int")]);
        r.triggers = vec![trigger("audit_row", "city", "SET @a = 1")];
        let c = mysql(
            schema_of(vec![table("city", &[("id", "int")])]),
            schema_of(vec![r]),
        );
        let rows = c.rows(
            RowFilter {
                query: "city",
                show_same: true,
            },
            &open(&[CompareKind::Table, CompareKind::Trigger]),
        );
        let labels: Vec<String> = rows
            .iter()
            .filter_map(|r| match r {
                CompareRow::Object(e) => Some(e.label()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"city".to_string()), "{labels:?}");
        assert!(labels.contains(&"city.audit_row".to_string()), "{labels:?}");
    }

    #[test]
    fn a_query_matching_nothing_leaves_no_headings_behind() {
        let c = mixed();
        let rows = c.rows(
            RowFilter {
                query: "no_such_object",
                show_same: true,
            },
            &open(&[CompareKind::Table]),
        );
        assert!(rows.is_empty(), "{rows:?}");
    }

    #[test]
    fn a_query_is_trimmed_and_case_insensitive() {
        let c = mixed();
        let hits = |q: &str| {
            c.rows(
                RowFilter {
                    query: q,
                    show_same: false,
                },
                &open(&[CompareKind::Table]),
            )
            .iter()
            .filter(|r| matches!(r, CompareRow::Object(_)))
            .count()
        };
        assert_eq!(hits("CHANGED"), 1);
        assert_eq!(hits("  changed  "), 1);
    }

    #[test]
    fn the_default_expansion_opens_every_kind_that_differs_and_no_other() {
        let mut left = schema_of(vec![
            table("changed", &[("id", "int")]),
            view("agreed_view", "select 1"),
        ]);
        let right = schema_of(vec![
            table("changed", &[("id", "int"), ("extra", "int")]),
            view("agreed_view", "select 1"),
        ]);
        left.enums = vec![];
        let c = SchemaComparison::of(&left, &right, SqlDialect::MySql);
        let seed = c.default_expanded();
        assert!(seed.contains("table"), "{seed:?}");
        assert!(
            !seed.contains("view"),
            "a kind that only agrees stays shut: {seed:?}"
        );
    }

    #[test]
    fn an_all_same_comparison_has_no_rows_to_show_by_default() {
        let t = || schema_of(vec![table("city", &[("id", "int")])]);
        let c = mysql(t(), t());
        assert!(
            c.rows(RowFilter::default(), &c.default_expanded())
                .is_empty()
        );
        // The objects are still there to be shown on request.
        assert_eq!(
            c.rows(
                RowFilter {
                    query: "",
                    show_same: true
                },
                &open(&[CompareKind::Table])
            )
            .len(),
            2,
            "one heading and one object"
        );
    }

    #[test]
    fn the_counts_total_everything_either_side_holds() {
        let c = mixed();
        assert_eq!(c.counts().total(), c.entries.len());
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
