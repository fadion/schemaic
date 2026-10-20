//! **What uses an object** — the views, routines, triggers, events and
//! foreign keys that depend on a table, a view, a column or a routine, read
//! off the loaded schema. The schema tree's *Used by* lists them, and a drop's
//! preview names them, so the question is asked before the change rather than
//! answered by the error after it.
//!
//! **No round trip, and no catalogue of dependencies.** Every engine's schema
//! read already carries each view's definition, each routine's and trigger's
//! body and each foreign key (`body_search`'s module doc), so the answer is a
//! scan of what is loaded. That is also why it is the same on every engine:
//! only SQL Server keeps a dependency catalogue worth the name, and a
//! PostgreSQL function body is opaque to `pg_depend` anyway.
//!
//! **A mention, in code, of the object as it would be resolved** — the
//! question `dump` asks before leaving an object out of a file, through the
//! same `dump::names_object`: a whole word outside strings and comments, its
//! qualifier read so `sales.emp` is not `dbo.emp`, a bare name reaching its
//! own namespace or the default one. A column is used where a body that uses
//! its table also names the column — the nearest a text scan gets, and it errs
//! toward listing: a dynamic-SQL string naming the object is not seen, and a
//! same-named column of another table in the same body is. A body that runs
//! *on* the table — its own trigger's, or the function a PostgreSQL trigger on
//! it runs — reads the row as `NEW.total` or `inserted.total` and need not
//! name the table at all, so there the column word alone is a use.
//!
//! A foreign key is exact: its `ref_table` is the object. A trigger *on* the
//! table is listed too, since dropping the table drops it.

use crate::intel::SqlDialect;
use crate::schema::{DbSchema, TriggerAction, display_name};

/// The object asked about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target<'a> {
    /// A table or view, or — with `column` — one of its columns.
    Relation {
        ns: Option<&'a str>,
        name: &'a str,
        column: Option<&'a str>,
    },
    /// A function or procedure.
    Routine { ns: Option<&'a str>, name: &'a str },
}

/// What kind of object depends on the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DependentKind {
    View,
    Routine,
    Trigger,
    Event,
    ForeignKey,
    /// A table's column default, generated column or check constraint that
    /// calls the routine.
    Expression,
}

impl DependentKind {
    /// The kind as a list heading says it, singular and plural.
    pub fn noun(self, n: usize) -> &'static str {
        let one = n == 1;
        match self {
            DependentKind::View => {
                if one {
                    "view"
                } else {
                    "views"
                }
            }
            DependentKind::Routine => {
                if one {
                    "routine"
                } else {
                    "routines"
                }
            }
            DependentKind::Trigger => {
                if one {
                    "trigger"
                } else {
                    "triggers"
                }
            }
            DependentKind::Event => {
                if one {
                    "event"
                } else {
                    "events"
                }
            }
            DependentKind::ForeignKey => {
                if one {
                    "foreign key"
                } else {
                    "foreign keys"
                }
            }
            DependentKind::Expression => {
                if one {
                    "table expression"
                } else {
                    "table expressions"
                }
            }
        }
    }
}

/// One object that depends on the target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependent {
    pub kind: DependentKind,
    /// The dependent's own name as the schema tree shows it
    /// (`schema::display_name`).
    pub name: String,
    /// What else there is to say: the table a trigger or foreign key is on,
    /// and its columns.
    pub detail: String,
    /// It belongs to the target — a trigger on the table, a key of the table
    /// onto itself — so it goes when the target does rather than breaking
    /// ([`drop_warning`] leaves it out).
    pub goes_with_it: bool,
}

impl Dependent {
    fn new(kind: DependentKind, name: String, detail: &str) -> Dependent {
        Dependent {
            kind,
            name,
            detail: detail.to_string(),
            goes_with_it: false,
        }
    }
}

/// Everything in `schema` that uses `target`, by kind, then name.
///
/// `database` is the one `schema` was read from. Where a qualifier names a
/// database rather than a schema inside it (MySQL — [`ref_schema_is_database`]),
/// the catalogue writes it into every view body and every key's `ref_schema`,
/// while the tables themselves carry no namespace at all; a qualifier equal to
/// `database` is therefore this database, and one naming any other is not.
/// A body's qualifier is asked separately ([`body_qualifier_is_database`]),
/// because SQLite's names a database (`main.orders`) while its keys carry none.
///
/// [`ref_schema_is_database`]: crate::ddl::ref_schema_is_database
/// [`body_qualifier_is_database`]: crate::ddl::body_qualifier_is_database
pub fn used_by(
    schema: &DbSchema,
    target: Target,
    dialect: SqlDialect,
    database: &str,
) -> Vec<Dependent> {
    let same_ns = |a: Option<&str>, b: Option<&str>| match (a, b) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        (None, None) => true,
        _ => false,
    };
    let (t_ns, t_name) = match target {
        Target::Relation { ns, name, .. } | Target::Routine { ns, name } => (ns, name),
    };
    let column = match target {
        Target::Relation { column, .. } => column,
        Target::Routine { .. } => None,
    };
    // A key's `ref_schema` names a database here, and the tables carry none:
    // the database's own name there is the target's.
    let qualifier_is_database = crate::ddl::ref_schema_is_database(dialect);
    // The same of a qualifier in a body — MySQL's, and SQLite's `main.`.
    let body_qualifier_is_database = crate::ddl::body_qualifier_is_database(dialect);
    // Does `text` name the target column, as a word in code? True when the
    // target is no column.
    let names_column = |text: &str, code: &[bool]| {
        column.is_none_or(|c| !crate::intel::code_word_hits_in(text, code, c).is_empty())
    };
    // Does `text`, written in `own`, use the target?
    let uses = |text: &str, own: Option<&str>| -> bool {
        if text.is_empty() {
            return false;
        }
        let code = crate::intel::code_mask(text, dialect);
        let names = |ns| crate::dump::names_object(text, &code, dialect, own, ns, t_name);
        (names(t_ns) || (body_qualifier_is_database && t_ns.is_none() && names(Some(database))))
            && names_column(text, &code)
    };
    // Does `text` — the body of something that runs *on* the target table, so
    // reads its row as `NEW.total` or `inserted.total` without naming the
    // table — name the target column?
    let uses_column_of_own_row = |text: &str| -> bool {
        column.is_some() && names_column(text, &crate::intel::code_mask(text, dialect))
    };
    let mut out = Vec::new();
    for t in &schema.tables {
        let own = t.schema.as_deref();
        let is_target = matches!(target, Target::Relation { .. })
            && same_ns(own, t_ns)
            && t.name.eq_ignore_ascii_case(t_name);
        let shown = display_name(own, &t.name);
        if t.is_view && !is_target {
            let text = t
                .create_sql
                .as_deref()
                .or(t.view_definition.as_deref())
                .unwrap_or_default();
            if uses(text, own) {
                out.push(Dependent::new(DependentKind::View, shown.clone(), ""));
            }
        }
        if let Target::Routine { .. } = target
            && !t.is_view
        {
            let mut exprs: Vec<&str> = Vec::new();
            for c in &t.columns {
                exprs.extend(c.generated.as_deref());
                exprs.extend(c.default.as_deref());
            }
            for ck in &t.check_constraints {
                exprs.push(&ck.expression);
            }
            if exprs.iter().any(|e| uses(e, own)) {
                out.push(Dependent::new(
                    DependentKind::Expression,
                    shown.clone(),
                    "a default, generated column or check calls it",
                ));
            }
        }
        if let Target::Relation { .. } = target {
            for fk in &t.foreign_keys {
                let ref_ns = match fk.ref_schema.as_deref() {
                    Some(db) if qualifier_is_database && db.eq_ignore_ascii_case(database) => own,
                    named => named.or(own),
                };
                let onto = same_ns(ref_ns, t_ns) && fk.ref_table.eq_ignore_ascii_case(t_name);
                let on_column =
                    column.is_none_or(|c| fk.ref_columns.iter().any(|r| r.eq_ignore_ascii_case(c)));
                if onto && on_column {
                    out.push(Dependent {
                        // A key of the table onto itself goes with the table.
                        goes_with_it: is_target && column.is_none(),
                        ..Dependent::new(
                            DependentKind::ForeignKey,
                            fk.name.clone(),
                            &format!("on {shown} ({})", fk.columns.join(", ")),
                        )
                    });
                }
            }
        }
        for tr in &t.triggers {
            let fires_on_it = is_target && column.is_none();
            let calls_it = match (&tr.action, target) {
                (TriggerAction::Body(body), _) => !is_target && uses(body, own),
                (TriggerAction::Function { name, .. }, Target::Routine { .. }) => {
                    function_is(name, t_ns, t_name)
                }
                (TriggerAction::Function { .. }, Target::Relation { .. }) => false,
            };
            // A trigger on the table reads the column off its own row, and
            // its body need not name the table at all. A PostgreSQL trigger
            // has no body here: the function it runs is listed below.
            let on_column = is_target
                && column.is_some_and(|c| {
                    tr.update_columns.iter().any(|u| u.eq_ignore_ascii_case(c))
                        || matches!(&tr.action, TriggerAction::Body(b) if uses_column_of_own_row(b))
                });
            if fires_on_it || calls_it || on_column {
                out.push(Dependent {
                    goes_with_it: fires_on_it,
                    ..Dependent::new(
                        DependentKind::Trigger,
                        display_name(tr.schema.as_deref().or(own), &tr.name),
                        &format!("on {shown}"),
                    )
                });
            }
        }
    }
    // The functions triggers on the target table run (PostgreSQL), which read
    // a column as `NEW.total` and never name the table.
    let target_trigger_functions: Vec<&str> = match column {
        Some(_) => schema
            .tables
            .iter()
            .filter(|t| same_ns(t.schema.as_deref(), t_ns) && t.name.eq_ignore_ascii_case(t_name))
            .flat_map(|t| &t.triggers)
            .filter_map(|tr| match &tr.action {
                TriggerAction::Function { name, .. } => Some(name.as_str()),
                TriggerAction::Body(_) => None,
            })
            .collect(),
        None => Vec::new(),
    };
    for r in &schema.routines {
        let own = r.schema.as_deref();
        let itself = matches!(target, Target::Routine { .. })
            && same_ns(own, t_ns)
            && r.name.eq_ignore_ascii_case(t_name);
        let runs_on_target = target_trigger_functions
            .iter()
            .any(|f| function_is(f, own, &r.name));
        if !itself && (uses(&r.body, own) || (runs_on_target && uses_column_of_own_row(&r.body))) {
            out.push(Dependent::new(
                DependentKind::Routine,
                display_name(own, &r.name),
                "",
            ));
        }
    }
    for e in &schema.events {
        if uses(&e.body, e.schema.as_deref()) {
            out.push(Dependent::new(
                DependentKind::Event,
                display_name(e.schema.as_deref(), &e.name),
                "",
            ));
        }
    }
    out.sort_by(|a, b| (a.kind, &a.name, &a.detail).cmp(&(b.kind, &b.name, &b.detail)));
    out.dedup();
    out
}

/// Is a PostgreSQL trigger's function `name` — emittable SQL, quoted and
/// qualified outside `public` — the routine `ns.routine`?
///
/// Split by `schema::split_type_name`, the reader for PostgreSQL's own
/// quoted, qualified spelling: a quoted name may hold a dot (`"audit.v2"`),
/// which splitting on the last one read as `v2` in `audit`. A name it cannot
/// read is no routine.
fn function_is(name: &str, ns: Option<&str>, routine: &str) -> bool {
    let Some((qual, bare)) = crate::schema::split_type_name(name.trim()) else {
        return false;
    };
    bare.eq_ignore_ascii_case(routine)
        && match (qual.as_deref(), ns) {
            (Some(q), Some(n)) => q.eq_ignore_ascii_case(n),
            (None, n) => n.is_none_or(|n| n.eq_ignore_ascii_case("public")),
            (Some(q), None) => q.eq_ignore_ascii_case("public"),
        }
}

/// The sentence a drop's preview adds for what it will break — `None` when
/// nothing outside the object uses it (a trigger on the dropped table goes
/// with it, and is no breakage). Names the first few, then counts the rest.
pub fn drop_warning(deps: &[Dependent]) -> Option<String> {
    drop_listing(deps).map(|l| l.drop_message())
}

/// [`drop_warning`] as a value the interface words: what the drop breaks.
pub fn drop_listing(deps: &[Dependent]) -> Option<Listing> {
    listing(deps)
}

/// What a change does to a name something else may use: the object's own
/// (`column: None`) or one of its columns, by its name before the change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Touch {
    pub column: Option<String>,
    pub kind: TouchKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchKind {
    /// Renamed in place, by the engine's own rename — what follows it is
    /// [`crate::ddl::rename_follows`]'s answer.
    Rename,
    /// Renamed by building it anew under the new name: a view or routine
    /// dropped and created (the engine has no verb for the rename, or the
    /// edit needs the drop anyway), or a column inside a table rebuild, whose
    /// replayed text still names the old one. Nothing the engine rewrites on
    /// a rename is rewritten.
    Recreate,
    Drop,
    /// The column's type changed, its name did not.
    Retype,
}

/// The names `changes` take away or alter, in the plan's order — what the
/// rename and column-edit previews ask [`used_by`] about. A drop of the whole
/// object is not here: its preview asks for itself, through [`drop_warning`].
/// A column renamed *and* retyped is one rename, the name being what breaks a
/// dependent first; a type spelled in another case is no change.
///
/// `table` is the plan's own (`ChangeSet::table`), the name the server holds:
/// a view re-created under another name is renamed, though no change says
/// "rename".
pub fn touched(changes: &[crate::ddl::Change], table: &str) -> Vec<Touch> {
    use crate::ddl::Change;
    let rebuilt = changes.iter().any(|c| matches!(c, Change::RebuildTable(_)));
    let whole = |kind| Some(Touch { column: None, kind });
    changes
        .iter()
        .filter_map(|c| match c {
            Change::RenameTable { .. }
            | Change::RenameView { .. }
            | Change::RenameRoutine { .. } => whole(TouchKind::Rename),
            Change::ReplaceView {
                draft,
                recreate: true,
                ..
            } if draft.name != table => whole(TouchKind::Recreate),
            Change::ReplaceRoutine {
                draft,
                server,
                recreate: true,
            } if draft.info.name != server.name => whole(TouchKind::Recreate),
            Change::DropColumn { name, .. } => Some(Touch {
                column: Some(name.clone()),
                kind: TouchKind::Drop,
            }),
            Change::AlterColumn { from, to, .. } => {
                let kind = if from.name != to.name {
                    if rebuilt {
                        TouchKind::Recreate
                    } else {
                        TouchKind::Rename
                    }
                } else if !from.type_name.eq_ignore_ascii_case(&to.type_name) {
                    TouchKind::Retype
                } else {
                    return None;
                };
                Some(Touch {
                    column: Some(from.name.clone()),
                    kind,
                })
            }
            _ => None,
        })
        .collect()
}

/// The sentence a rename's or column edit's preview adds for `touch`, whose
/// object is called `object` — `None` when nothing outside the object uses it.
/// What goes with the object is left out as [`drop_warning`] leaves it: a
/// trigger on a table follows the table's rename on every engine.
///
/// **And so is what follows the rename** on `dialect`
/// ([`crate::ddl::rename_follows`]): a PostgreSQL view holds the table by
/// OID and SQLite rewrites one, so neither "still names orders after the
/// rename", and saying so sent the user to fix what was not broken. A
/// [`TouchKind::Recreate`] carries nothing across but a key — the one
/// dependent that names a table rather than a view or a routine, and which
/// a rebuild puts back against the new table.
pub fn change_warning(
    touch: &Touch,
    object: &str,
    deps: &[Dependent],
    dialect: SqlDialect,
) -> Option<String> {
    change_note(touch, object, deps, dialect).map(|n| n.message())
}

/// [`change_warning`] as a value the interface words ([`UsedByNote`]).
pub fn change_note(
    touch: &Touch,
    object: &str,
    deps: &[Dependent],
    dialect: SqlDialect,
) -> Option<UsedByNote> {
    let follows = |kind: DependentKind| match touch.kind {
        TouchKind::Rename => crate::ddl::rename_follows(dialect, kind),
        TouchKind::Recreate => kind == DependentKind::ForeignKey,
        TouchKind::Drop | TouchKind::Retype => false,
    };
    let deps: Vec<Dependent> = deps.iter().filter(|d| !follows(d.kind)).cloned().collect();
    let listing = listing(&deps)?;
    let (subject, name) = match &touch.column {
        Some(c) => (UsedBySubject::Column(c.clone()), c.clone()),
        None => (
            UsedBySubject::Object(object.to_string()),
            object.to_string(),
        ),
    };
    let then = match touch.kind {
        TouchKind::Rename | TouchKind::Recreate => UsedByThen::StillNames(name),
        TouchKind::Drop => UsedByThen::BreaksOnDrop,
        TouchKind::Retype => UsedByThen::CheckNewType,
    };
    Some(UsedByNote {
        subject,
        listing,
        then,
    })
}

/// What a change's *Used by* sentence is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsedBySubject {
    Column(String),
    /// The object itself, by its display name.
    Object(String),
}

/// What a change's *Used by* sentence says follows for the dependents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsedByThen {
    /// Renamed: they still name it by this, the old name.
    StillNames(String),
    BreaksOnDrop,
    CheckNewType,
}

/// One *Used by* sentence of a change's preview ([`change_warning`]) as a
/// value: `subject` is used by `listing`, and `then`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsedByNote {
    pub subject: UsedBySubject,
    pub listing: Listing,
    pub then: UsedByThen,
}

impl UsedByNote {
    /// The English.
    pub fn message(&self) -> String {
        let subject = match &self.subject {
            UsedBySubject::Column(c) => format!("Column {c}"),
            UsedBySubject::Object(o) => o.clone(),
        };
        let then = match &self.then {
            UsedByThen::StillNames(name) => format!("They still name {name} after the rename."),
            UsedByThen::BreaksOnDrop => "They break when it is dropped.".to_string(),
            UsedByThen::CheckNewType => "Check them against its new type.".to_string(),
        };
        format!("{subject} is used by {}. {then}", self.listing.message())
    }
}

/// The dependents a sentence names, as a value: how many of each kind, in
/// order, the first few names and how many more there are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing {
    pub kinds: Vec<(DependentKind, usize)>,
    pub names: Vec<String>,
    pub more: usize,
}

impl Listing {
    /// The English: `2 views and 1 routine: a, b, c`.
    pub fn message(&self) -> String {
        let counted: Vec<String> = self
            .kinds
            .iter()
            .map(|(k, n)| format!("{n} {}", k.noun(*n)))
            .collect();
        let counted = match counted.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => counted.join(""),
        };
        let more = if self.more > 0 {
            format!(" and {} more", self.more)
        } else {
            String::new()
        };
        format!("{counted}: {}{more}", self.names.join(", "))
    }

    /// The drop's sentence around it ([`drop_warning`]).
    pub fn drop_message(&self) -> String {
        format!("Used by {}.", self.message())
    }
}

/// The *Used by* sentences a preview of `plan` shows: one per name it
/// renames, drops or retypes ([`touched`]) that something in `schema` uses,
/// asked under the name **before** the plan — the server's, which is the
/// set's `table`, or a renamed routine's `from` (or, re-created under a new
/// name, its `server`). Empty when nothing does.
/// `database` is the one `schema` was read from, as [`used_by`] takes it.
pub fn change_warnings(
    schema: &DbSchema,
    plan: &crate::ddl::ChangeSet,
    database: &str,
) -> Vec<String> {
    change_notes(schema, plan, database)
        .iter()
        .map(UsedByNote::message)
        .collect()
}

/// [`change_warnings`] as values the interface words.
pub fn change_notes(
    schema: &DbSchema,
    plan: &crate::ddl::ChangeSet,
    database: &str,
) -> Vec<UsedByNote> {
    use crate::ddl::Change;
    let routine = plan.changes.iter().find_map(|c| match c {
        Change::RenameRoutine { from, .. } => Some(from),
        Change::ReplaceRoutine {
            draft,
            server,
            recreate: true,
        } if draft.info.name != server.name => Some(server),
        _ => None,
    });
    let (ns, name) = match routine {
        Some(r) => (r.schema.as_deref(), r.name.as_str()),
        None => (plan.schema.as_deref(), plan.table.as_str()),
    };
    let shown = display_name(ns, name);
    touched(&plan.changes, &plan.table)
        .iter()
        .filter_map(|t| {
            let target = match routine {
                Some(_) => Target::Routine { ns, name },
                None => Target::Relation {
                    ns,
                    name,
                    column: t.column.as_deref(),
                },
            };
            let deps = used_by(schema, target, plan.dialect, database);
            change_note(t, &shown, &deps, plan.dialect)
        })
        .collect()
}

/// `2 views and 1 routine: a, b, c` — the dependents that would break, by
/// kind and then by name, the first few named and the rest counted. `None`
/// when only what goes with the object uses it.
fn listing(deps: &[Dependent]) -> Option<Listing> {
    const NAMED: usize = 4;
    let deps: Vec<&Dependent> = deps.iter().filter(|d| !d.goes_with_it).collect();
    if deps.is_empty() {
        return None;
    }
    let mut kinds: Vec<(DependentKind, usize)> = Vec::new();
    for d in &deps {
        match kinds.iter_mut().find(|(k, _)| *k == d.kind) {
            Some((_, n)) => *n += 1,
            None => kinds.push((d.kind, 1)),
        }
    }
    let mut names: Vec<String> = deps.iter().take(NAMED).map(|d| d.name.clone()).collect();
    names.dedup();
    Some(Listing {
        kinds,
        names,
        more: deps.len().saturating_sub(NAMED),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddl::Change;
    use crate::schema::{
        CheckInfo, ColumnInfo, ForeignKeyInfo, RoutineInfo, TableInfo, TriggerInfo,
    };

    const MY: SqlDialect = SqlDialect::MySql;
    const PG: SqlDialect = SqlDialect::Postgres;

    fn table(name: &str) -> TableInfo {
        TableInfo {
            name: name.into(),
            columns: vec![ColumnInfo {
                name: "id".into(),
                type_name: "int".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn view(name: &str, sql: &str) -> TableInfo {
        TableInfo {
            is_view: true,
            view_definition: Some(sql.into()),
            ..table(name)
        }
    }

    fn routine(name: &str, body: &str) -> std::sync::Arc<RoutineInfo> {
        std::sync::Arc::new(RoutineInfo {
            name: name.into(),
            body: body.into(),
            ..Default::default()
        })
    }

    fn rel(name: &str) -> Target<'_> {
        Target::Relation {
            ns: None,
            name,
            column: None,
        }
    }

    fn names(deps: &[Dependent]) -> Vec<(DependentKind, &str)> {
        deps.iter().map(|d| (d.kind, d.name.as_str())).collect()
    }

    #[test]
    fn views_routines_and_foreign_keys_that_use_a_table_are_listed() {
        let mut child = table("order_lines");
        child.foreign_keys.push(ForeignKeyInfo {
            name: "fk_lines_order".into(),
            columns: vec!["order_id".into()],
            ref_table: "orders".into(),
            ref_columns: vec!["id".into()],
            ..Default::default()
        });
        let schema = DbSchema {
            tables: vec![
                table("orders"),
                child,
                view("big_orders", "SELECT * FROM orders WHERE total > 100"),
                view("other", "SELECT * FROM customers"),
                // A mention in a string or a comment is no use.
                view(
                    "noisy",
                    "SELECT 'orders' AS label -- orders\nFROM customers",
                ),
            ],
            routines: vec![routine("archive", "INSERT INTO hist SELECT * FROM orders")],
            ..Default::default()
        };
        assert_eq!(
            names(&used_by(&schema, rel("orders"), MY, "shop")),
            [
                (DependentKind::View, "big_orders"),
                (DependentKind::Routine, "archive"),
                (DependentKind::ForeignKey, "fk_lines_order"),
            ]
        );
    }

    /// What MySQL's catalogue actually hands back: a view body rewritten as
    /// `` `shop`.`orders` `` (`ddl::view_definition_is_qualified`) and a key's
    /// `ref_schema` holding the database (`ddl::ref_schema_is_database`), while
    /// the tables themselves have no namespace. Both name the database the
    /// schema was read from, so both are uses; the same spelling naming
    /// another database is not.
    #[test]
    fn a_mysql_catalogue_qualifier_naming_this_database_is_a_use() {
        let mut child = table("order_lines");
        child.foreign_keys.push(ForeignKeyInfo {
            name: "fk_lines_order".into(),
            columns: vec!["order_id".into()],
            ref_schema: Some("shop".into()),
            ref_table: "orders".into(),
            ref_columns: vec!["id".into()],
            ..Default::default()
        });
        let mut elsewhere = table("archive_lines");
        elsewhere.foreign_keys.push(ForeignKeyInfo {
            name: "fk_archive_order".into(),
            columns: vec!["order_id".into()],
            ref_schema: Some("archive".into()),
            ref_table: "orders".into(),
            ref_columns: vec!["id".into()],
            ..Default::default()
        });
        let schema = DbSchema {
            tables: vec![
                table("orders"),
                child,
                elsewhere,
                view(
                    "totals",
                    "select `shop`.`orders`.`id` AS `id` from `shop`.`orders`",
                ),
                view(
                    "old_totals",
                    "select `archive`.`orders`.`id` AS `id` from `archive`.`orders`",
                ),
            ],
            ..Default::default()
        };
        assert_eq!(
            names(&used_by(&schema, rel("orders"), MY, "shop")),
            [
                (DependentKind::View, "totals"),
                (DependentKind::ForeignKey, "fk_lines_order"),
            ]
        );
        // A column of it, through the same qualified spelling.
        let id = Target::Relation {
            ns: None,
            name: "orders",
            column: Some("id"),
        };
        assert_eq!(
            names(&used_by(&schema, id, MY, "shop")),
            [
                (DependentKind::View, "totals"),
                (DependentKind::ForeignKey, "fk_lines_order"),
            ]
        );
    }

    #[test]
    fn a_trigger_on_the_table_is_listed_and_one_elsewhere_that_writes_it() {
        let mut orders = table("orders");
        orders.triggers.push(TriggerInfo {
            name: "orders_audit".into(),
            table: "orders".into(),
            action: TriggerAction::Body("INSERT INTO audit VALUES (1)".into()),
            ..Default::default()
        });
        let mut lines = table("lines");
        lines.triggers.push(TriggerInfo {
            name: "lines_touch".into(),
            table: "lines".into(),
            action: TriggerAction::Body("UPDATE orders SET touched = 1".into()),
            ..Default::default()
        });
        let schema = DbSchema {
            tables: vec![orders, lines],
            ..Default::default()
        };
        let got = used_by(&schema, rel("orders"), MY, "shop");
        assert_eq!(
            names(&got),
            [
                (DependentKind::Trigger, "lines_touch"),
                (DependentKind::Trigger, "orders_audit"),
            ]
        );
        assert_eq!(got[0].detail, "on lines");
        // The table's own trigger goes with it; the other table's breaks.
        assert!(!got[0].goes_with_it && got[1].goes_with_it);
        assert_eq!(
            drop_warning(&got).as_deref(),
            Some("Used by 1 trigger: lines_touch.")
        );
    }

    #[test]
    fn another_namespaces_same_name_is_not_the_target() {
        let mut sales = view("v", "SELECT * FROM sales.emp");
        sales.schema = Some("hr".into());
        let mut bare = view("w", "SELECT * FROM emp");
        bare.schema = Some("sales".into());
        let schema = DbSchema {
            tables: vec![sales, bare],
            ..Default::default()
        };
        let hr_emp = Target::Relation {
            ns: Some("hr"),
            name: "emp",
            column: None,
        };
        assert!(used_by(&schema, hr_emp, PG, "app").is_empty());
        let sales_emp = Target::Relation {
            ns: Some("sales"),
            name: "emp",
            column: None,
        };
        assert_eq!(names(&used_by(&schema, sales_emp, PG, "app")).len(), 2);
    }

    #[test]
    fn a_column_is_used_where_its_table_and_its_name_both_are() {
        let mut child = table("lines");
        child.foreign_keys.push(ForeignKeyInfo {
            name: "fk_code".into(),
            columns: vec!["order_code".into()],
            ref_table: "orders".into(),
            ref_columns: vec!["code".into()],
            ..Default::default()
        });
        let schema = DbSchema {
            tables: vec![
                table("orders"),
                child,
                view("by_total", "SELECT id, total FROM orders"),
                view("by_code", "SELECT code FROM orders"),
            ],
            ..Default::default()
        };
        let col = |c| Target::Relation {
            ns: None,
            name: "orders",
            column: Some(c),
        };
        assert_eq!(
            names(&used_by(&schema, col("code"), MY, "shop")),
            [
                (DependentKind::View, "by_code"),
                (DependentKind::ForeignKey, "fk_code"),
            ]
        );
        assert_eq!(
            names(&used_by(&schema, col("total"), MY, "shop")),
            [(DependentKind::View, "by_total")]
        );
    }

    #[test]
    fn a_routine_is_used_by_callers_expressions_and_pg_triggers() {
        let mut t = table("items");
        t.check_constraints.push(CheckInfo {
            name: "ck".into(),
            expression: "valid_sku(sku)".into(),
            ..Default::default()
        });
        t.triggers.push(TriggerInfo {
            name: "items_trg".into(),
            table: "items".into(),
            action: TriggerAction::Function {
                name: "valid_sku".into(),
                args: vec![],
            },
            ..Default::default()
        });
        let schema = DbSchema {
            tables: vec![t, view("skus", "SELECT valid_sku(x) FROM y")],
            routines: vec![
                routine("valid_sku", "SELECT valid_sku_inner($1)"),
                routine("caller", "PERFORM valid_sku('a')"),
            ],
            ..Default::default()
        };
        let target = Target::Routine {
            ns: None,
            name: "valid_sku",
        };
        assert_eq!(
            names(&used_by(&schema, target, PG, "app")),
            [
                (DependentKind::View, "skus"),
                (DependentKind::Routine, "caller"),
                (DependentKind::Trigger, "items_trg"),
                (DependentKind::Expression, "items"),
            ]
        );
    }

    #[test]
    fn a_qualified_pg_trigger_function_matches_only_its_schema() {
        assert!(function_is("audit.\"log\"", Some("audit"), "log"));
        assert!(!function_is("audit.log", Some("public"), "log"));
        assert!(function_is("log", Some("public"), "log"));
        assert!(function_is("log", None, "log"));
    }

    #[test]
    fn the_drop_warning_counts_by_kind_and_names_the_first_few() {
        assert_eq!(drop_warning(&[]), None);
        let d = |kind, name: &str| Dependent::new(kind, name.into(), "");
        let own_trigger = Dependent {
            goes_with_it: true,
            ..d(DependentKind::Trigger, "audit")
        };
        assert_eq!(
            drop_warning(std::slice::from_ref(&own_trigger)),
            None,
            "it goes with the table"
        );
        let deps = vec![
            d(DependentKind::View, "a"),
            d(DependentKind::View, "b"),
            d(DependentKind::Routine, "c"),
            d(DependentKind::ForeignKey, "d"),
            d(DependentKind::ForeignKey, "e"),
        ];
        assert_eq!(
            drop_warning(&deps).unwrap(),
            "Used by 2 views, 1 routine and 2 foreign keys: a, b, c, d and 1 more."
        );
    }

    fn col(name: &str, ty: &str) -> Box<ColumnInfo> {
        Box::new(ColumnInfo {
            name: name.into(),
            type_name: ty.into(),
            ..Default::default()
        })
    }

    fn alter(from: Box<ColumnInfo>, to: Box<ColumnInfo>) -> Change {
        Change::AlterColumn {
            from,
            to,
            position: None,
            inline_check: None,
        }
    }

    #[test]
    fn a_rename_of_the_object_touches_it_whole() {
        assert_eq!(
            touched(&[Change::RenameTable { to: "o2".into() }], "orders"),
            vec![Touch {
                column: None,
                kind: TouchKind::Rename
            }]
        );
        assert_eq!(
            touched(&[Change::RenameView { to: "v2".into() }], "v"),
            vec![Touch {
                column: None,
                kind: TouchKind::Rename
            }]
        );
    }

    #[test]
    fn a_column_is_touched_under_its_old_name() {
        let got = touched(
            &[
                alter(col("total", "int"), col("amount", "int")),
                Change::DropColumn {
                    name: "note".into(),
                    type_name: "text".into(),
                },
                alter(col("qty", "int"), col("qty", "bigint")),
            ],
            "orders",
        );
        assert_eq!(
            got,
            vec![
                Touch {
                    column: Some("total".into()),
                    kind: TouchKind::Rename
                },
                Touch {
                    column: Some("note".into()),
                    kind: TouchKind::Drop
                },
                Touch {
                    column: Some("qty".into()),
                    kind: TouchKind::Retype
                },
            ]
        );
    }

    /// A rename that also changes the type is reported once, as the rename —
    /// the name is what breaks a dependent first.
    #[test]
    fn a_renamed_and_retyped_column_is_one_rename() {
        assert_eq!(
            touched(&[alter(col("a", "int"), col("b", "bigint"))], "t"),
            vec![Touch {
                column: Some("a".into()),
                kind: TouchKind::Rename
            }]
        );
    }

    /// Nothing that leaves the column's name and type alone — a default, a
    /// nullability, a type spelled in another case, an added column.
    #[test]
    fn an_edit_that_keeps_name_and_type_touches_nothing() {
        let mut nullable = col("a", "int");
        nullable.nullable = true;
        assert_eq!(touched(&[alter(col("a", "int"), nullable)], "t"), vec![]);
        assert_eq!(
            touched(&[alter(col("a", "int"), col("a", "INT"))], "t"),
            vec![]
        );
        assert_eq!(
            touched(
                &[Change::AddColumn {
                    column: col("z", "int"),
                    position: None,
                }],
                "t"
            ),
            vec![]
        );
        assert_eq!(touched(&[], "t"), vec![]);
    }

    #[test]
    fn the_change_warning_says_what_the_change_does_to_its_users() {
        let d = |kind, name: &str| Dependent::new(kind, name.into(), "");
        let deps = vec![d(DependentKind::View, "a"), d(DependentKind::Routine, "p")];
        let warn = |column: Option<&str>, kind| {
            change_warning(
                &Touch {
                    column: column.map(Into::into),
                    kind,
                },
                "orders",
                &deps,
                MY,
            )
            .unwrap()
        };
        assert_eq!(
            warn(None, TouchKind::Rename),
            "orders is used by 1 view and 1 routine: a, p. They still name orders after the rename."
        );
        assert_eq!(
            warn(Some("total"), TouchKind::Rename),
            "Column total is used by 1 view and 1 routine: a, p. They still name total after the rename."
        );
        assert_eq!(
            warn(Some("note"), TouchKind::Drop),
            "Column note is used by 1 view and 1 routine: a, p. They break when it is dropped."
        );
        assert_eq!(
            warn(Some("qty"), TouchKind::Retype),
            "Column qty is used by 1 view and 1 routine: a, p. Check them against its new type."
        );
    }

    /// A foreign key is held by the server against the object, not its name:
    /// renaming the table it points at, or the column, re-points the key on
    /// every engine (checked live on MariaDB, MySQL 8, PostgreSQL, SQL Server
    /// and SQLite). So a rename does not leave a key naming the old name, and
    /// saying it does was false. A drop or a retype still reaches the key.
    #[test]
    fn a_rename_does_not_claim_a_foreign_key_keeps_the_old_name() {
        let d = |kind, name: &str| Dependent::new(kind, name.into(), "");
        let fk_only = vec![d(DependentKind::ForeignKey, "fk_lines")];
        let rename_in = |dialect, column: Option<&str>, deps: &[Dependent]| {
            change_warning(
                &Touch {
                    column: column.map(Into::into),
                    kind: TouchKind::Rename,
                },
                "orders",
                deps,
                dialect,
            )
        };
        let rename = |column: Option<&str>, deps: &[Dependent]| rename_in(MY, column, deps);
        for dialect in SqlDialect::ALL {
            assert_eq!(rename_in(dialect, None, &fk_only), None, "{dialect:?}");
            assert_eq!(
                rename_in(dialect, Some("id"), &fk_only),
                None,
                "{dialect:?}"
            );
        }
        let both = vec![
            d(DependentKind::View, "v"),
            d(DependentKind::ForeignKey, "fk_lines"),
        ];
        assert_eq!(
            rename(None, &both).as_deref(),
            Some("orders is used by 1 view: v. They still name orders after the rename.")
        );
        let drop = change_warning(
            &Touch {
                column: None,
                kind: TouchKind::Drop,
            },
            "orders",
            &fk_only,
            MY,
        );
        assert_eq!(
            drop.as_deref(),
            Some("orders is used by 1 foreign key: fk_lines. They break when it is dropped.")
        );
    }

    fn plan(table: &str, changes: Vec<Change>) -> crate::ddl::ChangeSet {
        crate::ddl::ChangeSet {
            table: table.into(),
            schema: None,
            dialect: MY,
            flavour: crate::schema::ServerFlavour::Unknown,
            changes,
        }
    }

    /// The composition the previews call: a plan's touched names, each asked
    /// of the loaded schema under its name *before* the plan — the server's
    /// name, which is the set's `table`, or a routine's `from`.
    #[test]
    fn a_plan_is_warned_about_by_what_uses_the_names_it_changes() {
        let mut orders = table("orders");
        orders.columns.push(ColumnInfo {
            name: "total".into(),
            type_name: "int".into(),
            ..Default::default()
        });
        let schema = DbSchema {
            tables: vec![
                orders,
                view("big", "SELECT id, total FROM orders WHERE total > 9"),
                view("ids", "SELECT id FROM orders"),
            ],
            routines: vec![routine("cap", "UPDATE orders SET total = 0")],
            ..Default::default()
        };
        assert_eq!(
            change_warnings(
                &schema,
                &plan("orders", vec![Change::RenameTable { to: "o2".into() }]),
                "shop"
            ),
            vec![
                "orders is used by 2 views and 1 routine: big, ids, cap. \
                 They still name orders after the rename."
                    .to_string()
            ]
        );
        // The column, not the table: `ids` names `orders` but not `total`.
        assert_eq!(
            change_warnings(
                &schema,
                &plan(
                    "orders",
                    vec![alter(col("total", "int"), col("amount", "int"))]
                ),
                "shop"
            ),
            vec![
                "Column total is used by 1 view and 1 routine: big, cap. \
                 They still name total after the rename."
                    .to_string()
            ]
        );
        assert_eq!(
            change_warnings(
                &schema,
                &plan("orders", vec![alter(col("id", "int"), col("id", "bigint"))]),
                "shop"
            ),
            vec![
                "Column id is used by 2 views: big, ids. Check them against its new type."
                    .to_string()
            ]
        );
        // What nothing uses says nothing, and a plan touching no name asks nothing.
        assert!(
            change_warnings(
                &schema,
                &plan("ids", vec![Change::RenameView { to: "x".into() }]),
                "shop"
            )
            .is_empty()
        );
        assert!(change_warnings(&schema, &plan("orders", vec![]), "shop").is_empty());
    }

    #[test]
    fn a_routine_rename_asks_after_the_routine_by_its_old_name() {
        let schema = DbSchema {
            tables: vec![table("t")],
            routines: vec![
                routine("calc", "SELECT 1"),
                routine("caller", "CALL calc()"),
            ],
            ..Default::default()
        };
        let from = Box::new(RoutineInfo {
            name: "calc".into(),
            ..Default::default()
        });
        assert_eq!(
            change_warnings(
                &schema,
                &plan(
                    "calc",
                    vec![Change::RenameRoutine {
                        from,
                        to: "calc2".into()
                    }]
                ),
                "shop"
            ),
            vec![
                "calc is used by 1 routine: caller. They still name calc after the rename."
                    .to_string()
            ]
        );
    }

    /// A trigger on the table follows a rename of it on every engine, so it is
    /// no breakage — as it is none for a drop.
    #[test]
    fn the_change_warning_leaves_out_what_goes_with_the_object() {
        let own_trigger = Dependent {
            goes_with_it: true,
            ..Dependent::new(DependentKind::Trigger, "audit".into(), "")
        };
        let touch = Touch {
            column: None,
            kind: TouchKind::Rename,
        };
        for dialect in SqlDialect::ALL {
            let own = std::slice::from_ref(&own_trigger);
            assert_eq!(change_warning(&touch, "orders", own, dialect), None);
            assert_eq!(change_warning(&touch, "orders", &[], dialect), None);
        }
    }

    const LT: SqlDialect = SqlDialect::Sqlite;
    const MS: SqlDialect = SqlDialect::MsSql;

    fn plan_in(dialect: SqlDialect, table: &str, changes: Vec<Change>) -> crate::ddl::ChangeSet {
        crate::ddl::ChangeSet {
            dialect,
            ..plan(table, changes)
        }
    }

    fn with_total(name: &str) -> TableInfo {
        let mut t = table(name);
        t.columns.push(ColumnInfo {
            name: "total".into(),
            type_name: "int".into(),
            ..Default::default()
        });
        t
    }

    fn body_trigger(name: &str, on: &str, body: &str) -> TriggerInfo {
        TriggerInfo {
            name: name.into(),
            table: on.into(),
            action: TriggerAction::Body(body.into()),
            ..Default::default()
        }
    }

    fn total_of(name: &str) -> Target<'_> {
        Target::Relation {
            ns: None,
            name,
            column: Some("total"),
        }
    }

    /// A trigger on the table reads its columns as `NEW.total` (MySQL,
    /// SQLite) or through `inserted` (SQL Server) and never names its own
    /// table, so asking the body to name the table first missed every one —
    /// and MySQL and SQL Server do not rewrite a trigger body when the column
    /// is renamed or dropped.
    #[test]
    fn a_trigger_on_the_table_is_listed_for_a_column_its_body_uses() {
        let cases = [
            (MY, "SET NEW.total = NEW.total * 1.2"),
            (
                MS,
                "SET NOCOUNT ON; IF EXISTS (SELECT 1 FROM inserted WHERE inserted.total < 0) \
                 THROW 50000, 'negative', 1;",
            ),
            (
                LT,
                "BEGIN SELECT RAISE(ABORT, 'negative') WHERE NEW.total < 0; END",
            ),
        ];
        for (dialect, body) in cases {
            let mut orders = with_total("orders");
            orders
                .triggers
                .push(body_trigger("orders_tax", "orders", body));
            let schema = DbSchema {
                tables: vec![orders],
                ..Default::default()
            };
            let got = used_by(&schema, total_of("orders"), dialect, "shop");
            assert_eq!(
                names(&got),
                [(DependentKind::Trigger, "orders_tax")],
                "{dialect:?}"
            );
            // It breaks with the column rather than going with it.
            assert!(!got[0].goes_with_it, "{dialect:?}");
            // A column its body does not name is not one it uses.
            let id = Target::Relation {
                ns: None,
                name: "orders",
                column: Some("id"),
            };
            assert!(
                used_by(&schema, id, dialect, "shop").is_empty(),
                "{dialect:?}"
            );
        }
    }

    /// PostgreSQL's trigger runs a function, and the function's body — not the
    /// trigger — is what reads `NEW.total`, never naming the table. It is the
    /// function a rename or a drop of the column breaks, so it is the one
    /// listed; a function a trigger on another table runs is not.
    #[test]
    fn a_pg_trigger_function_on_the_table_is_listed_for_a_column_it_uses() {
        let fires = |table: &str, function: &str| TriggerInfo {
            name: format!("{table}_trg"),
            table: table.into(),
            action: TriggerAction::Function {
                name: function.into(),
                args: vec![],
            },
            ..Default::default()
        };
        let mut orders = with_total("orders");
        orders.triggers.push(fires("orders", "orders_tax"));
        let mut lines = with_total("lines");
        lines.triggers.push(fires("lines", "lines_tax"));
        let body = "BEGIN NEW.total := NEW.total * 1.2; RETURN NEW; END";
        let schema = DbSchema {
            tables: vec![orders, lines],
            routines: vec![routine("orders_tax", body), routine("lines_tax", body)],
            ..Default::default()
        };
        assert_eq!(
            names(&used_by(&schema, total_of("orders"), PG, "app")),
            [(DependentKind::Routine, "orders_tax")]
        );
    }

    /// SQL Server's spellings of a qualified name: bracketed, and three-part
    /// with the database in front — and a bare name inside a module in another
    /// schema, which still reaches `dbo`, the default.
    #[test]
    fn sql_server_qualifiers_are_read_as_its_catalogue_writes_them() {
        let in_ns = |mut t: TableInfo, ns: &str| {
            t.schema = Some(ns.into());
            t
        };
        let schema = DbSchema {
            tables: vec![
                in_ns(table("orders"), "dbo"),
                in_ns(
                    view(
                        "bracketed",
                        "CREATE VIEW [dbo].[bracketed] AS SELECT id FROM [dbo].[orders]",
                    ),
                    "dbo",
                ),
                in_ns(
                    view(
                        "three_part",
                        "CREATE VIEW three_part AS SELECT id FROM shop.dbo.orders",
                    ),
                    "dbo",
                ),
                in_ns(
                    view(
                        "other_schema",
                        "CREATE VIEW sales.other_schema AS SELECT id FROM [sales].[orders]",
                    ),
                    "sales",
                ),
            ],
            routines: vec![std::sync::Arc::new(RoutineInfo {
                name: "bare_in_sales".into(),
                schema: Some("sales".into()),
                body: "SELECT id FROM orders".into(),
                ..Default::default()
            })],
            ..Default::default()
        };
        let dbo = Target::Relation {
            ns: Some("dbo"),
            name: "orders",
            column: None,
        };
        assert_eq!(
            names(&used_by(&schema, dbo, MS, "shop")),
            [
                (DependentKind::View, "dbo.bracketed"),
                (DependentKind::View, "dbo.three_part"),
                (DependentKind::Routine, "sales.bare_in_sales"),
            ]
        );
    }

    /// SQLite's qualifier names a database — `main`, or an attached one — as
    /// MySQL's does, while its tables carry no namespace at all; `main.orders`
    /// is the table, and `aux.orders` is not.
    #[test]
    fn a_sqlite_qualifier_naming_this_database_is_a_use() {
        let mut other = table("other");
        other.triggers.push(body_trigger(
            "other_log",
            "other",
            "BEGIN INSERT INTO log SELECT id FROM main.orders; END",
        ));
        let schema = DbSchema {
            tables: vec![
                table("orders"),
                other,
                view("v", "CREATE VIEW v AS SELECT * FROM main.orders"),
                view(
                    "elsewhere",
                    "CREATE VIEW elsewhere AS SELECT * FROM aux.orders",
                ),
            ],
            ..Default::default()
        };
        assert_eq!(
            names(&used_by(&schema, rel("orders"), LT, "main")),
            [
                (DependentKind::View, "v"),
                (DependentKind::Trigger, "other_log"),
            ]
        );
    }

    fn rename_table(dialect: SqlDialect, schema: &DbSchema) -> Vec<String> {
        change_warnings(
            schema,
            &plan_in(
                dialect,
                "orders",
                vec![Change::RenameTable { to: "o2".into() }],
            ),
            "shop",
        )
    }

    /// PostgreSQL holds a view against the table's OID, not its name, and
    /// SQLite's `ALTER TABLE … RENAME` rewrites every view and trigger that
    /// names it — so on those two a view does not "still name orders after the
    /// rename", while on MySQL and SQL Server it does.
    #[test]
    fn a_rename_warns_only_about_dependents_that_do_not_follow_it() {
        let mut lines = table("lines");
        lines.triggers.push(body_trigger(
            "lines_touch",
            "lines",
            "UPDATE orders SET id = id",
        ));
        let schema = DbSchema {
            tables: vec![
                with_total("orders"),
                lines,
                view("big", "SELECT id, total FROM orders"),
            ],
            routines: vec![routine("cap", "UPDATE orders SET total = 0")],
            ..Default::default()
        };
        assert_eq!(
            rename_table(MY, &schema),
            vec![
                "orders is used by 1 view, 1 routine and 1 trigger: big, cap, lines_touch. \
                 They still name orders after the rename."
                    .to_string()
            ]
        );
        // PostgreSQL: the view follows by OID; the function body is text.
        assert_eq!(
            rename_table(PG, &schema),
            vec![
                "orders is used by 1 routine: cap. They still name orders after the rename."
                    .to_string()
            ]
        );
        // SQLite: the rename rewrites the view and the trigger alike (and it
        // stores no routines).
        let schema = DbSchema {
            routines: vec![],
            ..schema
        };
        assert!(rename_table(LT, &schema).is_empty());
        // A column renamed by SQLite's own `RENAME COLUMN` follows too ...
        let rename_total = alter(col("total", "int"), col("amount", "int"));
        assert!(
            change_warnings(
                &schema,
                &plan_in(LT, "orders", vec![rename_total.clone()]),
                "shop"
            )
            .is_empty()
        );
        // ... but not one the twelve-step rebuild carries, whose replayed
        // text still names the old column.
        let orders = with_total("orders");
        let rebuild = Change::RebuildTable(Box::new(crate::ddl::Rebuild {
            current: orders.clone(),
            draft: crate::ddl::TableDraft::from_table(&orders),
        }));
        assert_eq!(
            change_warnings(
                &schema,
                &plan_in(LT, "orders", vec![rebuild, rename_total]),
                "shop"
            ),
            vec![
                "Column total is used by 1 view: big. They still name total after the rename."
                    .to_string()
            ]
        );
    }

    /// A PostgreSQL function renamed in place keeps every view, key default
    /// and trigger bound to it by OID; only another function's body, which is
    /// text, still calls it by the old name.
    #[test]
    fn a_pg_routine_rename_warns_only_about_bodies_that_call_it() {
        let mut items = table("items");
        items.check_constraints.push(CheckInfo {
            name: "ck".into(),
            expression: "valid_sku(sku)".into(),
            ..Default::default()
        });
        items.triggers.push(TriggerInfo {
            name: "items_trg".into(),
            table: "items".into(),
            action: TriggerAction::Function {
                name: "valid_sku".into(),
                args: vec![],
            },
            ..Default::default()
        });
        let schema = DbSchema {
            tables: vec![items, view("skus", "SELECT valid_sku(x) FROM y")],
            routines: vec![
                routine("valid_sku", "SELECT 1"),
                routine("caller", "PERFORM valid_sku('a')"),
            ],
            ..Default::default()
        };
        let from = Box::new(RoutineInfo {
            name: "valid_sku".into(),
            ..Default::default()
        });
        assert_eq!(
            change_warnings(
                &schema,
                &plan_in(
                    PG,
                    "valid_sku",
                    vec![Change::RenameRoutine {
                        from,
                        to: "sku_ok".into()
                    }]
                ),
                "app"
            ),
            vec![
                "valid_sku is used by 1 routine: caller. They still name valid_sku after the rename."
                    .to_string()
            ]
        );
    }

    /// SQL Server and SQLite have no verb for a view rename, so the view
    /// editor drops the view and creates it under the new name — one
    /// `ReplaceView { recreate: true }` and no `RenameView`. Every view that
    /// named the old one still does, on every engine, and SQL Server lets the
    /// `DROP VIEW` through under a view that is not schema-bound.
    #[test]
    fn a_view_renamed_by_dropping_and_creating_it_is_warned_about() {
        for dialect in [MS, LT] {
            let v1 = view("v1", "CREATE VIEW v1 AS SELECT id FROM t");
            let schema = DbSchema {
                tables: vec![
                    table("t"),
                    v1.clone(),
                    view("v2", "CREATE VIEW v2 AS SELECT * FROM v1"),
                ],
                ..Default::default()
            };
            let mut draft = crate::ddl::ViewDraft::from_table(&v1).unwrap();
            draft.name = "v1b".into();
            let cs = crate::ddl::diff_view(&v1, &draft, dialect);
            assert!(
                !cs.changes
                    .iter()
                    .any(|c| matches!(c, Change::RenameView { .. })),
                "{dialect:?}: {:?}",
                cs.changes
            );
            assert_eq!(
                change_warnings(&schema, &cs, "main"),
                vec!["v1 is used by 1 view: v2. They still name v1 after the rename.".to_string()],
                "{dialect:?}"
            );
        }
    }

    /// MySQL and SQL Server rename a routine by dropping it and creating it
    /// under the new name, and the callers still name the old one.
    #[test]
    fn a_routine_renamed_by_dropping_and_creating_it_is_warned_about() {
        for dialect in [MY, MS] {
            let calc = RoutineInfo {
                name: "calc".into(),
                body: "SELECT 1".into(),
                ..Default::default()
            };
            let schema = DbSchema {
                tables: vec![table("t")],
                routines: vec![
                    std::sync::Arc::new(calc.clone()),
                    routine("caller", "CALL calc()"),
                ],
                ..Default::default()
            };
            let mut draft = crate::ddl::RoutineDraft::from_info(&calc);
            draft.info.name = "calc2".into();
            let cs = crate::ddl::diff_routine(&calc, &draft, dialect);
            assert_eq!(
                change_warnings(&schema, &cs, "shop"),
                vec![
                    "calc is used by 1 routine: caller. They still name calc after the rename."
                        .to_string()
                ],
                "{dialect:?}"
            );
        }
    }

    /// The trigger's function name is emittable SQL, and a quoted name may
    /// hold a dot: `"audit.v2"` is one name in the default schema, not `v2`
    /// in `audit`.
    #[test]
    fn a_quoted_trigger_function_with_a_dot_is_still_the_routine() {
        assert!(function_is("\"audit.v2\"", Some("public"), "audit.v2"));
        assert!(!function_is("\"audit.v2\"", Some("audit"), "v2"));
        assert!(function_is("s.\"f.g\"", Some("s"), "f.g"));
        assert!(function_is("\"a\"\"b\"", None, "a\"b"));
        // A name it cannot read is no routine, rather than a guess.
        assert!(!function_is("\"unterminated", None, "unterminated"));
    }

    /// The two capabilities this module asks, answered per engine.
    #[test]
    fn which_dependents_follow_a_rename_and_which_qualifiers_name_the_database() {
        use crate::ddl::{body_qualifier_is_database, rename_follows};
        use DependentKind as K;
        for d in SqlDialect::ALL {
            assert!(rename_follows(d, K::ForeignKey), "{d:?}");
        }
        for k in [K::View, K::Routine, K::Trigger, K::Event, K::Expression] {
            assert!(!rename_follows(MY, k), "{k:?}");
            assert!(!rename_follows(MS, k), "{k:?}");
        }
        for k in [K::View, K::Trigger, K::Expression] {
            assert!(rename_follows(PG, k), "{k:?}");
        }
        assert!(!rename_follows(PG, K::Routine));
        assert!(rename_follows(LT, K::View) && rename_follows(LT, K::Trigger));
        assert!(!rename_follows(LT, K::Routine));
        assert!(body_qualifier_is_database(MY) && body_qualifier_is_database(LT));
        assert!(!body_qualifier_is_database(PG) && !body_qualifier_is_database(MS));
        // Not the key's question flipped: SQLite's keys still carry no database.
        assert!(!crate::ddl::ref_schema_is_database(LT));
    }
}
