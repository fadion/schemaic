# `core/launch.rs`

`launch.rs` — **what may be handed to an OS launcher**: the boundary where a string the app
merely *displayed* becomes a string the operating system *executes*. Two rules hold for
everything in it, both stated in the module doc. **No shell, with one named exception** — the
argv these functions emit is executed directly, nothing they emit is parsed by `cmd`, `sh` or
PowerShell *by this app*, so a byte that is syntax to a shell is inert; the alternative, filtering
every metacharacter of every shell, is how `&` would come to be refused inside a query string
while `%` still expanded. The exception is `xdg-open`, the Linux arm of `url_open_argv`, which
`xdg-utils` ships as a `#!/bin/sh` script — so the admitted sub-delims do reach an interpreter
there, and the argument that they are inert in it is an argument about third-party code that
nothing here re-checks. It was not verified against a real `xdg-open` (none on the machine), which
is recorded rather than glossed; the guarantee that remains is one argv element carrying only RFC
3986's allowlist. `explorer` and `open` are genuine executables and the rule holds on those two
exactly as stated, which is what the `LAUNCHERS` table's second column records. And **the
string is validated where it stops being data**, not at the site that produced it: a producer may
have its own good reasons to be permissive — the terminal's link tagger admits `&` because a query
string needs one — and the launcher does not inherit them.
`openable_url` and `url_open_argv` are the browser half, and they exist because a clicked
terminal link ran arbitrary commands. `open_url` guarded the *scheme* only and then built
`cmd /C start "" <url>` itself; Rust's Windows argument encoder quotes an argument only when it
is empty or contains a space or a tab, so `&` arrived at `cmd`'s parser raw and was read there as
a statement separator. At the other end `schemaic-term`'s `is_url_char` admits `&`, correctly,
and `tag_links` marks any run of more than eight such bytes starting at an `http(s)://` — so
**any text the terminal panel had merely printed** (a `cat`, a `git log` commit message, an AI
CLI streaming a fetched page) containing `https://<8+ chars>&<command>` was drawn as a clickable
link, and one click ran `<command>`. Measured with `start` swapped for an inert `echo`: two
commands ran. Windows only — the macOS and Linux arms already passed the URL to `open`/`xdg-open`
as a single argv entry.
The shape is part of the answer, which is why `url_open_argv` returns the whole argv rather than
a `bool`: a URL is only as safe as the program it is handed to. On Windows that program is
`explorer`, which takes argv and is already this app's launcher for a folder — the `cmd /C start`
idiom existed only to get a detached browser, which `explorer` gives directly. `openable_url` is
an allowlist of RFC 3986's unreserved and reserved sets and nothing else (no control byte, no
space, no non-ASCII, and none of `"`, `<`, `>`, `^`, `|`, `\`, `` ` ``, `{`, `}`), with a
case-insensitive `http`/`https` scheme, a non-empty authority, and well-formed
percent-encoding — that last one because `%windir%` is a legal-looking path fragment and `cmd`
expands it *before* it parses anything, so a `%` that does not begin a `%XX` escape is the tell
of a string written for a shell rather than for a browser. The sub-delims `&`, `;`, `$`, `'`,
`(`, `)` are **kept**, deliberately, because a real query string needs them and nothing
downstream reads them as syntax. `no_shell_ever_reads_a_url` is what holds that: it asserts the
*composition*, that a hostile URL is either refused outright or reaches a program that is not a
shell as exactly one argv element, so a future launcher that reintroduces a shell fails it
however well the gate filters.
`psql_target` is the same rule with no shell anywhere in sight, which makes it the clearer
instance. **libpq re-parses the value of `-d`**: an argument containing `=`, or beginning
`postgresql://`/`postgres://`, is taken as a whole *conninfo string*, so a database named
`dbname=postgres host=evil.example.com` overrides the `-h` three arguments earlier and sends the
session — with `PGPASSWORD` in its environment — to a server of the name's choosing, while
Schemaic's own header goes on naming the intended one (measured against PostgreSQL 16). The name
comes from the *server*, so it is not the user's to be trusted with: on a shared or compromised
server anyone who may `CREATE DATABASE` writes it, and the schema tree's "Open in CLI" put it
straight on the argv. psql has no `--` terminator to hide behind — the detection is on the
*value*, not the position — so this one **refuses**, with a message, which is a shape the caller
already had for "no client found". The MySQL half of the same finding is fixed the other way
round, in `main.rs`'s `mysql_shell_config`: a `--` before the positional database name, because
`--pager=touch /tmp/PWN` is dangerous only as an *option* and only because of where it sits in
the argv (that name ran a shell command on the user's first query — measured, MariaDB 10.11.14
under a pty). A name filter here would be the wrong tool for it.
`mysql_cli_tls_args` and `psql_cli_tls_env` carry the connection's `tls` to the spawned client,
which neither builder used to do at all: `conn.tls` reached neither, so the client fell back to
its own default — `--ssl-mode=PREFERRED` / `sslmode=prefer`, which accepts an unencrypted socket
and verifies no certificate — while the password went over it in `MYSQL_PWD`/`PGPASSWORD`, and
the app's header, whose own socket really was encrypted, went on reporting TLS. **Saying nothing
is not neutral**, so each is non-empty wherever an omitted flag would change the meaning — for
MySQL's client that is every rung *including* `Disable`, its default being `--ssl-mode=PREFERRED`,
which accepts an unencrypted socket if the server offers one and verifies no certificate. MySQL
gets an `--ssl-mode=` flag for every rung plus `--ssl-ca`/`--ssl-cert`/`--ssl-key`; psql gets
`PGSSLMODE`/`PGSSLROOTCERT`/`PGSSLCERT`/`PGSSLKEY`, and the **environment rather than the argv**
is not symmetry with the password — psql has no `--sslmode` flag at all, the setting exists only
inside a conninfo string, which is the one thing `psql_target` refuses to let this path build.
Which files are named is left to `connection::Tls::ca_file`/`uses_client_cert` rather than
re-decided here, so a non-verifying mode names no CA and half a client pair names neither.
**Which MySQL-family client is being spawned is a capability, `MysqlClient::of(prog)`**, because
the two spellings are mutually fatal: `--ssl-mode` is unknown to MariaDB's client and MySQL
removed `--ssl` in 8.0.26. MariaDB's arm has one rung where the **empty** answer is the right
one, and it is measured rather than inherited from the sentence above: `Prefer` emits nothing,
and MariaDB 10.11.14's client with no `--ssl` flag at all, against a server reporting
`have_ssl = YES`, comes back with `Ssl_cipher = TLS_AES_256_GCM_SHA384`. It is opportunistic, so
the empty answer really is this rung, and `--ssl` there would turn a preference into a
requirement. Had it measured blank instead, the CLI session would have been plaintext while the
app's own driver on the same `Connection` was encrypted and the terminal badged it as this
connection's client. `mysql` staying Oracle's is a known residual — on a modern MariaDB install
`mysql` is a symlink to the MariaDB client and no inspection of the *name* tells them apart — so
this closes the case that was measured, a box whose only client is `mariadb`, and leaves the
symlink case as it was rather than trading it for a new one.
**`VerifyCa` is `--ssl` plus `--ssl-ca` and no `--ssl-verify-server-cert`, and that is the rung,
not a dropped check** — also measured, 2026-10-23, with the MariaDB 10.11.14 client against a
TLS-enabled 10.11 server: `--ssl --ssl-ca=<an unrelated self-made CA>` is refused (*TLS/SSL error:
self-signed certificate in certificate chain*), and the server's own CA connects. So the client
verifies the chain on `--ssl-ca` alone, and `--ssl-verify-server-cert` adds only the host-name
check, which is precisely what Verify CA leaves out and `VerifyFull` adds. Read as "Verify CA
sends nothing that verifies", the arm invites a fix that turns it into Verify full.
**That contract spent a while attached to the wrong item.** `44ccfa4` inserted `MysqlClient`'s
doc *between* `mysql_cli_tls_args`' doc block and its signature, so the "non-empty, saying
nothing is not neutral" paragraph became the opening of the published rustdoc for that *enum*
while the function carried no doc at all; `missing_docs` is off, so `cargo doc -D warnings` saw
nothing to complain about.
`wsl_tls_blocker` is the one case where a transport setting cannot be expressed at all: the three
certificate paths travel as argv or as environment values, and a Windows-shaped one (a drive
letter, or any backslash) names a file the Linux client cannot open — which fails as a missing
file or, depending on the rung, proceeds unverified. So it refuses, the direction
`SslMode::STRICTEST` already sends a guess about encryption. A POSIX-shaped path is left alone,
including a `/mnt/c/…` the user typed themselves: this is not a ban on WSL plus TLS, only on
handing a Linux process a drive letter. **That refusal is what lets `wrap_launcher` forward every
variable across `WSLENV` with `/u` and never `/p`** — `/p` is the flag for a value needing
Win→WSL path translation, and by the time a WSL config is built there is deliberately no such
value left to translate.
`tunnelled_verify_blocker` is its sibling for the other transport a CLI session cannot express.
The app's own connect path rewrites the endpoint to `127.0.0.1:<local>` and carries
`hostname_override: Some(conn.host)` in the same step; the CLI builders take that rewritten
`Connection` with its `tls` block untouched, and neither client has an equivalent — `mysql` has
no hostname-override option and `psql` would need the `PGHOST`/`PGHOSTADDR` split — so the client
was told to dial the loopback and check the certificate against it, and refused its own server's
certificate. Only the two verifying rungs ask: `require` and below name no certificate, so the
loopback address *is* the address and nothing is compared. A refusal rather than a silent drop to
`require`, which would open a session that says TLS and checks nothing on a connection whose
whole point is that it checks — the direction `SslMode::STRICTEST` already sends a guess.
`sqlcmd_args` is the SQL Server client's argv, and the one where the database name needs no
refusal at all. Every value is **attached to its flag** — `-d<name>`, `-U<user>`, `-S…` — so
neither `sqlcmd` (the ODBC one or the Go rewrite, which share the name and the flags) can read
one as an option, and the value itself is taken verbatim: measured against ODBC `sqlcmd` 18, a
database named `x;Encrypt=no;Server=192.0.2.1` opens *that database* on *this server*, and one
with a leading `-` does too, so there is no connection-string re-read for a `psql_target` to
refuse (`sqlcmd_takes_every_value_attached_to_its_flag`). Don't split a value off its flag to
tidy the argv: the attachment is what makes the leading `-` inert. The **host** is the one value
`sqlcmd` parses (`host\instance`, `host,port`, a `tcp:`/`np:`/`lpc:`/`admin:` prefix), so it is
first read as the connect reads it (`connection::sql_server_address`) — `tcp:` dropped,
`(local)` as `localhost`, an inline port winning over the saved one — and then forced onto TCP
as `-Stcp:host,port`, an IPv6 literal bracketed: `h,1500` and `h\SQLEXPRESS,1500` both go on as
`-Stcp:h,1500`. Before that reading (S5-L1-02) any `,` was refused, so `db\SQLEXPRESS,1500`
met the HOST message here while the importer dialled `db:1500`. What is left that `sqlcmd`
would reparse is refused rather than guessed at — an `np:`/`lpc:`/`admin:` prefix, a `;`, a
second `\`, a `,` with no port after it (`a_host_sqlcmd_would_reparse_is_refused`). **A named
instance is the one `\` let through**, being `sqlcmd`'s own spelling: `server\INSTANCE` (`connection::sql_server_instance`) goes on as
`-Stcp:server\INSTANCE` with no port, since the saved one is not the instance's and ODBC
`sqlcmd` 18 asks SQL Server Browser itself — measured, an instance with no Browser behind it
answers *Error Locating Server/Instance Specified*, not a syntax error
(`a_named_instance_reaches_sqlcmd_without_a_port`). The rest of the refusal still holds around
it: a second `\`, a `,`, `;` or `:` in the instance, or a prefix on the server part other than
the one `tcp:` the reading drops — so `tcp:h\X` goes on as `-Stcp:h\X` and `(local)\X` as
`-Stcp:localhost\X`. `-I` is
always sent: the client's
`QUOTED_IDENTIFIER` default is off, where an index over a computed column is Msg 1934, and every
Schemaic session has it on. TLS lands on the three things `sqlcmd` can say, each measured:
`disable` → `-No`, unencrypted; `prefer` and `require` → `-Nm -C`, encrypted with the
certificate not judged (SQL Server always offers TLS, so `prefer` has nothing to fall back
from); `verify-ca` and `verify-full` → `-Nm` alone, verified against the system store — a
self-signed certificate fails — which checks the name for `verify-ca` too, the stricter of the
two answers on offer (`each_tls_rung_lands_on_sqlcmds_flags`). **A private CA file is refused,
not dropped**: `sqlcmd` has no option for one — `-J` pins the server's own certificate, a
different trust — and dropping it would verify against a store the connection did not ask for.
A client certificate and key are refused the same way: TDS signs no one in by one, so there is
nowhere to send them, and a session that silently left them out is not the one asked for.
So no certificate path ever reaches this argv, which is why `mssql_shell_config` has no
`wsl_tls_blocker` to ask. **It signs in the way the connection does**, taking the caller's
`effective_auth`: `-U<user>` for a SQL login, `-E` — a trusted connection, the same flag in both
clients — for Windows sign-in with no user sent, and a refusal for Entra, since `sqlcmd` cannot
be handed the Azure CLI's token and falling back to a SQL login would be a different sign-in from
the one saved (`sqlcmd_signs_in_the_way_the_connection_does`). The two cloud IAM modes have an
arm of their own, refused with `AuthMode::external_client_refusal`'s answer — never in force
on SQL Server (`AuthMode::offered`), so it is there for a hand-made one. `mssql_shell_config` adds the one
refusal the argv cannot see: Windows sign-in through a WSL `sqlcmd`, a Linux process having no
Windows identity to present. The password travels as `SQLCMDPASSWORD`, which both clients read,
never as `-P` — `sqlcmd_shell_puts_password_in_env_not_argv` in `app/main.rs`, which pins its
`WSLENV` forwarding as well — and only for a mode that sends one (`uses_credentials`).
`DOCKER_PS_ARGS` and `docker_inspect_args` are the Docker import's two calls
(`conn_sources::docker_source`). The first is a constant; the second is built from the first's
output, which makes it this module's question even with no shell in between — **another
program's output on its way back into a process**. Each line must be a container id as Docker
spells one, 64 lowercase hex digits (`--no-trunc`) or 12, and any other line refuses the whole
call rather than being dropped, since a `docker` that printed something else is not the program
this was written against (`a_line_that_is_not_a_container_id_refuses_the_call`, whose cases
include a `--format=` line that would otherwise have reached the argv as an option).
`--type=container` keeps an image sharing an id prefix out of the answer, and empty output is an
empty argv — no second call at all (`nothing_running_is_no_call_at_all`).
`cli_flag_value(field, value)` is the check for **a connection's own setting as the value of a
cloud CLI's flag** — `core::cloud_iam`'s `aws rds generate-db-auth-token --hostname <host>
--username <user>`, the first argv in which a sign-in CLI is handed anything but constants. The
CLIs read any word beginning with `-` as an option, so a user named `--profile=prod` would sign
in under somebody else's profile rather than fail; an empty value is refused because a flag
with none takes the next flag as one, and a control character because none belongs in a host or
a user (`a_value_shaped_like_an_option_is_refused`, under `cloud_iam.rs`, is what exercises it).
**The backup-and-restore half** is the argv for the engines' own tools, which the database
menu's *Back up* and *Restore from file* run in the terminal panel as *Open in CLI* runs a client
— argv only, the password in the tool's environment variable. `backup_file_target` is the check
every file passes. The path is one the user picked in a dialog, so nothing in it is
server-supplied, but a CR, LF or NUL (or a blank name) is refused, because `mysql`'s `source`
reads to the end of its line and what follows a break would be a second command —
`x.sql\nDROP DATABASE shop` is the test's case
(`a_mysql_restore_sources_the_file_and_refuses_a_line_break`). **A line break is not `mysql`'s only
hazard, so its restore asks `mysql_source_path` too**: the path is spliced into the client's own
command line, which reads `source …` as a command only when the line holds neither its
delimiter `;` nor `\g` — otherwise the line goes to its parser, where `;` cuts the path, `\g`
sends what came before it to the server as SQL, and on Unix `\!` runs a shell command (measured
on MariaDB 10.11's client with a name holding `\! touch …;`). So a path with a `;` or a `\` is
refused, and so is one with space at either end, which the client trims
(`a_mysql_restore_refuses_a_path_its_client_would_parse`). On Windows every path is backslashed
— `C:\Users\me\git\` holds a `\g` — so `mysql_restore_args` passes `cfg!(windows)` and the
separators are turned to `/` first, which the client takes there; that arm has not been run
against a Windows client, none being installed when it landed. `pg_dump_args` writes the custom
format (`-F c -f <file> -d <db>`), which `pg_restore` reads selectively, and the database still
goes through `psql_target`, `pg_dump`'s `-d` being libpq's and re-read as a conninfo string by the
same rule (`pg_dump_writes_the_custom_format_to_the_picked_file`). **`pg_restore_args` returns
the program as well as the argv, because the file decides it**: a `.sql` (in any case) is a plain
script and goes to `psql -v ON_ERROR_STOP=1 -f`, so a half-restored database stops at its first
error rather than being reported done; anything else is `pg_restore --no-owner -- <file>`, the
`--` ahead of a name that may begin with `-`, and `--no-owner` because a dump names roles this
server may not have (`a_sql_script_restores_through_psql_and_a_dump_through_pg_restore`).
**Both PostgreSQL builders take the role init SQL sets**, because the tools run outside the
app's connections and the init SQL every one of those runs never reaches them: a `NOINHERIT`
login with `SET ROLE` in its init SQL got *permission denied* from `pg_dump` without `--role`
and succeeded with it, and with it both restore paths create objects owned by the role
(verified on PostgreSQL 16). `init_role(init_sql, dialect)` reads a plain `SET ROLE <name>` —
the last one, `NONE` being none — from `init_sql::check`'s statements, so a statement init SQL
refuses is never read, and answers `None` on every other engine
(`a_plain_set_role_in_init_sql_is_the_tools_role`). **The name is the role's own spelling**:
a bare one folded where `SqlDialect::folds_unquoted_names`, a quoted one as written — the tools
quote what they are given (`fmtId`), so `SET ROLE AppOwner` once reached `pg_dump` as
`--role=AppOwner` and failed every backup with *role "AppOwner" does not exist* (measured on
PostgreSQL 16) while the app's own connections, folding it, worked. `pg_dump` and `pg_restore` take it as
`--role=<r>`, and a role that is empty, leads with `-` or holds a line break refuses the launch
instead; `psql` has no `--role`, so a `.sql` restore runs `-c "SET ROLE <quoted>"` before its
`-f`, `psql` running the two in order in one session. The role is the one setting there that
decides *whose* the work is; the rest of init SQL still reaches no tool here.
`mysqldump_args` is `--single-transaction --routines --triggers --events --result-file=<file> --
<db>` — one consistent snapshot of the transactional tables with no lock held, the database name
behind `--` for `mysql_shell_config`'s reason, and the TLS flags handed in from
`mysql_cli_tls_args` for whichever tool was found
(`mysqldump_snapshots_with_routines_and_ends_options_before_the_name`); `mysql_restore_args` is
`--execute=source <file> -- <db>`, there being no shell to redirect a file into the client. Which
tool was found is why **`MysqlClient::of` reads `mariadb-dump` as MariaDB too**: the dump tool
spells TLS as its client does, and taken for Oracle's it would be handed an `--ssl-mode` it does
not know (`mariadbs_dump_tool_spells_tls_as_its_client_does`). **And it is why MariaDB's names are
asked for first** — `MYSQL_DUMP_TOOLS` is `mariadb-dump` then `mysqldump`, `MYSQL_RESTORE_TOOLS`
`mariadb` then `mysql`. Where MariaDB's clients are installed (Debian's and Ubuntu's default)
`mysqldump` and `mysql` are symlinks to them; found by those names they read as Oracle's to
`MysqlClient::of`, were handed `--ssl-mode`, which they refuse, and every backup and restore
failed. No Oracle install ships `mariadb-dump`, so the order costs nothing there
(`mariadbs_tools_are_tried_before_the_names_they_share`); *Open in CLI*'s own `mysql`-symlink
residual, on `MysqlClient::of`, is unchanged. `mssql_backup_sql` is T-SQL
rather than an argv — `BACKUP DATABASE [db] TO DISK = N'<file>' WITH COPY_ONLY, INIT`, quoted
through `export::ident_sql` and `sql_literal`
(`a_sql_server_backup_is_quoted_and_leaves_the_backup_chain_alone`). The server writes the file,
so the name is a **bare** one and lands in the server's own default backup folder (verified on
SQL Server 2022 when this landed); `COPY_ONLY` leaves the differential base a scheduled backup
plan relies on where it was; `INIT` replaces a file of the same name rather than appending to it
— which, with the minute-resolution stamp below, means a second backup of one database within the
same minute replaces the first. `backup_stamp_now` is the local time as `%Y%m%d-%H%M`, and
`backup_file_name(db, stamp, ext)` the default name, every character but an alphanumeric, `-`,
`_` or `.` turned to `_` (`a_backup_file_name_is_safe_whatever_the_database_is_called`).
**`restore_offered(dialect)` is an exhaustive capability**, so a fifth engine answers it: MySQL
and PostgreSQL yes; SQL Server no, its `RESTORE` reading a file *on the server* and needing the
database to itself; SQLite no, a backup there being a database file one opens as a connection
(`restore_is_offered_where_a_client_restores_a_local_file`). `backup_writes_server(dialect)` is
the other exhaustive capability on this path: true for SQL Server alone, whose backup is a
`BACKUP DATABASE` the server runs and writes — a file in its own folder, replaced by `INIT`, and
history rows in `msdb` — and false where the tool reads the server into a file on this machine
(`only_sql_servers_backup_writes_on_the_server`); the write-guard invariant's backup paragraph
is what asks it. `restore_runs_a_script(file, dialect)` says whether the restore hands the file
to a client **as a script**, whose `USE` or `\connect` can carry it to another database, so that
it is read first (`script::database_switches`): on MySQL always, on PostgreSQL a `.sql` —
`pg_restore_args`' own rule — and never a `pg_restore` archive, which restores into `-d` unless
asked to create its database (`the_restores_that_run_a_script_are_the_ones_read_first`).
`restore_log_entry(connection, db, file)` is the write-log line a restore writes as it launches —
`Source::Restore`, no `sql`, the detail `restore <file> into <db>, run in the terminal`, its
outcome `Unknown`, because the client in the terminal and not Schemaic sees how it ends
(`a_restore_is_logged_as_launched_with_its_outcome_unknown`).
**Nothing here that refuses answers with a sentence**: every refusal is a `Refusal`, fifteen
`Copy` variants — one per *reason* rather than per function, `psql_target` having two and
`sqlcmd_args` four of its own — and `AuthMode::external_client_refusal`'s `CloudToken` is the
fifteenth, so a launcher and the sign-in check it asks answer through one type.
`Refusal::message` is the English, the text each function used to return as a `&'static str`,
and what the app's `refusal_text` is held to (*The string catalogue*, under *UI conventions*);
`Refusal::ALL` is the list both sides' tests walk. **It has no `Display`, on purpose**: no
headless surface says one, and with it a caller's `why.to_string()` would compile and stay
English in every language — the missing impl is the guard, so don't add one for convenience.
`each_launcher_refuses_with_its_own_reason` has every function answer the variant it should
and asserts the answers, in order, are `ALL`, so a renderer keyed on the variant says the right
sentence and no variant goes unreached. `cli_flag_value` is the one refusal here that is not a
`Refusal`: it is `cloud_iam`'s sign-in, met as a connect error rather than a launcher's
refusal, so it answers a `model::SignInFailure::BadValue` naming the field and the problem —
the type the rest of that sign-in's failures are (under `core::model`).
