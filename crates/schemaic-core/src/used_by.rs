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
//! same-named column of another table in the same body is.
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
pub fn used_by(schema: &DbSchema, target: Target, dialect: SqlDialect) -> Vec<Dependent> {
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
    // Does `text`, written in `own`, use the target?
    let uses = |text: &str, own: Option<&str>| -> bool {
        if text.is_empty() {
            return false;
        }
        let code = crate::intel::code_mask(text, dialect);
        crate::dump::names_object(text, &code, dialect, own, t_ns, t_name)
            && column.is_none_or(|c| !crate::intel::code_word_hits_in(text, &code, c).is_empty())
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
                let ref_ns = fk.ref_schema.as_deref().or(own);
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
            let on_column = is_target
                && column.is_some_and(|c| {
                    tr.update_columns.iter().any(|u| u.eq_ignore_ascii_case(c))
                        || matches!(&tr.action, TriggerAction::Body(b) if uses(b, own))
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
    for r in &schema.routines {
        let own = r.schema.as_deref();
        let itself = matches!(target, Target::Routine { .. })
            && same_ns(own, t_ns)
            && r.name.eq_ignore_ascii_case(t_name);
        if !itself && uses(&r.body, own) {
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
fn function_is(name: &str, ns: Option<&str>, routine: &str) -> bool {
    let unquote = |s: &str| s.trim().trim_matches('"').replace("\"\"", "\"");
    let (qual, bare) = match name.rsplit_once('.') {
        Some((q, b)) => (Some(unquote(q)), unquote(b)),
        None => (None, unquote(name)),
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
    Some(format!("Used by {}.", listing(deps)?))
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
    Rename,
    Drop,
    /// The column's type changed, its name did not.
    Retype,
}

/// The names `changes` take away or alter, in the plan's order — what the
/// rename and column-edit previews ask [`used_by`] about. A drop of the whole
/// object is not here: its preview asks for itself, through [`drop_warning`].
/// A column renamed *and* retyped is one rename, the name being what breaks a
/// dependent first; a type spelled in another case is no change.
pub fn touched(changes: &[crate::ddl::Change]) -> Vec<Touch> {
    use crate::ddl::Change;
    changes
        .iter()
        .filter_map(|c| match c {
            Change::RenameTable { .. }
            | Change::RenameView { .. }
            | Change::RenameRoutine { .. } => Some(Touch {
                column: None,
                kind: TouchKind::Rename,
            }),
            Change::DropColumn { name, .. } => Some(Touch {
                column: Some(name.clone()),
                kind: TouchKind::Drop,
            }),
            Change::AlterColumn { from, to, .. } => {
                let kind = if from.name != to.name {
                    TouchKind::Rename
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
pub fn change_warning(touch: &Touch, object: &str, deps: &[Dependent]) -> Option<String> {
    let listed = listing(deps)?;
    let (subject, name) = match &touch.column {
        Some(c) => (format!("Column {c}"), c.as_str()),
        None => (object.to_string(), object),
    };
    let then = match touch.kind {
        TouchKind::Rename => format!("They still name {name} after the rename."),
        TouchKind::Drop => "They break when it is dropped.".to_string(),
        TouchKind::Retype => "Check them against its new type.".to_string(),
    };
    Some(format!("{subject} is used by {listed}. {then}"))
}

/// The *Used by* sentences a preview of `plan` shows: one per name it
/// renames, drops or retypes ([`touched`]) that something in `schema` uses,
/// asked under the name **before** the plan — the server's, which is the
/// set's `table`, or a renamed routine's `from`. Empty when nothing does.
pub fn change_warnings(schema: &DbSchema, plan: &crate::ddl::ChangeSet) -> Vec<String> {
    use crate::ddl::Change;
    let routine = plan.changes.iter().find_map(|c| match c {
        Change::RenameRoutine { from, .. } => Some(from),
        _ => None,
    });
    let (ns, name) = match routine {
        Some(r) => (r.schema.as_deref(), r.name.as_str()),
        None => (plan.schema.as_deref(), plan.table.as_str()),
    };
    let shown = display_name(ns, name);
    touched(&plan.changes)
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
            change_warning(t, &shown, &used_by(schema, target, plan.dialect))
        })
        .collect()
}

/// `2 views and 1 routine: a, b, c` — the dependents that would break, by
/// kind and then by name, the first few named and the rest counted. `None`
/// when only what goes with the object uses it.
fn listing(deps: &[Dependent]) -> Option<String> {
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
    let counted: Vec<String> = kinds
        .iter()
        .map(|(k, n)| format!("{n} {}", k.noun(*n)))
        .collect();
    let mut names: Vec<&str> = deps.iter().take(NAMED).map(|d| d.name.as_str()).collect();
    let more = deps.len().saturating_sub(NAMED);
    let more = (more > 0).then(|| format!(" and {more} more"));
    names.dedup();
    let counted = match counted.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => counted.join(""),
    };
    Some(format!(
        "{counted}: {}{}",
        names.join(", "),
        more.unwrap_or_default()
    ))
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
            names(&used_by(&schema, rel("orders"), MY)),
            [
                (DependentKind::View, "big_orders"),
                (DependentKind::Routine, "archive"),
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
        let got = used_by(&schema, rel("orders"), MY);
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
        assert!(used_by(&schema, hr_emp, PG).is_empty());
        let sales_emp = Target::Relation {
            ns: Some("sales"),
            name: "emp",
            column: None,
        };
        assert_eq!(names(&used_by(&schema, sales_emp, PG)).len(), 2);
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
            names(&used_by(&schema, col("code"), MY)),
            [
                (DependentKind::View, "by_code"),
                (DependentKind::ForeignKey, "fk_code"),
            ]
        );
        assert_eq!(
            names(&used_by(&schema, col("total"), MY)),
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
            names(&used_by(&schema, target, PG)),
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
            touched(&[Change::RenameTable { to: "o2".into() }]),
            vec![Touch {
                column: None,
                kind: TouchKind::Rename
            }]
        );
        assert_eq!(
            touched(&[Change::RenameView { to: "v2".into() }]),
            vec![Touch {
                column: None,
                kind: TouchKind::Rename
            }]
        );
    }

    #[test]
    fn a_column_is_touched_under_its_old_name() {
        let got = touched(&[
            alter(col("total", "int"), col("amount", "int")),
            Change::DropColumn {
                name: "note".into(),
                type_name: "text".into(),
            },
            alter(col("qty", "int"), col("qty", "bigint")),
        ]);
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
            touched(&[alter(col("a", "int"), col("b", "bigint"))]),
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
        assert_eq!(touched(&[alter(col("a", "int"), nullable)]), vec![]);
        assert_eq!(touched(&[alter(col("a", "int"), col("a", "INT"))]), vec![]);
        assert_eq!(
            touched(&[Change::AddColumn {
                column: col("z", "int"),
                position: None,
            }]),
            vec![]
        );
        assert_eq!(touched(&[]), vec![]);
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
                &plan("orders", vec![Change::RenameTable { to: "o2".into() }])
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
                )
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
                &plan("orders", vec![alter(col("id", "int"), col("id", "bigint"))])
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
                &plan("ids", vec![Change::RenameView { to: "x".into() }])
            )
            .is_empty()
        );
        assert!(change_warnings(&schema, &plan("orders", vec![])).is_empty());
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
                )
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
        assert_eq!(change_warning(&touch, "orders", &[own_trigger]), None);
        assert_eq!(change_warning(&touch, "orders", &[]), None);
    }
}
