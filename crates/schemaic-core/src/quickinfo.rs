//! The object a name in the editor names — a table, a view, a column or a
//! routine — resolved against the loaded schemas, for the hover's quick-info
//! ([`describe`]) and Ctrl+click's go to definition. [`name_at`] is the one
//! answer both read.
//!
//! The statement's own scope decides what a bare or qualified name means
//! ([`intel::statement_scope`], the AST's where the statement parses): an
//! alias names its table, a bare word a table in scope or one of their
//! columns. What no loaded schema holds resolves to nothing — the database is
//! the semantic authority, and a guess here would open the wrong object.

use crate::intel::{self, SqlDialect, TableRef, TkKind};
use crate::schema::{DbSchema, RoutineKind, TableInfo};

/// What [`name_at`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamedObject {
    /// A table or a view.
    Table {
        database: String,
        schema: Option<String>,
        table: String,
        view: bool,
    },
    Column {
        database: String,
        schema: Option<String>,
        table: String,
        column: String,
    },
    Routine {
        database: String,
        schema: Option<String>,
        name: String,
        kind: RoutineKind,
    },
}

/// A name under the pointer and what it names: `span` is the part of the
/// dotted name the offset is on (`o` of `o.total`), which is what a hover
/// underlines and a tip anchors to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameHit {
    pub span: (usize, usize),
    pub object: NamedObject,
}

/// The loaded schemas a name may resolve in, each with its database's name.
pub type Loaded<'a> = [(&'a str, &'a DbSchema)];

/// What the name at `offset` names, or `None` — off a name, inside a string
/// or comment, or naming nothing a loaded schema holds.
pub fn name_at(
    sql: &str,
    offset: usize,
    dialect: SqlDialect,
    loaded: &Loaded<'_>,
    active_db: Option<&str>,
) -> Option<NameHit> {
    let offset = offset.min(sql.len());
    let (lo, hi) = crate::sql::statement_range(sql, offset, dialect);
    if offset < lo || offset > hi || intel::caret_in_literal_or_comment(sql, lo, offset, dialect) {
        return None;
    }
    let toks = intel::tokenize_range(sql, lo, hi, dialect);
    let word = |i: usize| match toks.get(i).map(|t| &t.kind) {
        Some(TkKind::Word(w)) => Some(w.as_str()),
        _ => None,
    };
    let dot = |i: usize| matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Dot));
    // The word the offset is on — or just after, as a caret placed by a click
    // at a word's end is.
    let k = toks
        .iter()
        .position(|t| matches!(t.kind, TkKind::Word(_)) && t.at <= offset && offset < t.end)
        .or_else(|| {
            toks.iter()
                .position(|t| matches!(t.kind, TkKind::Word(_)) && t.end == offset)
        })?;
    // The whole dotted chain around it.
    let mut first = k;
    while first >= 2 && dot(first - 1) && word(first - 2).is_some() {
        first -= 2;
    }
    let mut last = k;
    while dot(last + 1) && word(last + 2).is_some() {
        last += 2;
    }
    let parts: Vec<&str> = (first..=last).step_by(2).filter_map(word).collect();
    let pos = (k - first) / 2;
    let called = matches!(toks.get(last + 1).map(|t| &t.kind), Some(TkKind::LParen));
    let span = (toks[k].at, toks[k].end);
    let scope = intel::statement_scope(sql, lo, hi, offset, dialect).tables;
    let r = Resolver {
        loaded,
        active_db,
        scope: &scope,
    };
    let object = r.resolve(&parts[..=pos], called && pos + 1 == parts.len())?;
    Some(NameHit { span, object })
}

/// The quick-info for `object`, as lines of plain text: what it is and its
/// name, then its type and constraints or its signature, then its comment.
pub fn describe(object: &NamedObject, loaded: &Loaded<'_>) -> Option<String> {
    let schema_of = |db: &str| loaded.iter().find(|(d, _)| *d == db).map(|(_, s)| *s);
    let named = |schema: &Option<String>, name: &str| match schema {
        Some(s) => format!("{s}.{name}"),
        None => name.to_string(),
    };
    let mut lines = Vec::new();
    match object {
        NamedObject::Table {
            database,
            schema,
            table,
            view,
        } => {
            let t = find_table(schema_of(database)?, schema.as_deref(), table)?;
            let what = if *view { "view" } else { "table" };
            lines.push(format!("{what} {} · {database}", named(schema, &t.name)));
            let n = t.columns.len();
            lines.push(format!("{n} column{}", if n == 1 { "" } else { "s" }));
            lines.extend(t.comment.clone().filter(|c| !c.trim().is_empty()));
        }
        NamedObject::Column {
            database,
            schema,
            table,
            column,
        } => {
            let t = find_table(schema_of(database)?, schema.as_deref(), table)?;
            let c = t
                .columns
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(column))?;
            lines.push(format!(
                "{}.{}  {}",
                named(schema, &t.name),
                c.name,
                c.type_name
            ));
            let mut traits = Vec::new();
            if c.primary_key {
                traits.push("primary key".to_string());
            }
            traits.push(if c.nullable { "null" } else { "not null" }.to_string());
            if c.auto_increment {
                traits.push("auto increment".to_string());
            }
            if let Some(d) = &c.default {
                traits.push(format!("default {d}"));
            }
            if let Some(g) = &c.generated {
                traits.push(format!("generated as {g}"));
            }
            lines.push(traits.join(" · "));
            lines.extend(c.comment.clone().filter(|c| !c.trim().is_empty()));
        }
        NamedObject::Routine {
            database,
            schema,
            name,
            ..
        } => {
            let r = schema_of(database)?.routines.iter().find(|r| {
                r.name.eq_ignore_ascii_case(name)
                    && (schema.is_none() || r.schema.as_deref() == schema.as_deref())
            })?;
            let kind = match r.kind {
                RoutineKind::Function => "function",
                RoutineKind::Procedure => "procedure",
            };
            let mut sig = format!("{kind} {}({})", named(&r.schema, &r.name), r.arguments);
            if !r.returns.trim().is_empty() {
                sig.push_str(&format!(" returns {}", r.returns));
            }
            lines.push(sig);
        }
    }
    Some(lines.join("\n"))
}

/// A table by name in one schema, the exact spelling first and then any case;
/// with no namespace written, any namespace (`public`/`dbo` first, as
/// [`DbSchema::find_table`] prefers).
fn find_table<'s>(db: &'s DbSchema, schema: Option<&str>, name: &str) -> Option<&'s TableInfo> {
    db.find_table(schema, name).or_else(|| {
        db.tables.iter().find(|t| {
            t.name.eq_ignore_ascii_case(name)
                && schema.is_none_or(|s| {
                    t.schema
                        .as_deref()
                        .is_some_and(|ts| ts.eq_ignore_ascii_case(s))
                })
        })
    })
}

struct Resolver<'a, 'b> {
    loaded: &'a Loaded<'b>,
    active_db: Option<&'a str>,
    scope: &'a [TableRef],
}

impl Resolver<'_, '_> {
    fn db(&self, name: &str) -> Option<(&str, &DbSchema)> {
        self.loaded
            .iter()
            .find(|(d, _)| d.eq_ignore_ascii_case(name))
            .map(|(d, s)| (*d, *s))
    }

    fn active(&self) -> Option<(&str, &DbSchema)> {
        self.active_db.and_then(|d| self.db(d))
    }

    /// A table `name`, in namespace `ns` — which is a database where one is
    /// loaded by that name (MySQL's `db.t`), else a schema of the active one.
    fn table(&self, ns: Option<&str>, name: &str) -> Option<NamedObject> {
        let (database, t) = match ns {
            Some(q) => match self
                .db(q)
                .and_then(|(d, s)| Some((d, find_table(s, None, name)?)))
            {
                Some(hit) => hit,
                None => {
                    let (d, s) = self.active()?;
                    (d, find_table(s, Some(q), name)?)
                }
            },
            None => {
                let (d, s) = self.active()?;
                (d, find_table(s, None, name)?)
            }
        };
        Some(NamedObject::Table {
            database: database.to_string(),
            schema: t.schema.clone(),
            table: t.name.clone(),
            view: t.is_view,
        })
    }

    /// The table a scope entry names.
    fn scoped(&self, r: &TableRef) -> Option<NamedObject> {
        self.table(r.db.as_deref(), &r.name)
    }

    /// The scope entry a qualifier names: its alias, or its name where it has
    /// none written.
    fn qualifier(&self, q: &str) -> Option<&TableRef> {
        self.scope
            .iter()
            .find(|r| {
                r.alias
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(q))
            })
            .or_else(|| self.scope.iter().find(|r| r.name.eq_ignore_ascii_case(q)))
    }

    fn column_of(&self, table: NamedObject, column: &str) -> Option<NamedObject> {
        let NamedObject::Table {
            database,
            schema,
            table,
            ..
        } = table
        else {
            return None;
        };
        let s = self.db(&database)?.1;
        let t = find_table(s, schema.as_deref(), &table)?;
        let c = t
            .columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(column))?;
        Some(NamedObject::Column {
            database,
            schema,
            table,
            column: c.name.clone(),
        })
    }

    fn routine(&self, ns: Option<&str>, name: &str) -> Option<NamedObject> {
        let (database, s, schema) = match ns {
            Some(q) => match self.db(q) {
                Some((d, s)) => (d, s, None),
                None => {
                    let (d, s) = self.active()?;
                    (d, s, Some(q))
                }
            },
            None => {
                let (d, s) = self.active()?;
                (d, s, None)
            }
        };
        let r = s.routines.iter().find(|r| {
            r.name.eq_ignore_ascii_case(name)
                && schema.is_none_or(|q| {
                    r.schema
                        .as_deref()
                        .is_some_and(|rs| rs.eq_ignore_ascii_case(q))
                })
        })?;
        Some(NamedObject::Routine {
            database: database.to_string(),
            schema: r.schema.clone(),
            name: r.name.clone(),
            kind: r.kind,
        })
    }

    /// What `parts` — a dotted name up to the part the offset is on — names.
    fn resolve(&self, parts: &[&str], called: bool) -> Option<NamedObject> {
        match parts {
            [name] if called => self.routine(None, name),
            [name] => self
                .qualifier(name)
                .and_then(|r| self.scoped(r))
                .or_else(|| self.table(None, name))
                .or_else(|| {
                    // Only where exactly one table in scope holds it: the
                    // server refuses one two tables hold, and naming either
                    // would be a guess. A table joined to itself is one.
                    let mut found: Vec<NamedObject> = Vec::new();
                    for c in self
                        .scope
                        .iter()
                        .filter_map(|r| self.scoped(r))
                        .filter_map(|t| self.column_of(t, name))
                    {
                        if !found.contains(&c) {
                            found.push(c);
                        }
                    }
                    (found.len() == 1).then(|| found.remove(0))
                }),
            [q, name] if called => self.routine(Some(q), name),
            [q, name] => match self.qualifier(q).and_then(|r| self.scoped(r)) {
                Some(t) => self.column_of(t, name),
                None => self.table(Some(q), name),
            },
            [a, b, name] => self
                .table(Some(a), b)
                .and_then(|t| self.column_of(t, name))
                // SQL Server's `db.schema.table`.
                .or_else(|| {
                    let (d, s) = self.db(a)?;
                    let t = find_table(s, Some(b), name)?;
                    Some(NamedObject::Table {
                        database: d.to_string(),
                        schema: t.schema.clone(),
                        table: t.name.clone(),
                        view: t.is_view,
                    })
                }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ColumnInfo, RoutineInfo};
    use std::sync::Arc;

    fn col(name: &str, ty: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            type_name: ty.to_string(),
            ..ColumnInfo::default()
        }
    }

    fn shop() -> DbSchema {
        let mut id = col("id", "int");
        id.primary_key = true;
        let mut total = col("total", "decimal(10,2)");
        total.comment = Some("Gross, with tax".to_string());
        total.default = Some("0".to_string());
        DbSchema {
            tables: vec![
                TableInfo {
                    name: "orders".to_string(),
                    columns: vec![id, total, col("customer_id", "int")],
                    comment: Some("One row a sale".to_string()),
                    ..TableInfo::default()
                },
                TableInfo {
                    name: "customers".to_string(),
                    columns: vec![col("id", "int"), col("name", "varchar(80)")],
                    ..TableInfo::default()
                },
                TableInfo {
                    name: "big_orders".to_string(),
                    columns: vec![col("id", "int")],
                    is_view: true,
                    ..TableInfo::default()
                },
            ],
            routines: vec![Arc::new(RoutineInfo {
                name: "tax".to_string(),
                kind: RoutineKind::Function,
                arguments: "amount decimal".to_string(),
                returns: "decimal".to_string(),
                ..RoutineInfo::default()
            })],
            ..DbSchema::default()
        }
    }

    fn at(sql: &str, needle: &str, nth: usize) -> Option<NameHit> {
        let s = shop();
        let loaded = [("shop", &s)];
        let off = sql.match_indices(needle).nth(nth).unwrap().0 + 1;
        name_at(sql, off, SqlDialect::MySql, &loaded, Some("shop"))
    }

    fn column(table: &str, column: &str) -> NamedObject {
        NamedObject::Column {
            database: "shop".to_string(),
            schema: None,
            table: table.to_string(),
            column: column.to_string(),
        }
    }

    fn table(name: &str, view: bool) -> NamedObject {
        NamedObject::Table {
            database: "shop".to_string(),
            schema: None,
            table: name.to_string(),
            view,
        }
    }

    #[test]
    fn an_alias_and_its_column_resolve_through_the_statements_scope() {
        let sql = "select o.total, c.name from orders o join customers c on c.id = o.customer_id";
        assert_eq!(
            at(sql, "total", 0).unwrap().object,
            column("orders", "total")
        );
        assert_eq!(
            at(sql, "name", 0).unwrap().object,
            column("customers", "name")
        );
        // The qualifier itself names its table, and the hit is that part.
        let hit = at(sql, "o.", 0).unwrap();
        assert_eq!(hit.object, table("orders", false));
        assert_eq!(&sql[hit.span.0..hit.span.1], "o");
        // A table in FROM, and a view.
        assert_eq!(
            at(sql, "customers", 0).unwrap().object,
            table("customers", false)
        );
        let v = "select * from big_orders";
        assert_eq!(
            at(v, "big_orders", 0).unwrap().object,
            table("big_orders", true)
        );
    }

    #[test]
    fn a_bare_column_is_found_among_the_tables_in_scope() {
        let sql = "select customer_id from orders";
        assert_eq!(
            at(sql, "customer_id", 0).unwrap().object,
            column("orders", "customer_id")
        );
        // In a database-qualified name too.
        let sql = "select * from shop.orders";
        assert_eq!(at(sql, "orders", 0).unwrap().object, table("orders", false));
    }

    /// **A bare column two tables in scope both hold names neither** — the
    /// server would refuse it as ambiguous, and Ctrl+click on it opened the
    /// first table's, a guess. The same table twice is still one.
    #[test]
    fn an_ambiguous_bare_column_names_nothing() {
        let sql = "select id from orders o join customers c on c.id = o.customer_id";
        assert_eq!(at(sql, "id", 0), None);
        // Qualified, it is no longer ambiguous.
        assert_eq!(at(sql, "id", 1).unwrap().object, column("customers", "id"));
        let twice = "select total from orders a join orders b on a.id = b.id";
        assert_eq!(
            at(twice, "total", 0).unwrap().object,
            column("orders", "total")
        );
    }

    #[test]
    fn a_call_names_a_routine() {
        let sql = "select tax(total) from orders";
        assert_eq!(
            at(sql, "tax", 0).unwrap().object,
            NamedObject::Routine {
                database: "shop".to_string(),
                schema: None,
                name: "tax".to_string(),
                kind: RoutineKind::Function,
            }
        );
    }

    #[test]
    fn nothing_resolves_off_a_known_name() {
        // Unknown, a keyword, inside a string, inside a comment.
        assert_eq!(at("select nope from orders", "nope", 0), None);
        assert_eq!(at("select 1 from orders", "elect", 0), None);
        assert_eq!(at("select 'orders' from orders", "orders", 0), None);
        assert_eq!(at("select 1 -- orders\nfrom orders", "orders", 0), None);
        // No loaded schema at all.
        assert_eq!(
            name_at(
                "select * from orders",
                15,
                SqlDialect::MySql,
                &[],
                Some("shop")
            ),
            None
        );
    }

    #[test]
    fn the_quick_info_says_what_the_object_is() {
        let s = shop();
        let loaded = [("shop", &s)];
        assert_eq!(
            describe(&column("orders", "total"), &loaded).unwrap(),
            "orders.total  decimal(10,2)\nnot null · default 0\nGross, with tax"
        );
        assert_eq!(
            describe(&column("orders", "id"), &loaded).unwrap(),
            "orders.id  int\nprimary key · not null"
        );
        assert_eq!(
            describe(&table("orders", false), &loaded).unwrap(),
            "table orders · shop\n3 columns\nOne row a sale"
        );
        let tax = at("select tax(1)", "tax", 0).unwrap().object;
        assert_eq!(
            describe(&tax, &loaded).unwrap(),
            "function tax(amount decimal) returns decimal"
        );
        assert_eq!(describe(&table("gone", false), &loaded), None);
    }
}
