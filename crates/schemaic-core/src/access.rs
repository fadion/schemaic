//! **Who may use what a replay drops and recreates** — the owner, grants and
//! row security a `DROP` takes with a table or a routine, read for a dump that
//! drops ([`ReplayAccess`], carried on `DbSchema::replay_access`) and put back
//! by the statements here.
//!
//! A dump's *Drop before create* replays the file onto a database that already
//! holds it, and on two servers the drop takes more than the object:
//!
//! - **PostgreSQL**: a recreated table or view belongs to whoever replayed,
//!   with the default ACL, row security off and no policies — so an
//!   application role lost `SELECT` (or kept it through `PUBLIC` while the
//!   policies that limited it were gone), under a success report.
//! - **SQL Server**: a recreated table or view holds no permission at all,
//!   and belongs to its schema's owner.
//! - **MySQL 8**: a routine has no `CREATE OR REPLACE`, and its `DROP`
//!   revokes every account's grant on it (`automatic_sp_privileges`), so the
//!   next call failed with *execute command denied*. MariaDB replaces in place
//!   and keeps them; a MySQL table's grants outlive its `DROP` in
//!   `mysql.tables_priv` and come back with it.
//!
//! Read at dump time, so what is put back is what the source held then; a
//! replay onto a server that lacks one of the roles or accounts stops at the
//! statement naming it, as `pg_dump`'s output without `--no-acl` does.

use crate::intel::SqlDialect;
use crate::schema::RoutineKind;

/// What [`crate::dump::plan`] puts back after a replay's drops, where the
/// dump read it — `None` in either half where it was not read: another
/// engine, a file that drops nothing, or (MySQL's `mysql.procs_priv`) an
/// account without the privilege to see it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayAccess {
    /// PostgreSQL and SQL Server: every table and view in the database (a
    /// materialized view and a foreign table too, on PostgreSQL).
    pub tables: Option<Vec<TableAccess>>,
    /// MySQL 8: every account's grant on a routine in the database.
    pub routines: Option<Vec<RoutineGrant>>,
}

impl ReplayAccess {
    /// The access read for one relation, by namespace and name.
    pub fn table(&self, schema: Option<&str>, name: &str) -> Option<&TableAccess> {
        self.tables
            .as_ref()?
            .iter()
            .find(|t| t.schema.as_deref() == schema && t.name == name)
    }

    /// The grants read for one routine, in read order — its name compared
    /// ignoring case, as MySQL compares a routine's.
    pub fn routine_grants<'a>(
        &'a self,
        name: &'a str,
        kind: RoutineKind,
    ) -> impl Iterator<Item = &'a RoutineGrant> + 'a {
        let folded = name.to_lowercase();
        self.routines
            .iter()
            .flatten()
            .filter(move |g| g.kind == kind && g.name.to_lowercase() == folded)
    }
}

/// **Does a table's `DROP` take its grants with it** on `dialect`, so that a
/// replay's recreated table comes back without them? PostgreSQL's and SQL
/// Server's do — an ACL and a permission belong to the object, not its name.
/// MySQL keeps a dropped table's privileges in `mysql.tables_priv` and they
/// apply again to the table recreated under its name; SQLite has no grants.
///
/// The dump's header asks it to say what a replay puts back, or that it
/// could not.
pub fn drop_resets_table_access(dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::Postgres | SqlDialect::MsSql => true,
        SqlDialect::MySql | SqlDialect::Sqlite => false,
    }
}

/// One privilege on a relation or one of its columns, as `aclexplode` or
/// `sys.database_permissions` reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Grant {
    /// The role it is granted to; `None` is `PUBLIC`.
    pub grantee: Option<String>,
    /// `SELECT`, `INSERT`, … as the catalogue spells it.
    pub privilege: String,
    /// Held `WITH GRANT OPTION`.
    pub grantable: bool,
    /// A column privilege's column; `None` for the relation's own.
    pub column: Option<String>,
    /// Granted, denied or revoked — the last two SQL Server's alone.
    pub state: GrantState,
}

/// What a [`Grant`] row says: SQL Server records a `DENY`, and a column's
/// `REVOKE` beneath a table-wide grant, as rows of their own
/// (`sys.database_permissions.state` `D` and `R`). PostgreSQL's ACL holds
/// grants only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GrantState {
    #[default]
    Grant,
    Deny,
    Revoke,
}

impl GrantState {
    /// `sys.database_permissions.state` — `G` and `W` (with grant option)
    /// are grants, `D` a deny, `R` a revoke.
    pub fn from_mssql(state: &str) -> GrantState {
        match state {
            "D" => GrantState::Deny,
            "R" => GrantState::Revoke,
            _ => GrantState::Grant,
        }
    }

    fn verb(self) -> (&'static str, &'static str) {
        match self {
            GrantState::Grant => ("GRANT", "TO"),
            GrantState::Deny => ("DENY", "TO"),
            GrantState::Revoke => ("REVOKE", "FROM"),
        }
    }
}

/// One row-security policy on a table (`pg_policy`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    pub name: String,
    /// `AS PERMISSIVE` (the default) rather than `AS RESTRICTIVE`.
    pub permissive: bool,
    /// `ALL`, `SELECT`, `INSERT`, `UPDATE` or `DELETE` — see
    /// [`pg_policy_command`].
    pub command: String,
    /// The roles it applies to, in order; `None` is `PUBLIC`.
    pub roles: Vec<Option<String>>,
    /// `USING`'s expression, as `pg_get_expr` prints it.
    pub using: Option<String>,
    /// `WITH CHECK`'s expression.
    pub check: Option<String>,
}

/// A relation's owner, grants and row security — PostgreSQL's whole, SQL
/// Server's without the last (its row-level security is a policy object of
/// its own, which a schema-bound one keeps the table from being dropped
/// under).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableAccess {
    pub schema: Option<String>,
    pub name: String,
    /// PostgreSQL's always; SQL Server's empty where the object belongs to
    /// its schema's owner (`sys.objects.principal_id` NULL), the usual case.
    pub owner: String,
    /// `relacl IS NULL`: the relation holds the default ACL, which a fresh
    /// `CREATE` gives it again, so nothing is revoked or granted for it.
    pub default_acl: bool,
    /// The relation's own privileges and its columns', in read order.
    pub grants: Vec<Grant>,
    pub row_security: bool,
    pub force_row_security: bool,
    pub policies: Vec<Policy>,
}

impl TableAccess {
    /// The statements that give a relation recreated as `qname` back what
    /// this read held.
    ///
    /// - **PostgreSQL** (`keyword` the relation's: `TABLE`, `VIEW`,
    ///   `MATERIALIZED VIEW`, `FOREIGN TABLE`): its owner; where its ACL was
    ///   not the default, `REVOKE ALL` from `PUBLIC` and the owner and every
    ///   grant again, as `pg_dump` writes it, so a privilege the owner had
    ///   given up stays given up; its column grants; its row security and
    ///   policies.
    /// - **SQL Server**: its owner where it is not its schema's
    ///   (`ALTER AUTHORIZATION`), and each `GRANT`, `DENY` and column
    ///   `REVOKE` on it — a new object holds none, so nothing is revoked
    ///   first.
    ///
    /// Empty on MySQL, whose table grants outlive a `DROP`
    /// ([`drop_resets_table_access`]), and on SQLite.
    pub fn restate_sql(&self, qname: &str, keyword: &str, dialect: SqlDialect) -> Vec<String> {
        let pg = match dialect {
            SqlDialect::Postgres => true,
            SqlDialect::MsSql => false,
            SqlDialect::MySql | SqlDialect::Sqlite => return Vec::new(),
        };
        let role = |r: &Option<String>| match r {
            Some(r) => crate::export::ident_sql(r, dialect),
            None => "PUBLIC".to_string(),
        };
        // `GRANT … ON TABLE` addresses every PostgreSQL relation kind, a
        // view's too; T-SQL's object class is `OBJECT::`.
        let on = if pg {
            format!("TABLE {qname}")
        } else {
            format!("OBJECT::{qname}")
        };
        let owner = crate::export::ident_sql(&self.owner, dialect);
        let mut out = Vec::new();
        if pg {
            out.push(format!("ALTER {keyword} {qname} OWNER TO {owner};"));
            if !self.default_acl {
                out.push(format!("REVOKE ALL ON {on} FROM PUBLIC;"));
                out.push(format!("REVOKE ALL ON {on} FROM {owner};"));
            }
        } else if !self.owner.is_empty() {
            out.push(format!("ALTER AUTHORIZATION ON {on} TO {owner};"));
        }
        // By grantee, state and grant option in first-seen order — two
        // privileges that disagree about either are two statements — the
        // relation's own first, then its columns', each privilege naming its
        // columns: `SELECT ("a", "b"), UPDATE ("b")`.
        type Key = (Option<String>, GrantState, bool);
        type Group<'a> = (Key, Vec<(&'a str, Vec<String>)>);
        let mut groups: Vec<Group> = Vec::new();
        for columns in [false, true] {
            let first = groups.len();
            for g in self.grants.iter().filter(|g| g.column.is_some() == columns) {
                let key = (g.grantee.clone(), g.state, g.grantable);
                let at = match groups[first..].iter().position(|(k, _)| *k == key) {
                    Some(i) => first + i,
                    None => {
                        groups.push((key, Vec::new()));
                        groups.len() - 1
                    }
                };
                let ps = &mut groups[at].1;
                let col = g
                    .column
                    .as_deref()
                    .map(|c| crate::export::ident_sql(c, dialect));
                match ps.iter_mut().find(|(p, _)| *p == g.privilege) {
                    Some((_, cs)) => cs.extend(col),
                    None => ps.push((&g.privilege, col.into_iter().collect())),
                }
            }
        }
        for ((to, state, grantable), ps) in &groups {
            let list = ps
                .iter()
                .map(|(p, cs)| {
                    if cs.is_empty() {
                        p.to_string()
                    } else {
                        format!("{p} ({})", cs.join(", "))
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            let (verb, prep) = state.verb();
            out.push(format!(
                "{verb} {list} ON {on} {prep} {}{};",
                role(to),
                if *grantable { " WITH GRANT OPTION" } else { "" }
            ));
        }
        if !pg {
            return out;
        }
        if self.row_security {
            out.push(format!("ALTER TABLE {qname} ENABLE ROW LEVEL SECURITY;"));
        }
        if self.force_row_security {
            out.push(format!("ALTER TABLE {qname} FORCE ROW LEVEL SECURITY;"));
        }
        for p in &self.policies {
            let roles = if p.roles.is_empty() {
                "PUBLIC".to_string()
            } else {
                p.roles.iter().map(role).collect::<Vec<_>>().join(", ")
            };
            let mut sql = format!(
                "CREATE POLICY {} ON {qname} AS {} FOR {} TO {roles}",
                crate::export::ident_sql(&p.name, dialect),
                if p.permissive {
                    "PERMISSIVE"
                } else {
                    "RESTRICTIVE"
                },
                p.command,
            );
            if let Some(u) = &p.using {
                sql.push_str(&format!(" USING ({u})"));
            }
            if let Some(c) = &p.check {
                sql.push_str(&format!(" WITH CHECK ({c})"));
            }
            sql.push(';');
            out.push(sql);
        }
        out
    }
}

/// `pg_policy.polcmd` as `CREATE POLICY … FOR` spells it — `*` and anything
/// unknown being `ALL`, the default.
pub fn pg_policy_command(polcmd: &str) -> &'static str {
    match polcmd {
        "r" => "SELECT",
        "a" => "INSERT",
        "w" => "UPDATE",
        "d" => "DELETE",
        _ => "ALL",
    }
}

/// One account's grant on a MySQL routine (`mysql.procs_priv`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoutineGrant {
    pub name: String,
    pub kind: RoutineKind,
    pub user: String,
    pub host: String,
    /// `Proc_priv`'s set as the server spells it: `Execute`, `Alter Routine`,
    /// `Grant`.
    pub privileges: Vec<String>,
}

impl RoutineGrant {
    /// The `GRANT` that gives the account back what it held on the routine,
    /// unqualified — the file creates the routine in the database it is
    /// replayed in, and the grant lands beside it. `None` where it held
    /// nothing a `GRANT` names.
    pub fn grant_sql(&self, dialect: SqlDialect) -> Option<String> {
        match dialect {
            SqlDialect::MySql => {}
            SqlDialect::Postgres | SqlDialect::Sqlite | SqlDialect::MsSql => return None,
        }
        let held = |p: &str| self.privileges.iter().any(|h| h.eq_ignore_ascii_case(p));
        let grantable = held("Grant");
        let named: Vec<&str> = [("Execute", "EXECUTE"), ("Alter Routine", "ALTER ROUTINE")]
            .into_iter()
            .filter(|(p, _)| held(p))
            .map(|(_, kw)| kw)
            .collect();
        let privileges = match (named.is_empty(), grantable) {
            (true, false) => return None,
            (true, true) => "USAGE".to_string(),
            (false, _) => named.join(", "),
        };
        Some(format!(
            "GRANT {privileges} ON {} {} TO {}@{}{};",
            self.kind.sql_keyword(),
            crate::export::ident_sql(&self.name, dialect),
            crate::schema::ddl_string(&self.user, dialect),
            crate::schema::ddl_string(&self.host, dialect),
            if grantable { " WITH GRANT OPTION" } else { "" }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG: SqlDialect = SqlDialect::Postgres;

    fn grant(to: Option<&str>, privilege: &str) -> Grant {
        Grant {
            grantee: to.map(str::to_string),
            privilege: privilege.into(),
            ..Default::default()
        }
    }

    fn orders() -> TableAccess {
        TableAccess {
            schema: Some("public".into()),
            name: "orders".into(),
            owner: "app_owner".into(),
            default_acl: true,
            ..Default::default()
        }
    }

    /// A table with the default ACL gets its owner back and nothing else —
    /// a fresh `CREATE` gives it the default again.
    #[test]
    fn a_default_acl_restates_only_the_owner() {
        assert_eq!(
            orders().restate_sql("\"public\".\"orders\"", "TABLE", PG),
            ["ALTER TABLE \"public\".\"orders\" OWNER TO \"app_owner\";"]
        );
        assert_eq!(
            orders().restate_sql("\"public\".\"v\"", "MATERIALIZED VIEW", PG),
            ["ALTER MATERIALIZED VIEW \"public\".\"v\" OWNER TO \"app_owner\";"]
        );
    }

    /// **An explicit ACL is put back whole**: revoked from `PUBLIC` and the
    /// owner, then each grantee's privileges in one statement, a held
    /// `WITH GRANT OPTION` in a statement of its own, and `PUBLIC` as the
    /// keyword rather than a role.
    #[test]
    fn an_explicit_acl_is_revoked_and_granted_back_by_grantee() {
        let mut t = orders();
        t.default_acl = false;
        t.grants = vec![
            grant(Some("app_owner"), "SELECT"),
            grant(Some("app_owner"), "INSERT"),
            grant(Some("app"), "SELECT"),
            Grant {
                grantable: true,
                ..grant(Some("app"), "UPDATE")
            },
            grant(Some("app"), "DELETE"),
            grant(None, "SELECT"),
        ];
        assert_eq!(
            t.restate_sql("q", "TABLE", PG),
            [
                "ALTER TABLE q OWNER TO \"app_owner\";",
                "REVOKE ALL ON TABLE q FROM PUBLIC;",
                "REVOKE ALL ON TABLE q FROM \"app_owner\";",
                "GRANT SELECT, INSERT ON TABLE q TO \"app_owner\";",
                "GRANT SELECT, DELETE ON TABLE q TO \"app\";",
                "GRANT UPDATE ON TABLE q TO \"app\" WITH GRANT OPTION;",
                "GRANT SELECT ON TABLE q TO PUBLIC;",
            ]
        );
    }

    /// An explicit ACL that is empty — every privilege revoked, the owner's
    /// too — is still revoked, so the recreated table is as closed as it was.
    #[test]
    fn an_empty_explicit_acl_still_revokes() {
        let mut t = orders();
        t.default_acl = false;
        assert_eq!(
            t.restate_sql("q", "VIEW", PG),
            [
                "ALTER VIEW q OWNER TO \"app_owner\";",
                "REVOKE ALL ON TABLE q FROM PUBLIC;",
                "REVOKE ALL ON TABLE q FROM \"app_owner\";",
            ]
        );
    }

    /// **Column privileges come back with their columns**, one statement per
    /// grantee naming each privilege's columns — and with a default table
    /// ACL too, since a column grant is no part of it.
    #[test]
    fn column_grants_are_granted_back_per_grantee() {
        let mut t = orders();
        let col = |to: &str, p: &str, c: &str| Grant {
            column: Some(c.into()),
            ..grant(Some(to), p)
        };
        t.grants = vec![
            col("app", "SELECT", "id"),
            col("app", "SELECT", "Total"),
            col("app", "UPDATE", "Total"),
            col("audit", "SELECT", "id"),
        ];
        assert_eq!(
            t.restate_sql("q", "TABLE", PG)[1..],
            [
                "GRANT SELECT (\"id\", \"Total\"), UPDATE (\"Total\") ON TABLE q TO \"app\";",
                "GRANT SELECT (\"id\") ON TABLE q TO \"audit\";",
            ]
        );
    }

    /// **Row security and its policies come back on**, forced where it was,
    /// each policy restated with its kind, command, roles and expressions —
    /// a dropped table came back with row security off, its rows open to
    /// every role its grants let in.
    #[test]
    fn row_security_and_policies_are_restated() {
        let mut t = orders();
        t.row_security = true;
        t.force_row_security = true;
        t.policies = vec![
            Policy {
                name: "own rows".into(),
                permissive: true,
                command: "ALL".into(),
                roles: vec![None],
                using: Some("(owner = CURRENT_USER)".into()),
                check: None,
            },
            Policy {
                name: "no_archive".into(),
                permissive: false,
                command: "UPDATE".into(),
                roles: vec![Some("app".into()), Some("Clerk".into())],
                using: Some("(NOT archived)".into()),
                check: Some("(total >= 0)".into()),
            },
        ];
        assert_eq!(
            t.restate_sql("q", "TABLE", PG)[1..],
            [
                "ALTER TABLE q ENABLE ROW LEVEL SECURITY;",
                "ALTER TABLE q FORCE ROW LEVEL SECURITY;",
                "CREATE POLICY \"own rows\" ON q AS PERMISSIVE FOR ALL TO PUBLIC \
                 USING ((owner = CURRENT_USER));",
                "CREATE POLICY \"no_archive\" ON q AS RESTRICTIVE FOR UPDATE TO \"app\", \
                 \"Clerk\" USING ((NOT archived)) WITH CHECK ((total >= 0));",
            ]
        );
    }

    /// A name that needs quoting is quoted, through the one quoter.
    #[test]
    fn a_role_that_needs_quoting_is_quoted() {
        let mut t = orders();
        t.owner = "Ops \"Team\"".into();
        assert_eq!(
            t.restate_sql("q", "TABLE", PG),
            ["ALTER TABLE q OWNER TO \"Ops \"\"Team\"\"\";"]
        );
    }

    /// Nothing is restated where a table's `DROP` keeps its grants (MySQL)
    /// or there are none (SQLite).
    #[test]
    fn no_engine_whose_drop_keeps_grants_restates_them() {
        let mut t = orders();
        t.default_acl = false;
        for d in [SqlDialect::MySql, SqlDialect::Sqlite] {
            assert!(!drop_resets_table_access(d), "{d:?}");
            assert!(t.restate_sql("q", "TABLE", d).is_empty(), "{d:?}");
        }
        assert!(drop_resets_table_access(PG));
        assert!(drop_resets_table_access(SqlDialect::MsSql));
    }

    /// **SQL Server's permissions come back as they were read**: an owner
    /// other than the schema's through `ALTER AUTHORIZATION`, then each
    /// grant, grant option, `DENY` and column `REVOKE` by grantee, on
    /// `OBJECT::`. Nothing is revoked first — a new object holds no
    /// permission — and a schema-owned table names no owner.
    #[test]
    fn sql_server_permissions_are_granted_denied_and_revoked_back() {
        let ms = SqlDialect::MsSql;
        let g = |to: &str, p: &str, state: GrantState, col: Option<&str>| Grant {
            grantee: Some(to.into()),
            privilege: p.into(),
            state,
            column: col.map(str::to_string),
            ..Default::default()
        };
        let mut t = TableAccess {
            name: "orders".into(),
            owner: "Auditor".into(),
            default_acl: true,
            grants: vec![
                g("app", "SELECT", GrantState::Grant, None),
                g("app", "INSERT", GrantState::Grant, None),
                Grant {
                    grantable: true,
                    ..g("lead", "SELECT", GrantState::Grant, None)
                },
                g("app", "DELETE", GrantState::Deny, None),
                g("app", "SELECT", GrantState::Revoke, Some("salary")),
                g("intern", "SELECT", GrantState::Grant, Some("id")),
            ],
            ..Default::default()
        };
        assert_eq!(
            t.restate_sql("[dbo].[orders]", "TABLE", ms),
            [
                "ALTER AUTHORIZATION ON OBJECT::[dbo].[orders] TO [Auditor];",
                "GRANT SELECT, INSERT ON OBJECT::[dbo].[orders] TO [app];",
                "GRANT SELECT ON OBJECT::[dbo].[orders] TO [lead] WITH GRANT OPTION;",
                "DENY DELETE ON OBJECT::[dbo].[orders] TO [app];",
                "REVOKE SELECT ([salary]) ON OBJECT::[dbo].[orders] FROM [app];",
                "GRANT SELECT ([id]) ON OBJECT::[dbo].[orders] TO [intern];",
            ]
        );
        t.owner.clear();
        t.grants.clear();
        assert!(t.restate_sql("[dbo].[orders]", "TABLE", ms).is_empty());
        assert_eq!(GrantState::from_mssql("W"), GrantState::Grant);
        assert_eq!(GrantState::from_mssql("D"), GrantState::Deny);
        assert_eq!(GrantState::from_mssql("R"), GrantState::Revoke);
    }

    #[test]
    fn a_policy_command_is_spelled_as_create_policy_takes_it() {
        for (c, want) in [
            ("*", "ALL"),
            ("r", "SELECT"),
            ("a", "INSERT"),
            ("w", "UPDATE"),
            ("d", "DELETE"),
        ] {
            assert_eq!(pg_policy_command(c), want, "{c}");
        }
    }

    fn rg(privileges: &[&str]) -> RoutineGrant {
        RoutineGrant {
            name: "f_total".into(),
            kind: RoutineKind::Function,
            user: "app".into(),
            host: "%".into(),
            privileges: privileges.iter().map(|p| p.to_string()).collect(),
        }
    }

    /// **A MySQL routine's grant is put back as it was held** — its
    /// privileges named, `Grant` as `WITH GRANT OPTION` — on an unqualified
    /// name, beside the `CREATE` that made it in the replay's database.
    #[test]
    fn a_routine_grant_is_granted_back_as_held() {
        let my = SqlDialect::MySql;
        assert_eq!(
            rg(&["Execute"]).grant_sql(my).as_deref(),
            Some("GRANT EXECUTE ON FUNCTION `f_total` TO 'app'@'%';")
        );
        let mut p = rg(&["Execute", "Alter Routine", "Grant"]);
        p.kind = RoutineKind::Procedure;
        p.user = "o'neil".into();
        assert_eq!(
            p.grant_sql(my).as_deref(),
            Some(
                "GRANT EXECUTE, ALTER ROUTINE ON PROCEDURE `f_total` TO 'o''neil'@'%' \
                 WITH GRANT OPTION;"
            )
        );
        assert_eq!(
            rg(&["Grant"]).grant_sql(my).as_deref(),
            Some("GRANT USAGE ON FUNCTION `f_total` TO 'app'@'%' WITH GRANT OPTION;")
        );
        assert_eq!(rg(&[]).grant_sql(my), None, "nothing held");
        assert_eq!(rg(&["Execute"]).grant_sql(SqlDialect::Postgres), None);
    }

    #[test]
    fn replay_access_finds_a_table_and_a_routines_grants() {
        let mut other = rg(&["Execute"]);
        other.name = "f_other".into();
        let a = ReplayAccess {
            tables: Some(vec![orders()]),
            routines: Some(vec![rg(&["Execute"]), other, rg(&["Alter Routine"])]),
        };
        assert!(a.table(Some("public"), "orders").is_some());
        assert!(a.table(None, "orders").is_none());
        assert_eq!(
            a.routine_grants("f_total", RoutineKind::Function).count(),
            2
        );
        assert_eq!(
            a.routine_grants("F_Total", RoutineKind::Function).count(),
            2,
            "MySQL compares a routine's name ignoring case"
        );
        assert_eq!(
            a.routine_grants("f_total", RoutineKind::Procedure).count(),
            0
        );
    }
}
