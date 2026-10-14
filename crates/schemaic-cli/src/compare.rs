//! `schemaic compare`'s decisions — which objects the script covers and what
//! is said about it — apart from the reading, so they are tested without a
//! server.
//!
//! **No differ of its own.** The comparison is `core::compare`'s, the one the
//! app's *Compare with* builds: [`SchemaComparison::of`] over two freshly
//! introspected schemas, and [`SchemaComparison::plan`] for the script. What
//! this adds is the command line's half: the `--table` names resolved to
//! comparison entries, and the plan's disclosures turned into stderr lines.
//! Nothing here runs anything; the script is printed for a person to read.

use schemaic_core::compare::{CompareEntry, CompareKind, SchemaComparison};

/// What `compare` prints: the script for stdout — `None` when there is
/// nothing to change — and the lines for stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub script: Option<String>,
    pub notes: Vec<String>,
}

/// The script that makes `target` match `source`, over every difference or
/// only the `tables` named (each with the triggers on it, as a comparison
/// opened from one table ticks them), and what to say about it.
///
/// `Err` is a `--table` that names nothing on either side — the command's
/// mistake, said before anything is printed.
pub fn report(
    cmp: &SchemaComparison,
    tables: &[String],
    target: &str,
    source: &str,
) -> Result<Report, String> {
    // The named objects as (namespace, name), and the keys their plan takes:
    // each object and the triggers on it, `selectable_keys_of`'s rule.
    let mut keys = std::collections::HashSet::new();
    let mut named: Vec<(Option<&str>, &str)> = Vec::new();
    for t in tables {
        let e = resolve(cmp, t)?;
        keys.extend(cmp.selectable_keys_of(e.schema.as_deref(), &e.name));
        named.push((e.schema.as_deref(), &e.name));
    }
    let in_scope = |e: &CompareEntry| {
        tables.is_empty()
            || named.iter().any(|&(s, n)| {
                e.schema.as_deref() == s
                    && match &e.table {
                        Some(t) => t == n,
                        None => e.name == n,
                    }
            })
    };
    let plan = if tables.is_empty() {
        cmp.plan(|_| true)
    } else {
        cmp.plan(|e| keys.contains(&e.key()))
    };
    // **Only the objects in scope.** `plan.omitted` covers the whole
    // comparison, so a trigger left out on another table was listed under "no
    // differences in a". Built here from the same two predicates `plan` keeps
    // them out by; the comparison's own notes (about reading, not one object)
    // follow as `plan.omitted` carries them.
    let left_out: Vec<String> = cmp
        .differences()
        .filter(|e| (e.needs_source() || e.unplannable()) && in_scope(e))
        .map(CompareEntry::omission_note)
        .collect();
    let scope = if tables.is_empty() {
        String::new()
    } else {
        format!(" in {}", tables.join(", "))
    };
    let mut notes = Vec::new();
    if plan.is_empty() && left_out.is_empty() {
        notes.push(format!(
            "no differences{scope}: {target} already matches {source}"
        ));
    } else if plan.is_empty() {
        // **Apart, though nothing can be scripted.** "No differences" over a
        // `left out:` line naming one was a contradiction — the app's compare
        // says the same thing apart (`AgreesExceptNoted`).
        notes.push(format!(
            "nothing to script{scope}: {target} differs from {source} only in what is left out below"
        ));
    } else {
        notes.push(format!(
            "the script makes {target} match {source}: {} — it has not been run",
            plan.subject()
        ));
    }
    notes.extend(
        plan.destructive()
            .into_iter()
            .map(|d| format!("destructive: {d}")),
    );
    notes.extend(
        plan.unsupported()
            .into_iter()
            .map(|u| format!("not in the script: {u}")),
    );
    notes.extend(left_out.iter().map(|o| format!("left out: {o}")));
    notes.extend(cmp.notes.iter().map(|n| format!("note: {n}")));
    let script = (!plan.is_empty()).then(|| plan.export_script());
    Ok(Report { script, notes })
}

/// The table or view a `--table` names, on either side: as the comparison
/// labels it (`orders`, `sales.orders`), else spelled with its namespace
/// (`public.orders`, which a label leaves bare), else by bare name if only one
/// namespace has it.
fn resolve<'a>(cmp: &'a SchemaComparison, name: &str) -> Result<&'a CompareEntry, String> {
    let same = |a: &str, b: &str| {
        if cmp.names_ignore_case {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    let objects: Vec<&CompareEntry> = cmp
        .entries
        .iter()
        .filter(|e| matches!(e.kind, CompareKind::Table | CompareKind::View) && e.table.is_none())
        .collect();
    if let Some(e) = objects.iter().find(|e| same(&e.label(), name)) {
        return Ok(e);
    }
    if let Some(e) = objects.iter().find(|e| {
        e.schema
            .as_deref()
            .is_some_and(|s| same(&format!("{s}.{}", e.name), name))
    }) {
        return Ok(e);
    }
    let bare: Vec<&&CompareEntry> = objects.iter().filter(|e| same(&e.name, name)).collect();
    match bare.as_slice() {
        [e] => Ok(e),
        [] => Err(format!(
            "no table or view named '{name}' in either database"
        )),
        several => Err(format!(
            "'{name}' names a table in more than one schema ({}); qualify it",
            several
                .iter()
                .map(|e| e.label())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::schema::{ColumnInfo, DbSchema, TableInfo};

    fn table(schema: Option<&str>, name: &str, cols: &[&str]) -> TableInfo {
        TableInfo {
            schema: schema.map(str::to_string),
            name: name.to_string(),
            columns: cols
                .iter()
                .map(|c| ColumnInfo {
                    name: c.to_string(),
                    type_name: "integer".to_string(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    fn db(tables: Vec<TableInfo>) -> DbSchema {
        DbSchema {
            tables,
            ..Default::default()
        }
    }

    fn sqlite(target: Vec<TableInfo>, source: Vec<TableInfo>) -> SchemaComparison {
        SchemaComparison::of(&db(target), &db(source), SqlDialect::Sqlite)
    }

    fn run(cmp: &SchemaComparison, tables: &[&str]) -> Result<Report, String> {
        let tables: Vec<String> = tables.iter().map(|t| t.to_string()).collect();
        report(cmp, &tables, "shop", "shop_staging")
    }

    /// **Two databases that agree print nothing on stdout** — an empty
    /// script is the answer, and a script made of a header alone would read
    /// as a migration to run.
    #[test]
    fn agreeing_databases_print_no_script() {
        let t = || vec![table(None, "a", &["id"])];
        let r = run(&sqlite(t(), t()), &[]).unwrap();
        assert_eq!(r.script, None);
        assert_eq!(
            r.notes,
            ["no differences: shop already matches shop_staging"]
        );
    }

    /// The script is the target made to match the source: what only the
    /// source has is created there, and stderr says nothing ran.
    #[test]
    fn the_script_brings_the_target_up_to_the_source() {
        let cmp = sqlite(
            vec![table(None, "a", &["id"])],
            vec![table(None, "a", &["id"]), table(None, "b", &["id"])],
        );
        let r = run(&cmp, &[]).unwrap();
        let script = r.script.expect("a difference to script");
        assert!(script.contains("CREATE TABLE"), "{script}");
        assert!(script.contains('b'), "{script}");
        assert!(
            r.notes[0].contains("1 object") && r.notes[0].contains("not been run"),
            "{:?}",
            r.notes
        );
    }

    /// **What only the target has is dropped, and that is said** — the line
    /// a person reads before running a script that loses a table's rows.
    #[test]
    fn a_drop_is_called_destructive() {
        let cmp = sqlite(
            vec![table(None, "a", &["id"]), table(None, "old", &["id"])],
            vec![table(None, "a", &["id"])],
        );
        let r = run(&cmp, &[]).unwrap();
        assert!(r.script.unwrap().contains("DROP TABLE"));
        assert!(
            r.notes
                .iter()
                .any(|n| n.starts_with("destructive: ") && n.contains("old")),
            "{:?}",
            r.notes
        );
    }

    /// **`--table` narrows the script to the objects named**, and the others'
    /// differences stay out of it rather than riding along.
    #[test]
    fn named_tables_narrow_the_script() {
        let cmp = sqlite(
            vec![],
            vec![table(None, "keep", &["id"]), table(None, "skip", &["id"])],
        );
        let script = run(&cmp, &["keep"]).unwrap().script.unwrap();
        assert!(script.contains("keep"), "{script}");
        assert!(!script.contains("skip"), "{script}");
    }

    /// A named table that matches already is no difference, said for that
    /// table rather than for the whole database.
    #[test]
    fn a_named_table_that_matches_is_said_to() {
        let cmp = sqlite(
            vec![table(None, "a", &["id"])],
            vec![table(None, "a", &["id"]), table(None, "b", &["id"])],
        );
        let r = run(&cmp, &["a"]).unwrap();
        assert_eq!(r.script, None);
        assert_eq!(
            r.notes,
            ["no differences in a: shop already matches shop_staging"]
        );
    }

    /// **A `--table` that names nothing is refused**, not quietly a plan of
    /// nothing — a typo would otherwise read as "they agree".
    #[test]
    fn an_unknown_table_is_refused() {
        let cmp = sqlite(vec![table(None, "a", &["id"])], vec![]);
        let err = run(&cmp, &["nope"]).unwrap_err();
        assert!(err.contains("nope"), "{err}");
    }

    /// A MySQL table carrying a trigger: a body `core::compare` keeps out of
    /// every plan (`needs_source`) and discloses instead.
    fn with_trigger(name: &str, trigger: &str) -> TableInfo {
        use schemaic_core::schema::{TriggerAction, TriggerInfo};
        TableInfo {
            triggers: vec![TriggerInfo {
                name: trigger.to_string(),
                table: name.to_string(),
                action: TriggerAction::Body("BEGIN SET @a = 1; END".to_string()),
                ..Default::default()
            }],
            ..table(None, name, &["id"])
        }
    }

    fn mysql(target: Vec<TableInfo>, source: Vec<TableInfo>) -> SchemaComparison {
        SchemaComparison::of(&db(target), &db(source), SqlDialect::MySql)
    }

    /// **A difference the script cannot carry is still a difference.** With
    /// a MySQL trigger the only thing apart, the plan is empty — and stderr
    /// said "no differences: … already matches …" over a `left out:` line
    /// naming the trigger that does not match. The app's compare says the
    /// same thing apart (`AgreesExceptNoted`); so must this.
    #[test]
    fn differences_only_left_out_are_not_called_no_differences() {
        let cmp = mysql(
            vec![table(None, "a", &["id"])],
            vec![with_trigger("a", "t_ins")],
        );
        let r = run(&cmp, &[]).unwrap();
        assert_eq!(r.script, None);
        assert!(!r.notes[0].starts_with("no differences"), "{:?}", r.notes);
        assert!(r.notes[0].contains("left out"), "{:?}", r.notes);
        assert!(
            r.notes
                .iter()
                .any(|n| n.starts_with("left out: ") && n.contains("t_ins")),
            "{:?}",
            r.notes
        );
    }

    /// **`--table` narrows what is said as well as what is scripted.** A
    /// trigger left out on another table is not about the table named, and
    /// listing it under "no differences in a" read as a contradiction.
    #[test]
    fn named_tables_narrow_the_left_out_lines_too() {
        let cmp = mysql(
            vec![table(None, "a", &["id"]), table(None, "b", &["id"])],
            vec![table(None, "a", &["id"]), with_trigger("b", "t_b")],
        );
        let r = run(&cmp, &["a"]).unwrap();
        assert_eq!(
            r.notes,
            ["no differences in a: shop already matches shop_staging"]
        );
        let r = run(&cmp, &["b"]).unwrap();
        assert!(r.notes.iter().any(|n| n.contains("t_b")), "{:?}", r.notes);
    }

    fn pg(target: Vec<TableInfo>, source: Vec<TableInfo>) -> SchemaComparison {
        SchemaComparison::of(&db(target), &db(source), SqlDialect::Postgres)
    }

    /// On PostgreSQL a name may be qualified the way the listings print it,
    /// or with `public.` spelled out; a bare name two schemas share is asked
    /// to be qualified rather than guessed.
    #[test]
    fn a_namespaced_table_resolves_by_label_or_qualified_name() {
        let source = vec![
            table(Some("public"), "orders", &["id"]),
            table(Some("sales"), "orders", &["id"]),
        ];
        let cmp = pg(vec![], source);
        for name in ["sales.orders", "public.orders", "orders"] {
            let got = run(&cmp, &[name]);
            if name == "orders" {
                // `orders` is public's label, so it is not ambiguous.
                assert!(got.unwrap().script.unwrap().contains("orders"));
                continue;
            }
            let script = got.unwrap().script.unwrap();
            let other = if name == "sales.orders" {
                "public"
            } else {
                "sales"
            };
            assert!(!script.contains(&format!("{other}.")), "{name}: {script}");
        }
        let cmp = pg(
            vec![],
            vec![
                table(Some("a"), "t", &["id"]),
                table(Some("b"), "t", &["id"]),
            ],
        );
        let err = run(&cmp, &["t"]).unwrap_err();
        assert!(err.contains("a.t") && err.contains("b.t"), "{err}");
    }
}
