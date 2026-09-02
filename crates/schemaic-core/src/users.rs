//! Users, roles and privileges — the accounts a server knows, and what each of
//! them is allowed to do. Pure over an already-fetched snapshot: no DB, no UI,
//! no engine.
//!
//! The *fetch* lives in `schemaic-db`, one query set per engine, because the
//! catalogues share nothing: MySQL/MariaDB keep accounts in `mysql.user` and
//! answer `SHOW GRANTS` with finished statements, while PostgreSQL keeps roles
//! in `pg_roles` and keeps their privileges as `aclitem` arrays hanging off each
//! object. Everything downstream of the fetch is here, because it doesn't: a
//! [`Principal`] means the same thing whichever engine produced it.
//!
//! **The privilege list is the engine's own sentences, not a table this module
//! invents.** On MySQL that is literally what `SHOW GRANTS` returned; on
//! PostgreSQL, whose catalogue has no such view, [`pg_grant_statements`]
//! reassembles the same shape from exploded ACL rows. One rendering, so the two
//! halves of the browser cannot disagree about what a privilege is called, and
//! so the form that grants and revokes offers the same words the browser reads
//! back.
//!
//! **Nothing here shows a secret.** MariaDB's `SHOW GRANTS` carries the account's
//! password hash inline (`IDENTIFIED BY PASSWORD '*01E8…'`), which is a
//! credential-equivalent for the older hashing plugins and has no business on
//! screen or in a copied buffer; [`redact_secrets`] is the one gate every grant
//! statement passes through on its way out of `schemaic-db`.

use serde::{Deserialize, Serialize};

use crate::intel::SqlDialect;
use crate::sql::{is_word_byte, skip_noncode};
use crate::text_ops::contains_ignore_ascii_case;

/// Does `dialect` have accounts to show at all?
///
/// MySQL/MariaDB and PostgreSQL are both server engines with a login system and
/// a privilege catalogue. **SQLite has neither, and this is a statement about
/// SQLite rather than unfinished work**: it is a library linked into this
/// process, and its access control is the filesystem's — the database file's
/// permissions, granted to an OS user by the OS, with nothing in the database to
/// name, list or grant. There is no account to browse and no statement that
/// would create one.
///
/// So a SQLite connection is told that in a sentence, rather than shown an empty
/// list that would read as "a server with no users".
pub fn supports_users(dialect: SqlDialect) -> bool {
    !matches!(dialect, SqlDialect::Sqlite)
}

/// Can accounts be *created and dropped* from here, not just read?
///
/// Exactly [`supports_users`] today — both server engines spell `CREATE USER`,
/// `DROP USER`, `GRANT` and `REVOKE` — and *computed* from it rather than
/// spelling out a second `!= Sqlite`, so a fourth engine that can list accounts
/// it may not create is one edit away rather than a second predicate to find.
pub fn supports_user_admin(dialect: SqlDialect) -> bool {
    supports_users(dialect)
}

/// A user or a role. Two words for one catalogue row on both engines — MySQL 8
/// and MariaDB both keep roles in `mysql.user`, and PostgreSQL merged users into
/// `pg_roles` in 8.1 — so this is a property of the row, never a separate list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrincipalKind {
    /// An account that can log in. The default, because that is what
    /// [`AccountDraft`] opens on: making a user is the ordinary act and making a
    /// role the deliberate one.
    #[default]
    User,
    /// A bag of privileges that is granted to accounts rather than logged into.
    Role,
}

impl PrincipalKind {
    /// The word the browser puts on the row.
    pub fn label(self) -> &'static str {
        match self {
            PrincipalKind::User => "User",
            PrincipalKind::Role => "Role",
        }
    }
}

/// One account, as the browser shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    /// The account name, unquoted and unescaped — the server's own bytes.
    pub name: String,
    /// The host pattern the account is scoped to, on MySQL/MariaDB where an
    /// account *is* the `(user, host)` pair and `'app'@'%'` and
    /// `'app'@'localhost'` are two different accounts with different passwords
    /// and different privileges.
    ///
    /// `None` on PostgreSQL, whose roles are not host-scoped at all — host rules
    /// live in `pg_hba.conf`, a file, and no catalogue publishes it.
    pub host: Option<String>,
    pub kind: PrincipalKind,
    /// An account the *server* created and maintains, rather than one an
    /// administrator made: MySQL's `mysql.sys`/`mysql.session`/`mysql.infoschema`
    /// and MariaDB's `mariadb.sys`, PostgreSQL's `pg_*` predefined roles.
    ///
    /// Kept rather than filtered, because "why can `pg_monitor` read that" is a
    /// real question — but sorted last and dimmed, so the handful of accounts
    /// someone actually administers are the ones at the top of the list.
    pub system: bool,
    /// What the engine says about the account, in display order, already
    /// rendered as text. Per-engine by construction — MariaDB has no
    /// `account_locked` and MySQL 8 has no `is_role`, so a shared struct of
    /// `Option` fields would be mostly holes and every reader would have to know
    /// which engine filled which.
    pub attributes: Vec<(String, String)>,
}

impl Principal {
    /// How the account is written for a person: `app@%` on MySQL/MariaDB, the
    /// bare role name on PostgreSQL. Not SQL — see [`account_sql`] for that.
    pub fn display(&self) -> String {
        match &self.host {
            Some(h) => format!("{}@{}", self.name, h),
            None => self.name.clone(),
        }
    }
}

/// Order the list: real accounts first, then server-owned ones, each group by
/// name and then host.
///
/// **`system` leads the key deliberately.** PostgreSQL 16 ships fourteen `pg_*`
/// predefined roles and a fresh cluster has two of its own, so sorting by name
/// alone buries `postgres` and whatever the administrator made in the middle of
/// a list that is four-fifths furniture.
pub fn sort_principals(list: &mut [Principal]) {
    list.sort_by(|a, b| {
        a.system
            .cmp(&b.system)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.host.cmp(&b.host))
    });
}

/// Does `p` match what was typed in the browser's filter box?
///
/// Case-insensitive over the display name — `app@%` — so both halves of a MySQL
/// account are searchable with one field, and typing `localhost` finds every
/// account scoped to it. An empty needle matches everything.
pub fn matches(p: &Principal, needle: &str) -> bool {
    let needle = needle.trim();
    needle.is_empty() || contains_ignore_ascii_case(&p.display(), needle)
}

/// The indices of `list` that [`matches()`] `needle`, in list order.
///
/// **Indices, and computed once.** The browser's list is virtualised, so the
/// filter has to be a value the scroll can index rather than a predicate each
/// row re-asks — and the view was answering it twice per keystroke, once to
/// build the rows and once for the footer's count, each call re-`format!`ing
/// every account's `display()`. At the ~1,000 accounts a shared server has that
/// was measured at ≥13 ms of identified work per keystroke on a 16.7 ms frame,
/// and the query behind it has no `LIMIT`.
pub fn filter_indices(list: &[Principal], needle: &str) -> Vec<usize> {
    let needle = needle.trim();
    if needle.is_empty() {
        return (0..list.len()).collect();
    }
    list.iter()
        .enumerate()
        .filter(|(_, p)| contains_ignore_ascii_case(&p.display(), needle))
        .map(|(i, _)| i)
        .collect()
}

/// The account as SQL names it, for the statement that asks about it.
///
/// MySQL spells an account as **two string literals** — `'app'@'%'` — not as an
/// identifier, so this is not a fifth identifier quoter and does not go through
/// `export::ident_sql`; it goes through the one *literal* quoter
/// ([`crate::schema::ddl_string`]), which is the rule that already knows MySQL
/// escapes backslashes inside a literal and PostgreSQL does not. A host of
/// `it's` would otherwise close the quote and change the statement.
///
/// PostgreSQL has no host part and names a role as an identifier, so there it is
/// the ordinary identifier quoting.
pub fn account_sql(p: &Principal, dialect: SqlDialect) -> String {
    match (&p.host, dialect) {
        (Some(h), _) => format!(
            "{}@{}",
            crate::schema::ddl_string(&p.name, dialect),
            crate::schema::ddl_string(h, dialect)
        ),
        (None, d) => crate::export::ident_sql(&p.name, d),
    }
}

// ── MySQL/MariaDB ────────────────────────────────────────────────────────────

/// One `mysql.user` row, as far as the *server in front of us* fills it in.
///
/// **Every field but the pair is optional, because the two servers disagree
/// about which columns exist.** MariaDB 10.11 has `is_role` and no
/// `account_locked`; MySQL 8.4 has `account_locked` and no `is_role`. Selecting
/// the union fails outright with `Unknown column`, so `schemaic-db` asks for the
/// widest set the server admits and leaves the rest `None` — and `None` here
/// means *this server does not publish it*, never *no*.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MyUserRow {
    pub user: String,
    pub host: String,
    /// Authentication plugin.
    pub plugin: Option<String>,
    /// `'Y'`/`'N'`. MariaDB and MySQL both have it.
    pub password_expired: Option<String>,
    /// `'Y'`/`'N'`, MariaDB only — the one flag that says a row is a role.
    pub is_role: Option<String>,
    /// `'Y'`/`'N'`, MySQL 8 only.
    pub account_locked: Option<String>,
}

/// Is a `'Y'`/`'N'` column set? Absent is not `'N'` — see [`MyUserRow`].
fn my_flag(v: &Option<String>) -> bool {
    v.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("Y"))
}

/// Fold `mysql.user` rows into the list the browser shows.
///
/// **Role detection is MariaDB's flag or nothing.** MariaDB marks a role with
/// `is_role = 'Y'`; MySQL 8 has no such column and no other catalogue answer —
/// `CREATE ROLE` there is implemented as a locked, password-expired user, and
/// reading that pair back as "role" would relabel every genuinely locked account
/// as one. So on MySQL every row is a [`PrincipalKind::User`] and the *locked*
/// attribute says the rest; a wrong label on a privilege screen is worse than a
/// missing one.
pub fn from_mysql_rows(rows: &[MyUserRow]) -> Vec<Principal> {
    let mut out: Vec<Principal> = rows
        .iter()
        .map(|r| {
            let kind = if my_flag(&r.is_role) {
                PrincipalKind::Role
            } else {
                PrincipalKind::User
            };
            let mut attributes = Vec::new();
            if let Some(p) = r.plugin.as_deref().filter(|p| !p.is_empty()) {
                attributes.push(("Authentication".to_string(), p.to_string()));
            }
            if r.account_locked.is_some() {
                attributes.push((
                    "Locked".to_string(),
                    yes_no(my_flag(&r.account_locked)).to_string(),
                ));
            }
            if r.password_expired.is_some() {
                attributes.push((
                    "Password expired".to_string(),
                    yes_no(my_flag(&r.password_expired)).to_string(),
                ));
            }
            Principal {
                name: r.user.clone(),
                // **A role carries no host, and the host the catalogue stored
                // for it is dropped here rather than worked around downstream.**
                // MariaDB keeps `''` and MySQL 8 keeps `'%'`, and the first
                // reading of this was that a statement naming a role has to
                // match what the server stored. The server says otherwise: on
                // MariaDB 10.11 `SHOW GRANTS FOR 'r'@''` is ERROR 1141,
                // `GRANT … TO 'r'@''` is ERROR 1133, and `DROP ROLE 'r'@''` is
                // a *syntax* error — its `DROP ROLE` grammar has no `@host` at
                // all — while the bare name works for all three, and on MySQL 8
                // a bare role name resolves to the `%` row it stored. Dropping
                // it in the fold fixes the display too: `Some("")` renders as
                // `readers@`.
                host: match kind {
                    PrincipalKind::Role => None,
                    PrincipalKind::User => Some(r.host.clone()),
                },
                kind,
                system: is_mysql_system_account(&r.user),
                attributes,
            }
        })
        .collect();
    sort_principals(&mut out);
    out
}

fn yes_no(b: bool) -> &'static str {
    if b { "Yes" } else { "No" }
}

/// The accounts the server ships and maintains itself.
///
/// Matched on the reserved `mysql.` prefix that MySQL 8 gives its own internal
/// accounts, plus MariaDB's `mariadb.` one. Both are *reserved* prefixes, not a
/// hard-coded list of names, so a future `mysql.something` lands on the right
/// side of the line without an edit here.
///
/// **`mysql` itself is the one exact name**, and it is not covered by the
/// prefix — `"mysql"` does not start with `"mysql."`. MariaDB's `mysql_install_db`
/// creates `mysql@localhost` for unix-socket authentication of the OS `mysql`
/// user, and left off this list it sorted among the administrator's own
/// accounts, undimmed, and was offered `Privileges` and `Drop` — the two actions
/// the browser withholds from server-owned accounts precisely because changing
/// them breaks the server. The cost of the exact match is an administrator who
/// named a real account `mysql`, who would find it read-only here; the cost of
/// leaving it off is a one-click `DROP USER` on the server's own login.
fn is_mysql_system_account(user: &str) -> bool {
    user.starts_with("mysql.") || user.starts_with("mariadb.") || user == "mysql"
}

/// Split an `information_schema` `GRANTEE` cell — `'app'@'%'` — back into the
/// account pair `mysql.user` would have given as two columns.
///
/// **The fallback path's parser, not the ordinary one.** `mysql.user` needs the
/// `SELECT` privilege on the `mysql` database, which an application account does
/// not have and should not be given; `information_schema.USER_PRIVILEGES` is
/// readable by anyone and shows them their own row, so a browser opened on an
/// ordinary connection lists that account instead of refusing outright. The cell
/// is SQL-quoted, and a host or user containing a quote arrives doubled, so this
/// unquotes through [`skip_noncode`] rather than splitting on `@` — `'a@b'@'%'`
/// is a legal account and splitting on the first `@` would invent two.
pub fn parse_grantee(s: &str) -> Option<(String, String)> {
    let b = s.as_bytes();
    let read = |i: usize| -> Option<(String, usize)> {
        if b.get(i)? != &b'\'' {
            return None;
        }
        let j = skip_noncode(b, i, SqlDialect::MySql)?;
        // Unterminated — `skip_noncode` answers "end of input", which is not a
        // literal this can unquote.
        if j <= i + 1 || b[j - 1] != b'\'' {
            return None;
        }
        // **Unquoted by the same rule `skip_noncode` scanned with.** It scans
        // this literal as MySQL, where a backslash escapes — so a cell of
        // `'o'brien'` is *one* literal to the scanner, and undoubling alone
        // would have left the backslash in the account name and built every
        // statement against an account that does not exist. `information_schema`
        // writes the doubled form, so this is the two rules agreeing rather than
        // a bug seen in the wild.
        Some((unquote_mysql_literal(&s[i + 1..j - 1]), j))
    };
    let (user, i) = read(0)?;
    if b.get(i)? != &b'@' {
        return None;
    }
    let (host, j) = read(i + 1)?;
    (j == b.len()).then_some((user, host))
}

/// The body of a MySQL single-quoted literal, with its escapes undone: a
/// doubled quote and a backslash-escaped one both become one quote, and a
/// doubled backslash becomes one.
///
/// Deliberately narrow — it undoes what [`parse_grantee`]'s cells can carry,
/// not the whole of MySQL's escape table (the newline, NUL and Ctrl-Z
/// spellings), because an account name holding one of those is not a case
/// `information_schema` produces and a half-known table is worse than a stated
/// one.
fn unquote_mysql_literal(body: &str) -> String {
    let b = body.as_bytes();
    let mut out = String::with_capacity(body.len());
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'\'' if b.get(i + 1) == Some(&b'\'') => {
                out.push('\'');
                i += 2;
            }
            b'\\' if i + 1 < b.len() => {
                // The escaped byte verbatim. `skip_noncode` scanned this literal
                // with the same backslash rule, so an escape here is one it
                // already treated as a pair — and the byte after it starts a
                // character, which is what makes slicing one byte on a boundary.
                let start = i + 1;
                i += 2;
                while i < b.len() && (b[i] & 0xC0) == 0x80 {
                    i += 1;
                }
                out.push_str(&body[start..i]);
            }
            _ => {
                let start = i;
                i += 1;
                while i < b.len() && (b[i] & 0xC0) == 0x80 {
                    i += 1;
                }
                out.push_str(&body[start..i]);
            }
        }
    }
    out
}

// ── PostgreSQL ───────────────────────────────────────────────────────────────

/// One `pg_roles` row. Unlike [`MyUserRow`] every field is present, because
/// PostgreSQL's role catalogue has had this shape since 9.5 and the one column
/// added since (`rolbypassrls`, 9.5) is older than any server this connects to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PgRoleRow {
    pub name: String,
    pub superuser: bool,
    pub inherit: bool,
    pub createrole: bool,
    pub createdb: bool,
    pub canlogin: bool,
    pub replication: bool,
    pub bypassrls: bool,
    /// `-1` for no limit, which is the overwhelming default.
    pub connlimit: i32,
    /// Password expiry, already rendered by the server. `None` when unset.
    pub valid_until: Option<String>,
}

/// Fold `pg_roles` rows into the list the browser shows.
///
/// **`rolcanlogin` is the user/role split**, which is the only split PostgreSQL
/// makes: `CREATE USER` is documented as `CREATE ROLE … LOGIN`, and the two
/// words name one catalogue. Attributes list only what is *set*, because a role
/// with nine "No" rows says nothing and the one that matters — `Superuser` —
/// stops standing out.
pub fn from_pg_rows(rows: &[PgRoleRow]) -> Vec<Principal> {
    let mut out: Vec<Principal> = rows
        .iter()
        .map(|r| {
            let mut attributes = Vec::new();
            let mut flag = |on: bool, label: &str| {
                if on {
                    attributes.push((label.to_string(), "Yes".to_string()));
                }
            };
            flag(r.superuser, "Superuser");
            flag(r.createrole, "Create role");
            flag(r.createdb, "Create database");
            flag(r.replication, "Replication");
            flag(r.bypassrls, "Bypass RLS");
            // Inherit is on by default, so its *absence* is the notable state.
            if !r.inherit {
                attributes.push(("Inherit".to_string(), "No".to_string()));
            }
            if r.connlimit >= 0 {
                attributes.push(("Connection limit".to_string(), r.connlimit.to_string()));
            }
            if let Some(v) = r.valid_until.as_deref().filter(|v| !v.is_empty()) {
                attributes.push(("Valid until".to_string(), v.to_string()));
            }
            Principal {
                name: r.name.clone(),
                host: None,
                kind: if r.canlogin {
                    PrincipalKind::User
                } else {
                    PrincipalKind::Role
                },
                system: r.name.starts_with("pg_"),
                attributes,
            }
        })
        .collect();
    sort_principals(&mut out);
    out
}

/// What kind of object an ACL entry hangs off. Only the four PostgreSQL
/// publishes in a form this browser can name completely.
///
/// **Functions and procedures are absent on purpose.** `GRANT EXECUTE ON
/// FUNCTION` has to name the argument types, an overloaded name alone is
/// ambiguous, and a statement that names the wrong overload is worse than a
/// [`Grants::note`] saying it is not covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PgObjectKind {
    Database,
    Schema,
    Table,
    Sequence,
}

impl PgObjectKind {
    /// The word `GRANT … ON <here> …` wants.
    pub fn keyword(self) -> &'static str {
        match self {
            PgObjectKind::Database => "DATABASE",
            PgObjectKind::Schema => "SCHEMA",
            PgObjectKind::Table => "TABLE",
            PgObjectKind::Sequence => "SEQUENCE",
        }
    }

    /// Every privilege the kind can carry, in the order PostgreSQL's own
    /// documentation lists them — which is the order a reader expects and the
    /// order the [`ALL PRIVILEGES`](pg_grant_statements) collapse is checked
    /// against.
    pub fn all_privileges(self) -> &'static [&'static str] {
        match self {
            PgObjectKind::Database => &["CREATE", "CONNECT", "TEMPORARY"],
            PgObjectKind::Schema => &["CREATE", "USAGE"],
            PgObjectKind::Table => &[
                "SELECT",
                "INSERT",
                "UPDATE",
                "DELETE",
                "TRUNCATE",
                "REFERENCES",
                "TRIGGER",
            ],
            PgObjectKind::Sequence => &["USAGE", "SELECT", "UPDATE"],
        }
    }
}

/// One row of `aclexplode(…)` joined back to the object it came from: a single
/// privilege, held by one grantee, on one object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgAclRow {
    pub kind: PgObjectKind,
    /// The namespace, for the kinds that live in one. `None` for a database.
    pub schema: Option<String>,
    pub name: String,
    /// `SELECT`, `USAGE`, `TEMPORARY` … as `aclexplode` spells it.
    pub privilege: String,
    pub grantable: bool,
}

/// Reassemble exploded ACL rows into the `GRANT` statements they came from.
///
/// PostgreSQL has no `SHOW GRANTS`, so this is the half of the browser that has
/// to *write* what MySQL merely repeats. Rows are grouped by object and by
/// whether they are grantable — `WITH GRANT OPTION` is part of the statement,
/// not of the privilege, so two privileges on one table that differ in it are
/// two statements and folding them into one would say something false.
///
/// **A complete set collapses to `ALL PRIVILEGES`**, which is both what the
/// administrator almost certainly typed and what fits on a line: the alternative
/// prints seven comma-separated words per table.
///
/// `grantee` is the **unquoted** name of the role the rows were fetched for, and
/// is quoted here like every other name in the statement. Passed in rather than
/// read off a row, because every row in a call is one principal's by
/// construction and a per-row grantee would invite a mixed list nobody checks;
/// taken raw rather than pre-quoted so this and [`account_sql`] — which quotes
/// for a statement that is *executed*, and so quotes unconditionally — cannot be
/// confused for each other at a call site.
/// What one emitted `GRANT` is about: an object — kind, namespace, name — and
/// whether the privileges on it carry `WITH GRANT OPTION`.
///
/// The option is part of the key rather than of the privilege because it is part
/// of the *statement*: two privileges on one table that disagree about it are
/// two statements, and folding them into one would say something false about the
/// one that isn't grantable.
type GrantKey = (PgObjectKind, Option<String>, String, bool);

pub fn pg_grant_statements(grantee: &str, rows: &[PgAclRow]) -> Vec<String> {
    let grantee = pg_ident(grantee);
    // Key → privileges, in first-seen order per group so a caller's ORDER BY
    // still decides how the statements are laid out; the privileges *within* a
    // group are ordered by the kind's own list.
    //
    // **Grouped in one pass, against the *last* key rather than against every
    // key so far.** The rows arrive sorted by object — every query in
    // `pg::fetch_grants` says so in its `ORDER BY` — so a row either continues
    // the group before it or starts a new one, and a linear search back through
    // the groups is work with no answer to find. It is also the difference
    // between one pass and a quadratic one on the input this feature exists to
    // read: a role with privileges on every table of a 500-table schema is
    // 3,500 rows and ~500 groups, which searched that way is the better part of
    // a million String comparisons plus two cloned Strings per row for a key
    // that is thrown away.
    //
    // A row out of order does not corrupt anything — it opens a second group for
    // the same object, and the object is named identically in both statements —
    // so the ordering is an optimisation the caller supplies rather than a
    // contract the caller can break.
    //
    // **`grantable` is part of the key, so it has to be part of the sort**, and
    // it was the one component the paragraph above did not name while the three
    // queries did not order by it. A table holding `INSERT`, `UPDATE` plainly
    // and `SELECT` `WITH GRANT OPTION` arrived interleaved by privilege name,
    // broke the run twice, and printed three statements where the administrator
    // wrote two — losing the `ALL PRIVILEGES` collapse in both fragments.
    let mut groups: Vec<(GrantKey, Vec<String>)> = Vec::new();
    for r in rows {
        let continues = groups
            .last()
            .is_some_and(|((kind, schema, name, grantable), _)| {
                *kind == r.kind
                    && schema.as_deref() == r.schema.as_deref()
                    && name == &r.name
                    && *grantable == r.grantable
            });
        if !continues {
            groups.push((
                (r.kind, r.schema.clone(), r.name.clone(), r.grantable),
                Vec::new(),
            ));
        }
        let privs = &mut groups.last_mut().expect("just pushed").1;
        if !privs.iter().any(|p| p == &r.privilege) {
            privs.push(r.privilege.clone());
        }
    }

    groups
        .into_iter()
        .map(|((kind, schema, name, grantable), mut privs)| {
            let order = kind.all_privileges();
            privs.sort_by_key(|p| {
                order
                    .iter()
                    .position(|k| k.eq_ignore_ascii_case(p))
                    // A privilege the table doesn't list — a newer server's, and
                    // one this build has no place for — sorts last rather than
                    // first, and is still printed.
                    .unwrap_or(order.len())
            });
            let complete = order.len() == privs.len()
                && order
                    .iter()
                    .all(|k| privs.iter().any(|p| p.eq_ignore_ascii_case(k)));
            let list = if complete {
                "ALL PRIVILEGES".to_string()
            } else {
                privs.join(", ")
            };
            let object = match &schema {
                Some(s) => format!("{}.{}", pg_ident(s), pg_ident(&name)),
                None => pg_ident(&name),
            };
            let mut sql = format!("GRANT {list} ON {} {object} TO {grantee}", kind.keyword());
            if grantable {
                sql.push_str(" WITH GRANT OPTION");
            }
            sql
        })
        .collect()
}

/// The `GRANT <role> TO <member>` statements for the roles a principal holds.
///
/// `member_of` is `(role name, admin option)` as `pg_auth_members` reports it;
/// `grantee`, as in [`pg_grant_statements`], is the unquoted role name.
pub fn pg_membership_statements(grantee: &str, member_of: &[(String, bool)]) -> Vec<String> {
    let grantee = pg_ident(grantee);
    member_of
        .iter()
        .map(|(role, admin)| {
            let mut sql = format!("GRANT {} TO {grantee}", pg_ident(role));
            if *admin {
                sql.push_str(" WITH ADMIN OPTION");
            }
            sql
        })
        .collect()
}

/// PostgreSQL identifier quoting for statements the user *reads*, which is what
/// these are — the browser shows them, and nothing runs them.
fn pg_ident(name: &str) -> String {
    crate::export::ident_if_needed(name, SqlDialect::Postgres)
}

// ── the fetched result ───────────────────────────────────────────────────────

/// One principal's privileges, as the browser shows them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grants {
    /// The statements, already through [`redact_secrets`].
    pub statements: Vec<String>,
    /// What the list does **not** cover, when the answer is necessarily partial.
    /// Rendered under the statements, because a privilege screen that is silently
    /// incomplete is the one way this feature can mislead: PostgreSQL keeps
    /// object privileges in the catalogue of the database that holds the object,
    /// so a connection sees exactly one database's worth of them and nothing at
    /// all about the others.
    pub note: Option<String>,
}

/// Whether the account browser offers a write, and if not, why.
///
/// **In `core` because it is a decision, not a rendering.** It lived inline in
/// an 860-line view with no `#[cfg(test)]` at all, so the ordering of its four
/// answers — which is the whole content of it — had nowhere to be pinned. The
/// order is load-bearing: *engine* first, because an engine with no accounts
/// must not merely dim the action but omit it, and *read-only* before
/// *database*, because a read-only connection is the more specific reason and
/// the one the user can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteGate {
    Allowed,
    /// SQLite — no accounts at all, so the action is not offered.
    NoEngineSupport,
    /// The connection is read-only. Offered and dimmed, not hidden.
    ReadOnly,
    /// No database selected, and an account plan runs in one.
    NoDatabase,
}

impl WriteGate {
    /// `read_only` is the **live** connection's and `dialect` the browser's
    /// target: one is a setting that can change while the browser is open, the
    /// other is what the browser is about and cannot.
    pub fn of(dialect: SqlDialect, read_only: bool, has_database: bool) -> WriteGate {
        if !supports_user_admin(dialect) {
            return WriteGate::NoEngineSupport;
        }
        if read_only {
            return WriteGate::ReadOnly;
        }
        if !has_database {
            return WriteGate::NoDatabase;
        }
        WriteGate::Allowed
    }

    /// Is the action **offered at all**? Absent where the engine has nothing to
    /// offer, present-but-dimmed where this connection or this moment does — the
    /// two different answers the rest of the app gives for the two different
    /// reasons it gives them.
    pub fn offered(self) -> bool {
        !matches!(self, WriteGate::NoEngineSupport)
    }

    pub fn enabled(self) -> bool {
        matches!(self, WriteGate::Allowed)
    }
}

/// The account list, and what it does **not** cover.
///
/// The same shape as [`Grants`], and for the same reason. A `Vec<Principal>` has
/// no channel for "the wide reads were refused, this is one account out of
/// eight" — so an ordinary application account, with no `SELECT` on `mysql`,
/// browsed a list of exactly itself, rendered as an ordinary complete list with
/// a footer reading "1 account". `Grants::note` was added in the same commit for
/// this exact reason and this half did not get it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Principals {
    pub list: Vec<Principal>,
    /// What the list does not cover, when the answer is necessarily partial.
    pub note: Option<String>,
}

impl Principals {
    /// A complete answer — every account the server has.
    pub fn complete(list: Vec<Principal>) -> Self {
        Self { list, note: None }
    }
}

/// The sentence [`Principals::note`] carries when the server refused every read
/// wide enough to see other accounts.
pub fn my_own_account_only_note() -> String {
    "This connection cannot read mysql.user, so this is not the server's account list — it is \
     the one account this connection can see, its own. Connect as an account with SELECT on the \
     mysql database to browse the rest."
        .to_string()
}

/// The sentence [`Grants::note`] carries on MySQL and MariaDB.
///
/// `SHOW GRANTS` is documented on both servers as **direct grants only**: a
/// privilege the account holds through a role it has been granted does not
/// appear, and no `USING` form is issued here that would expand one. The list
/// was shipped with `note: None`, so a role-based setup — the normal way a
/// modern MySQL 8 or MariaDB installation is provisioned — showed an account
/// with almost nothing on it and said nothing about why.
pub fn my_scope_note() -> String {
    "Privileges held through a granted role are not expanded: SHOW GRANTS lists what is granted \
     directly to the account, and the roles it is a member of, but not what those roles \
     themselves hold. Open the role's own privileges to see those."
        .to_string()
}

/// The sentence [`Grants::note`] carries on PostgreSQL, naming the database the
/// connection is attached to.
///
/// `extra` is what this particular role's read could not express — see
/// [`pg_implicit_note`]. It goes first, because "this role is a superuser" is
/// the sentence that changes how the rest of the screen should be read.
pub fn pg_scope_note(database: &str, extra: Option<&str>) -> String {
    let base = format!(
        "Schema, table and sequence privileges are those in {database} — PostgreSQL keeps them in \
         each database's own catalogue, so privileges in other databases are not listed. \
         Privileges held through role membership or granted to PUBLIC are not expanded, and \
         neither are the privileges a role holds by owning an object or by being a superuser."
    );
    match extra {
        Some(e) => format!("{e} {base}"),
        None => base,
    }
}

/// What this role holds that no ACL entry records, as a sentence — or `None`
/// when there is nothing to say.
///
/// **The reason this exists rather than a fifth ACL query.** PostgreSQL's
/// `aclexplode` reads only *explicit* entries, and three of the commonest ways a
/// role gets its power leave none: a superuser bypasses every check, an owner
/// holds every privilege on what it owns, and a `pg_*` predefined role's powers
/// are wired into the server. Clicking `pg_read_all_data` therefore printed
/// **"This account holds no privileges."** — the pane's own words — and an owner
/// of fourteen tables got one statement about a database they were not looking
/// at, under a note that enumerated its omissions and did not include these.
///
/// They are stated rather than emitted as `GRANT`s because they are not grants:
/// there is no statement that would produce them and none that would revoke
/// them, so a line shaped like one would be a lie in the other direction.
pub fn pg_implicit_note(
    role: &str,
    superuser: bool,
    owns: usize,
    database: &str,
) -> Option<String> {
    // A superuser's sentence subsumes both of the others: it already holds every
    // privilege on everything, so counting what it owns adds nothing.
    if superuser {
        return Some(
            "This role is a SUPERUSER: it bypasses every permission check, so it holds every \
             privilege on every object whether or not anything below says so."
                .to_string(),
        );
    }
    let mut out: Vec<String> = Vec::new();
    if is_pg_predefined(role) {
        out.push(format!(
            "{role} is one of PostgreSQL's predefined roles: what it can do is built into the \
             server rather than granted, so it is recorded in no catalogue this reads."
        ));
    }
    if owns > 0 {
        out.push(format!(
            "This role owns {owns} object{s} in {database} (schemas, tables, sequences or the \
             database itself). An owner holds every privilege on what it owns, and PostgreSQL \
             records no grant for that, so none of it is listed below.",
            s = if owns == 1 { "" } else { "s" },
        ));
    }
    (!out.is_empty()).then(|| out.join(" "))
}

/// Is this one of PostgreSQL's own `pg_*` roles?
///
/// The same prefix test `pg::roles` uses to keep them out of a *grantable* role
/// list — a prefix comparison, with no escaping to depend on. They are kept in
/// the account browser deliberately, because "why can this account read that" is
/// the question it exists to answer, which is exactly why their powers have to
/// be accounted for.
pub fn is_pg_predefined(role: &str) -> bool {
    role.starts_with("pg_")
}

/// The same sentence for a connection with **no database selected**.
///
/// The object privileges are not read at all in that case, and this says so.
/// The alternative shipped once and is the reason this exists: with nothing
/// selected the fetch stands on PostgreSQL's *maintenance* database, and running
/// the `pg_namespace`/`pg_class` queries there returned that database's schema
/// and table privileges — a database the user did not choose and is not looking
/// at — under no note at all. A list that names `public.users` while the user is
/// thinking about another database is worse than a list that stops short and
/// says why.
pub fn pg_no_database_note(extra: Option<&str>) -> String {
    let base = "Schema, table and sequence privileges are not listed: PostgreSQL keeps them in \
         each database's own catalogue, and this connection has no database selected. Open a \
         query tab on a database to see them. Privileges held through role membership or granted \
         to PUBLIC are not expanded, and neither are the privileges a role holds by owning an \
         object or by being a superuser.";
    match extra {
        Some(e) => format!("{e} {base}"),
        None => base.to_string(),
    }
}

/// Strip password material out of a grant statement.
///
/// MariaDB's `SHOW GRANTS` answers with the account's stored hash inline —
/// `GRANT ALL PRIVILEGES ON *.* TO `app`@`%` IDENTIFIED BY PASSWORD '*01E86B…'` —
/// and for `mysql_native_password` that hash *is* the credential: the client
/// proves knowledge of it, so anyone who reads it off a screen or a screenshot
/// can authenticate as the account. MySQL 8 dropped it from `SHOW GRANTS`, and
/// this is what makes the two engines agree.
///
/// **The rule is positional, not a list of spellings.** Every literal after an
/// `IDENTIFIED` keyword is replaced, *except* one introduced by `WITH` or `VIA`,
/// which names the plugin rather than the secret — so `IDENTIFIED WITH
/// 'caching_sha2_password' AS '$A$005$…'` keeps the plugin and loses the hash,
/// and `IDENTIFIED BY PASSWORD '*01E8…'`, `IDENTIFIED BY 'plaintext'` and
/// MariaDB's `IDENTIFIED VIA … USING '*01E8…'` all lose theirs, without this
/// having to know which server wrote which. Scanning stops at `REQUIRE`, whose
/// literals are X.509 subjects and issuers — public by nature, and the reason a
/// blanket "redact every literal" would be worse rather than safer.
///
/// Literal boundaries come from [`skip_noncode`], the one SQL boundary lexer, so
/// an apostrophe inside a hash cannot end the span early.
pub fn redact_secrets(stmt: &str, dialect: SqlDialect) -> String {
    let b = stmt.as_bytes();
    let mut out = String::with_capacity(stmt.len());
    let mut i = 0usize;
    // Set once `IDENTIFIED` has been seen and until `REQUIRE` clears it.
    let mut in_identified = false;
    // The last keyword seen at code level, to tell a plugin name from a secret.
    let mut prev_word = String::new();
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, dialect) {
            let span = &stmt[i..j];
            // Only a single-quoted literal, which is the one spelling a secret
            // arrives in on either engine. MySQL also reads `"…"` as a string,
            // but no server writes an `IDENTIFIED` clause that way, and on
            // PostgreSQL the same bytes are an identifier — redacting them would
            // blank out an object name.
            if in_identified
                && b[i] == b'\''
                && !prev_word.eq_ignore_ascii_case("WITH")
                && !prev_word.eq_ignore_ascii_case("VIA")
            {
                out.push_str("<hidden>");
            } else {
                out.push_str(span);
            }
            // A quoted identifier — the account in `TO `app`@`%`` — is not a
            // keyword, but it must not leave a stale one behind either.
            prev_word.clear();
            i = j;
            continue;
        }
        if is_word_byte(b[i]) {
            let start = i;
            while i < b.len() && is_word_byte(b[i]) {
                i += 1;
            }
            let word = &stmt[start..i];
            if word.eq_ignore_ascii_case("IDENTIFIED") {
                in_identified = true;
            } else if word.eq_ignore_ascii_case("REQUIRE") {
                in_identified = false;
            }
            prev_word = word.to_string();
            out.push_str(word);
            continue;
        }
        // Whitespace and punctuation carry no keyword and clear nothing: `TO
        // `app`@`%` IDENTIFIED BY PASSWORD '…'` has an `@` between words.
        out.push(b[i] as char);
        i += 1;
    }
    out
}

// ── writing: what a GRANT or a REVOKE says ───────────────────────────────────

/// What a privilege applies to.
///
/// **The two engines mean different things by the same words**, which is why
/// this is one enum with per-dialect arms rather than a shared shape: a MySQL
/// `GRANT SELECT ON db.*` reaches every table in the database, while a
/// PostgreSQL `GRANT … ON DATABASE d` grants privileges on the *database object*
/// — CONNECT, CREATE, TEMPORARY — and says nothing at all about the tables in
/// it. Folding those into one "database level" would produce a form whose
/// meaning changed under the user when they switched connections.
///
/// Which arms an engine offers is [`levels_for`], and what each can carry is
/// [`privileges_for`]. Nothing here is a `dialect ==` at a call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantLevel {
    /// Every database on the server — MySQL's `*.*`.
    ///
    /// **PostgreSQL has no such level**, and this is a statement about
    /// PostgreSQL rather than a gap: its cluster-wide powers are *role
    /// attributes* (`SUPERUSER`, `CREATEDB`, `REPLICATION`), carried on the role
    /// and set with `ALTER ROLE` — not privileges granted on an object, and not
    /// something `GRANT` can express.
    Global,
    /// MySQL `db.*`; PostgreSQL `DATABASE d`. See the note above — the same word
    /// for two different reaches.
    Database(String),
    /// PostgreSQL only. MySQL has no namespace inside a database.
    Schema(String),
    /// One table. `qualifier` is the database on MySQL and the schema on
    /// PostgreSQL — whatever the level immediately above a table is called
    /// there, which is the only thing the statement needs it for.
    Table { qualifier: String, name: String },
    /// PostgreSQL only. MySQL has no sequence object to grant on.
    Sequence { qualifier: String, name: String },
}

/// The kinds of [`GrantLevel`] an engine offers, without a value in them — what
/// the form's level picker is built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantLevelKind {
    Global,
    Database,
    Schema,
    Table,
    Sequence,
}

impl GrantLevelKind {
    /// The word the level picker shows.
    pub fn label(self) -> &'static str {
        match self {
            GrantLevelKind::Global => "Whole server",
            GrantLevelKind::Database => "Database",
            GrantLevelKind::Schema => "Schema",
            GrantLevelKind::Table => "Table",
            GrantLevelKind::Sequence => "Sequence",
        }
    }
}

impl GrantLevel {
    pub fn kind(&self) -> GrantLevelKind {
        match self {
            GrantLevel::Global => GrantLevelKind::Global,
            GrantLevel::Database(_) => GrantLevelKind::Database,
            GrantLevel::Schema(_) => GrantLevelKind::Schema,
            GrantLevel::Table { .. } => GrantLevelKind::Table,
            GrantLevel::Sequence { .. } => GrantLevelKind::Sequence,
        }
    }

    /// Everything after `ON`, already quoted — including PostgreSQL's object
    /// keyword, which MySQL does not use.
    ///
    /// This is SQL that will be **executed**, so names go through
    /// [`crate::export::ident_sql`] and its unconditional quoting rather than
    /// the read-it-yourself rule the browser's list uses.
    fn object_sql(&self, dialect: SqlDialect) -> String {
        let q = |n: &str| crate::export::ident_sql(n, dialect);
        match self {
            GrantLevel::Global => "*.*".to_string(),
            // **Every dialect named**, not `_ =>`. This is the file's only
            // dialect match, and a catch-all here answers for an engine nobody
            // has looked at: SQLite reaches this today only through a caller
            // that skipped `supports_user_admin`, and a fourth engine would take
            // PostgreSQL's spelling silently rather than failing to compile.
            // `levels_for` already gives SQLite an empty list for the same
            // reason, so the arms below are what a wrong call site gets rather
            // than what a user sees.
            GrantLevel::Database(d) => match dialect {
                SqlDialect::MySql => format!("{}.*", q(d)),
                SqlDialect::Postgres | SqlDialect::Sqlite => format!("DATABASE {}", q(d)),
            },
            GrantLevel::Schema(s) => format!("SCHEMA {}", q(s)),
            GrantLevel::Table { qualifier, name } => match dialect {
                SqlDialect::MySql => format!("{}.{}", q(qualifier), q(name)),
                SqlDialect::Postgres | SqlDialect::Sqlite => {
                    format!("TABLE {}.{}", q(qualifier), q(name))
                }
            },
            GrantLevel::Sequence { qualifier, name } => {
                format!("SEQUENCE {}.{}", q(qualifier), q(name))
            }
        }
    }
}

/// The levels `dialect` can grant at, in the order the picker lists them —
/// widest first, which is the order they nest.
pub fn levels_for(dialect: SqlDialect) -> &'static [GrantLevelKind] {
    match dialect {
        SqlDialect::MySql => &[
            GrantLevelKind::Global,
            GrantLevelKind::Database,
            GrantLevelKind::Table,
        ],
        SqlDialect::Postgres => &[
            GrantLevelKind::Database,
            GrantLevelKind::Schema,
            GrantLevelKind::Table,
            GrantLevelKind::Sequence,
        ],
        // Unreachable through the UI — `supports_user_admin` is the gate — and an
        // empty list rather than a panic, so a caller that skipped the gate gets
        // a picker with nothing in it instead of a crash.
        SqlDialect::Sqlite => &[],
    }
}

/// Every privilege `dialect` accepts at `level`, in the order its own
/// documentation lists them.
///
/// **Curated, not exhaustive, on MySQL's global level.** `GRANT` there also
/// takes the server-administration privileges (`SHUTDOWN`, `SUPER`,
/// `REPLICATION SLAVE`, and in MySQL 8 several dozen dynamic ones like
/// `BACKUP_ADMIN`) — a list that differs by server *and by version*, that no
/// catalogue publishes as a menu, and that nobody should be handed a checkbox
/// for next to `SELECT`. What is here is the set that acts on data and schema,
/// which is what this form is for; anything else is a statement to write in the
/// editor, where the whole language is available.
///
/// PostgreSQL's lists are the complete ones, and are *read off*
/// [`PgObjectKind::all_privileges`] rather than restated — the same table the
/// browser's [`pg_grant_statements`] collapses `ALL PRIVILEGES` against, so a
/// set this form offers and a set that reads back as complete cannot disagree.
pub fn privileges_for(dialect: SqlDialect, level: GrantLevelKind) -> &'static [&'static str] {
    match (dialect, level) {
        (SqlDialect::MySql, GrantLevelKind::Global | GrantLevelKind::Database) => &[
            "SELECT",
            "INSERT",
            "UPDATE",
            "DELETE",
            "CREATE",
            "DROP",
            "ALTER",
            "INDEX",
            "REFERENCES",
            "CREATE VIEW",
            "SHOW VIEW",
            "TRIGGER",
            "EXECUTE",
            "CREATE ROUTINE",
            "ALTER ROUTINE",
            "CREATE TEMPORARY TABLES",
            "LOCK TABLES",
            "EVENT",
        ],
        (SqlDialect::MySql, GrantLevelKind::Table) => &[
            "SELECT",
            "INSERT",
            "UPDATE",
            "DELETE",
            "CREATE",
            "DROP",
            "ALTER",
            "INDEX",
            "REFERENCES",
            "CREATE VIEW",
            "SHOW VIEW",
            "TRIGGER",
        ],
        (SqlDialect::Postgres, GrantLevelKind::Database) => PgObjectKind::Database.all_privileges(),
        (SqlDialect::Postgres, GrantLevelKind::Schema) => PgObjectKind::Schema.all_privileges(),
        (SqlDialect::Postgres, GrantLevelKind::Table) => PgObjectKind::Table.all_privileges(),
        (SqlDialect::Postgres, GrantLevelKind::Sequence) => PgObjectKind::Sequence.all_privileges(),
        // A level the engine does not have — `levels_for` never offers it, and an
        // empty list is what a form built from one should show.
        _ => &[],
    }
}

/// One `GRANT` or `REVOKE` as the form describes it, before it is a statement.
///
/// The same struct for both directions: what a revoke takes away is exactly what
/// a grant gives, and `WITH GRANT OPTION` is the one asymmetry — a revoke of it
/// alone is `REVOKE GRANT OPTION`, which is not what this form does, so the flag
/// is simply ignored on the revoke side rather than given a second field nobody
/// sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivilegeChange {
    pub account: Principal,
    pub level: GrantLevel,
    /// Privileges as [`privileges_for`] spells them. Empty is refused by
    /// [`privilege_sql`] rather than emitted — `GRANT ON …` is a syntax error,
    /// and an empty selection is a form nobody finished.
    pub privileges: Vec<String>,
    pub with_grant_option: bool,
}

/// The statement, or `None` when there is nothing to say.
///
/// `None` for an empty privilege list, which is the one way this can be handed a
/// change that has no statement: `GRANT ON db.* TO 'a'@'%'` is a syntax error,
/// and the form's Apply is what should be refusing it — this is the backstop, so
/// a caller that forgot emits nothing rather than a broken statement the preview
/// then offers to run.
pub fn privilege_sql(c: &PrivilegeChange, dialect: SqlDialect, revoke: bool) -> Option<String> {
    if c.privileges.is_empty() {
        return None;
    }
    let list = c.privileges.join(", ");
    let object = c.level.object_sql(dialect);
    let account = account_sql(&c.account, dialect);
    Some(if revoke {
        format!("REVOKE {list} ON {object} FROM {account}")
    } else {
        let mut sql = format!("GRANT {list} ON {object} TO {account}");
        if c.with_grant_option {
            sql.push_str(" WITH GRANT OPTION");
        }
        sql
    })
}

/// What the grant form is about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GrantSubject {
    #[default]
    Privileges,
    Role,
}

impl GrantSubject {
    pub fn label(self) -> &'static str {
        match self {
            GrantSubject::Privileges => "Privileges",
            GrantSubject::Role => "Role",
        }
    }
}

/// The grant form's state, before it is a [`PrivilegeChange`].
///
/// **Two name fields for five levels**, because that is how the levels nest: a
/// database and a schema each name one thing, a table and a sequence name one
/// thing *inside* another, and the whole-server level names none. Giving each
/// level its own pair of fields would be five forms with one shape.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GrantDraft {
    /// Privileges on an object, or membership of a role. **One form, because
    /// they are one question** — *what may this account do* — asked at two
    /// levels of indirection, and because the direction toggle, the account and
    /// the preview are the same for both. Two forms would be two places to keep
    /// the direction right.
    pub subject: GrantSubject,
    /// Take away rather than give. The one field that changes which statement
    /// this becomes, and it is a field rather than two forms for the reason
    /// [`PrivilegeChange`] serves both directions: what a revoke takes away is
    /// exactly what a grant gives.
    pub revoke: bool,
    /// The role, for [`GrantSubject::Role`]. Free text with the browser's own
    /// account list behind it as a shortcut, like every other name field here —
    /// a role created since the browser opened can still be typed.
    pub role: String,
    /// `WITH ADMIN OPTION`, the role half's counterpart to
    /// [`GrantDraft::with_grant_option`]. Ignored on a revoke.
    pub with_admin_option: bool,
    /// `None` until a level is picked — the state the form opens in, and the
    /// reason [`GrantDraft::level`] can answer "not yet".
    pub level: Option<GrantLevelKind>,
    /// The database (`Database`), the schema (`Schema`), or the thing the table
    /// or sequence is *in*: its database on MySQL, its schema on PostgreSQL.
    pub qualifier: String,
    /// The table or sequence. Unused at the three levels above it.
    pub name: String,
    pub privileges: Vec<String>,
    pub with_grant_option: bool,
}

impl GrantDraft {
    /// The level this draft names, or `None` while it names none — no level
    /// picked, or a level whose fields are still empty.
    pub fn level(&self) -> Option<GrantLevel> {
        let q = self.qualifier.trim();
        let n = self.name.trim();
        match self.level? {
            GrantLevelKind::Global => Some(GrantLevel::Global),
            GrantLevelKind::Database => {
                (!q.is_empty()).then(|| GrantLevel::Database(q.to_string()))
            }
            GrantLevelKind::Schema => (!q.is_empty()).then(|| GrantLevel::Schema(q.to_string())),
            GrantLevelKind::Table => (!q.is_empty() && !n.is_empty()).then(|| GrantLevel::Table {
                qualifier: q.to_string(),
                name: n.to_string(),
            }),
            GrantLevelKind::Sequence => {
                (!q.is_empty() && !n.is_empty()).then(|| GrantLevel::Sequence {
                    qualifier: q.to_string(),
                    name: n.to_string(),
                })
            }
        }
    }

    /// The change this draft describes, or `None` while it describes none.
    ///
    /// **The form's one completeness question, asked in one place** — a level
    /// with its names filled in and at least one privilege ticked. Apply reads
    /// exactly this, so a form that cannot produce a statement cannot offer one:
    /// the alternative is an enabled button whose plan turns out to be
    /// `GRANT ON …`, which [`privilege_sql`] would then have to refuse a second
    /// time, further from the user.
    pub fn change(&self, account: &Principal) -> Option<PrivilegeChange> {
        if self.privileges.is_empty() {
            return None;
        }
        Some(PrivilegeChange {
            account: account.clone(),
            level: self.level()?,
            privileges: self.privileges.clone(),
            with_grant_option: self.with_grant_option,
        })
    }

    /// The role membership this draft describes, or `None` while it names no
    /// role.
    ///
    /// The role is built with **no host**, on both engines: MySQL accepts a bare
    /// role name in `GRANT` and treats it as `'role'@'%'`, and writing a host we
    /// guessed would name a different account than the one the user typed. It is
    /// the same choice [`AccountDraft::principal`] makes when it creates one.
    pub fn role_change(&self, account: &Principal) -> Option<RoleChange> {
        let role = self.role.trim();
        if role.is_empty() {
            return None;
        }
        Some(RoleChange {
            role: Principal {
                name: role.to_string(),
                host: None,
                kind: PrincipalKind::Role,
                system: false,
                attributes: Vec::new(),
            },
            member: account.clone(),
            with_admin_option: self.with_admin_option,
        })
    }

    /// Is the form complete enough to produce a statement? The one question
    /// Apply asks, whichever subject the form is on.
    pub fn is_ready(&self, account: &Principal) -> bool {
        match self.subject {
            GrantSubject::Privileges => self.change(account).is_some(),
            GrantSubject::Role => self.role_change(account).is_some(),
        }
    }

    /// Tick or untick one privilege, keeping the list in the order
    /// [`privileges_for`] gives — so the statement reads the way the engine's
    /// own documentation lists them however the boxes were clicked.
    pub fn toggle(&mut self, privilege: &str, order: &[&'static str]) {
        if let Some(i) = self.privileges.iter().position(|p| p == privilege) {
            self.privileges.remove(i);
            return;
        }
        self.privileges.push(privilege.to_string());
        self.privileges.sort_by_key(|p| {
            order
                .iter()
                .position(|k| k.eq_ignore_ascii_case(p))
                .unwrap_or(order.len())
        });
    }
}

/// One role membership, in either direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleChange {
    /// The role being handed out.
    pub role: Principal,
    /// Who gets it.
    pub member: Principal,
    /// `WITH ADMIN OPTION` — the right to grant the role on. Ignored on the
    /// revoke side, like [`PrivilegeChange::with_grant_option`].
    pub with_admin_option: bool,
}

pub fn role_sql(c: &RoleChange, dialect: SqlDialect, revoke: bool) -> String {
    let role = account_sql(&c.role, dialect);
    let member = account_sql(&c.member, dialect);
    if revoke {
        format!("REVOKE {role} FROM {member}")
    } else {
        let mut sql = format!("GRANT {role} TO {member}");
        if c.with_admin_option {
            sql.push_str(" WITH ADMIN OPTION");
        }
        sql
    }
}

/// A new account, as the form describes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountDraft {
    pub name: String,
    /// MySQL/MariaDB only — see [`Principal::host`]. Empty means `%`, which is
    /// what MySQL itself defaults an unqualified `CREATE USER` to.
    pub host: String,
    pub kind: PrincipalKind,
    /// **Held in memory only, and never written anywhere but the statement.**
    /// Not persisted, not logged, and not carried into the browser's state — the
    /// draft is dropped as soon as the plan is built. See
    /// [`account_draft_sql`]'s note on the preview.
    pub password: String,
}

impl AccountDraft {
    /// The account this draft would create, for the statements that name it.
    pub fn principal(&self, dialect: SqlDialect) -> Principal {
        Principal {
            name: self.name.clone(),
            // A role has no host on either engine's `CREATE ROLE`, and MySQL
            // fills one in itself; carrying `None` keeps `account_sql` from
            // writing an `@'%'` the statement must not have.
            host: match (dialect, self.kind) {
                (SqlDialect::MySql, PrincipalKind::User) => Some(if self.host.trim().is_empty() {
                    "%".to_string()
                } else {
                    self.host.trim().to_string()
                }),
                _ => None,
            },
            kind: self.kind,
            system: false,
            attributes: Vec::new(),
        }
    }
}

/// `CREATE USER` / `CREATE ROLE`, or `None` for a draft with no name.
///
/// **The password is in the statement, and the statement is shown in the
/// preview.** That is deliberate and it is the only honest option: the preview
/// is the app's one gate between a plan and a server, and a statement it showed
/// with the password blanked would not be the statement it ran. What follows
/// from it is that the *preview* is where a password is briefly visible, and
/// nowhere else — [`redact_secrets`] keeps it out of everything read back from
/// the server, and the draft is dropped once the plan is built.
///
/// An empty password emits no `IDENTIFIED BY` clause at all, which is a real and
/// useful account on both engines: PostgreSQL's is one that must authenticate
/// some other way (`peer`, `cert`, `trust`), MySQL's is one that has not been
/// given a password yet.
pub fn account_draft_sql(d: &AccountDraft, dialect: SqlDialect) -> Option<String> {
    if d.name.trim().is_empty() {
        return None;
    }
    let who = account_sql(&d.principal(dialect), dialect);
    let keyword = match d.kind {
        PrincipalKind::User => "CREATE USER",
        PrincipalKind::Role => "CREATE ROLE",
    };
    let mut sql = format!("{keyword} {who}");
    if !d.password.is_empty() {
        // A role takes no password on either engine — MySQL rejects it, and a
        // PostgreSQL role with `LOGIN` off has nothing to use one for.
        if d.kind == PrincipalKind::User {
            sql.push_str(&format!(
                " {} {}",
                match dialect {
                    // PostgreSQL's `CREATE USER` already implies LOGIN; the
                    // password clause is a bare `PASSWORD`.
                    SqlDialect::Postgres | SqlDialect::Sqlite => "PASSWORD",
                    SqlDialect::MySql => "IDENTIFIED BY",
                },
                crate::schema::ddl_string(&d.password, dialect)
            ));
        }
    }
    Some(sql)
}

/// `DROP USER` / `DROP ROLE` for an existing account.
pub fn drop_account_sql(p: &Principal, dialect: SqlDialect) -> String {
    let keyword = match p.kind {
        PrincipalKind::User => "DROP USER",
        PrincipalKind::Role => "DROP ROLE",
    };
    format!("{keyword} {}", account_sql(p, dialect))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn my(user: &str, host: &str) -> MyUserRow {
        MyUserRow {
            user: user.to_string(),
            host: host.to_string(),
            ..Default::default()
        }
    }

    // ── capability ───────────────────────────────────────────────────────────

    #[test]
    fn sqlite_has_no_accounts_and_both_server_engines_do() {
        assert!(!supports_users(SqlDialect::Sqlite));
        assert!(supports_users(SqlDialect::MySql));
        assert!(supports_users(SqlDialect::Postgres));
    }

    #[test]
    fn admin_is_computed_from_browsing_rather_than_restated() {
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert_eq!(supports_user_admin(d), supports_users(d), "{d:?}");
        }
    }

    // ── MySQL/MariaDB folding ────────────────────────────────────────────────

    /// **A role carries no host, because no statement that names one accepts
    /// a host.** MariaDB stores a role in `mysql.user` with `host = ''`, and
    /// measured against 10.11 every statement this module builds from that pair
    /// is refused: `SHOW GRANTS FOR 'r'@''` answers ERROR 1141, `GRANT … TO
    /// 'r'@''` answers ERROR 1133 "Can't find any matching row in the user
    /// table", and `DROP ROLE 'r'@''` is ERROR 1064 — a *syntax* error, because
    /// MariaDB's `DROP ROLE` grammar has no `@host` in it at all. The bare name
    /// works for all three, and on MySQL 8 — where a role is stored under `%` —
    /// a bare role name resolves to exactly that account.
    ///
    /// It is dropped in the fold rather than in each statement builder because
    /// the display name is wrong too: `Some("")` renders as `readers@`, with a
    /// trailing `@`, in the list, the detail heading, the preview's subject and
    /// the Drop confirm's title.
    #[test]
    fn a_role_carries_no_host_because_no_statement_naming_one_accepts_it() {
        let rows = [MyUserRow {
            is_role: Some("Y".into()),
            ..my("readers", "")
        }];
        let p = &from_mysql_rows(&rows)[0];
        assert_eq!(p.host, None);
        assert_eq!(p.display(), "readers");
        assert_eq!(account_sql(p, SqlDialect::MySql), "`readers`");
        assert_eq!(
            drop_account_sql(p, SqlDialect::MySql),
            "DROP ROLE `readers`"
        );
    }

    /// And a *user* keeps its host, which is the half that must not move: a
    /// MySQL account **is** the pair.
    #[test]
    fn a_user_keeps_the_host_that_makes_it_a_distinct_account() {
        let p = &from_mysql_rows(&[my("app", "localhost")])[0];
        assert_eq!(p.host.as_deref(), Some("localhost"));
        assert_eq!(account_sql(p, SqlDialect::MySql), "'app'@'localhost'");
    }

    #[test]
    fn a_mariadb_role_row_reads_as_a_role() {
        let rows = [MyUserRow {
            is_role: Some("Y".into()),
            ..my("readers", "")
        }];
        let out = from_mysql_rows(&rows);
        assert_eq!(out[0].kind, PrincipalKind::Role);
    }

    /// MySQL 8 has no `is_role`, and its roles are stored as locked,
    /// password-expired users. Reading that pair back as "role" would relabel
    /// every genuinely locked account, so a MySQL row is always a user.
    #[test]
    fn a_locked_mysql_account_is_not_guessed_to_be_a_role() {
        let rows = [MyUserRow {
            account_locked: Some("Y".into()),
            password_expired: Some("Y".into()),
            ..my("app", "%")
        }];
        let out = from_mysql_rows(&rows);
        assert_eq!(out[0].kind, PrincipalKind::User);
        assert!(out[0].attributes.contains(&("Locked".into(), "Yes".into())));
    }

    /// An absent column is "this server does not publish it", so the attribute
    /// is absent too — not reported as `No`, which would be a claim.
    #[test]
    fn a_column_the_server_lacks_produces_no_attribute() {
        let out = from_mysql_rows(&[my("app", "%")]);
        assert!(out[0].attributes.iter().all(|(k, _)| k != "Locked"));
        assert!(
            out[0]
                .attributes
                .iter()
                .all(|(k, _)| k != "Password expired")
        );
    }

    /// `mysql@localhost` is in the list because it is the one the prefix misses:
    /// MariaDB's own socket-auth account, which was sorting among the
    /// administrator's and being offered a Drop.
    #[test]
    fn server_owned_mysql_accounts_sort_last() {
        let rows = [
            my("mysql.sys", "localhost"),
            my("mariadb.sys", "localhost"),
            my("mysql", "localhost"),
            my("zeta", "%"),
        ];
        let out = from_mysql_rows(&rows);
        assert_eq!(out[0].name, "zeta");
        assert!(out[1..].iter().all(|p| p.system), "{out:?}");
    }

    /// And the prefix still only catches a *prefix*: an account whose name
    /// merely begins with those letters is the administrator's.
    #[test]
    fn an_account_that_only_looks_like_the_servers_is_not_marked_as_it() {
        for name in ["mysqldump", "mysql_backup", "mariadbctl"] {
            let out = from_mysql_rows(&[my(name, "%")]);
            assert!(!out[0].system, "{name}");
        }
    }

    #[test]
    fn one_account_name_two_hosts_stay_two_accounts_in_host_order() {
        let out = from_mysql_rows(&[my("app", "%"), my("app", "localhost")]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].display(), "app@%");
        assert_eq!(out[1].display(), "app@localhost");
    }

    // ── the GRANTEE fallback parser ──────────────────────────────────────────

    #[test]
    fn a_grantee_cell_splits_into_the_pair_mysql_user_would_have_given() {
        assert_eq!(parse_grantee("'app'@'%'"), Some(("app".into(), "%".into())));
        assert_eq!(
            parse_grantee("'mysql.sys'@'localhost'"),
            Some(("mysql.sys".into(), "localhost".into()))
        );
    }

    /// `@` is legal inside an account name, so the split is the quoting, not the
    /// first `@` — which would have read this as user `a` on host `b'@'%'`.
    #[test]
    fn an_at_sign_inside_the_name_does_not_split_it() {
        assert_eq!(parse_grantee("'a@b'@'%'"), Some(("a@b".into(), "%".into())));
    }

    /// **The unquoter's rule has to be the scanner's.** `skip_noncode` reads
    /// this literal as MySQL, where a backslash escapes — so a cell of
    /// `'o\'brien'` is one literal to it, and undoubling alone left the
    /// backslash in the account name and built every statement against an
    /// account that does not exist.
    #[test]
    fn a_backslash_escape_is_undone_the_way_the_scanner_read_it() {
        assert_eq!(
            parse_grantee(r"'o\'brien'@'%'"),
            Some(("o'brien".into(), "%".into()))
        );
        assert_eq!(
            parse_grantee(r"'back\\slash'@'%'"),
            Some((r"back\slash".into(), "%".into()))
        );
    }

    /// A multi-byte name survives both paths — the escape branch slices one byte
    /// past a backslash, which is only a char boundary because the byte after an
    /// escape starts a character.
    #[test]
    fn a_non_ascii_account_name_is_unquoted_whole() {
        assert_eq!(
            parse_grantee("'café'@'%'"),
            Some(("café".into(), "%".into()))
        );
    }

    #[test]
    fn a_doubled_quote_comes_back_as_one() {
        assert_eq!(
            parse_grantee("'o''brien'@'%'"),
            Some(("o'brien".into(), "%".into()))
        );
    }

    #[test]
    fn a_cell_that_is_not_an_account_pair_is_refused_rather_than_guessed() {
        for s in ["", "app@%", "'app'", "'app'@", "'app'@'%' ", "'app'@'%'x"] {
            assert_eq!(parse_grantee(s), None, "{s:?}");
        }
    }

    /// The composition the fallback actually performs: parse the cell, fold the
    /// pair, and get back the same display name the privileged path produces.
    #[test]
    fn a_parsed_grantee_folds_into_the_same_principal_as_a_mysql_user_row() {
        let (user, host) = parse_grantee("'app'@'localhost'").unwrap();
        let out = from_mysql_rows(&[my(&user, &host)]);
        assert_eq!(out[0].display(), "app@localhost");
        assert_eq!(account_sql(&out[0], SqlDialect::MySql), "'app'@'localhost'");
    }

    // ── PostgreSQL folding ───────────────────────────────────────────────────

    fn pg(name: &str, canlogin: bool) -> PgRoleRow {
        PgRoleRow {
            name: name.to_string(),
            canlogin,
            inherit: true,
            connlimit: -1,
            ..Default::default()
        }
    }

    #[test]
    fn rolcanlogin_is_the_user_role_split() {
        let out = from_pg_rows(&[pg("app", true), pg("readers", false)]);
        let app = out.iter().find(|p| p.name == "app").unwrap();
        let readers = out.iter().find(|p| p.name == "readers").unwrap();
        assert_eq!(app.kind, PrincipalKind::User);
        assert_eq!(readers.kind, PrincipalKind::Role);
    }

    #[test]
    fn a_postgres_role_has_no_host_part_in_its_display_name() {
        let out = from_pg_rows(&[pg("app", true)]);
        assert_eq!(out[0].host, None);
        assert_eq!(out[0].display(), "app");
    }

    #[test]
    fn only_the_attributes_that_are_set_are_listed() {
        let out = from_pg_rows(&[PgRoleRow {
            superuser: true,
            ..pg("postgres", true)
        }]);
        let keys: Vec<&str> = out[0].attributes.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"Superuser"));
        assert!(!keys.contains(&"Create role"));
        // -1 is "no limit" and says nothing worth a row.
        assert!(!keys.contains(&"Connection limit"));
    }

    #[test]
    fn a_role_that_does_not_inherit_says_so_because_the_default_is_that_it_does() {
        let out = from_pg_rows(&[PgRoleRow {
            inherit: false,
            ..pg("app", true)
        }]);
        assert!(out[0].attributes.contains(&("Inherit".into(), "No".into())));
    }

    #[test]
    fn predefined_pg_roles_sort_last() {
        let out = from_pg_rows(&[pg("pg_monitor", false), pg("zeta", true)]);
        assert_eq!(out[0].name, "zeta");
        assert!(out[1].system);
    }

    // ── the filter ───────────────────────────────────────────────────────────

    #[test]
    fn the_filter_searches_both_halves_of_a_mysql_account() {
        let p = &from_mysql_rows(&[my("app", "localhost")])[0];
        assert!(matches(p, "APP"));
        assert!(matches(p, "localhost"));
        assert!(matches(p, "app@local"));
        assert!(!matches(p, "prod"));
        assert!(matches(p, "   "));
    }

    // ── account_sql ──────────────────────────────────────────────────────────

    /// `account_sql` quotes for SQL that is **executed** (`SHOW GRANTS FOR …`),
    /// so a PostgreSQL role is quoted unconditionally — unlike the grantee
    /// inside a statement the user only reads.
    #[test]
    fn a_mysql_account_is_two_literals_and_a_postgres_role_is_an_identifier() {
        let my_acct = &from_mysql_rows(&[my("app", "%")])[0];
        assert_eq!(account_sql(my_acct, SqlDialect::MySql), "'app'@'%'");
        let pg_role = &from_pg_rows(&[pg("app", true)])[0];
        assert_eq!(account_sql(pg_role, SqlDialect::Postgres), "\"app\"");
    }

    #[test]
    fn a_quote_in_an_account_name_cannot_end_the_literal() {
        let p = &from_mysql_rows(&[my("o'brien", "%")])[0];
        assert_eq!(account_sql(p, SqlDialect::MySql), "'o''brien'@'%'");
    }

    /// The read-only rendering is the other rule: a plain name stays bare, and
    /// one that would not survive PostgreSQL's case folding is quoted.
    #[test]
    fn a_grantee_needing_quotes_gets_them_in_a_statement_the_user_reads() {
        assert_eq!(
            pg_membership_statements("Reporting Team", &[("readers".to_string(), false)]),
            vec!["GRANT readers TO \"Reporting Team\""]
        );
    }

    // ── PostgreSQL grant reassembly ──────────────────────────────────────────

    fn acl(kind: PgObjectKind, schema: Option<&str>, name: &str, priv_: &str) -> PgAclRow {
        PgAclRow {
            kind,
            schema: schema.map(str::to_string),
            name: name.to_string(),
            privilege: priv_.to_string(),
            grantable: false,
        }
    }

    #[test]
    fn privileges_on_one_object_become_one_statement_in_the_documented_order() {
        let rows = [
            acl(PgObjectKind::Table, Some("public"), "users", "INSERT"),
            acl(PgObjectKind::Table, Some("public"), "users", "SELECT"),
        ];
        assert_eq!(
            pg_grant_statements("app", &rows),
            vec!["GRANT SELECT, INSERT ON TABLE public.users TO app"]
        );
    }

    #[test]
    fn a_complete_set_collapses_to_all_privileges() {
        let rows: Vec<PgAclRow> = PgObjectKind::Schema
            .all_privileges()
            .iter()
            .map(|p| acl(PgObjectKind::Schema, None, "public", p))
            .collect();
        assert_eq!(
            pg_grant_statements("app", &rows),
            vec!["GRANT ALL PRIVILEGES ON SCHEMA public TO app"]
        );
    }

    /// `WITH GRANT OPTION` belongs to the statement, so two privileges that
    /// disagree about it are two statements — merging them would claim the
    /// grantable one for both.
    #[test]
    fn grantable_and_plain_privileges_on_one_object_stay_separate() {
        let rows = [
            acl(PgObjectKind::Table, Some("public"), "users", "SELECT"),
            PgAclRow {
                grantable: true,
                ..acl(PgObjectKind::Table, Some("public"), "users", "UPDATE")
            },
        ];
        assert_eq!(
            pg_grant_statements("app", &rows),
            vec![
                "GRANT SELECT ON TABLE public.users TO app",
                "GRANT UPDATE ON TABLE public.users TO app WITH GRANT OPTION",
            ]
        );
    }

    /// The grouping walks the rows once and compares each against the group
    /// before it, which is correct only because the fetch orders by object. This
    /// is the interleaved input that would break a one-pass grouper that assumed
    /// more than it is given: it must still produce statements naming the right
    /// privileges on the right objects, even if it opens two groups for one.
    #[test]
    fn rows_out_of_object_order_still_name_the_right_privileges() {
        let rows = [
            acl(PgObjectKind::Table, Some("public"), "users", "SELECT"),
            acl(PgObjectKind::Table, Some("public"), "orders", "INSERT"),
            acl(PgObjectKind::Table, Some("public"), "users", "UPDATE"),
        ];
        let out = pg_grant_statements("app", &rows);
        assert_eq!(
            out,
            vec![
                "GRANT SELECT ON TABLE public.users TO app",
                "GRANT INSERT ON TABLE public.orders TO app",
                "GRANT UPDATE ON TABLE public.users TO app",
            ]
        );
    }

    /// **The order the fetch actually delivers**, for the key component the
    /// grouper's own comment does not mention. `grantable` is the fourth part of
    /// the group key, and the three ACL queries order by
    /// `nspname, relname, privilege_type` with no `is_grantable` term — so a
    /// table holding `INSERT`, `UPDATE` plainly and `SELECT` with grant option
    /// arrived interleaved and printed **three** statements where the
    /// administrator wrote two.
    ///
    /// **Both orders, so the requirement is visible rather than assumed.** The
    /// fix itself is three `ORDER BY` clauses in `pg::fetch_grants`, which no
    /// unit test here can reach — so this pins the property that makes them
    /// necessary: the same three rows produce two statements in one order and
    /// three in the other, and it is the *fetch* that decides which arrives.
    #[test]
    fn a_mixed_grant_option_object_is_split_only_when_the_rows_interleave() {
        let grantable = |mut r: PgAclRow| {
            r.grantable = true;
            r
        };
        // What `ORDER BY privilege_type` alone delivered: INSERT(f), SELECT(t),
        // UPDATE(f) — three statements for one table, and no `ALL PRIVILEGES`
        // collapse available in either fragment.
        let interleaved = [
            acl(PgObjectKind::Table, Some("public"), "users", "INSERT"),
            grantable(acl(PgObjectKind::Table, Some("public"), "users", "SELECT")),
            acl(PgObjectKind::Table, Some("public"), "users", "UPDATE"),
        ];
        assert_eq!(pg_grant_statements("app", &interleaved).len(), 3);

        // What `ORDER BY is_grantable, privilege_type` delivers: the two the
        // administrator actually wrote.
        let grouped = [
            acl(PgObjectKind::Table, Some("public"), "users", "INSERT"),
            acl(PgObjectKind::Table, Some("public"), "users", "UPDATE"),
            grantable(acl(PgObjectKind::Table, Some("public"), "users", "SELECT")),
        ];
        assert_eq!(
            pg_grant_statements("app", &grouped),
            vec![
                "GRANT INSERT, UPDATE ON TABLE public.users TO app",
                "GRANT SELECT ON TABLE public.users TO app WITH GRANT OPTION",
            ]
        );
    }

    /// And the ordinary case — the one the queries actually deliver — is one
    /// statement per object.
    #[test]
    fn rows_in_object_order_become_one_statement_per_object() {
        let rows = [
            acl(PgObjectKind::Table, Some("public"), "orders", "SELECT"),
            acl(PgObjectKind::Table, Some("public"), "orders", "INSERT"),
            acl(PgObjectKind::Table, Some("public"), "users", "SELECT"),
        ];
        assert_eq!(
            pg_grant_statements("app", &rows),
            vec![
                "GRANT SELECT, INSERT ON TABLE public.orders TO app",
                "GRANT SELECT ON TABLE public.users TO app",
            ]
        );
    }

    #[test]
    fn a_database_grant_names_no_schema() {
        let rows = [acl(PgObjectKind::Database, None, "warehouse", "CONNECT")];
        assert_eq!(
            pg_grant_statements("app", &rows),
            vec!["GRANT CONNECT ON DATABASE warehouse TO app"]
        );
    }

    #[test]
    fn an_object_name_needing_quotes_gets_them_in_the_statement() {
        let rows = [acl(PgObjectKind::Table, Some("public"), "Order", "SELECT")];
        assert_eq!(
            pg_grant_statements("app", &rows),
            vec!["GRANT SELECT ON TABLE public.\"Order\" TO app"]
        );
    }

    /// A privilege a newer server publishes and this build has no place for is
    /// printed rather than dropped, and does not pass for a complete set.
    #[test]
    fn an_unknown_privilege_is_kept_last_and_blocks_the_all_collapse() {
        let mut rows: Vec<PgAclRow> = PgObjectKind::Schema
            .all_privileges()
            .iter()
            .map(|p| acl(PgObjectKind::Schema, None, "public", p))
            .collect();
        rows.push(acl(PgObjectKind::Schema, None, "public", "MAINTAIN"));
        assert_eq!(
            pg_grant_statements("app", &rows),
            vec!["GRANT CREATE, USAGE, MAINTAIN ON SCHEMA public TO app"]
        );
    }

    #[test]
    fn role_membership_renders_with_and_without_admin_option() {
        assert_eq!(
            pg_membership_statements(
                "app",
                &[
                    ("readers".to_string(), false),
                    ("writers".to_string(), true)
                ]
            ),
            vec![
                "GRANT readers TO app",
                "GRANT writers TO app WITH ADMIN OPTION",
            ]
        );
    }

    // ── redaction ────────────────────────────────────────────────────────────

    /// The statement MariaDB 10.11 actually answers `SHOW GRANTS` with.
    #[test]
    fn mariadbs_inline_password_hash_never_reaches_the_screen() {
        let stmt = "GRANT ALL PRIVILEGES ON *.* TO `schemaic`@`%` IDENTIFIED BY PASSWORD \
                    '*01E86B61E37A95BF82B53ACA83AF06D6BA89793C' WITH GRANT OPTION";
        let out = redact_secrets(stmt, SqlDialect::MySql);
        assert!(!out.contains("01E86B61"), "{out}");
        assert_eq!(
            out,
            "GRANT ALL PRIVILEGES ON *.* TO `schemaic`@`%` IDENTIFIED BY PASSWORD <hidden> \
             WITH GRANT OPTION"
        );
    }

    #[test]
    fn a_plaintext_password_goes_the_same_way() {
        let out = redact_secrets(
            "CREATE USER 'a'@'%' IDENTIFIED BY 'hunter2'",
            SqlDialect::MySql,
        );
        assert_eq!(out, "CREATE USER 'a'@'%' IDENTIFIED BY <hidden>");
    }

    /// The literal after `WITH` names the plugin, not the secret — keeping it is
    /// what makes the redaction readable rather than a row of placeholders.
    #[test]
    fn the_plugin_name_survives_and_the_hash_after_it_does_not() {
        let stmt = "CREATE USER `a`@`%` IDENTIFIED WITH 'caching_sha2_password' AS '$A$005$xyz'";
        assert_eq!(
            redact_secrets(stmt, SqlDialect::MySql),
            "CREATE USER `a`@`%` IDENTIFIED WITH 'caching_sha2_password' AS <hidden>"
        );
    }

    #[test]
    fn mariadbs_via_using_spelling_is_covered_too() {
        let stmt =
            "GRANT USAGE ON *.* TO `a`@`%` IDENTIFIED VIA mysql_native_password USING '*01E8'";
        assert_eq!(
            redact_secrets(stmt, SqlDialect::MySql),
            "GRANT USAGE ON *.* TO `a`@`%` IDENTIFIED VIA mysql_native_password USING <hidden>"
        );
    }

    /// **The `VIA` half of the exemption, on an input that actually reaches
    /// it.** The test above feeds a *bare-word* plugin, which never gets as far
    /// as the literal branch — so deleting `&& !prev_word…("VIA")` changed
    /// nothing any test supplied and the whole suite stayed green. MariaDB
    /// quotes the plugin here whenever it is not an identifier, which `ed25519`
    /// is not on every build.
    #[test]
    fn a_quoted_plugin_name_after_via_survives_and_the_hash_after_it_does_not() {
        let stmt = "GRANT USAGE ON *.* TO `a`@`%` IDENTIFIED VIA 'ed25519' USING 'HASH'";
        assert_eq!(
            redact_secrets(stmt, SqlDialect::MySql),
            "GRANT USAGE ON *.* TO `a`@`%` IDENTIFIED VIA 'ed25519' USING <hidden>"
        );
    }

    /// X.509 subjects and issuers are public by nature and are what a blanket
    /// "redact every literal after IDENTIFIED" would have destroyed.
    #[test]
    fn a_require_clause_keeps_its_literals() {
        let stmt = "GRANT USAGE ON *.* TO `a`@`%` IDENTIFIED BY PASSWORD '*01E8' \
                    REQUIRE SUBJECT '/CN=app'";
        let out = redact_secrets(stmt, SqlDialect::MySql);
        assert!(out.contains("'/CN=app'"), "{out}");
        assert!(!out.contains("*01E8"), "{out}");
    }

    /// The account name is a literal too, and it comes *before* `IDENTIFIED`.
    #[test]
    fn the_account_the_grant_is_for_is_not_redacted() {
        let out = redact_secrets("GRANT USAGE ON *.* TO 'app'@'localhost'", SqlDialect::MySql);
        assert_eq!(out, "GRANT USAGE ON *.* TO 'app'@'localhost'");
    }

    /// The one boundary lexer, not a hand-rolled quote search: a backslash
    /// escape inside the hash must not end the span early and leave the tail on
    /// screen.
    #[test]
    fn an_escaped_quote_inside_the_secret_does_not_end_it_early() {
        let stmt = "CREATE USER 'a'@'%' IDENTIFIED BY 'pa\\'ss word'";
        let out = redact_secrets(stmt, SqlDialect::MySql);
        assert_eq!(out, "CREATE USER 'a'@'%' IDENTIFIED BY <hidden>");
    }

    #[test]
    fn a_statement_with_no_secret_is_returned_unchanged() {
        for stmt in [
            "GRANT SELECT ON `ckdb`.* TO `probe`@`%`",
            "GRANT `testrole`@`%` TO `probe`@`%`",
            "GRANT USAGE ON *.* TO `probe`@`%`",
        ] {
            assert_eq!(redact_secrets(stmt, SqlDialect::MySql), stmt);
        }
    }

    // ── the note ─────────────────────────────────────────────────────────────

    #[test]
    fn the_postgres_note_names_the_database_it_is_limited_to() {
        assert!(pg_scope_note("warehouse", None).contains("warehouse"));
    }

    /// The two notes have to say *different* things, because the lists behind
    /// them are different: one covers a database, the other covers none.
    #[test]
    fn the_no_database_note_does_not_claim_to_cover_one() {
        let n = pg_no_database_note(None);
        assert!(n.contains("not listed"), "{n}");
        assert_ne!(n, pg_scope_note("postgres", None));
    }

    /// **The note has to enumerate what the read cannot see, or it certifies a
    /// wrong answer as complete.** `aclexplode` reads explicit entries only, so
    /// ownership, superuser and a `pg_*` role's wired-in powers leave nothing
    /// for it to find — and clicking `pg_read_all_data` printed "This account
    /// holds no privileges."
    #[test]
    fn the_postgres_note_admits_the_privileges_no_acl_entry_records() {
        for n in [pg_scope_note("warehouse", None), pg_no_database_note(None)] {
            assert!(n.contains("owning an object"), "{n}");
            assert!(n.contains("superuser"), "{n}");
            assert!(n.contains("role membership"), "{n}");
        }
    }

    /// And when the read *can* say which of them applies, it leads with it: the
    /// superuser sentence changes how everything below it should be read.
    #[test]
    fn a_superusers_note_says_so_before_it_says_anything_else() {
        let su =
            pg_implicit_note("root", true, 0, "warehouse").expect("a superuser has something say");
        assert!(su.contains("SUPERUSER"), "{su}");
        assert!(pg_scope_note("warehouse", Some(&su)).starts_with(&su));

        // Ownership is counted, and named with the database it was counted in —
        // the count is per-database for the same reason the ACL read is.
        let owns = pg_implicit_note("app", false, 14, "warehouse").expect("an owner does too");
        assert!(owns.contains("14"), "{owns}");
        assert!(owns.contains("warehouse"), "{owns}");
        assert!(pg_implicit_note("app", false, 1, "w").is_some_and(|s| s.contains("1 object in")));

        // A role that is neither adds no sentence, rather than a sentence
        // saying it owns nothing — the note is already long.
        assert_eq!(pg_implicit_note("app", false, 0, "warehouse"), None);
        // A superuser's sentence wins over the ownership count: it subsumes it.
        assert_eq!(
            pg_implicit_note("root", true, 14, "warehouse"),
            pg_implicit_note("root", true, 0, "warehouse")
        );
    }

    /// **The case the finding was reported from.** `pg_read_all_data` is a
    /// superuser of nothing and owns nothing, so every ACL query returns empty
    /// and the pane printed "This account holds no privileges." — about a role
    /// whose entire purpose is reading every table in the cluster.
    #[test]
    fn a_predefined_role_is_not_reported_as_holding_nothing() {
        let n = pg_implicit_note("pg_read_all_data", false, 0, "warehouse")
            .expect("a predefined role's powers are recorded nowhere this reads");
        assert!(n.contains("pg_read_all_data"), "{n}");
        assert!(n.contains("predefined"), "{n}");
        // Both, when both apply, in one sentence rather than two notes.
        let both = pg_implicit_note("pg_monitor", false, 3, "w").expect("both");
        assert!(both.contains("predefined"), "{both}");
        assert!(both.contains("3 objects"), "{both}");

        assert!(is_pg_predefined("pg_read_all_data"));
        assert!(!is_pg_predefined("pgbouncer"));
        assert!(!is_pg_predefined("app"));
    }

    /// MySQL's list was shipped with `note: None`, and `SHOW GRANTS` is
    /// documented on both servers as direct-only — so on a role-provisioned
    /// server most of what an account can do was missing with nothing saying so.
    #[test]
    fn the_mysql_note_admits_that_a_role_is_not_expanded() {
        let n = my_scope_note();
        assert!(n.contains("role"), "{n}");
        assert!(n.contains("directly"), "{n}");
    }

    /// The filter, as the virtualised list consumes it: positions, in order,
    /// agreeing with `matches` on every element.
    #[test]
    fn the_filter_answers_positions_and_agrees_with_the_predicate() {
        // `from_mysql_rows` sorts, so positions are read back by name rather
        // than assumed — which is also the property under test: the indices are
        // into the list as it stands, because that is what the row builder
        // indexes.
        let list = from_mysql_rows(&[my("app", "%"), my("admin", "localhost"), my("bob", "%")]);
        let at = |name: &str| list.iter().position(|p| p.name == name).expect(name);
        // An empty needle is every index — not an empty list, which would read
        // as "no account matches" on an unfiltered browser.
        assert_eq!(filter_indices(&list, ""), vec![0, 1, 2]);
        assert_eq!(filter_indices(&list, "   "), vec![0, 1, 2]);
        // Both halves of a MySQL account are searchable.
        assert_eq!(filter_indices(&list, "localhost"), vec![at("admin")]);
        assert_eq!(filter_indices(&list, "app@"), vec![at("app")]);
        let mut any_host = vec![at("app"), at("bob")];
        any_host.sort_unstable();
        assert_eq!(filter_indices(&list, "%"), any_host);
        assert!(filter_indices(&list, "nobody").is_empty());
        // And it cannot disagree with the predicate the footer's count and the
        // row's own highlighting still use.
        for needle in ["", "a", "localhost", "%", "APP"] {
            let by_predicate: Vec<usize> = list
                .iter()
                .enumerate()
                .filter(|(_, p)| matches(p, needle))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(filter_indices(&list, needle), by_predicate, "{needle:?}");
        }
    }

    /// **The order of the four answers is the whole content of this gate**, and
    /// it had no test at all — it lived inline in a view with no test module.
    #[test]
    fn the_write_gate_answers_the_most_specific_reason_first() {
        use SqlDialect::{MySql, Postgres, Sqlite};
        // An engine with no accounts omits the action rather than dimming it,
        // whatever else is true — a dimmed button invites a hunt for the setting
        // that would enable it, and there isn't one.
        for read_only in [true, false] {
            for has_db in [true, false] {
                let g = WriteGate::of(Sqlite, read_only, has_db);
                assert_eq!(g, WriteGate::NoEngineSupport);
                assert!(!g.offered(), "{g:?}");
                assert!(!g.enabled(), "{g:?}");
            }
        }
        for d in [MySql, Postgres] {
            // Read-only wins over no-database: it is the reason the user can act
            // on, and it is true regardless of which database is selected.
            assert_eq!(WriteGate::of(d, true, true), WriteGate::ReadOnly);
            assert_eq!(WriteGate::of(d, true, false), WriteGate::ReadOnly);
            assert_eq!(WriteGate::of(d, false, false), WriteGate::NoDatabase);
            assert_eq!(WriteGate::of(d, false, true), WriteGate::Allowed);
            // Every refusal is offered-and-dimmed, and only one is enabled.
            for g in [WriteGate::of(d, true, true), WriteGate::of(d, false, false)] {
                assert!(g.offered(), "{g:?}");
                assert!(!g.enabled(), "{g:?}");
            }
            assert!(WriteGate::of(d, false, true).enabled());
        }
    }

    /// The account list's own note, for the rung where every wider read was
    /// refused. It has to say the list is *not* the server's — a footer reading
    /// "1 account" is otherwise indistinguishable from a server that has one.
    #[test]
    fn the_restricted_account_list_says_it_is_not_the_servers() {
        let n = my_own_account_only_note();
        assert!(n.contains("mysql.user"), "{n}");
        assert!(n.contains("not the server's account list"), "{n}");
        // A complete list carries none, so the note's presence is the signal.
        assert_eq!(Principals::complete(Vec::new()).note, None);
    }

    // ── the levels and their privileges ──────────────────────────────────────

    /// The two levels each engine hasn't got, stated as capability rather than
    /// as an engine comparison at a call site.
    #[test]
    fn each_engine_offers_only_the_levels_it_has() {
        let my = levels_for(SqlDialect::MySql);
        assert!(my.contains(&GrantLevelKind::Global));
        assert!(!my.contains(&GrantLevelKind::Schema));
        assert!(!my.contains(&GrantLevelKind::Sequence));

        let pg = levels_for(SqlDialect::Postgres);
        // PostgreSQL's cluster-wide powers are role attributes, not privileges
        // on an object, so there is no statement this level could emit.
        assert!(!pg.contains(&GrantLevelKind::Global));
        assert!(pg.contains(&GrantLevelKind::Schema));

        assert!(levels_for(SqlDialect::Sqlite).is_empty());
    }

    /// Every level an engine offers has something to grant at it — a picker
    /// entry that opens an empty checkbox list is a dead end.
    #[test]
    fn every_offered_level_has_privileges_to_offer() {
        for d in [SqlDialect::MySql, SqlDialect::Postgres] {
            for &lvl in levels_for(d) {
                assert!(
                    !privileges_for(d, lvl).is_empty(),
                    "{d:?} offers {lvl:?} with nothing to grant"
                );
            }
        }
    }

    /// The composition that matters: what the form offers at a PostgreSQL level
    /// is the same list `pg_grant_statements` collapses to `ALL PRIVILEGES`, so
    /// ticking every box and reading the result back agree.
    #[test]
    fn granting_every_postgres_table_privilege_reads_back_as_all_privileges() {
        let privs: Vec<String> = privileges_for(SqlDialect::Postgres, GrantLevelKind::Table)
            .iter()
            .map(|s| s.to_string())
            .collect();
        let rows: Vec<PgAclRow> = privs
            .iter()
            .map(|p| acl(PgObjectKind::Table, Some("public"), "users", p))
            .collect();
        assert_eq!(
            pg_grant_statements("app", &rows),
            vec!["GRANT ALL PRIVILEGES ON TABLE public.users TO app"]
        );
    }

    // ── the statements ───────────────────────────────────────────────────────

    fn my_account() -> Principal {
        from_mysql_rows(&[my("app", "%")]).remove(0)
    }

    fn pg_account() -> Principal {
        from_pg_rows(&[pg("app", true)]).remove(0)
    }

    fn change(account: Principal, level: GrantLevel, privs: &[&str]) -> PrivilegeChange {
        PrivilegeChange {
            account,
            level,
            privileges: privs.iter().map(|s| s.to_string()).collect(),
            with_grant_option: false,
        }
    }

    #[test]
    fn a_mysql_grant_names_the_object_the_way_mysql_does() {
        let c = change(my_account(), GrantLevel::Global, &["SELECT", "INSERT"]);
        assert_eq!(
            privilege_sql(&c, SqlDialect::MySql, false).unwrap(),
            "GRANT SELECT, INSERT ON *.* TO 'app'@'%'"
        );
        let c = change(
            my_account(),
            GrantLevel::Database("shop".into()),
            &["SELECT"],
        );
        assert_eq!(
            privilege_sql(&c, SqlDialect::MySql, false).unwrap(),
            "GRANT SELECT ON `shop`.* TO 'app'@'%'"
        );
        let c = change(
            my_account(),
            GrantLevel::Table {
                qualifier: "shop".into(),
                name: "orders".into(),
            },
            &["SELECT"],
        );
        assert_eq!(
            privilege_sql(&c, SqlDialect::MySql, false).unwrap(),
            "GRANT SELECT ON `shop`.`orders` TO 'app'@'%'"
        );
    }

    /// PostgreSQL needs the object keyword MySQL has no use for, and the same
    /// level word means a different reach on each — which is why the two are
    /// separate arms rather than one format string.
    #[test]
    fn a_postgres_grant_carries_the_object_keyword() {
        let c = change(
            pg_account(),
            GrantLevel::Database("shop".into()),
            &["CONNECT"],
        );
        assert_eq!(
            privilege_sql(&c, SqlDialect::Postgres, false).unwrap(),
            "GRANT CONNECT ON DATABASE \"shop\" TO \"app\""
        );
        let c = change(
            pg_account(),
            GrantLevel::Schema("public".into()),
            &["USAGE"],
        );
        assert_eq!(
            privilege_sql(&c, SqlDialect::Postgres, false).unwrap(),
            "GRANT USAGE ON SCHEMA \"public\" TO \"app\""
        );
        let c = change(
            pg_account(),
            GrantLevel::Sequence {
                qualifier: "public".into(),
                name: "order_id_seq".into(),
            },
            &["USAGE"],
        );
        assert_eq!(
            privilege_sql(&c, SqlDialect::Postgres, false).unwrap(),
            "GRANT USAGE ON SEQUENCE \"public\".\"order_id_seq\" TO \"app\""
        );
    }

    #[test]
    fn with_grant_option_is_on_the_grant_and_never_on_the_revoke() {
        let mut c = change(my_account(), GrantLevel::Global, &["SELECT"]);
        c.with_grant_option = true;
        assert!(
            privilege_sql(&c, SqlDialect::MySql, false)
                .unwrap()
                .ends_with(" WITH GRANT OPTION")
        );
        assert_eq!(
            privilege_sql(&c, SqlDialect::MySql, true).unwrap(),
            "REVOKE SELECT ON *.* FROM 'app'@'%'"
        );
    }

    /// `GRANT ON …` is a syntax error, and the preview must never be offered one.
    #[test]
    fn a_change_with_no_privileges_has_no_statement() {
        let c = change(my_account(), GrantLevel::Global, &[]);
        assert_eq!(privilege_sql(&c, SqlDialect::MySql, false), None);
        assert_eq!(privilege_sql(&c, SqlDialect::MySql, true), None);
    }

    #[test]
    fn a_role_grant_reverses_into_a_revoke_with_no_option_clause() {
        let c = RoleChange {
            role: from_pg_rows(&[pg("readers", false)]).remove(0),
            member: pg_account(),
            with_admin_option: true,
        };
        assert_eq!(
            role_sql(&c, SqlDialect::Postgres, false),
            "GRANT \"readers\" TO \"app\" WITH ADMIN OPTION"
        );
        assert_eq!(
            role_sql(&c, SqlDialect::Postgres, true),
            "REVOKE \"readers\" FROM \"app\""
        );
    }

    /// **The one statement where both quoting rules meet.** On MySQL and
    /// MariaDB a role has no host, so it is an *identifier*; the member is the
    /// `(user, host)` pair, so it is two *literals* — one statement, two rules,
    /// which is the entire reason `account_sql` branches on `host` rather than
    /// on dialect. The test above is PostgreSQL on both sides, where the two
    /// rules coincide and the composition proves nothing.
    #[test]
    fn a_mysql_role_grant_names_the_role_as_an_identifier_and_the_member_as_a_pair() {
        let c = RoleChange {
            role: from_mysql_rows(&[MyUserRow {
                user: "readers".into(),
                host: String::new(),
                is_role: Some("Y".into()),
                ..Default::default()
            }])
            .remove(0),
            member: my_account(),
            with_admin_option: true,
        };
        assert_eq!(
            role_sql(&c, SqlDialect::MySql, false),
            "GRANT `readers` TO 'app'@'%' WITH ADMIN OPTION"
        );
        assert_eq!(
            role_sql(&c, SqlDialect::MySql, true),
            "REVOKE `readers` FROM 'app'@'%'"
        );
    }

    /// **The revoke direction, at every level and on both engines.** It was
    /// pinned at exactly one shape — MySQL at `Global` — so four of `object_sql`'s
    /// five arms had never been asked to spell a `REVOKE … FROM` at all, and
    /// PostgreSQL never revoked anything in the default suite.
    #[test]
    fn a_revoke_names_the_same_object_the_grant_did_at_every_level() {
        let cases: Vec<(SqlDialect, Principal, GrantLevel, &str)> = vec![
            (SqlDialect::MySql, my_account(), GrantLevel::Global, "*.*"),
            (
                SqlDialect::MySql,
                my_account(),
                GrantLevel::Database("shop".into()),
                "`shop`.*",
            ),
            (
                SqlDialect::MySql,
                my_account(),
                GrantLevel::Table {
                    qualifier: "shop".into(),
                    name: "orders".into(),
                },
                "`shop`.`orders`",
            ),
            (
                SqlDialect::Postgres,
                pg_account(),
                GrantLevel::Database("shop".into()),
                "DATABASE \"shop\"",
            ),
            (
                SqlDialect::Postgres,
                pg_account(),
                GrantLevel::Schema("sales".into()),
                "SCHEMA \"sales\"",
            ),
            (
                SqlDialect::Postgres,
                pg_account(),
                GrantLevel::Table {
                    qualifier: "sales".into(),
                    name: "orders".into(),
                },
                "TABLE \"sales\".\"orders\"",
            ),
            (
                SqlDialect::Postgres,
                pg_account(),
                GrantLevel::Sequence {
                    qualifier: "sales".into(),
                    name: "orders_id_seq".into(),
                },
                "SEQUENCE \"sales\".\"orders_id_seq\"",
            ),
        ];
        for (dialect, account, level, object) in cases {
            let c = change(account, level.clone(), &["SELECT"]);
            let grant = privilege_sql(&c, dialect, false).expect("a grant");
            let revoke = privilege_sql(&c, dialect, true).expect("a revoke");
            // The same object, named identically both ways — the arm is shared,
            // and this is what says the revoke reaches it.
            assert!(grant.contains(object), "{level:?}: {grant}");
            assert!(revoke.contains(object), "{level:?}: {revoke}");
            // …and the keyword and the preposition are the ones that undo it.
            assert!(grant.starts_with("GRANT "), "{grant}");
            assert!(revoke.starts_with("REVOKE "), "{revoke}");
            assert!(grant.contains(" TO "), "{grant}");
            assert!(revoke.contains(" FROM "), "{revoke}");
            assert!(!revoke.contains(" TO "), "{revoke}");
        }
    }

    // ── the grant form's draft ───────────────────────────────────────────────

    #[test]
    fn a_draft_with_no_level_picked_describes_nothing() {
        let d = GrantDraft::default();
        assert_eq!(d.level(), None);
        assert_eq!(d.change(&my_account()), None);
    }

    #[test]
    fn a_level_whose_names_are_still_empty_describes_nothing() {
        for kind in [
            GrantLevelKind::Database,
            GrantLevelKind::Schema,
            GrantLevelKind::Table,
        ] {
            let d = GrantDraft {
                level: Some(kind),
                ..Default::default()
            };
            assert_eq!(d.level(), None, "{kind:?}");
        }
        // A table with a qualifier and no name is still half a name.
        let d = GrantDraft {
            level: Some(GrantLevelKind::Table),
            qualifier: "shop".into(),
            ..Default::default()
        };
        assert_eq!(d.level(), None);
    }

    /// The whole-server level names nothing, so it is complete the moment it is
    /// picked — the one level for which an empty name field is right.
    #[test]
    fn the_whole_server_level_needs_no_name() {
        let d = GrantDraft {
            level: Some(GrantLevelKind::Global),
            ..Default::default()
        };
        assert_eq!(d.level(), Some(GrantLevel::Global));
    }

    /// The completeness question the Apply button reads: a level *and* a
    /// privilege. Either alone is a form nobody finished.
    #[test]
    fn a_level_with_no_privileges_ticked_still_describes_nothing() {
        let d = GrantDraft {
            level: Some(GrantLevelKind::Global),
            ..Default::default()
        };
        assert!(d.level().is_some());
        assert_eq!(d.change(&my_account()), None);
    }

    /// The composition Apply actually performs: a complete draft reaches a
    /// statement, and it is the statement the level says it is.
    #[test]
    fn a_complete_draft_reaches_the_statement_its_level_describes() {
        let mut d = GrantDraft {
            level: Some(GrantLevelKind::Table),
            qualifier: "shop".into(),
            name: "orders".into(),
            ..Default::default()
        };
        d.toggle(
            "SELECT",
            privileges_for(SqlDialect::MySql, GrantLevelKind::Table),
        );
        let c = d.change(&my_account()).unwrap();
        assert_eq!(
            privilege_sql(&c, SqlDialect::MySql, false).unwrap(),
            "GRANT SELECT ON `shop`.`orders` TO 'app'@'%'"
        );
    }

    /// Ticked in any order, listed in the engine's — so two people who picked
    /// the same privileges get the same statement.
    #[test]
    fn privileges_are_listed_in_the_engines_order_however_they_were_ticked() {
        let order = privileges_for(SqlDialect::MySql, GrantLevelKind::Table);
        let mut d = GrantDraft::default();
        for p in ["DELETE", "SELECT", "INSERT"] {
            d.toggle(p, order);
        }
        assert_eq!(d.privileges, vec!["SELECT", "INSERT", "DELETE"]);
    }

    #[test]
    fn ticking_a_privilege_twice_unticks_it() {
        let order = privileges_for(SqlDialect::MySql, GrantLevelKind::Table);
        let mut d = GrantDraft::default();
        d.toggle("SELECT", order);
        d.toggle("SELECT", order);
        assert!(d.privileges.is_empty());
    }

    // ── creating and dropping ────────────────────────────────────────────────

    fn draft(name: &str, kind: PrincipalKind) -> AccountDraft {
        AccountDraft {
            name: name.to_string(),
            kind,
            ..Default::default()
        }
    }

    #[test]
    fn a_mysql_user_with_no_host_gets_the_one_mysql_would_have_defaulted_to() {
        let d = draft("app", PrincipalKind::User);
        assert_eq!(
            account_draft_sql(&d, SqlDialect::MySql).unwrap(),
            "CREATE USER 'app'@'%'"
        );
    }

    #[test]
    fn a_password_is_spelled_the_way_each_engine_spells_it() {
        let d = AccountDraft {
            password: "hunter2".into(),
            host: "localhost".into(),
            ..draft("app", PrincipalKind::User)
        };
        assert_eq!(
            account_draft_sql(&d, SqlDialect::MySql).unwrap(),
            "CREATE USER 'app'@'localhost' IDENTIFIED BY 'hunter2'"
        );
        assert_eq!(
            account_draft_sql(&d, SqlDialect::Postgres).unwrap(),
            "CREATE USER \"app\" PASSWORD 'hunter2'"
        );
    }

    /// A quote in a password must not end the literal and change the statement —
    /// the same rule an account name goes through, and the reason both take the
    /// one literal quoter.
    #[test]
    fn a_quote_in_a_password_cannot_end_the_statement() {
        let d = AccountDraft {
            password: "it's".into(),
            ..draft("app", PrincipalKind::User)
        };
        assert_eq!(
            account_draft_sql(&d, SqlDialect::MySql).unwrap(),
            "CREATE USER 'app'@'%' IDENTIFIED BY 'it''s'"
        );
    }

    /// A role takes no password on either engine, and no host: `CREATE ROLE
    /// 'r'@'%' IDENTIFIED BY …` is rejected by MySQL and meaningless on
    /// PostgreSQL.
    #[test]
    fn a_role_is_created_bare() {
        let d = AccountDraft {
            password: "ignored".into(),
            host: "localhost".into(),
            ..draft("readers", PrincipalKind::Role)
        };
        assert_eq!(
            account_draft_sql(&d, SqlDialect::MySql).unwrap(),
            "CREATE ROLE `readers`"
        );
        assert_eq!(
            account_draft_sql(&d, SqlDialect::Postgres).unwrap(),
            "CREATE ROLE \"readers\""
        );
    }

    #[test]
    fn an_empty_password_emits_no_clause_at_all() {
        let d = draft("app", PrincipalKind::User);
        assert!(
            !account_draft_sql(&d, SqlDialect::Postgres)
                .unwrap()
                .contains("PASSWORD")
        );
    }

    #[test]
    fn a_nameless_draft_has_no_statement() {
        assert_eq!(
            account_draft_sql(&draft("   ", PrincipalKind::User), SqlDialect::MySql),
            None
        );
    }

    #[test]
    fn dropping_says_user_or_role_to_match_what_it_is() {
        assert_eq!(
            drop_account_sql(&my_account(), SqlDialect::MySql),
            "DROP USER 'app'@'%'"
        );
        let role = from_pg_rows(&[pg("readers", false)]).remove(0);
        assert_eq!(
            drop_account_sql(&role, SqlDialect::Postgres),
            "DROP ROLE \"readers\""
        );
    }
}
