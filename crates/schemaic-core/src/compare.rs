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

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashSet};

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
    /// **The namespace is in it, on both arms.** A trigger's arm read
    /// `format!("{table}.{name}")` and dropped the schema its own [`key`] keeps,
    /// so two triggers named `t` on `city` in two PostgreSQL namespaces drew as
    /// two identical rows — same text in the tree, same heading over the diff
    /// pane. Worse than looking alike: [`SchemaComparison::selectable_keys`]
    /// filters on this text, so typing `archive` matched neither of them and
    /// returned an empty set. The one object that differed was invisible *and*
    /// unselectable, and *Select all* was a silent no-op over it.
    ///
    /// [`key`]: CompareEntry::key
    pub fn label(&self) -> String {
        let mut out = match &self.table {
            // The qualifier goes on the *table*, not on the trigger — a
            // trigger's namespace is its table's, and `city.app.t` is not a name
            // anyone would write. The same order `key` composes.
            Some(t) => format!("{}.{}", display_name(self.schema.as_deref(), t), self.name),
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
            && !ddl::schema_body_is_emittable(self.changes.dialect)
            && matches!(
                self.status,
                ObjectStatus::OnlyRight | ObjectStatus::Differing
            )
    }

    /// Do the two sides' **texts** differ while the structured comparison says
    /// they agree?
    ///
    /// A row marked `Same` with a full red-and-green pane under it is a
    /// contradiction the reader cannot resolve, and it is reachable on SQLite:
    /// `status` comes from [`crate::ddl::diff`] over the model, while
    /// `left_ddl`/`right_ddl` come from [`crate::schema::TableInfo::create_ddl`],
    /// which on that engine returns `sqlite_master.sql` **verbatim** — a
    /// deliberate fidelity decision. Two tables the differ calls identical can
    /// therefore carry different whitespace, quoting or clause order, and the
    /// pane draws every one of those as a change.
    ///
    /// Nothing is hidden on the strength of this: the pane still shows both
    /// texts, because they really do differ and the user may want to see how.
    /// What it adds is the sentence saying the difference is not a migration.
    pub fn text_differs_though_same(&self) -> bool {
        self.status == ObjectStatus::Same && self.left_ddl != self.right_ddl
    }

    /// A difference this comparison **has no statement for** — it is on one
    /// side only, or the two sides differ, and the change set is empty.
    ///
    /// One case reaches it today: a view whose model says it is not one, which
    /// is what a view definition the connecting role cannot read looks like
    /// (`empty_set` builds the set for it). `status_of` reads the change set
    /// only on a *two*-sided pair, so a one-sided one keeps `OnlyLeft` /
    /// `OnlyRight` however empty its set is — the row was a difference, the plan
    /// counted it, and `emit()` wrote nothing for it: "Applied 0 statements to 1
    /// object".
    ///
    /// Kept out of a plan and disclosed by it, exactly as
    /// [`CompareEntry::needs_source`] is — the two are the same shape of
    /// problem, an object the tree can show and the migration cannot carry.
    ///
    /// **It asks what the set `emit()`s, not whether the set is empty.** Those
    /// are different questions, and the second one admitted the first input that
    /// defeats it: `columns_equal` raises a change for a PostgreSQL identity
    /// kind (`GENERATED ALWAYS` vs `BY DEFAULT`) that `pg_column_clauses` has no
    /// arm for, so the set is non-empty and emits nothing. The entry was counted
    /// as a planned object, the preview listed one change over an empty SQL box,
    /// Apply was enabled, and the success line read *"Applied 0 statements to 1
    /// object"* — the exact sentence above. Nothing else could catch it:
    /// `ChangeSet::unsupported` filters on `supports_change`, which is `true`
    /// for a PostgreSQL `AlterColumn` whatever the clause builder can express.
    ///
    /// An empty set emits nothing, so the case this was written for is still
    /// covered.
    pub fn unplannable(&self) -> bool {
        self.status.is_difference() && self.changes.emit().is_empty()
    }

    /// The one-line disclosure for an entry a plan cannot carry — see
    /// [`SchemaPlan::omitted`].
    fn omission_note(&self) -> String {
        let why = if self.needs_source() {
            "its body must be re-read from the server before it can be applied"
        } else if self.changes.is_empty() {
            "this comparison has no statement for it — its definition could not be read"
        } else {
            // The third case, and it is not about reading: the difference was
            // seen and this engine's emitter has no clause for it. Saying "its
            // definition could not be read" there sends the reader to check a
            // privilege that is fine.
            "this comparison has no statement for the difference it found"
        };
        format!("{} {} — {why}", self.kind.label(), self.label())
    }
}

/// How many objects fell into each status — the compare header's summary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompareCounts {
    pub same: usize,
    pub differing: usize,
    pub only_left: usize,
    pub only_right: usize,
    /// Objects whose match this comparison **cannot vouch for** — see
    /// [`CompareEntry::uncertain`]. Counted across every status, and overlapping
    /// them: an uncertain object is also one of the four above.
    ///
    /// **It is here because it was nowhere else.** The flag was drawn only as a
    /// per-row hint, and an uncertain match is overwhelmingly an object that
    /// came out `Same` — which `show_same: false`, the default, hides. So the
    /// one case the flag exists to disclose was the one case nothing disclosed:
    /// a lossy PostgreSQL index compares equal whatever the server holds, and
    /// the tree said the two agreed.
    pub uncertain: usize,
}

impl<'a> FromIterator<&'a CompareEntry> for CompareCounts {
    /// The tally of any run of entries.
    ///
    /// **One tally, because there were two.** `SchemaComparison::counts` and
    /// `SchemaComparison::rows` each carried a byte-identical four-arm `match`
    /// filling this struct, a hundred lines apart, differing only in what they
    /// iterated. A fifth `ObjectStatus` is a compile error in neither — both
    /// matches are exhaustive over today's four — so both would keep compiling
    /// and one would silently stop counting the new one, and the header
    /// summary and the per-group headings would then disagree. That is the
    /// thing `uncertain`'s own doc was added to prevent in the other
    /// direction.
    ///
    /// The `uncertain` bump is here too, and it is deliberately not an arm of
    /// the match: an uncertain object is also one of the four, and the summary
    /// line says both things about it.
    fn from_iter<I: IntoIterator<Item = &'a CompareEntry>>(entries: I) -> Self {
        let mut c = CompareCounts::default();
        for e in entries {
            match e.status {
                ObjectStatus::Same => c.same += 1,
                ObjectStatus::Differing => c.differing += 1,
                ObjectStatus::OnlyLeft => c.only_left += 1,
                ObjectStatus::OnlyRight => c.only_right += 1,
            }
            if e.uncertain {
                c.uncertain += 1;
            }
        }
        c
    }
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
    /// A foreign-key cycle among the **right** schema's tables: no creation
    /// order satisfies every reference, so one edge was broken.
    ///
    /// Separate from [`SchemaComparison::cycles_drop`] because the two are
    /// different facts about different schemas, and folding them together with
    /// an `||` let a tangle on one side raise the other side's warning — a plan
    /// that only created one unreferenced table read "This can't be undone"
    /// because two untouched tables elsewhere referenced each other.
    pub cycles_create: bool,
    /// The same, among the **left** schema's tables, for the drop order — a
    /// referencing table has to be dropped before the table it references, and
    /// a cycle means no order does that either.
    pub cycles_drop: bool,
    /// PostgreSQL namespaces the **right** side's objects live in that the left
    /// side has none of — the prerequisite a migration into them needs and had
    /// no way to state.
    ///
    /// A namespace is not an object this comparison pairs (nothing introspects
    /// an empty one, and there is nothing in it to diff), so `CREATE TABLE
    /// reporting.sales` was emitted against a database with no `reporting` in
    /// it: PostgreSQL refuses that statement, and with it the transaction the
    /// whole migration runs in. [`SchemaComparison::plan`] turns the ones a plan
    /// actually needs into `CREATE SCHEMA` statements ahead of everything else.
    ///
    /// Empty on MySQL and SQLite by construction rather than by a dialect test:
    /// neither has a level between the database and the table, so every
    /// object's namespace there is `None`.
    pub new_namespaces: Vec<String>,
    /// Constraint and index names a table this comparison drops still holds and
    /// a table it creates needs — see [`occupied_names`], which is what a table
    /// **rename** produces on every engine.
    ///
    /// The resolvable ones are already settled: their drop was pulled ahead of
    /// the creates, and they are kept only so a reader can see the plan is
    /// deliberately not in phase order. The rest are disclosed through
    /// [`SchemaPlan::destructive`], for [`SchemaPlan::cycles`]' reason — the
    /// statements are all there and one of them will be refused.
    pub name_clashes: Vec<NameClash>,
}

/// Why a comparison's tree is empty — see [`SchemaComparison::empty_reason`].
///
/// Four arms and not three: "nothing matched" and "what matched, you asked not
/// to see" are different answers, and giving the second the first's sentence
/// tells the user their filter is wrong when it is the toggle beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmptyRows {
    /// Neither side holds a single object.
    NothingToCompare,
    /// There is no filter, and every object either side holds agrees.
    EverythingAgrees,
    /// The filter matched no object's name at all.
    NoMatch,
    /// The filter matched, but only objects the two schemas agree on — and
    /// *Include identical* is off.
    OnlyIdenticalMatched,
}

impl EmptyRows {
    /// What the empty state says.
    pub fn message(self) -> &'static str {
        match self {
            EmptyRows::NothingToCompare => "Neither database holds anything to compare.",
            EmptyRows::EverythingAgrees => "These two schemas match, object for object.",
            EmptyRows::NoMatch => "Nothing matches that filter.",
            EmptyRows::OnlyIdenticalMatched => {
                "Everything matching that filter is identical — turn on Include identical to see it."
            }
        }
    }
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
        // The right side read as if it had come from the left side's database.
        // Two identical databases are identical here and not one object earlier;
        // see [`as_read_from`] for what rides on the difference. Every read below
        // — the entries, the drafts they diff, and the two `CREATE` texts a
        // reader sees side by side — is of these, so the verdict and the text
        // under it stay the same fact.
        let left = &*without_definer(left);
        let right = &*without_definer(right);
        let right = &*as_read_from(right, left, dialect);
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
            // Tables and views carry a dependency rank; every other kind ties
            // at zero and falls through to the name.
            //
            // `Differing` takes the **create** ranks, alongside `OnlyRight` and
            // in the same phase: an `ALTER` and a `CREATE` are both moving
            // toward the right schema, so the right schema's order is what puts
            // each one after whatever it names. Nothing here derives a rank of
            // its own — it is a lookup into `order_tables`' one answer.
            let ranked = matches!(e.kind, CompareKind::Table | CompareKind::View);
            let rank = match e.status {
                ObjectStatus::OnlyRight | ObjectStatus::Differing if ranked => {
                    creates.get(&name).copied().unwrap_or(0)
                }
                ObjectStatus::OnlyLeft if ranked => drops.get(&name).copied().unwrap_or(0),
                _ => 0,
            };
            (ph, kind_rank(e.kind, e.status), rank, e.key())
        });

        // ── name clashes between a table being dropped and one being created ─
        //
        // See [`occupied_names`]. A renamed table is a drop plus a create that
        // both still carry the same constraint and index names, and the phases
        // put every create ahead of every drop — so the create is refused for a
        // name the table two statements below still holds.
        let clashes = clashes_between(&entries, left, right, dialect);
        // Pull each resolvable drop to the front of the table phase. `retain` +
        // `splice` rather than a re-sort: the order everything else is in was
        // just computed and is not up for revision.
        let pulled: BTreeSet<String> = clashes
            .iter()
            .filter(|c| c.resolved)
            .map(|c| c.freed_by.clone())
            .collect();
        if !pulled.is_empty() {
            let moved: Vec<CompareEntry> = entries
                .iter()
                .filter(|e| pulled.contains(&e.key()))
                .cloned()
                .collect();
            entries.retain(|e| !pulled.contains(&e.key()));
            // Ahead of the whole table phase, not merely of the create it
            // collides with: an earlier create in the same phase may hold the
            // other half of a two-way rename, and a drop with nothing pointing
            // at it is safe anywhere after the dependents come off.
            let at = entries
                .iter()
                .position(|e| phase(e.kind, e.status) >= 2)
                .unwrap_or(entries.len());
            entries.splice(at..at, moved);
        }

        // ── namespaces ──────────────────────────────────────────────────────
        //
        // Read off the entries rather than off `DbSchema`, which holds no list
        // of them: an entry's status says which sides it is on, and a namespace
        // matters here exactly when an object lives in it.
        let namespaces = |on: fn(ObjectStatus) -> bool| -> BTreeSet<String> {
            entries
                .iter()
                .filter(|e| on(e.status))
                .filter_map(|e| e.schema.clone())
                .collect()
        };
        let on_left = |st| {
            matches!(
                st,
                ObjectStatus::OnlyLeft | ObjectStatus::Differing | ObjectStatus::Same
            )
        };
        let on_right = |st| {
            matches!(
                st,
                ObjectStatus::OnlyRight | ObjectStatus::Differing | ObjectStatus::Same
            )
        };
        let left_ns = namespaces(on_left);
        let new_namespaces: Vec<String> = namespaces(on_right)
            .into_iter()
            .filter(|ns| !left_ns.contains(ns))
            .collect();

        SchemaComparison {
            entries,
            dialect,
            cycles_create: c1,
            cycles_drop: c2,
            new_namespaces,
            name_clashes: clashes,
        }
    }

    /// Just the entries that would contribute statements, in plan order.
    pub fn differences(&self) -> impl Iterator<Item = &CompareEntry> {
        self.entries.iter().filter(|e| e.status.is_difference())
    }

    /// The per-status tally.
    pub fn counts(&self) -> CompareCounts {
        self.entries.iter().collect()
    }

    /// Do **either** schema's foreign keys form a cycle — the comparison-level
    /// fact, for a header that is about the two schemas rather than about a
    /// plan.
    ///
    /// A plan asks the narrower question and gets a narrower answer: see
    /// [`SchemaComparison::plan`], which pairs each half with the kind of
    /// statement it can actually break.
    pub fn cycles(&self) -> bool {
        self.cycles_create || self.cycles_drop
    }

    /// Does any difference carry a body the caller must refresh from the lazy
    /// source before the SQL is trustworthy? See [`CompareEntry::needs_source`].
    pub fn needs_source(&self) -> bool {
        self.differences().any(CompareEntry::needs_source)
    }

    /// Why the tree has no rows to show — the sentence the empty state owes the
    /// reader, as a decision rather than as a string.
    ///
    /// **Only meaningful when [`SchemaComparison::rows`] came back empty**, and
    /// it takes the same [`RowFilter`] so the two cannot disagree about which
    /// emptiness this is. The view had these three arms inside a
    /// `dyn_container`'s child closure, where nothing could reach them, and one
    /// of the three was wrong: a filter that *matched* — but matched only
    /// objects the two schemas agree on, with *Include identical* off — was
    /// reported as "Nothing matches that filter", which is a claim about the
    /// filter over a result the `show_same` toggle produced.
    pub fn empty_reason(&self, filter: RowFilter<'_>) -> EmptyRows {
        if self.entries.is_empty() {
            return EmptyRows::NothingToCompare;
        }
        let needle = filter.query.trim().to_lowercase();
        if needle.is_empty() {
            // No filter, and no rows: everything either side holds agrees, and
            // agreement is hidden by default.
            return EmptyRows::EverythingAgrees;
        }
        // The name filter alone, exactly as `rows` applies it — the `show_same`
        // half is what this arm is about, so it is deliberately not asked here.
        let matched = self
            .entries
            .iter()
            .any(|e| crate::schema::object_name_matches(&e.label(), &needle));
        if matched {
            EmptyRows::OnlyIdenticalMatched
        } else {
            EmptyRows::NoMatch
        }
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
            let counts: CompareCounts = visible[i..end].iter().copied().collect();
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
            .filter(|e| !e.needs_source() && !e.unplannable())
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
    /// claim work that isn't there. Neither is an entry
    /// [`CompareEntry::needs_source`] flags, whatever `include` says: that is
    /// [`is_planned`]'s rule, restated here so a caller that writes its own
    /// predicate cannot route an untrustworthy body into `emit`. What it *does*
    /// do is record them — see [`SchemaPlan::omitted`].
    pub fn plan(&self, include: impl Fn(&CompareEntry) -> bool) -> SchemaPlan {
        let chosen: Vec<ChangeSet> = self
            .differences()
            .filter(|e| include(e) && !e.needs_source() && !e.unplannable())
            .map(|e| e.changes.clone())
            .collect();
        // **The namespaces this plan needs, ahead of everything that names
        // them.** A namespace is not an object the comparison pairs, so a table
        // ticked into a namespace the left side does not have emitted
        // `CREATE TABLE reporting.sales` against a database with no `reporting`
        // — which PostgreSQL refuses, and with it the transaction the whole
        // migration runs in. `unsupported()` was empty and Apply was live.
        //
        // Only the ones a *set in this plan* actually names: the comparison's
        // list is about the two schemas, and a plan over one ticked table has no
        // business creating a namespace for an object the user left out.
        let mut sets: Vec<ChangeSet> = self
            .new_namespaces
            .iter()
            .filter(|ns| {
                chosen
                    .iter()
                    .any(|s| s.schema.as_deref() == Some(ns.as_str()))
            })
            .map(|ns| {
                ddl::single(
                    ns,
                    None,
                    self.dialect,
                    Change::CreateSchema {
                        name: ns.clone(),
                        owner: None,
                    },
                )
            })
            .collect();
        sets.extend(chosen);
        // **What this plan cannot carry, in the plan itself.** These entries
        // have no tick-box, so nothing the user does adds them — and until this
        // field existed nothing said so past the tree either: the footer counted
        // the objects that *were* included, the preview's subject repeated that
        // count, `unsupported()` was empty so the withheld block stayed hidden,
        // and Apply reported "Applied N statements to 1 object" over a
        // three-difference comparison. `SchemaComparison::needs_source`, written
        // to disclose exactly this, had no production caller at all.
        let omitted: Vec<String> = self
            .differences()
            .filter(|e| e.needs_source() || e.unplannable())
            .map(CompareEntry::omission_note)
            .collect();
        // **A cycle breaks a plan that creates a table and one that drops
        // one, and they are not the same cycle.** A `CREATE` carries an inline
        // foreign key with nothing to point at yet; a `DROP` is refused while
        // anything still references the table. So the create-order tangle is
        // asked of a plan that creates, the drop-order tangle of one that
        // drops — and a plan doing neither is unaffected however tangled either
        // schema is.
        //
        // The old spelling got both halves wrong at once: `self.cycles` was
        // `c1 || c2`, so a tangle among the *left* schema's tables raised the
        // *create*-order warning; and the whole thing was gated on
        // `creates_a_table`, under a comment asserting "a plan of pure alters
        // **or drops** is unaffected" — which is false of drops, and left
        // `DROP TABLE b; DROP TABLE a;` to be refused at statement 1 with no
        // warning at all.
        //
        // Each half still errs toward warning: `dump::order_tables` reports
        // *that* there is a cycle, not which edge, so whether the selected
        // tables are the ones in it can't be answered from a set. Over-reporting
        // costs a sentence in the risk block, which is the cheap side.
        let has = |f: fn(&Change) -> bool| sets.iter().flat_map(|s| s.changes.iter()).any(f);
        let creates_a_table = has(|c| matches!(c, Change::CreateTable(_)));
        let drops_a_table = has(|c| matches!(c, Change::DropTable));
        // **Only the clashes this plan's own ticks produce.** A comparison-level
        // clash between two objects the user left out is not this plan's
        // problem, and saying so above Apply would be a warning about
        // statements that are not in the script.
        let chosen_keys: BTreeSet<String> = self
            .differences()
            .filter(|e| include(e) && !e.needs_source() && !e.unplannable())
            .map(CompareEntry::key)
            .collect();
        let clashes: Vec<String> = self
            .name_clashes
            .iter()
            .filter(|c| !c.resolved)
            .filter(|c| chosen_keys.contains(&c.freed_by) && chosen_keys.contains(&c.claimed_by))
            .map(NameClash::note)
            .collect();
        SchemaPlan {
            sets,
            dialect: self.dialect,
            cycles: (self.cycles_create && creates_a_table) || (self.cycles_drop && drops_a_table),
            omitted,
            clashes,
        }
    }
}

/// Is this object in the plan a selection describes? **The one predicate**, so
/// the footer's count, the button's enabled state and the statements that
/// actually get built cannot answer differently.
///
/// It is a function and not a closure written at each site because the two have
/// already disagreed once: the footer counted `selected.contains(key)` while the
/// builder also excluded a blocked body, which put a confident "1 object" over a
/// button that built an empty plan and returned. Counting and building have to
/// ask the same question, and the cheapest way to guarantee that is for there to
/// be only one.
///
/// A blocked body is excluded here rather than only where the tick is drawn: it
/// is the tick-box's absence that stops one being selected, and a key that
/// arrived in the set by any other route (a comparison replaced under a stale
/// selection, a future "invert") must still not reach [`SchemaPlan::emit`].
/// [`SchemaComparison::plan`] enforces the same exclusion on its own account, so
/// the two cannot drift apart either.
///
/// **In this crate rather than in the view that calls it.** It was the single
/// predicate deciding which objects reach an irreversible `Db::run_ddl`, written
/// inside a 1,135-line Floem module with no test module at all, while
/// `SchemaComparison::plan` already took it as a parameter — so the decision was
/// untestable for no reason but where it sat.
pub fn is_planned(e: &CompareEntry, selected: &HashSet<String>) -> bool {
    selected.contains(&e.key()) && !e.needs_source() && !e.unplannable()
}

/// What the compare footer says about the tick set — the sentence that stands
/// next to the button the plan is built from.
///
/// Pure and here rather than inside the footer's `label` closure, for
/// [`SchemaPlan::subject`]'s reason: the two are the same sentence about the
/// same number, one before the preview and one inside it, and they were two
/// hand-rolled plurals a hundred lines apart, beside `preview_title` — which
/// was extracted precisely so a modal's words could be tested.
pub fn selection_note(planned: usize) -> String {
    match planned {
        0 => "Nothing selected.".to_string(),
        n => format!(
            "{n} {} selected",
            crate::text::plural(n, "object", "objects")
        ),
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
    /// Differences the comparison holds that **no** plan built from it can
    /// carry — one line each, naming the object and why.
    ///
    /// Distinct from [`SchemaPlan::unsupported`] in what it asks of the reader,
    /// which is why it is a second list rather than more lines in that one.
    /// `unsupported` means *this* plan writes less than its own change list
    /// promises, so applying it would apply half an edit and Apply is refused
    /// until the offending tick is cleared. This means a difference is not in
    /// the plan at all: what is here is complete, and there is no tick to clear
    /// — the objects have no tick-box. So it discloses and does not refuse.
    pub omitted: Vec<String>,
    /// One line per name a table this plan drops still holds and a table it
    /// creates needs, where no order resolves it — see [`NameClash`]. Reported
    /// through [`SchemaPlan::destructive`] for [`SchemaPlan::cycles`]' reason.
    pub clashes: Vec<String>,
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
    ///
    /// **The `CREATE SCHEMA` sets do not count.** `plan()` prepends one per
    /// namespace a chosen set names and the left side lacks, so that
    /// `CREATE TABLE reporting.sales` has a `reporting` to land in — but a
    /// namespace is not an object the comparison pairs, has no tick-box, and
    /// was never in the number the user pressed. Counting it made the footer
    /// say *"1 object selected"* and the preview behind it *"2 objects in
    /// shop"*, with the success line reading "Applied 2 statements to 2
    /// objects", and the gap grew with the number of new namespaces.
    ///
    /// `is_planned`'s doc states the rule this belongs to: the footer's count,
    /// the button's enabled state and the statements that actually get built
    /// cannot answer differently.
    pub fn len(&self) -> usize {
        self.sets
            .iter()
            .filter(|s| !s.changes.iter().all(ddl::is_namespace_change))
            .count()
    }

    /// What the preview modal is **about** — its title's second half, and the
    /// noun its success line reads "Applied N statements to …".
    ///
    /// A count rather than a name, because a plan has no single object to name.
    /// Here rather than at the call site beside `preview_title`, which was
    /// extracted precisely so a modal's words could be tested: this string was
    /// hand-rolled inside a click closure with its own `if n == 1` beside it.
    pub fn subject(&self) -> String {
        let n = self.len();
        format!("{n} {}", crate::text::plural(n, "object", "objects"))
    }

    /// [`SchemaPlan::subject`] with the database it lands in named beside it —
    /// `2 objects in shop`.
    ///
    /// **The one surface authorising this has to say which database it changes.**
    /// The preview modal's title qualifies its subject with the database only
    /// when the subject is an *object name* (`DdlPreview::qualified`), and a
    /// plan's subject is a count, so `shop.2 objects` is nonsense and the title
    /// fell back to naming the connection alone: *"Apply changes to My MariaDB ·
    /// 2 objects"*, on a feature whose whole subject is **two databases on one
    /// connection**, with the comparison deliberately closed behind it. Beside
    /// rather than in front, because it is not a qualifier — it is the other
    /// half of the address.
    ///
    /// It travels into the success line too (`Applied N statements to …`), which
    /// is the other place that had no way to say where the statements went.
    pub fn subject_in(&self, database: &str) -> String {
        format!("{} in {database}", self.subject())
    }

    /// Every statement, in the order they must run.
    pub fn emit(&self) -> Vec<String> {
        self.sets.iter().flat_map(ChangeSet::emit).collect()
    }

    /// The script as it may **leave** a preview — for the clipboard and for the
    /// editor tab, split on `;` by the app's own splitter.
    ///
    /// **The only script this type produces.** There used to be a third,
    /// `script()`, which no production caller ever reached: `preview_of_plan`
    /// takes `emit()` and `export_script()`, and a workspace grep found only
    /// this module's own tests. It was nonetheless the surface six of them read
    /// a plan through, so the ordering the suite pinned was the ordering of a
    /// string nobody was ever shown — and it was the one builder of the three
    /// that joined statements without going through [`ddl::client_script`],
    /// which exists precisely because a MySQL routine's `CREATE` is
    /// deliberately unterminated and two of those run together when joined for
    /// a reader. A third builder skipping that fix, kept alive by tests, is the
    /// next caller's trap. Those six tests now assert what Copy and Open in
    /// editor produce.
    ///
    /// **A compare plan carries no MySQL body at all**, and this doc used to
    /// say the opposite — that it was "the most likely thing to carry several
    /// of them". Since [`CompareEntry::needs_source`], `plan` excludes every
    /// MySQL routine, trigger and event that is being created or redefined and
    /// discloses them through [`SchemaPlan::omitted`] instead, because the body
    /// `information_schema` hands back has had its escapes resolved. Only a
    /// `DROP` gets through, and a `DROP` has no body. So
    /// [`ddl::client_script`]'s `DELIMITER` branch is **unreachable from here**:
    /// it is gated on `MySql`, and no MySQL statement reaching this function
    /// carries an internal `;`.
    ///
    /// Its other rule — terminate every statement — is reachable in principle
    /// and a no-op in practice: on the two engines whose bodies *do* reach a
    /// plan, the emitters already end each statement in `;`. The call stays
    /// because this is not the place that gets to know that. `client_script` is
    /// the one function that owns "what a client splitting on `;` needs", and
    /// the day a re-read MySQL body reaches a plan — the fix `needs_source`
    /// defers rather than forecloses — the wrapping has to be here already.
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
        out.extend(self.clashes.iter().cloned());
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
///
/// **Created and altered tables share one phase**, and that is the whole of the
/// dependency ordering between them. Splitting them put every `CREATE TABLE`
/// ahead of every `ALTER`, so `CREATE TABLE child (… REFERENCES parent(code))`
/// was emitted before the `ALTER TABLE parent ADD code` it names — refused by
/// the server, with the statements before it already applied and no DDL
/// rollback on MySQL. Swapping the two phases only moves the failure to the
/// other shape (an `ALTER` adding a foreign key onto a table the plan is about
/// to create). Neither order is right in general, and there is no need to pick
/// one: [`fk_rank`] over the **right** schema already answers both, because
/// that is the schema both the create and the alter are moving toward.
fn phase(kind: CompareKind, status: ObjectStatus) -> u8 {
    match status {
        // Never planned. Sorted past everything so the difference entries keep
        // their order regardless of how many untouched objects sit between them.
        ObjectStatus::Same => 9,
        ObjectStatus::OnlyLeft if kind.depends_on_tables() => 0,
        ObjectStatus::OnlyRight | ObjectStatus::Differing if kind.is_type() => 1,
        ObjectStatus::OnlyRight | ObjectStatus::Differing if kind == CompareKind::Table => 2,
        ObjectStatus::OnlyLeft if kind == CompareKind::Table => 3,
        ObjectStatus::OnlyRight | ObjectStatus::Differing => 4,
        ObjectStatus::OnlyLeft => 5,
    }
}

/// The identifiers a table **occupies beyond itself** — the ones another table
/// in the same scope cannot also use while this one exists.
///
/// A rename is the single most ordinary difference a schema-compare tool is
/// opened for, and the pair it produces is always a drop plus a create: rename
/// `orders` to `orders_old` on the left and the comparison yields `OnlyLeft
/// table:orders_old` and `OnlyRight table:orders`. Neither `RENAME TABLE` nor
/// `ALTER TABLE … RENAME TO` rewrites the names *inside* a table, so both sides
/// still carry `fk_orders_customer` (or `orders_ibfk_1`, or `orders_pkey`) — and
/// the plan emitted the `CREATE` before the `DROP`. MySQL and MariaDB scope a
/// foreign-key constraint name to the **database**, so the create is refused
/// with `ERROR 1826`; PostgreSQL scopes an index name to the **namespace**, so
/// `orders_pkey` already exists and takes the transaction with it. Neither
/// MySQL nor MariaDB rolls DDL back, so the user is left half-migrated under a
/// message naming a constraint they never asked about.
///
/// **Per engine, because the scope is per engine** — a capability, not a
/// dialect test dressed up as one:
/// - MySQL/MariaDB key foreign-key constraint names to the database and index
///   names to the table, so only the foreign keys are here.
/// - PostgreSQL puts indexes and table-level constraints in the **same**
///   namespace as tables, so both are.
/// - SQLite keys index names to the database and names no foreign key at all.
fn occupied_names(t: &TableInfo, dialect: SqlDialect) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let qualify = |n: &str| display_name(t.schema.as_deref(), n);
    match dialect {
        SqlDialect::MySql => {
            out.extend(t.foreign_keys.iter().map(|fk| qualify(&fk.name)));
        }
        SqlDialect::Postgres => {
            out.extend(t.foreign_keys.iter().map(|fk| qualify(&fk.name)));
            out.extend(t.indexes.iter().map(|ix| qualify(&ix.name)));
            out.extend(
                t.check_constraints
                    .iter()
                    .filter(|c| !c.column_level)
                    .map(|c| qualify(&c.name)),
            );
        }
        SqlDialect::Sqlite => {
            out.extend(t.indexes.iter().map(|ix| qualify(&ix.name)));
        }
    }
    out.remove(&String::new());
    out
}

/// A name a table being dropped still holds and a table being created needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameClash {
    /// [`CompareEntry::key`] of the `OnlyLeft` table that still holds the name.
    pub freed_by: String,
    /// [`CompareEntry::key`] of the `OnlyRight` table that needs it.
    pub claimed_by: String,
    /// The identifier itself, qualified the way the engine scopes it.
    pub name: String,
    /// True when the drop was pulled ahead of the create, which settles it.
    /// False when something on the left still references the dropped table, so
    /// no order works and the clash is disclosed instead.
    pub resolved: bool,
}

impl NameClash {
    /// The sentence above Apply: which name, which two objects, and why no
    /// order fixes it. Named for the objects rather than for the statement,
    /// like every other line in [`SchemaPlan::destructive`].
    pub fn note(&self) -> String {
        let strip = |k: &str| k.split_once(':').map_or(k, |(_, n)| n).to_string();
        format!(
            "{} still holds {}, which creating {} needs — and {} cannot be dropped first \
             because another table references it. One statement will be refused, and \
             neither MySQL nor MariaDB rolls DDL back.",
            strip(&self.freed_by),
            self.name,
            strip(&self.claimed_by),
            strip(&self.freed_by),
        )
    }
}

/// Every clash between a table this comparison would drop and one it would
/// create, in create order.
///
/// Only `OnlyLeft` against `OnlyRight`: a `Differing` table keeps its identity,
/// so its constraint names are the *same table's* and the `ALTER` path already
/// drops and re-adds each one in the right order.
fn clashes_between(
    entries: &[CompareEntry],
    left: &DbSchema,
    right: &DbSchema,
    dialect: SqlDialect,
) -> Vec<NameClash> {
    let table_of = |src: &[TableInfo], e: &CompareEntry| -> Option<TableInfo> {
        src.iter()
            .find(|t| !t.is_view && t.name == e.name && t.schema.as_deref() == e.schema.as_deref())
            .cloned()
    };
    let is_table = |e: &&CompareEntry| e.kind == CompareKind::Table;
    let drops: Vec<(&CompareEntry, TableInfo)> = entries
        .iter()
        .filter(is_table)
        .filter(|e| e.status == ObjectStatus::OnlyLeft)
        .filter_map(|e| table_of(&left.tables, e).map(|t| (e, t)))
        .collect();
    if drops.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for create in entries
        .iter()
        .filter(is_table)
        .filter(|e| e.status == ObjectStatus::OnlyRight)
    {
        let Some(made) = table_of(&right.tables, create) else {
            continue;
        };
        let claimed = occupied_names(&made, dialect);
        for (drop, held) in &drops {
            let Some(name) = occupied_names(held, dialect)
                .intersection(&claimed)
                .next()
                .cloned()
            else {
                continue;
            };
            out.push(NameClash {
                freed_by: drop.key(),
                claimed_by: create.key(),
                name,
                resolved: nothing_else_references(&held.name, held.schema.as_deref(), &left.tables),
            });
        }
    }
    out
}

/// Is a table safe to drop **early** — before the creates that need its names?
///
/// Only when nothing else on the left side points at it. A referencing table
/// has to be dropped (or have its foreign key altered away) first, and both of
/// those sit at or after the phase the pull would move this in front of; moving
/// the drop past them trades one refused statement for another. Where the
/// answer is no, the clash is disclosed rather than reordered — the posture
/// [`SchemaPlan::cycles`] already takes for a statement the server will refuse.
fn nothing_else_references(table: &str, schema: Option<&str>, left: &[TableInfo]) -> bool {
    let me = display_name(schema, table);
    !left.iter().any(|t| {
        display_name(t.schema.as_deref(), &t.name) != me
            && t.foreign_keys
                .iter()
                .any(|fk| display_name(fk.ref_schema.as_deref(), &fk.ref_table) == me)
    })
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

/// Each table's **and view's** position in dependency order, by qualified name,
/// plus whether a cycle had to be broken. `reverse` is for the drop phase: a
/// referencing table has to go before the table it references.
///
/// [`crate::dump::order_tables`] is the sort — a second topological order over
/// the same edges is a second answer to one question.
///
/// **Views are in it**, which they were not: `chosen` filtered them out, so the
/// half of `order_tables` written for them never ran and two created views fell
/// through to their names. `CREATE VIEW a_totals AS … FROM z_detail` on a
/// target where `z_detail` does not exist yet is the ERROR 1146 that half
/// exists to prevent, and `dump.rs` says so at the code that prevents it.
fn fk_rank(
    tables: &[TableInfo],
    dialect: SqlDialect,
    reverse: bool,
) -> (BTreeMap<String, usize>, bool) {
    let chosen: Vec<String> = tables
        .iter()
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

/// Read the **right** side as if it had been read from the left side's
/// database, so that comparing two databases compares the schema in them.
///
/// A comparison is between two *databases*, and on MySQL two of the fields the
/// differ reads carry the name of the database they were read from rather than
/// anything about the object:
///
/// - a foreign key's [`ForeignKeyInfo::ref_schema`] is `REFERENCED_TABLE_SCHEMA`,
///   which on that engine *is* the database ([`ddl::ref_schema_is_database`]);
/// - a view's [`TableInfo::view_definition`] is the server's rewritten body,
///   qualified throughout ([`ddl::view_definition_is_qualified`]).
///
/// Left alone, every table holding a key and every view came out `Differing`
/// between two identical databases, and — the half that costs data — the plan
/// runs against the **left** database while naming the **right** one:
/// `ADD CONSTRAINT … REFERENCES <right>.parent` puts the left database's
/// referential integrity in another database, and `CREATE OR REPLACE VIEW`
/// re-points the left view at the right database's rows. Neither is named
/// anywhere in the preview; `destructive()` is empty for both.
///
/// **Re-addressed rather than stripped**, and the difference matters twice.
/// Stripping loses the distinction the fix has to keep — MySQL allows a key into
/// another database, one of those *is* a difference, and
/// [`ddl::fks_equal`]'s rule that an absent namespace matches an explicit one
/// would have made a cross-database key compare equal to a local one. And a
/// re-addressed right side is the side the plan is *built from*, so the
/// statement that comes out already names the database it will run against
/// instead of leaving the server to guess.
///
/// The left side is never touched: it is the target, and it is already in its
/// own terms.
///
/// [`ViewOptions::definer`] is cleared on both sides, and that is a different
/// judgement — see [`without_definer`].
///
/// Borrows when there is nothing to re-address, which is every PostgreSQL and
/// SQLite comparison, a pair whose two databases are named the same, and any
/// side that did not record where it came from.
fn as_read_from<'a>(
    right: &'a DbSchema,
    left: &DbSchema,
    dialect: SqlDialect,
) -> Cow<'a, DbSchema> {
    let from = right.database.as_deref().filter(|d| !d.is_empty());
    let to = left.database.as_deref().filter(|d| !d.is_empty());
    let (fk, body) = (
        ddl::ref_schema_is_database(dialect),
        ddl::view_definition_is_qualified(dialect),
    );
    let Some(from) = from.filter(|f| Some(*f) != to && (fk || body)) else {
        return Cow::Borrowed(right);
    };
    let mut out = right.clone();
    for t in &mut out.tables {
        if fk {
            for k in &mut t.foreign_keys {
                if k.ref_schema.as_deref() == Some(from) {
                    k.ref_schema = to.map(str::to_string);
                }
            }
        }
        if body
            && t.is_view
            && let Some(def) = t.view_definition.as_mut()
        {
            *def = requalify(def, from, to, dialect);
        }
    }
    Cow::Owned(out)
}

/// Clear every `DEFINER`, on both sides, before the two are compared.
///
/// It is a **server** account, so it does not compare across two servers at all
/// — the same reading [`CompareEntry::uncertain`] takes of an index the model
/// only partly read. And the consequence of comparing it is not a spurious row:
/// the `CREATE OR REPLACE DEFINER = <the other server's account> VIEW …` that
/// follows is *accepted* by the left server, after which every `SELECT` on the
/// view is `ERROR 1449 … does not exist` and the view is permanently unusable
/// (measured on MariaDB 10.11.14).
///
/// The cost of the other side of the trade — a deliberate definer change is not
/// migrated, and a view replaced for some other reason takes the running account
/// — is a feature not offered rather than an object destroyed.
///
/// **All four of them**, not the view's alone. A trigger, a stored routine and
/// an event each carry a definer, each restated by its emitter, and each
/// compared by a whole-struct equality — so a two-server comparison reported
/// every one of them Differing although the bodies were byte-identical, and the
/// plan (ticked by default, built for the **left** database) carried the right
/// server's account into it. Both outcomes are bad and one destroys: without
/// `SUPER`/`SET_USER_ID` the `CREATE` is refused `ERROR 1227` and, MySQL DDL not
/// being transactional, the `DROP TRIGGER` above it has already run; with it,
/// every `INSERT` into the table then fails `ERROR 1449 … does not exist`.
/// `ReplaceTrigger`'s own risk line states the first half and nothing asked.
///
/// `definer` is `None` on every engine but MySQL's, so this is a no-op elsewhere
/// and needs no predicate of its own; it borrows when nothing carries one.
fn without_definer(side: &DbSchema) -> Cow<'_, DbSchema> {
    let any_view = side
        .tables
        .iter()
        .any(|t| t.view_options.as_ref().is_some_and(|o| o.definer.is_some()));
    let any_trigger = side
        .tables
        .iter()
        .any(|t| t.triggers.iter().any(|g| g.definer.is_some()));
    let any_routine = side.routines.iter().any(|r| r.definer.is_some());
    let any_event = side.events.iter().any(|e| e.definer.is_some());
    if !(any_view || any_trigger || any_routine || any_event) {
        return Cow::Borrowed(side);
    }
    let mut out = side.clone();
    for t in &mut out.tables {
        if let Some(o) = t.view_options.as_mut() {
            o.definer = None;
        }
        for g in &mut t.triggers {
            g.definer = None;
        }
    }
    for r in &mut out.routines {
        if r.definer.is_some() {
            std::sync::Arc::make_mut(r).definer = None;
        }
    }
    for e in &mut out.events {
        if e.definer.is_some() {
            std::sync::Arc::make_mut(e).definer = None;
        }
    }
    Cow::Owned(out)
}

/// Rewrite every `<from>.` qualifier in `sql` to `<to>.`, dropping it entirely
/// when there is no `to`.
///
/// Three things it must not do, each of which is a way to corrupt a body while
/// making it *look* normalised:
///
/// - **Not inside a string literal or a comment.** The one boundary lexer
///   answers that, through [`crate::intel::code_word_hits`] — the same call
///   [`crate::dump::order_tables`] makes for the same reason.
/// - **Not a column of the same name.** `` `t`.`shop` `` is a column; a
///   qualifier is the hit with no `.` in front of it and a `.` behind it.
/// - **Not a bare word that happens to match.** A hit must be the whole
///   identifier, quoted or not, which is what `code_word_hits`' boundary rule
///   gives — and for the quoted form the quote bytes are checked here, since the
///   needle is the name without them.
///
/// Leaves `` `db`.`t`.`col` `` addressed at `t`.`col`: only the leading
/// qualifier is the address, and the two behind it are the object.
fn requalify(sql: &str, from: &str, to: Option<&str>, dialect: SqlDialect) -> String {
    let b = sql.as_bytes();
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    for (lo, hi) in crate::intel::code_word_hits(sql, from, dialect) {
        // The quoted spelling takes its quote bytes with it; the bare one is the
        // hit as found. Asked of the one per-byte quote table, so SQLite's three
        // spellings — including `[x]`, which does not close with the byte it
        // opened with — all answer.
        let quoted = lo
            .checked_sub(1)
            .and_then(|i| crate::intel::ident_quote(dialect, b[i]).map(|(close, _)| (i, close)))
            .filter(|(_, close)| b.get(hi) == Some(close));
        let (lo, hi) = match quoted {
            Some((open, _)) => (open, hi + 1),
            None => (lo, hi),
        };
        // A `.` in front means this is the qualified half, not the qualifier.
        if lo > 0 && b[lo - 1] == b'.' {
            continue;
        }
        // A `.` behind is what makes it a qualifier at all.
        if b.get(hi) != Some(&b'.') {
            continue;
        }
        cuts.push((lo, hi + 1));
    }
    if cuts.is_empty() {
        return sql.to_string();
    }
    // Through the one identifier quoter, which on MySQL is the backtick form the
    // server wrote the rest of the body in.
    let replacement = to.map(|t| format!("{}.", crate::export::ident_sql(t, dialect)));
    let mut out = String::with_capacity(sql.len());
    let mut at = 0usize;
    for (lo, hi) in cuts {
        out.push_str(&sql[at..lo]);
        if let Some(r) = replacement.as_deref() {
            out.push_str(r);
        }
        at = hi;
    }
    out.push_str(&sql[at..]);
    out
}

// ── per-kind entries ────────────────────────────────────────────────────────

/// An empty set addressed at an object, for the one case a builder can't answer
/// (a view whose model says it isn't one — which is what a definition the
/// connecting role cannot read looks like).
///
/// **On a two-sided pair it reads as [`ObjectStatus::Same`]** — `status_of`
/// asks the change set there, and "nothing here can state a change" is the
/// truthful answer. On a *one*-sided pair it does not: `status_of` reads only
/// which sides are present, so the entry would keep `OnlyLeft` / `OnlyRight`
/// and be a difference with no statement behind it. That is what
/// [`CompareEntry::unplannable`] is for; this function's doc used to claim the
/// `Same` reading for both.
///
/// **And no call site reaches it today.** [`table_entry`] guards each of its
/// arms on `is_view`, and [`ViewDraft::from_table`] returns `None` only for a
/// table that is *not* a view — so the two conditions cannot both hold. It is
/// kept as the answer for a builder that one day can't draft an object it was
/// handed, which is why `unplannable` exists on the reading side rather than an
/// `unreachable!()` here: a set that says nothing is a safe value, and an entry
/// that is a difference with nothing behind it is not something a plan should
/// count either way.
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

    /// **A `Same` row can still have two different texts under it.** On SQLite
    /// `create_ddl` returns `sqlite_master.sql` verbatim — a deliberate fidelity
    /// decision — while `status` comes from the structured differ over the
    /// model, so two tables the differ calls identical can carry different
    /// whitespace, quoting or clause order. Every one of those draws as a
    /// red-and-green change under a row labelled identical, which is a
    /// contradiction the reader cannot resolve.
    #[test]
    fn a_same_entry_can_still_hold_two_different_texts_on_sqlite() {
        let side = |sql: &str| TableInfo {
            name: "city".to_string(),
            columns: vec![col("id", "int")],
            create_sql: Some(sql.to_string()),
            ..Default::default()
        };
        let c = SchemaComparison::of(
            &schema_of(vec![side("CREATE TABLE city (id int)")]),
            &schema_of(vec![side(
                "CREATE TABLE \"city\" (
  id int
)",
            )]),
            SqlDialect::Sqlite,
        );
        let e = find(&c, "table:city");
        assert_eq!(e.status, ObjectStatus::Same, "the model says they agree");
        assert_ne!(e.left_ddl, e.right_ddl, "and the stored text does not");
        assert!(e.text_differs_though_same());
    }

    /// A table with one named foreign key, for the rename cases below.
    fn child(name: &str, fk: &str, parent: &str) -> TableInfo {
        TableInfo {
            foreign_keys: vec![ForeignKeyInfo {
                name: fk.to_string(),
                columns: vec!["customer_id".to_string()],
                ref_table: parent.to_string(),
                ref_columns: vec!["id".to_string()],
                ..Default::default()
            }],
            ..table(name, &[("id", "int"), ("customer_id", "int")])
        }
    }

    fn at(sql: &str, needle: &str) -> usize {
        sql.find(needle)
            .unwrap_or_else(|| panic!("no {needle} in:\n{sql}"))
    }

    /// **A rename is a drop plus a create, and they share their inside names.**
    /// `RENAME TABLE orders TO orders_old` leaves `fk_orders_customer` on
    /// `orders_old`; the right side still has it on `orders`. The phases put
    /// every `CREATE TABLE` ahead of every `DROP TABLE`, so the create was
    /// emitted while the dropped table still held the name — `ERROR 1826` on
    /// MySQL, `relation … already exists` on PostgreSQL, and no DDL rollback to
    /// undo the statements before it.
    ///
    /// Asserted over the **emitted script**, which is the composition `phase` →
    /// `plan` → `emit`; the existing ordering tests all stop at `phase`, which
    /// is why the order they pin is the one that fails here.
    #[test]
    fn a_renamed_tables_drop_runs_before_the_create_that_needs_its_name() {
        let c = mysql(
            schema_of(vec![
                table("customers", &[("id", "int")]),
                child("orders_old", "fk_orders_customer", "customers"),
            ]),
            schema_of(vec![
                table("customers", &[("id", "int")]),
                child("orders", "fk_orders_customer", "customers"),
            ]),
        );
        let sql = c.plan(|_| true).emit().join("\n");
        assert!(
            at(&sql, "DROP TABLE") < at(&sql, "CREATE TABLE"),
            "the create is still first:\n{sql}"
        );
        // Settled by the order, so nothing is disclosed above Apply.
        assert!(c.plan(|_| true).clashes.is_empty());
    }

    /// The same shape on PostgreSQL, where it needs no foreign key at all: an
    /// index name is unique per namespace and a rename does not touch
    /// `orders_pkey`.
    #[test]
    fn a_postgres_rename_clashes_on_the_index_name_too() {
        let indexed = |name: &str| TableInfo {
            indexes: vec![crate::schema::IndexInfo {
                name: "orders_pkey".to_string(),
                unique: true,
                ..Default::default()
            }],
            ..table(name, &[("id", "int")])
        };
        let c = SchemaComparison::of(
            &schema_of(vec![indexed("orders_old")]),
            &schema_of(vec![indexed("orders")]),
            SqlDialect::Postgres,
        );
        let sql = c.plan(|_| true).emit().join("\n");
        assert!(
            at(&sql, "DROP TABLE") < at(&sql, "CREATE TABLE"),
            "the create is still first:\n{sql}"
        );
    }

    /// **And the pull is refused where it would trade one refusal for another.**
    /// If something on the left still references the table being dropped, that
    /// referencing table has to come off first — which is at or after the phase
    /// the pull moves in front of. So the order is left alone and the clash is
    /// said out loud, which is the call `cycles` already makes.
    #[test]
    fn a_referenced_table_is_not_pulled_ahead_but_is_disclosed() {
        let plan = referenced_rename().plan(|_| true);
        let sql = plan.emit().join("\n");
        assert!(
            at(&sql, "CREATE TABLE") < at(&sql, "DROP TABLE `orders_old`"),
            "the drop was pulled ahead of a table that still points at it:\n{sql}"
        );
        let said = plan.destructive().join(" ");
        assert!(said.contains("fk_orders_customer"), "{said}");
        assert!(said.contains("orders_old"), "{said}");
    }

    /// Left renames `orders` to `orders_old` and `shipments` still points at
    /// the renamed table, so the drop cannot be pulled ahead of the create that
    /// needs its constraint name.
    fn referenced_rename() -> SchemaComparison {
        mysql(
            schema_of(vec![
                table("customers", &[("id", "int")]),
                child("orders_old", "fk_orders_customer", "customers"),
                child("shipments", "fk_ship_orders", "orders_old"),
            ]),
            schema_of(vec![
                table("customers", &[("id", "int")]),
                child("orders", "fk_orders_customer", "customers"),
            ]),
        )
    }

    /// A clash between two objects the user did not tick is not this plan's
    /// problem, and a warning about statements that are not in the script is
    /// noise the reader cannot act on.
    #[test]
    fn a_clash_outside_the_ticked_set_is_not_reported() {
        let c = referenced_rename();
        assert!(!c.plan(|_| true).clashes.is_empty(), "the fixture clashes");
        assert!(c.plan(|e| e.key() != "table:orders").clashes.is_empty());
        assert!(c.plan(|e| e.key() != "table:orders_old").clashes.is_empty());
    }

    /// The per-engine scope, which is what decides whether two tables can hold
    /// one name at all: MySQL keys an index name to its table and a foreign key
    /// to the database, PostgreSQL puts both in the namespace, SQLite names no
    /// foreign key.
    #[test]
    fn the_names_a_table_occupies_are_the_ones_its_engine_scopes_outside_it() {
        let t = TableInfo {
            indexes: vec![crate::schema::IndexInfo {
                name: "ix_orders_customer".to_string(),
                ..Default::default()
            }],
            ..child("orders", "fk_orders_customer", "customers")
        };
        let names = |d| occupied_names(&t, d).into_iter().collect::<Vec<_>>();
        assert_eq!(names(SqlDialect::MySql), vec!["fk_orders_customer"]);
        assert_eq!(
            names(SqlDialect::Postgres),
            vec!["fk_orders_customer", "ix_orders_customer"]
        );
        assert_eq!(names(SqlDialect::Sqlite), vec!["ix_orders_customer"]);
    }

    /// It is only ever about a `Same` row — a differing one's pane is a diff and
    /// needs no excuse — and it stays quiet when the two texts really are equal.
    #[test]
    fn the_text_note_is_quiet_where_there_is_nothing_to_explain() {
        let same = || schema_of(vec![table("city", &[("id", "int")])]);
        let c = mysql(same(), same());
        assert!(!find(&c, "table:city").text_differs_though_same());

        let d = mysql(
            schema_of(vec![table("city", &[("id", "int")])]),
            schema_of(vec![table("city", &[("id", "int"), ("name", "text")])]),
        );
        let e = find(&d, "table:city");
        assert_eq!(e.status, ObjectStatus::Differing);
        assert!(
            !e.text_differs_though_same(),
            "a differing row's pane is a diff and needs no explanation"
        );
    }

    // ── a difference with no statement behind it ─────────────────────────────

    /// **A difference with no statement behind it must not be counted as one
    /// the plan carries.** `status_of` reads the change set only on a *two*-
    /// sided pair, so a one-sided entry keeps `OnlyLeft` / `OnlyRight` however
    /// empty its set is — the row is a difference, the plan counts it as an
    /// object, and `emit()` writes nothing for it: "Applied 0 statements to 1
    /// object". `empty_set`'s own doc claimed the `Same` reading for both arms,
    /// which is where the belief that this could not happen came from.
    ///
    /// **Built by hand, because no schema can currently produce it.**
    /// `table_entry` guards its arms on `is_view` and `ViewDraft::from_table`
    /// refuses only a non-view, so the two conditions `empty_set` sits behind
    /// cannot both hold — the review found it unreachable and it still is. The
    /// property is about the reading, not about that one builder, so it is
    /// stated over the value.
    #[test]
    fn a_difference_with_no_statement_is_disclosed_rather_than_counted() {
        let orphan = |status: ObjectStatus| CompareEntry {
            kind: CompareKind::View,
            schema: None,
            name: "secret_v".to_string(),
            table: None,
            signature: None,
            status,
            changes: empty_set("secret_v", None, SqlDialect::MySql),
            uncertain: false,
            left_ddl: String::new(),
            right_ddl: String::new(),
        };

        let only_right = orphan(ObjectStatus::OnlyRight);
        assert!(only_right.changes.is_empty());
        assert!(only_right.unplannable(), "a difference it cannot express");
        assert!(
            !is_planned(&only_right, &HashSet::from([only_right.key()])),
            "and no selection can put it in a plan"
        );

        // `Same` is not a difference, so there is nothing to carry and nothing
        // to disclose — the arm `empty_set`'s doc was actually describing.
        let agreed = orphan(ObjectStatus::Same);
        assert!(!agreed.unplannable());
    }

    /// And through a comparison: such an entry is not an object the preview
    /// counts, and the plan says what it left behind.
    #[test]
    fn an_unplannable_entry_reaches_the_plans_omitted_list() {
        let c = SchemaComparison {
            entries: vec![CompareEntry {
                kind: CompareKind::View,
                schema: None,
                name: "secret_v".to_string(),
                table: None,
                signature: None,
                status: ObjectStatus::OnlyRight,
                changes: empty_set("secret_v", None, SqlDialect::MySql),
                uncertain: false,
                left_ddl: String::new(),
                right_ddl: String::new(),
            }],
            dialect: SqlDialect::MySql,
            cycles_create: false,
            cycles_drop: false,
            new_namespaces: Vec::new(),
            name_clashes: Vec::new(),
        };
        let plan = c.plan(|_| true);
        assert_eq!(plan.len(), 0, "nothing to apply");
        assert!(plan.emit().is_empty());
        assert_eq!(plan.omitted.len(), 1, "{:?}", plan.omitted);
        assert!(plan.omitted[0].contains("secret_v"), "{:?}", plan.omitted);
        // No tick-box either, for `needs_source`'s reason.
        assert!(
            c.selectable_keys(RowFilter {
                query: "",
                show_same: false
            })
            .is_empty()
        );
    }

    /// **A change set that is not empty and emits nothing is the same problem,
    /// and the predicate could not see it.**
    ///
    /// `unplannable` asked `changes.is_empty()` while its own doc states the
    /// rule as "a difference this comparison **has no statement for**" and names
    /// *"Applied 0 statements to 1 object"* as the outcome it prevents. Those
    /// are two different questions, and the second one admitted the first input
    /// that defeats it: `columns_equal` raises a change for a PostgreSQL
    /// identity kind that `pg_column_clauses` had no arm for — so the entry was
    /// `Differing`, the plan counted it, the preview listed one change over an
    /// empty SQL box, Apply was enabled, and the success line read that exact
    /// sentence. The next compare found the same difference: a sync that never
    /// converges.
    ///
    /// Nothing else could catch it. `ChangeSet::unsupported` filters on
    /// `supports_change`, which is `true` for a PostgreSQL `AlterColumn`
    /// whatever the clause builder can express.
    ///
    /// That particular input now emits (see
    /// `a_postgres_identity_kind_is_a_statement_rather_than_a_silence`), so the
    /// property is stated over a set that emits nothing for any reason — which
    /// is what the predicate is actually about, and what keeps the *next* such
    /// change from arriving silently.
    #[test]
    fn a_difference_the_emitter_cannot_express_is_disclosed_rather_than_counted() {
        // A change set that is not empty and has nothing to say: an
        // `AlterColumn` from a column to itself. `pg_column_clauses` finds no
        // difference to spell, so `emit()` is empty.
        let c = col("id", "integer");
        let set = ChangeSet {
            table: "city".to_string(),
            schema: None,
            dialect: SqlDialect::Postgres,
            flavour: crate::schema::ServerFlavour::Unknown,
            changes: vec![crate::ddl::Change::AlterColumn {
                from: Box::new(c.clone()),
                to: Box::new(c),
                position: None,
                inline_check: None,
            }],
        };
        assert!(!set.is_empty(), "the premise: the set is not empty");
        assert!(set.emit().is_empty(), "and it says nothing");

        let entry = CompareEntry {
            kind: CompareKind::Table,
            schema: None,
            name: "city".to_string(),
            table: None,
            signature: None,
            status: ObjectStatus::Differing,
            changes: set,
            uncertain: false,
            left_ddl: String::new(),
            right_ddl: String::new(),
        };
        assert!(
            entry.unplannable(),
            "a difference this comparison has no statement for"
        );
        let c = SchemaComparison {
            entries: vec![entry],
            dialect: SqlDialect::Postgres,
            cycles_create: false,
            cycles_drop: false,
            new_namespaces: Vec::new(),
            name_clashes: Vec::new(),
        };
        let plan = c.plan(|_| true);
        assert_eq!(plan.len(), 0, "not an object the plan applies");
        assert_eq!(plan.omitted.len(), 1, "{:?}", plan.omitted);
        assert!(plan.omitted[0].contains("city"), "{:?}", plan.omitted);
        assert!(
            plan.omitted[0].contains("no statement for the difference"),
            "and it says which of the two reasons: {:?}",
            plan.omitted
        );
    }

    /// **And the input that found it is a statement now**, rather than a
    /// disclosure: PostgreSQL spells the identity *kind* with
    /// `ALTER COLUMN … SET GENERATED`, and `pg_column_clauses` had arms for the
    /// expression, the type, the collation, nullability, the default and
    /// `auto_increment` — and none for this.
    #[test]
    fn a_postgres_identity_kind_is_a_statement_rather_than_a_silence() {
        let side = |always: bool| {
            let mut c = col("id", "integer");
            c.auto_increment = true;
            c.identity_always = always;
            from_db(
                "app",
                vec![TableInfo {
                    name: "city".to_string(),
                    columns: vec![c],
                    ..Default::default()
                }],
            )
        };
        let c = SchemaComparison::of(&side(true), &side(false), SqlDialect::Postgres);
        assert_eq!(find(&c, "table:city").status, ObjectStatus::Differing);
        let sql = c.plan(|_| true).emit().join(
            "
",
        );
        assert!(sql.contains("SET GENERATED BY DEFAULT"), "{sql}");
        // And the other direction.
        let c = SchemaComparison::of(&side(false), &side(true), SqlDialect::Postgres);
        let sql = c.plan(|_| true).emit().join(
            "
",
        );
        assert!(sql.contains("SET GENERATED ALWAYS"), "{sql}");
    }

    // ── an enum's dependents come off the side the DDL runs on ───────────────

    /// **Four lines of rationale, load-bearing through `RecreateEnum`, and
    /// nothing reached it.** `type_dependents` was never called with a schema
    /// that produced a non-empty result in this module's suite: every enum
    /// fixture stood on its own, so the argument was always `[]` and the
    /// sentence about *which side* it is read from could not have been wrong in
    /// a way a test would notice.
    ///
    /// PostgreSQL has no `ALTER TYPE … DROP VALUE`, so narrowing an enum is a
    /// recreate: every column declared with the type has to be re-cast around
    /// the swap. Those columns are the **left** side's — they are what the
    /// statements will run against — and reading the right side's would list
    /// columns that are not there to cast.
    #[test]
    fn an_enums_dependents_are_the_left_sides_columns() {
        let mood = |vals: &[&str]| EnumInfo {
            name: "mood".to_string(),
            schema: Some("public".to_string()),
            values: vals.iter().map(|v| v.to_string()).collect(),
            comment: None,
        };
        // A column of that type on each side, under *different* table names, so
        // "which side" is a question with two visible answers.
        let user_of = |table: &str| TableInfo {
            name: table.to_string(),
            schema: Some("public".to_string()),
            columns: vec![ColumnInfo {
                name: "m".to_string(),
                type_name: "mood".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let left = DbSchema {
            tables: vec![user_of("left_only")],
            enums: vec![mood(&["sad", "ok", "glad"])],
            ..Default::default()
        };
        let right = DbSchema {
            tables: vec![user_of("right_only")],
            // Narrowed: `glad` is gone, so this is a recreate rather than an
            // `ADD VALUE`.
            enums: vec![mood(&["sad", "ok"])],
            ..Default::default()
        };

        let c = SchemaComparison::of(&left, &right, SqlDialect::Postgres);
        let e = find(&c, "enum:mood");
        assert_eq!(e.status, ObjectStatus::Differing);
        let sql = e.changes.emit().join("\n");
        assert!(
            sql.contains("left_only"),
            "the columns to re-cast are the ones the statements will run against: {sql}"
        );
        assert!(
            !sql.contains("right_only"),
            "the right side's columns are not there to cast: {sql}"
        );
    }

    /// And a widened enum is not a recreate at all — `ADD VALUE` needs no
    /// dependents, which is the arm the test above must not be confused with.
    #[test]
    fn a_widened_enum_needs_no_dependents() {
        let mood = |vals: &[&str]| EnumInfo {
            name: "mood".to_string(),
            schema: Some("public".to_string()),
            values: vals.iter().map(|v| v.to_string()).collect(),
            comment: None,
        };
        let side = |vals: &[&str]| DbSchema {
            enums: vec![mood(vals)],
            ..Default::default()
        };
        let c = SchemaComparison::of(
            &side(&["sad"]),
            &side(&["sad", "glad"]),
            SqlDialect::Postgres,
        );
        let sql = find(&c, "enum:mood").changes.emit().join("\n");
        assert!(sql.contains("ADD VALUE"), "{sql}");
    }

    // ── a match the comparison cannot vouch for ──────────────────────────────

    /// **The flag's own case was the one nothing disclosed.** A lossy index —
    /// a PostgreSQL expression index whose keys the model never read — compares
    /// equal whatever the server holds, so the object it is on comes out
    /// `Same`. `uncertain` was drawn only as a per-row hint, and `show_same:
    /// false` (the default) hides that row: the tree said the two schemas
    /// agreed about something it had never fully read.
    ///
    /// The tally is over the whole comparison, which is the one surface that is
    /// about the whole comparison rather than about a row.
    #[test]
    fn an_uncertain_match_is_counted_even_though_its_row_is_hidden() {
        let lossy = || crate::schema::IndexInfo {
            name: "ix_expr".to_string(),
            lossy: true,
            ..Default::default()
        };
        let mut side = table("city", &[("id", "int")]);
        side.indexes = vec![lossy()];
        let c = mysql(schema_of(vec![side.clone()]), schema_of(vec![side]));

        assert_eq!(c.counts().same, 1, "the two agree, as far as it can tell");
        assert_eq!(c.counts().uncertain, 1, "and it cannot tell far enough");
        // The row itself is hidden by default, which is the whole point.
        assert!(
            c.rows(
                RowFilter {
                    query: "",
                    show_same: false
                },
                &c.default_expanded()
            )
            .is_empty()
        );
    }

    /// It overlaps the four statuses rather than being a fifth: an uncertain
    /// object is still `Same` or `Differing`, and the summary line says both
    /// things about it.
    #[test]
    fn the_uncertain_tally_does_not_come_out_of_the_status_tallies() {
        let lossy = || crate::schema::IndexInfo {
            name: "ix_expr".to_string(),
            lossy: true,
            ..Default::default()
        };
        let mut left = table("city", &[("id", "int")]);
        left.indexes = vec![lossy()];
        let mut right = table("city", &[("id", "int"), ("name", "text")]);
        right.indexes = vec![lossy()];
        let c = mysql(schema_of(vec![left]), schema_of(vec![right]));
        let n = c.counts();
        assert_eq!(n.differing, 1);
        assert_eq!(n.uncertain, 1);
        assert_eq!(
            n.total(),
            1,
            "one object, counted once — `uncertain` is a second fact about it"
        );
    }

    // ── namespaces the migration has to make first ───────────────────────────

    /// **A table cannot be created in a namespace that is not there.** A
    /// namespace is not an object this comparison pairs — nothing introspects an
    /// empty one and there is nothing in it to diff — so a right-only
    /// `reporting.sales` emitted `CREATE TABLE reporting.sales` against a
    /// database with no `reporting` in it. PostgreSQL refuses that statement and
    /// with it the transaction the whole migration runs in, while
    /// `unsupported()` was empty and Apply was live.
    #[test]
    fn a_table_in_a_new_namespace_gets_its_create_schema_first() {
        let in_ns = |ns: &str, name: &str| TableInfo {
            name: name.to_string(),
            schema: Some(ns.to_string()),
            columns: vec![col("id", "int")],
            ..Default::default()
        };
        let c = SchemaComparison::of(
            &schema_of(vec![in_ns("public", "city")]),
            &schema_of(vec![in_ns("public", "city"), in_ns("reporting", "sales")]),
            SqlDialect::Postgres,
        );
        assert_eq!(c.new_namespaces, vec!["reporting".to_string()]);

        let plan = c.plan(|_| true);
        let stmts = plan.emit();
        let schema = stmts.iter().position(|s| s.contains("CREATE SCHEMA"));
        let table = stmts.iter().position(|s| s.contains("sales"));
        assert!(
            schema.is_some() && schema < table,
            "the namespace has to exist before the table in it: {stmts:?}"
        );
        assert!(
            stmts
                .iter()
                .all(|s| !s.contains("CREATE SCHEMA \"public\"")),
            "a namespace both sides already have is not created: {stmts:?}"
        );
    }

    /// And only for a namespace the plan actually reaches into. The
    /// comparison's list is about the two schemas; a plan over one ticked table
    /// has no business creating a namespace for an object the user left out.
    #[test]
    fn an_unticked_objects_namespace_is_not_created() {
        let in_ns = |ns: &str, name: &str| TableInfo {
            name: name.to_string(),
            schema: Some(ns.to_string()),
            columns: vec![col("id", "int")],
            ..Default::default()
        };
        let c = SchemaComparison::of(
            &schema_of(vec![in_ns("public", "city")]),
            &schema_of(vec![
                in_ns("public", "city"),
                in_ns("reporting", "sales"),
                in_ns("audit", "log"),
            ]),
            SqlDialect::Postgres,
        );
        assert_eq!(c.new_namespaces.len(), 2, "{:?}", c.new_namespaces);
        let plan = c.plan(|e| e.name == "sales");
        let stmts = plan.emit();
        assert!(stmts.iter().any(|s| s.contains("reporting")), "{stmts:?}");
        assert!(
            stmts.iter().all(|s| !s.contains("audit")),
            "nothing here is going into `audit`: {stmts:?}"
        );
    }

    /// MySQL and SQLite have no level between the database and the table, so
    /// every object's namespace there is `None` and the list is empty by
    /// construction rather than by a dialect test.
    #[test]
    fn an_engine_with_no_namespaces_never_creates_one() {
        let c = mysql(
            schema_of(vec![]),
            schema_of(vec![table("fresh", &[("id", "int")])]),
        );
        assert!(c.new_namespaces.is_empty());
        assert!(
            c.plan(|_| true)
                .emit()
                .iter()
                .all(|s| !s.contains("CREATE SCHEMA"))
        );
    }

    // ── the two sentences about a count ──────────────────────────────────────

    /// **One number, two surfaces, and they were two hand-rolled plurals a
    /// hundred lines apart** — inside a footer's `label` closure and inside a
    /// click closure, both beside `preview_title`, which was extracted precisely
    /// so a modal's words could be tested.
    #[test]
    fn a_plans_subject_and_the_footers_count_agree_and_read_as_english() {
        let one = mysql(
            schema_of(vec![]),
            schema_of(vec![table("fresh", &[("id", "int")])]),
        )
        .plan(|_| true);
        assert_eq!(one.subject(), "1 object");
        assert_eq!(selection_note(1), "1 object selected");

        let two = mysql(
            schema_of(vec![table("gone", &[("id", "int")])]),
            schema_of(vec![table("fresh", &[("id", "int")])]),
        )
        .plan(|_| true);
        assert_eq!(two.subject(), "2 objects");
        assert_eq!(selection_note(2), "2 objects selected");

        // The empty plan is the one the footer says something else about: there
        // is no button to press, so the sentence is not a count at all.
        assert_eq!(SchemaPlan::default().subject(), "0 objects");
        assert_eq!(selection_note(0), "Nothing selected.");

        // **And a `CREATE SCHEMA` the plan prepended is not an object.** Every
        // case above is MySQL, where `new_namespaces` is empty by
        // construction, which is why they were green over this: on PostgreSQL,
        // ticking one table into a namespace the left side lacks made the
        // footer read "1 object selected" over a preview reading "2 objects",
        // and the success line "Applied 2 statements to 2 objects".
        let in_ns = |ns: &str, name: &str| TableInfo {
            name: name.to_string(),
            schema: Some(ns.to_string()),
            columns: vec![col("id", "int")],
            ..Default::default()
        };
        let plan = SchemaComparison::of(
            &schema_of(vec![in_ns("public", "city")]),
            &schema_of(vec![in_ns("public", "city"), in_ns("reporting", "sales")]),
            SqlDialect::Postgres,
        )
        .plan(|_| true);
        assert!(
            plan.emit().iter().any(|s| s.contains("CREATE SCHEMA")),
            "the fixture has to prepend one for this to mean anything: {:?}",
            plan.emit()
        );
        assert_eq!(plan.subject(), "1 object");
        assert_eq!(selection_note(1), "1 object selected");
    }

    /// **And the preview names the database, not just the connection.** The
    /// title qualifies its subject with the database only when the subject is an
    /// object *name*, and a plan's is a count — so it fell back to the
    /// connection alone, on a feature whose whole subject is two databases on
    /// one connection, with the comparison closed behind the modal.
    #[test]
    fn a_plans_subject_names_the_database_it_lands_in() {
        let plan = mysql(
            schema_of(vec![]),
            schema_of(vec![table("fresh", &[("id", "int")])]),
        )
        .plan(|_| true);
        assert_eq!(plan.subject_in("shop"), "1 object in shop");
        assert_eq!(
            SchemaPlan::default().subject_in("shop"),
            "0 objects in shop"
        );
    }

    // ── why the tree is empty ────────────────────────────────────────────────

    /// **The arm that was wrong.** A filter that matched — but matched only
    /// objects the two schemas agree on, with *Include identical* off — was
    /// reported as "Nothing matches that filter", which is a claim about the
    /// filter over a result the toggle beside it produced. The user retypes the
    /// filter instead of pressing the checkbox next to it.
    #[test]
    fn a_filter_that_matched_only_agreements_says_so_rather_than_blaming_itself() {
        let both = || {
            schema_of(vec![
                table("city", &[("id", "int")]),
                table("country", &[("id", "int")]),
            ])
        };
        let c = mysql(both(), both());
        let f = RowFilter {
            query: "city",
            show_same: false,
        };
        assert!(c.rows(f, &c.default_expanded()).is_empty());
        assert_eq!(c.empty_reason(f), EmptyRows::OnlyIdenticalMatched);
        assert!(
            c.empty_reason(f).message().contains("Include identical"),
            "the message points at the control that would show it: {}",
            c.empty_reason(f).message()
        );
    }

    /// The other three arms, which the old inline form got right and which have
    /// to keep being right now that they are one function.
    #[test]
    fn the_other_three_emptinesses_read_as_themselves() {
        // Nothing either side holds.
        let none = SchemaComparison::of(
            &DbSchema::default(),
            &DbSchema::default(),
            SqlDialect::MySql,
        );
        assert_eq!(
            none.empty_reason(RowFilter {
                query: "",
                show_same: false
            }),
            EmptyRows::NothingToCompare
        );

        // Two identical schemas, no filter.
        let both = || schema_of(vec![table("city", &[("id", "int")])]);
        let same = mysql(both(), both());
        assert_eq!(
            same.empty_reason(RowFilter {
                query: "",
                show_same: false
            }),
            EmptyRows::EverythingAgrees
        );

        // A filter that names nothing either side has.
        assert_eq!(
            same.empty_reason(RowFilter {
                query: "zzz",
                show_same: false
            }),
            EmptyRows::NoMatch
        );
    }

    /// Every arm has its own sentence — a message table where two arms shared
    /// one would be the bug this exists to fix, wearing a different hat.
    #[test]
    fn each_emptiness_says_something_different() {
        let all = [
            EmptyRows::NothingToCompare,
            EmptyRows::EverythingAgrees,
            EmptyRows::NoMatch,
            EmptyRows::OnlyIdenticalMatched,
        ];
        let msgs: HashSet<&str> = all.iter().map(|r| r.message()).collect();
        assert_eq!(msgs.len(), all.len(), "{msgs:?}");
    }

    /// **A trigger's label carries its namespace, because a filter reads it.**
    /// Two triggers named `t` on `city`, one in each of two PostgreSQL
    /// namespaces, drew as two identical rows — and `selectable_keys` filters on
    /// this very text, so typing `archive` matched neither and returned an empty
    /// set: the one object that differed was invisible *and* unselectable, and
    /// *Select all* was a silent no-op over it. Its `key` had the namespace all
    /// along, which is why nothing about the plan was wrong — only what the user
    /// could see and reach.
    #[test]
    fn a_trigger_label_says_which_namespace_its_table_is_in() {
        let with_trigger = |ns: &str, body: &str| {
            let mut t = TableInfo {
                name: "city".to_string(),
                schema: Some(ns.to_string()),
                columns: vec![col("id", "int")],
                ..Default::default()
            };
            t.triggers = vec![TriggerInfo {
                name: "t".to_string(),
                schema: Some(ns.to_string()),
                table: "city".to_string(),
                action: TriggerAction::Body(body.to_string()),
                ..Default::default()
            }];
            t
        };
        let c = SchemaComparison::of(
            &schema_of(vec![
                with_trigger("app", "BEGIN END"),
                with_trigger("archive", "BEGIN END"),
            ]),
            &schema_of(vec![
                with_trigger("app", "BEGIN END"),
                with_trigger("archive", "SELECT 2"),
            ]),
            SqlDialect::Postgres,
        );
        let labels: Vec<String> = c.entries.iter().map(CompareEntry::label).collect();
        assert!(
            labels.contains(&"archive.city.t".to_string()),
            "the namespace is on the table, not on the trigger: {labels:?}"
        );
        assert!(labels.contains(&"app.city.t".to_string()), "{labels:?}");

        // And the filter that reads those labels can now reach the one that
        // differs.
        let shown = c.selectable_keys(RowFilter {
            query: "archive",
            show_same: false,
        });
        assert_eq!(shown.len(), 1, "{shown:?}");
        assert!(shown[0].contains("archive"), "{shown:?}");
    }

    /// **`phase` itself, which nothing reached.** Every ordering test in this
    /// module goes through `SchemaComparison::of` and varies the
    /// [`CompareKind`], so `kind_rank`'s axis was covered and this one — the
    /// one carrying dependency ordering between creates, alters and drops — was
    /// at zero: swapping two of its literals left the whole suite green.
    ///
    /// Stated as the relation rather than as the numbers, so renumbering the
    /// phases is free and reordering them is not.
    #[test]
    fn the_plan_phases_run_in_dependency_order() {
        use CompareKind::{Enum, Table, Trigger};
        use ObjectStatus::{Differing, OnlyLeft, OnlyRight, Same};
        let ph = phase;

        // Dependents come off first: a trigger on a table being dropped has to
        // go before the DROP TABLE, or the drop is refused.
        assert!(ph(Trigger, OnlyLeft) < ph(Table, OnlyLeft));
        // A type is created before any table that names it.
        assert!(ph(Enum, OnlyRight) < ph(Table, OnlyRight));
        assert!(ph(Enum, Differing) < ph(Table, Differing));
        // Created and altered tables share one phase — the order between them
        // is a dependency question `fk_rank` answers, not a phase question.
        assert_eq!(ph(Table, OnlyRight), ph(Table, Differing));
        // Tables are dropped after they are created or altered, and dependents
        // go back on after that.
        assert!(ph(Table, Differing) < ph(Table, OnlyLeft));
        assert!(ph(Table, OnlyLeft) < ph(Trigger, OnlyRight));
        // A dependent being *dropped* is the very first thing that happens —
        // ahead even of the types, since it may name one.
        assert!(ph(Trigger, OnlyLeft) < ph(Enum, OnlyRight));
        // `Same` is never planned, and sorts past everything so the differences
        // keep their order however many agreements sit between them.
        for kind in [Table, Trigger, Enum] {
            for st in [OnlyRight, Differing, OnlyLeft] {
                assert!(ph(kind, st) < ph(kind, Same), "{kind:?}/{st:?}");
            }
        }
    }

    /// A foreign key onto `to`, on a column named `other`.
    fn fk_to(to: &str) -> ForeignKeyInfo {
        ForeignKeyInfo {
            name: format!("fk_{to}"),
            columns: vec!["other".to_string()],
            ref_table: to.to_string(),
            ref_columns: vec!["id".to_string()],
            ..Default::default()
        }
    }

    /// The plan's statements' leading words, in order — enough to read a
    /// dependency order off without depending on whole statement text.
    fn heads(plan: &SchemaPlan) -> Vec<String> {
        plan.emit()
            .iter()
            .map(|s| s.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
            .collect()
    }

    // ── the ordering axis: status, not kind ──────────────────────────────────
    //
    // Every ordering test above this line varies the `CompareKind` and asserts a
    // `Vec<CompareKind>`, holding `ObjectStatus` fixed — which covers the axis
    // `kind_rank` owns and leaves the axis `phase` owns at zero. That is the
    // axis carrying dependency ordering *between* creates, alters and drops,
    // and every ordering bug this module has had lived on it. These pin it.

    /// **A new table referencing a column an `ALTER` has yet to add.** Right has
    /// `child(other) REFERENCES parent(code)` and `parent` gains `code` in the
    /// same comparison — one create, one alter. Creates ran as a whole phase
    /// before alters, so `CREATE TABLE child` went out first, MySQL refused it,
    /// and the rest of the plan was already half-applied.
    ///
    /// Creates and alters now share one phase ordered by the **right** schema's
    /// foreign-key order, which answers both directions of this at once.
    #[test]
    fn an_alter_that_a_create_depends_on_runs_before_it() {
        let parent_l = table("parent", &[("id", "int")]);
        let parent_r = table("parent", &[("id", "int"), ("code", "int")]);
        let mut child = table("child", &[("id", "int"), ("other", "int")]);
        child.foreign_keys = vec![fk_to("parent")];

        let c = mysql(schema_of(vec![parent_l]), schema_of(vec![parent_r, child]));
        let h = heads(&c.plan(|_| true));
        let alter = h.iter().position(|s| s.contains("`parent`"));
        let create = h.iter().position(|s| s.contains("`child`"));
        assert!(
            alter < create,
            "the ALTER adding parent.code must precede the CREATE naming it: {h:?}"
        );
    }

    /// **And the other direction, which a plain swap of the two phases would
    /// have broken.** An existing table gains a foreign key onto a table only
    /// the right side has: the `CREATE` must come first.
    #[test]
    fn a_create_that_an_alter_depends_on_runs_before_it() {
        let existing_l = table("orders", &[("id", "int"), ("other", "int")]);
        let mut existing_r = table("orders", &[("id", "int"), ("other", "int")]);
        existing_r.foreign_keys = vec![fk_to("customers")];
        let customers = table("customers", &[("id", "int")]);

        let c = mysql(
            schema_of(vec![existing_l]),
            schema_of(vec![existing_r, customers]),
        );
        let h = heads(&c.plan(|_| true));
        let create = h.iter().position(|s| s.contains("`customers`"));
        let alter = h.iter().position(|s| s.contains("`orders`"));
        assert!(
            create < alter,
            "the CREATE must precede the ALTER whose foreign key names it: {h:?}"
        );
    }

    /// **Two altered tables have a dependency order too.** `fk_rank` reached
    /// only the create and drop phases, so two `Differing` tables sorted
    /// alphabetically: `a_child` before `z_parent`, and the `ALTER` adding the
    /// referenced column ran second.
    #[test]
    fn two_altered_tables_are_ordered_by_their_foreign_keys_not_their_names() {
        let a_l = table("a_child", &[("id", "int"), ("other", "int")]);
        let mut a_r = table("a_child", &[("id", "int"), ("other", "int")]);
        a_r.foreign_keys = vec![fk_to("z_parent")];
        let z_l = table("z_parent", &[("id", "int")]);
        let z_r = table("z_parent", &[("id", "int"), ("code", "int")]);

        let c = mysql(schema_of(vec![a_l, z_l]), schema_of(vec![a_r, z_r]));
        let h = heads(&c.plan(|_| true));
        let child = h.iter().position(|s| s.contains("`a_child`"));
        let parent = h.iter().position(|s| s.contains("`z_parent`"));
        assert!(
            parent < child,
            "alphabetical is not a dependency order: {h:?}"
        );
    }

    /// **A view is created after the view it selects from.** `fk_rank` filtered
    /// views out of `order_tables`' `chosen`, so the view-dependency half of
    /// that sort — which exists because `CREATE VIEW ... FROM other_view` on a
    /// target lacking `other_view` is ERROR 1146 — never ran, and two created
    /// views sorted by name.
    #[test]
    fn a_view_is_created_after_the_view_its_body_selects_from() {
        let base = view("z_detail", "SELECT 1 AS n");
        let on_top = view("a_totals", "SELECT sum(n) FROM z_detail");
        let c = mysql(schema_of(vec![]), schema_of(vec![on_top, base]));
        let h = heads(&c.plan(|_| true));
        let detail = h.iter().position(|s| s.contains("z_detail"));
        let totals = h.iter().position(|s| s.contains("a_totals"));
        assert!(
            detail < totals,
            "a view's body names the view above it: {h:?}"
        );
    }

    /// **A drop-only cycle is a cycle.** `plan.cycles` was gated on the plan
    /// creating a table, and the comment defending that gate said "a plan of
    /// pure alters **or drops** is unaffected". It is not: `DROP TABLE b` before
    /// `DROP TABLE a` is refused at statement 1 when each references the other,
    /// and neither MySQL nor MariaDB rolls DDL back.
    #[test]
    fn a_drop_only_plan_reports_its_cycle() {
        let mut a = table("a", &[("id", "int"), ("other", "int")]);
        a.foreign_keys = vec![fk_to("b")];
        let mut b = table("b", &[("id", "int"), ("other", "int")]);
        b.foreign_keys = vec![fk_to("a")];
        let c = mysql(schema_of(vec![a, b]), schema_of(vec![]));
        let plan = c.plan(|_| true);
        assert!(
            plan.cycles,
            "two mutually referencing drops cannot be ordered"
        );
        assert!(plan.destructive().iter().any(|r| r.contains("cycle")));
    }

    /// **And the two cycles are different facts about different schemas.** The
    /// *left* side is tangled and the right side is not — the migration is
    /// exactly the untangling, plus one unreferenced new table. `cycles = c1 ||
    /// c2` folded the left side's drop-order tangle into the create-order
    /// warning, so this plan got the cycle sentence and "This can't be undone"
    /// over a `CREATE TABLE` that references nothing at all.
    #[test]
    fn a_left_side_cycle_does_not_warn_a_plan_that_only_creates() {
        let mut a = table("a", &[("id", "int"), ("other", "int")]);
        a.foreign_keys = vec![fk_to("b")];
        let mut b = table("b", &[("id", "int"), ("other", "int")]);
        b.foreign_keys = vec![fk_to("a")];
        let left = schema_of(vec![a.clone(), b.clone()]);
        // The right side keeps both tables and drops the foreign keys, so its
        // own create order is untangled.
        let (mut a_r, mut b_r) = (a, b);
        a_r.foreign_keys.clear();
        b_r.foreign_keys.clear();
        let right = schema_of(vec![a_r, b_r, table("fresh", &[("id", "int")])]);
        let c = mysql(left, right);
        assert!(c.cycles_drop, "the left side is the tangled one");
        assert!(!c.cycles_create, "the right side is not");
        let plan = c.plan(|_| true);
        assert!(
            !plan.cycles,
            "nothing here is dropped, and the creation order is satisfiable"
        );
        assert!(!plan.destructive().iter().any(|r| r.contains("cycle")));
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
        let sql = c.plan(|_| true).editor_script();
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
        assert!(
            plan.editor_script().contains("`town`"),
            "{}",
            plan.editor_script()
        );
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
        assert!(
            plan.editor_script().contains("`a`"),
            "{}",
            plan.editor_script()
        );
        assert!(
            !plan.editor_script().contains("`b`"),
            "{}",
            plan.editor_script()
        );
    }

    #[test]
    fn only_left_plans_a_drop_and_only_right_plans_a_create() {
        let c = mysql(
            schema_of(vec![table("gone", &[("id", "int")])]),
            schema_of(vec![table("fresh", &[("id", "int")])]),
        );
        let sql = c.plan(|_| true).editor_script().to_uppercase();
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
        assert!(c.cycles());
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
        assert!(c.cycles(), "the schema still has the cycle");
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
        //
        // This is an identity by construction — `export_script` returns
        // `editor_script()` on this input — so it pins the *pass-through* and
        // nothing else. The branch that matters is pinned by
        // `a_plan_carrying_an_account_change_does_not_export_the_password`.
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
        let sql = c.plan(|_| true).editor_script();
        let at = |n: &str| sql.find(n).unwrap_or_else(|| panic!("no {n} in {sql}"));
        assert!(at("`parent`") < at("`child`"), "{sql}");
        assert!(!c.cycles());
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
        assert!(c.cycles());
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
            plan.editor_script().starts_with("-- INCOMPLETE"),
            "the script has to admit it is partial: {}",
            plan.editor_script()
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
            plan.editor_script().starts_with("CREATE TABLE"),
            "{}",
            plan.editor_script()
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
        //
        // **The expectation is built from `ddl` alone** and never reads
        // `plan.sets`. The previous form was
        // `assert_eq!(plan.emit(), plan.sets.iter().flat_map(ChangeSet::emit))`
        // — the same expression as `SchemaPlan::emit`'s own body, so it held
        // for every input and for every future body keeping that first step,
        // which is the one thing it was written to catch. `!is_empty()` saved
        // it from being vacuous, not from being tautological.
        let gone = table("gone", &[("id", "int")]);
        let fresh = table("fresh", &[("id", "int")]);
        let c = mysql(
            schema_of(vec![gone.clone()]),
            schema_of(vec![fresh.clone()]),
        );
        let plan = c.plan(|_| true);

        let mut expected = ddl::create(&TableDraft::from_table(&fresh), SqlDialect::MySql).emit();
        expected.extend(ddl::single(&gone.name, None, SqlDialect::MySql, Change::DropTable).emit());
        assert!(
            expected.len() >= 2,
            "the fixture has to plan both: {expected:?}"
        );
        assert_eq!(plan.emit(), expected);
    }

    /// **The redaction branch of [`SchemaPlan::export_script`] had no coverage
    /// anywhere in the workspace.** Its sibling test asserts
    /// `export_script() == editor_script()` on a plan with no account change —
    /// and the body's first line is `if no account change { return
    /// editor_script() }`, so that assertion is `f() == f()` and cannot fail.
    ///
    /// No builder puts an account change in a compare plan *today*, which is
    /// precisely the situation the method's own doc says it is written to
    /// survive: the property is "no plaintext password leaves this modal", and
    /// a future builder should inherit it rather than have to read the prose.
    /// So the plan is assembled here by hand.
    #[test]
    fn a_plan_carrying_an_account_change_does_not_export_the_password() {
        let draft = crate::users::AccountDraft {
            name: "reporter".to_string(),
            host: "%".to_string(),
            kind: crate::users::PrincipalKind::User,
            password: "hunter2-in-the-clear".to_string(),
        };
        let name = draft.name.clone();
        let plan = SchemaPlan {
            sets: vec![ddl::single(
                &name,
                None,
                SqlDialect::MySql,
                Change::CreateAccount(Box::new(draft)),
            )],
            dialect: SqlDialect::MySql,
            cycles: false,
            omitted: Vec::new(),
            clashes: Vec::new(),
        };
        assert!(
            plan.editor_script().contains("hunter2-in-the-clear"),
            "the fixture has to carry a password for this to mean anything: {}",
            plan.editor_script()
        );

        let out = plan.export_script();
        assert!(
            !out.contains("hunter2-in-the-clear"),
            "a password reached the clipboard and a saved editor tab: {out}"
        );
        assert!(out.contains(ddl::PASSWORD_PLACEHOLDER), "{out}");
        assert!(
            out.contains("CREATE USER") || out.to_uppercase().contains("CREATE USER"),
            "the statement itself still travels: {out}"
        );
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
        // **Deliberately not MySQL**, which is where this test used to be: a
        // MySQL body arrives escape-mangled, so no compare plan can carry one
        // and `plan` now says so rather than emitting it — see
        // `a_mysql_body_is_kept_out_of_the_plan_and_disclosed_by_it`. That
        // move takes `client_script`'s DELIMITER wrapping out of reach from
        // here, so this is **not** a test of it; `editor_script`'s doc says so.
        //
        // What is left to pin is the composition SQLite actually runs: a body
        // full of `;` has to leave `editor_script` as one statement the app's
        // own splitter keeps whole. The assertion used to be
        // `script.contains("t_ins")`, which held for any join of any emit.
        let mut t = table("city", &[("id", "int")]);
        t.triggers = vec![trigger(
            "t_ins",
            "city",
            "BEGIN SET @a = 1; SET @b = 2; END",
        )];
        let c = SchemaComparison::of(
            &schema_of(vec![table("city", &[("id", "int")])]),
            &schema_of(vec![t]),
            SqlDialect::Sqlite,
        );
        let script = c.plan(|_| true).editor_script();
        assert!(script.contains("t_ins"), "{script}");

        // The property the name claims, rather than a substring: the app's own
        // splitter has to hand this back as **one** statement with the body
        // whole, and terminated. The two `;` between BEGIN and END are what
        // would cut it into three.
        let stmts = crate::sql::executable_statements(&script, SqlDialect::Sqlite);
        assert_eq!(stmts.len(), 1, "the splitter cut the body up: {stmts:?}");
        assert!(
            stmts[0].contains("SET @b = 2; END"),
            "the tail of the body did not survive: {}",
            stmts[0]
        );
        assert!(
            script.trim_end().ends_with(';'),
            "a client splitting on `;` needs the last statement terminated: {script}"
        );
    }

    /// **The disclosure the preview modal was missing.** On MySQL every
    /// routine, trigger and event difference is kept out of the plan — the body
    /// `information_schema` hands back has had its escapes resolved, so the
    /// `CREATE` written from it is not the routine that was there. The row in
    /// the tree said so; nothing past it did. `unsupported()` was empty, so the
    /// withheld block stayed hidden and Apply stayed enabled; the preview's
    /// subject was a count of what *was* included, and success read "Applied N
    /// statements to 1 object" over a three-difference comparison.
    ///
    /// `SchemaComparison::needs_source`, written to disclose exactly this, had
    /// no production caller at all — its only three call sites were its own
    /// three asserts.
    #[test]
    fn a_mysql_body_is_kept_out_of_the_plan_and_disclosed_by_it() {
        let left = table("city", &[("id", "int")]);
        let mut right = table("city", &[("id", "int"), ("name", "text")]);
        right.triggers = vec![trigger("t_ins", "city", "BEGIN SET @a = 1; END")];
        let c = mysql(schema_of(vec![left]), schema_of(vec![right]));
        assert!(c.needs_source(), "a differing MySQL trigger is in there");

        let plan = c.plan(|_| true);
        // The table's ALTER is the whole plan …
        assert_eq!(plan.len(), 1);
        assert!(
            plan.emit().iter().all(|s| !s.contains("TRIGGER")),
            "{:?}",
            plan.emit()
        );
        // … and Apply is not refused, because what *is* in the plan is complete.
        assert!(plan.unsupported().is_empty());
        // But the plan says what it left behind, naming the object.
        assert_eq!(plan.omitted.len(), 1);
        assert!(plan.omitted[0].contains("t_ins"), "{:?}", plan.omitted);
    }

    /// The exclusion is `plan`'s own, not only the caller's. A key that reached
    /// the selection by some other route — a comparison replaced under a stale
    /// set, a future "invert" — still cannot put an untrustworthy body into
    /// `emit`.
    #[test]
    fn a_blocked_body_cannot_be_planned_however_the_predicate_answers() {
        let mut right = table("city", &[("id", "int")]);
        right.triggers = vec![trigger("t_ins", "city", "BEGIN SET @a = 1; END")];
        let c = mysql(
            schema_of(vec![table("city", &[("id", "int")])]),
            schema_of(vec![right]),
        );
        let blocked = c
            .differences()
            .find(|e| e.needs_source())
            .expect("the trigger");
        let mut selected = HashSet::new();
        selected.insert(blocked.key());
        assert!(!is_planned(blocked, &selected), "no tick-box, no plan");
        // And even a predicate that says yes to everything cannot.
        assert!(c.plan(|_| true).emit().is_empty());
        assert_eq!(c.plan(|_| true).omitted.len(), 1);
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
                uncertain: 0,
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
        assert!(plan.editor_script().is_empty());
        assert!(plan.destructive().is_empty());
        assert!(plan.unsupported().is_empty());
        assert!(plan.summaries().is_empty());
    }

    #[test]
    fn comparing_two_empty_schemas_yields_no_entries() {
        let c = mysql(DbSchema::default(), DbSchema::default());
        assert!(c.entries.is_empty());
        assert_eq!(c.counts(), CompareCounts::default());
        assert!(!c.cycles());
    }

    // ── two databases, one schema ───────────────────────────────────
    //
    // Every "these two agree" fixture above builds one expression and compares
    // it with itself, so none of them can see a field carrying *where the side
    // was read from* rather than what is in it — which is the only thing a real
    // comparison ever holds two different values of. These build the two sides
    // separately, as two databases.

    /// The side as a real read of `db` hands it over: the tables, plus the
    /// address the server stamps on them.
    fn from_db(db: &str, tables: Vec<TableInfo>) -> DbSchema {
        DbSchema {
            tables,
            database: Some(db.to_string()),
            ..Default::default()
        }
    }

    fn fk(name: &str, ref_schema: &str, ref_table: &str) -> ForeignKeyInfo {
        ForeignKeyInfo {
            name: name.to_string(),
            columns: vec!["parent_id".to_string()],
            ref_schema: Some(ref_schema.to_string()),
            ref_table: ref_table.to_string(),
            ref_columns: vec!["id".to_string()],
            ..Default::default()
        }
    }

    fn child_of(db: &str) -> DbSchema {
        let mut child = table("child", &[("id", "int"), ("parent_id", "int")]);
        child.foreign_keys = vec![fk("fk_p", db, "parent")];
        from_db(db, vec![table("parent", &[("id", "int")]), child])
    }

    /// **The bug, at the field it lives in.** `REFERENCED_TABLE_SCHEMA` on
    /// MySQL/MariaDB *is* the database, so two structurally identical databases
    /// disagree about every table holding a foreign key — and the plan, ticked
    /// by default, emits `ADD CONSTRAINT fk_p … REFERENCES sc_cmp_b.parent`
    /// against `sc_cmp_a`. MariaDB 10.11.14 refuses that statement (errno 121)
    /// and MySQL 8.4.11 refuses it (1826); run as the two statements Copy hands
    /// the user, both accept it, and `sc_cmp_a.child`'s referential integrity
    /// then lives in `sc_cmp_b`.
    #[test]
    fn two_databases_holding_one_foreign_key_compare_the_same() {
        let c = mysql(child_of("sc_cmp_a"), child_of("sc_cmp_b"));
        let e = find(&c, "table:child");
        assert_eq!(e.status, ObjectStatus::Same, "changes: {:?}", e.changes);
        assert!(c.plan(|_| true).emit().is_empty());
    }

    /// And the premise, so the test above cannot pass for the wrong reason: it
    /// is the `ref_schema` field alone that used to differ, and clearing it on
    /// both sides is the control run that isolated it.
    #[test]
    fn the_foreign_key_pair_differs_only_in_where_it_was_read_from() {
        let (mut l, mut r) = (child_of("sc_cmp_a"), child_of("sc_cmp_b"));
        assert_ne!(l.tables[1].foreign_keys, r.tables[1].foreign_keys);
        for s in [&mut l, &mut r] {
            s.tables[1].foreign_keys[0].ref_schema = None;
        }
        assert_eq!(l.tables[1].foreign_keys, r.tables[1].foreign_keys);
    }

    /// **What the normalisation must not swallow, and the reason it
    /// re-addresses rather than strips.** MySQL allows a key into another
    /// database, and one of those is a real difference — but `fks_equal` reads
    /// an absent namespace as "the same one", so a right side cleared to `None`
    /// would have compared equal to *any* left namespace, turning this Differing
    /// into a Same. Rewriting `sc_cmp_b` to `sc_cmp_a` keeps the field explicit
    /// on both sides and the comparison honest.
    #[test]
    fn a_genuinely_cross_database_foreign_key_still_differs() {
        let side = |own: &str, points_at: &str| {
            let mut child = table("child", &[("id", "int"), ("parent_id", "int")]);
            child.foreign_keys = vec![fk("fk_p", points_at, "parent")];
            from_db(own, vec![child])
        };
        let c = mysql(side("sc_cmp_a", "sc_cmp_a"), side("sc_cmp_b", "warehouse"));
        assert_eq!(find(&c, "table:child").status, ObjectStatus::Differing);
    }

    /// A side that never said where it came from is left exactly as it arrived
    /// — the normaliser guesses at no address.
    #[test]
    fn a_side_with_no_recorded_database_is_not_normalised() {
        let mut anonymous = child_of("sc_cmp_a");
        anonymous.database = None;
        let c = mysql(anonymous, child_of("sc_cmp_a"));
        assert_eq!(
            find(&c, "table:child").status,
            ObjectStatus::Same,
            "both still name sc_cmp_a, so nothing differs either way"
        );
    }

    /// PostgreSQL's `ref_schema` is a real namespace inside one database — part
    /// of the object, not its address — so a difference there stays a
    /// difference whatever the sides call themselves.
    #[test]
    fn a_postgres_namespace_is_not_an_address_to_subtract() {
        let side = |ns: &str| {
            let mut child = table("child", &[("id", "int"), ("parent_id", "int")]);
            child.schema = Some("public".to_string());
            child.foreign_keys = vec![fk("fk_p", ns, "parent")];
            DbSchema {
                tables: vec![child],
                database: Some("shop".to_string()),
                ..Default::default()
            }
        };
        let c = SchemaComparison::of(&side("public"), &side("archive"), SqlDialect::Postgres);
        assert_eq!(find(&c, "table:child").status, ObjectStatus::Differing);
    }

    /// **The same bug on views, and this one nothing later refuses.**
    /// `VIEW_DEFINITION` is the server's rewritten body, qualified with the
    /// database it lives in, so two databases holding a byte-identical
    /// `CREATE VIEW` disagree — and `CREATE OR REPLACE VIEW` succeeds, leaving
    /// `sc_cmp_a.v` reading `sc_cmp_b`'s rows.
    #[test]
    fn two_databases_holding_one_view_compare_the_same() {
        let side = |db: &str| {
            from_db(
                db,
                vec![view(
                    "v",
                    &format!("select `{db}`.`t`.`id` AS `id` from `{db}`.`t`"),
                )],
            )
        };
        let c = mysql(side("sc_cmp_a"), side("sc_cmp_b"));
        let e = find(&c, "view:v");
        assert_eq!(e.status, ObjectStatus::Same, "changes: {:?}", e.changes);
        assert!(c.plan(|_| true).emit().is_empty());
    }

    /// A view body that really differs still differs — and the statement that
    /// migrates it names the database it will run against, not the one it was
    /// read from. That second half is what re-addressing buys over stripping:
    /// the emitted body is correct on the left server rather than merely
    /// unqualified.
    #[test]
    fn a_real_view_difference_survives_the_subtraction() {
        let side = |db: &str, cols: &str| {
            from_db(
                db,
                vec![view("v", &format!("select {cols} from `{db}`.`t`"))],
            )
        };
        let c = mysql(
            side("sc_cmp_a", "`sc_cmp_a`.`t`.`id` AS `id`"),
            side(
                "sc_cmp_b",
                "`sc_cmp_b`.`t`.`id` AS `id`,`sc_cmp_b`.`t`.`n` AS `n`",
            ),
        );
        assert_eq!(find(&c, "view:v").status, ObjectStatus::Differing);
        let sql = c.plan(|_| true).emit().join("\n");
        assert!(sql.contains("`n`"), "{sql}");
        assert!(!sql.contains("sc_cmp_b"), "{sql}");
        assert!(sql.contains("`sc_cmp_a`.`t`"), "{sql}");
    }

    /// **`DEFINER` is a server account, so it does not compare across servers.**
    /// Two servers' `shop.v` differ in it, and the emitted
    /// `CREATE OR REPLACE DEFINER = deploy@10.0.0.7` is *accepted* by the left
    /// server — after which every `SELECT` on the view is
    /// `ERROR 1449 … does not exist` and the view is permanently unusable.
    #[test]
    fn a_view_definer_neither_differs_nor_rides_into_the_statement() {
        let side = |definer: &str| {
            let mut v = view("v", "select `t`.`id` AS `id` from `t`");
            v.view_options = Some(ViewOptions {
                definer: Some(definer.to_string()),
                ..Default::default()
            });
            from_db("shop", vec![v])
        };
        let c = mysql(side("schemaic@localhost"), side("deploy@10.0.0.7"));
        assert_eq!(find(&c, "view:v").status, ObjectStatus::Same);

        // And when something else *does* differ, the replacement takes the
        // running account rather than the other server's.
        let mut right = side("deploy@10.0.0.7");
        right.tables[0].view_definition =
            Some("select `t`.`id` AS `id`,`t`.`n` AS `n` from `t`".to_string());
        let d = mysql(side("schemaic@localhost"), right);
        let sql = d.plan(|_| true).emit().join("\n");
        assert!(sql.contains("`n`"), "{sql}");
        assert!(!sql.contains("DEFINER"), "{sql}");
    }

    /// **The other three definers, which the same rule is about.**
    ///
    /// `TriggerInfo`, `RoutineInfo` and `EventInfo` each carry one, each
    /// restated by its emitter, and each compared by a whole-struct equality —
    /// so a two-server comparison reported every trigger, routine and event
    /// Differing although the bodies are byte-identical, and the plan (ticked by
    /// default, built for the **left** database) carried the right server's
    /// account into it.
    ///
    /// Both outcomes are bad and one destroys: without `SUPER`/`SET_USER_ID`
    /// the `CREATE` is refused with `ERROR 1227` — and MySQL DDL is not
    /// transactional, so the `DROP TRIGGER` above it has already run and the
    /// trigger is simply gone. With it, the `CREATE` succeeds and every `INSERT`
    /// into the table then fails `ERROR 1449 … does not exist`: the table is
    /// unwritable. The routine arm gives the same 1449 on every `CALL`; the
    /// event arm gives an event that never fires again — and an event has no
    /// caller, so the definer's rights are the *only* rights its body runs with.
    #[test]
    fn a_trigger_routine_or_event_definer_neither_differs_nor_rides_into_the_statement() {
        let side = |definer: &str| {
            let mut t = TableInfo {
                name: "city".to_string(),
                columns: vec![col("id", "int")],
                ..Default::default()
            };
            let mut tr = trigger("t_ins", "city", "BEGIN END");
            tr.definer = Some(definer.to_string());
            t.triggers = vec![tr];
            let mut s = from_db("shop", vec![t]);
            s.routines = vec![std::sync::Arc::new(RoutineInfo {
                name: "f".to_string(),
                definer: Some(definer.to_string()),
                body: "BEGIN RETURN 1; END".to_string(),
                ..Default::default()
            })];
            s.events = vec![std::sync::Arc::new(EventInfo {
                name: "e".to_string(),
                definer: Some(definer.to_string()),
                body: "BEGIN END".to_string(),
                ..Default::default()
            })];
            s
        };
        let c = mysql(side("schemaic@localhost"), side("deploy@10.0.0.7"));
        for key in ["trigger:city.t_ins", "function:f()", "event:e"] {
            assert_eq!(find(&c, key).status, ObjectStatus::Same, "{key}");
        }

        // And when something else *does* differ, the replacement takes the
        // running account rather than the other server's.
        let mut right = side("deploy@10.0.0.7");
        right.tables[0].triggers[0].action =
            crate::schema::TriggerAction::Body("BEGIN SET @x = 1; END".to_string());
        let d = mysql(side("schemaic@localhost"), right);
        assert_eq!(
            find(&d, "trigger:city.t_ins").status,
            ObjectStatus::Differing
        );
        assert_eq!(
            d.differences().count(),
            1,
            "only the body, and only the trigger"
        );

        // And the subtraction really reaches all four carriers, not only the
        // three the comparison happened to route through here.
        let carrying = side("deploy@10.0.0.7");
        let cleared = without_definer(&carrying);
        assert!(cleared.tables[0].triggers[0].definer.is_none());
        assert!(cleared.routines[0].definer.is_none());
        assert!(cleared.events[0].definer.is_none());
        // Nothing to clear borrows rather than clones.
        let plain = from_db("shop", vec![table("city", &[("id", "int")])]);
        assert!(matches!(
            without_definer(&plain),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    /// A column named after the database is not a qualifier: the qualifier is
    /// the one with no `.` in front of it.
    #[test]
    fn a_column_named_after_the_database_is_left_alone() {
        let out = requalify(
            "select `shop`.`t`.`shop` AS `shop` from `shop`.`t`",
            "shop",
            None,
            SqlDialect::MySql,
        );
        assert_eq!(out, "select `t`.`shop` AS `shop` from `t`");
    }

    /// The re-address half: the new name is written through the one identifier
    /// quoter, in the form the server wrote the rest of the body in.
    #[test]
    fn a_qualifier_is_rewritten_to_the_other_database() {
        let out = requalify(
            "select `sc_cmp_b`.`t`.`id` AS `id` from `sc_cmp_b`.`t`",
            "sc_cmp_b",
            Some("sc_cmp_a"),
            SqlDialect::MySql,
        );
        assert_eq!(
            out,
            "select `sc_cmp_a`.`t`.`id` AS `id` from `sc_cmp_a`.`t`"
        );
    }

    /// Nor is a string literal that spells it, which is what makes the one
    /// boundary lexer the right tool rather than a text replace.
    #[test]
    fn a_string_literal_spelling_the_database_is_not_a_qualifier() {
        let out = requalify(
            "select `shop`.`t`.`id` AS `id` from `shop`.`t` where `t`.`tag` = 'shop.x'",
            "shop",
            None,
            SqlDialect::MySql,
        );
        assert_eq!(
            out,
            "select `t`.`id` AS `id` from `t` where `t`.`tag` = 'shop.x'"
        );
    }

    /// The bare spelling, and a name that merely starts with the database's.
    #[test]
    fn stripping_takes_whole_identifiers_only() {
        assert_eq!(
            requalify(
                "select shop.t.id from shop.t",
                "shop",
                None,
                SqlDialect::MySql
            ),
            "select t.id from t"
        );
        assert_eq!(
            requalify(
                "select shopping.t.id from shopping.t",
                "shop",
                None,
                SqlDialect::MySql
            ),
            "select shopping.t.id from shopping.t"
        );
    }

    /// Nothing to subtract leaves the body untouched, byte for byte.
    #[test]
    fn a_body_with_no_qualifier_is_returned_as_it_came() {
        let body = "select `t`.`id` AS `id` from `t`";
        assert_eq!(requalify(body, "shop", None, SqlDialect::MySql), body);
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
