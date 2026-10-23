# `core/conn_import.rs`

`conn_import.rs` — reading connections **out of other tools**: a pasted URL/DSN, DBeaver's
`data-sources.json`, DataGrip's (or any JetBrains IDE's) `dataSources.xml`, the three
plain-text files the command-line clients read — `~/.my.cnf`, `~/.pgpass`,
`~/.pg_service.conf` — the database containers Docker is running here (`docker inspect`'s
output, which the app runs rather than reads), and **Schemaic's own export**, the one source this module also writes
(`export_connections`, at the end of this entry). Pure over *text*: `conn_sources` in the app finds and reads the files,
this decides what they mean, which is what makes the whole surface unit-testable. Every parser
answers with `Imported` values carrying **`id: 0`** and a *suggested* name — an import is a
proposal, and the app assigns the real id and uniquifies the name only when the user accepts a
row. Nothing here touches the keyring.
`scan` is the one entry point, and its **order is the design**: `.pgpass` passwords are applied
(`fill_missing_passwords`) after every source has parsed, so a `*:*:*:me:secret` line can
complete the twelve server-only rows DataGrip's own configuration just handed over — not a
project's, which is a file the user was sent (`PgpassScope`, below); `dedupe` collapses repeats
*after* that, keeping the **first** description of a server, so the survivor is the one that
already has the name a human chose — and **merging rather than discarding**, through `absorb`,
which takes from the dropped row whatever the survivor lacks — an empty password, and the TLS
material below — and nothing else. There is deliberately **no `ssh` arm**: a tunnel is not a
field a source can merely be missing, it decides *which* machine `db.internal:5432` is, so two
rows differing in it are two connections and `same_endpoint` says so; moving one across gave the
survivor an SSH session to a jump host the user never configured for it, on every connect, with
no SSH column in the review list to show it (the review row now names the tunnel's server after
the target, `… via user@host:port`, beside an `ImportNote::ThroughSsh`). It only discarded, on the strength of
`fill_missing_passwords` having run first, and that function's only matcher opens with
`is_postgres`: there is no `~/.my.cnf` analogue, so on MySQL the source order did the *opposite*
of the rationale `conn_sources::discover` states for it — a DBeaver row, whose passwords this
module deliberately does not read, swallowed the `~/.my.cnf` row that had one, and `scan` then
tagged the survivor `ImportNote::NoPassword` with the password sitting in plaintext in a file
the same scan had just read. Merging closes the class rather than that one file, and takes
**only what the survivor lacks**, a later and worse row never overwriting a better one's value.
**DBeaver's jump-server settings are not read**: its SSH handler writes `SshTunnel::jump` empty
on purpose, since a route half carried over would connect by a path the user did not configure.
And `same_endpoint` compares `jump` beside the SSH host and port, for the reason it compares
those: a saved connection through a jump host and an imported direct row to the same bastion
are two routes, and `mark_existing` would otherwise untick the imported one as already saved.
**TLS is the exception to "a field at a time", because the three paths are not independent
fields and merging them one by one builds a configuration neither source described.** The mode
merges by strength (`SslMode::stronger_of`, never an equality sentinel — that branch was once
guarded by `a.tls == Tls::default()` while `blank()` was floored at `Prefer`, so it was false for
every row this module produces and the merge simply never ran). Its material does not follow the
same rule. A `VerifyFull` row with an empty `ca_path` is not *missing* a CA — in every client
imported from here an absent path means the system trust store, the ordinary way to verify a
public certificate — so filling it from a weaker row pins the trust anchor to a CA that did not
issue the server's certificate, and **every** connect then fails with a verification error the
user has no way to trace back to an import they ran once. The CA crosses only onto an empty
`ca_path` and only while the survivor's own mode does not protect *strictly* more than the
absorbed row's (`SslMode::protects_more_than`): at that point the dropped row's verification is
at least as demanding, and its anchor is the one that belongs with it. The client certificate and
its key cross **as a unit** or not at all — taken a field at a time, a survivor holding a
certificate takes the other row's key and the handshake fails on a mismatched pair, a config that
existed in neither source.
The dropped row's notes do not come across, being about a row that is gone; the one exception is
`NoPassword`, retracted from a survivor that has just gained one. And `mark_existing` runs last, so a
row that repeats a saved connection is shown with `ImportNote::AlreadySaved` and left unticked
rather than hidden — a stale saved copy is exactly the case where the import is the one worth
keeping. `same_endpoint` is that identity, and deliberately *not* the app's `conn_id`, which a
row that has never been saved cannot have: engine + host + port + database + user, or the file
path on SQLite (separators normalised, since a JDBC URL writes `/` on a platform whose paths
use `\`). The **user is part of it** — two logins on one server are two connections — **and so
is the sign-in mode in force** (`effective_auth`), as in `Connection::targets_same_server`: a
Windows row and an Entra row for one server collapsed into one, the Entra connection dropped by
`dedupe` or unticked as `AlreadySaved` against a saved Windows one. `needs_password` asks the
mode too (`uses_credentials`), so a Windows row whose string named a `User Id` no longer says
*No password in the source* (`the_sign_in_mode_is_part_of_the_endpoint_and_of_the_password_note`).
`merge_rows`/`merge_skipped` fold one source's result into a list already on screen, and they
live here rather than at the three call sites because a paste, a picked file and a scan all add
to the same list: the moment they disagree about ticking or about duplicates, the user gets a
different answer depending on which button they pressed. `merge_rows` **appends, never
replaces** — the UI keys its selection by index into that list, so growth at the end is the
whole of what makes those indices safe — and answers a repeat with the position already holding
it, ticking through `Imported::preselected` so a row duplicating a *saved* connection arrives
unticked whatever produced it. `merge_skipped` is the other half, and exists because it was
missing: without it a second scan turned three Oracle data sources into "6 entries were not
imported", naming each one twice.
**Both of those are now bounded, and the bound is on what is *kept*, never on what is counted.**
*Choose a file…* has no type filter and an unrecognised name is read as a list of URLs by
design — "a `.env`, or a scratch file of connection strings" — so pointing it at a shell history,
a log or a large `.csv` produces one `Skipped` per line: ~140,000 of them at the 4 MiB read cap
and ~30 bytes a line, every one allocated and deduped against every other, handed to a modal that
shows about five at a time. `SKIPPED_CAP` (200) is how many are kept and
`ImportScan::skipped_hidden` counts the rest, because *"N entries were not imported"* is exactly
what tells someone they pointed the picker at the wrong thing, and the case where N is enormous
is the one that has to survive. The cap is applied in `ImportScan::skip` — the same constructor
that redacts the name, and for the same reason: a parser that grew its own bound is a parser that
can forget one —
and again in `merge` and `merge_skipped`, which `skip` cannot see into (two files of 150 junk
lines each must not put 300 on the list because neither reached the bound alone). **That the cap
is in three places is why `a_file_of_junk_lines_yields_a_bounded_skip_list` asserts on
`parse_url_scan`'s output *before* the folded one**: its first version asserted only the folded
result and passed against the unfixed tree, because `merge` capped while the parser went on
allocating 50,000 `Skipped` structs on the way in — which is the memory the cap exists to refuse.
Removing `skip`'s own cap fails that first assertion and nowhere else. `merge_skipped`
also dedupes through a `HashSet` rather than `contains`, which was O(n²) in the number of skipped
entries: measured on this exact shape at 10,000 → 74 ms, 20,000 → 184 ms, 40,000 → 989 ms, on the
UI thread with no cancel and no progress. `Skipped`/`SkipReason` gained `Hash` for it, and
`UrlError` with them once `SkipReason::BadUrl` came to carry one. The cap
makes that belt and braces, which is the point — a cap is a policy that could be raised, and the
shape underneath it should still be right. `ROW_VIEW_CAP` (500) and `row_overflow_sentence` are
the same rule for the rows the modal *offers*; `connection_import.rs`'s entry has what they
bound. `fill_missing_passwords` **retracts
`ImportNote::NoPassword`** when it fills — the order inside `scan` would do on its own, but a
caller completing an already-scanned row (a hand-picked file, whose passwords arrive
afterwards) would otherwise show "No password in the source" on a row that has one.
**Who wrote the rows decides what a `.pgpass` may complete, and `scan` asks it per row**
(`PgpassScope`). `OwnClients` lets a wildcard line complete any row with nothing noted;
`PickedFile` — what the app passes for *Choose a file…* — lets one complete a row only where the
same `.pgpass` names that host outright, and notes `ImportNote::PasswordFromPgpass` on each row
it fills, which `Imported::preselected` leaves unticked. `scan` used to hold `OwnClients` for
everything it read, on the reasoning that a scanned row was a server the user already had
configured — but project discovery (`recent_project_dirs`, and `conn_sources`'s
`~/DataGripProjects`) reads every JetBrains project's `.idea/dataSources.xml`, and that file
arrives with any cloned repository: a wildcard `*:*:*:bob:hunter2` handed its password to
whatever host the repository named, the row ticked and unnoted. `is_project_file` is the test —
a DataGrip row whose origin ends `.idea/dataSources.xml`, either separator — and those rows take
`PickedFile` through the private `fill_missing_passwords_by`, of which `fill_missing_passwords`
is now the one-scope form. The IDE's own `options/dataSources.xml` stays `OwnClients`, and so do
DBeaver's files, which live inside DBeaver's own workspace directory rather than in a project
(`scan_treats_a_projects_data_sources_as_a_picked_file`).
**The last notes `scan` adds are about what the review row's target line does not say.** That
line was `user@host:port/db` and nothing more (`row_target`, in `connection_import.rs`, which now
appends a tunnel's `via user@host:port`), so a row that signs in with the user's own identity,
tunnels, or runs SQL of its own read exactly like one that types a password at a server. `note_what_the_target_hides` runs on every row from every
source — a Schemaic export carries its sign-in, its tunnel and its init SQL as written, and a
hand-written one carries whatever its author put there — and adds `ImportNote::SignsInAs(mode)`
for any mode but `Password` (Windows sign-in's NTLM exchange with the named host; a Microsoft
Entra, AWS IAM or GCP IAM token minted from the user's own account), `ThroughSsh` for an enabled
tunnel on a networked engine, and `RunsInitSql` for init SQL that is not blank. **`SignsInAs`
unticks the row** (`Imported::preselected`): a pasted string's or a sent file's host is nobody's
promise, and an Entra or Google token opens every database the account can reach — the
`PasswordFromPgpass` outcome by another route. The other two are said and leave the tick alone
(`a_row_that_signs_in_with_the_users_identity_is_named_and_unticked`,
`an_export_row_names_its_tunnel_init_sql_and_sign_in`). Three answers were decided against.
`shareable` does **not** reset `auth` on export: a note on the importing side covers a
hand-written file too, which an export-side reset cannot, and it keeps a team's sharing of Entra
connections working. There is no host-suffix allowlist for an Entra token, since SQL Server 2022
on-premises signs in with Entra under any host name. And there is no TLS floor under a Windows
row, because SQL Server encrypts the login packet, which carries the SSPI exchange, even under
`Disable`.
**Every row starts at `SslMode::Prefer`, not at `Tls::default()`.** `blank` is written out field
by field so that a field added to `Connection` makes *it* fail to compile and an import has to
decide what the new field is — which is exactly the decision the TLS mode never got. Each parser
sets a mode only where the key is *present*, so the ordinary shape (a `~/.pg_service.conf` with
no `sslmode` line, a URL with no query) inherited `Disable`, `Connection::tls_plan` then
answered `None`, and the handshake, `password`-method authentication where `pg_hba` selects it,
and every row of every result crossed in cleartext the same network `psql` was encrypting —
with nothing saying so, the review list having no TLS column and there being no `ImportNote`
for a changed transport.
Every source's own default negotiates TLS (libpq's `sslmode` is `prefer`, `mysql` 8 and
Connector/J default to `PREFERRED`), and `Prefer` costs nothing `Disable` saved: it never
refuses a server plaintext would have reached, which is the only cost `SslMode::Disable`'s doc
claims for being the default of a **hand-typed** connection — and an import is not one, it has
a prior configuration to preserve. A source that says `disable` still wins, by assignment; this
is only the floor under one that says nothing. Raising it moved the MySQL boolean spellings with
it: `ssl`/`usessl`/`requiressl` raised the mode when it was `Disable`, which stopped raising
anything the moment the floor became `Prefer`, so they go through `stronger_of(SslMode::Require)`
— `useSSL=true` is a positive assertion and has to reach `Require` from wherever it starts.
**`ImportNote::PortAssumed` is the same honesty about a port.** A `.pgpass` line may write `*`
for the port — one credential serving every port on a host — which does not parse, so the row
keeps the engine default. That is the only answer available and it is still a *guess*, and this
module's rule is to decide only what can be decided, so `parse_pgpass` says it on the row rather
than letting the form show a number the file never named.
`parse_url` accepts more than a strict URL parser would, because the strings people hold are
not strict URLs: a `DATABASE_URL=…` line lifted out of a `.env` (with `export`, with quotes), a
`jdbc:` wrapper, SQLAlchemy's `postgresql+psycopg2`, libpq's comma-separated host list, a
bracketed IPv6 literal, and the `?user=&password=&sslmode=` parameters JDBC carries instead of
a userinfo. A scheme it doesn't know is `UrlError::UnknownScheme`, *except* when only digits
follow it — that is `localhost:3306`, and calling its host an unknown engine sends the reader
looking in the wrong place. Under `jdbc:` two more grammars are read (`jdbc_authority`): a
Connector/J or MariaDB Connector/J sub-protocol (`jdbc:mysql:loadbalance://h1,h2/db`) is
dropped, and PgJDBC's host-less `jdbc:postgresql:reports` is that database on `localhost`.
Both used to import as an ordinary ticked row whose host was the sub-protocol or the database
name, the database lost (`a_jdbc_sub_protocol_is_not_the_host`,
`a_host_less_pgjdbc_url_is_localhost`) — and since project discovery reads every project the
IDE lists, they arrived without the user picking the file.
**SQL Server has a grammar of its own, and it is not a URL's.** `engine_for_scheme` takes
`sqlserver` (JDBC, Prisma) and `mssql` (SQLAlchemy's `mssql+pyodbc`, node) as `MSSQL`, and jTDS's
`jdbc:jtds:sqlserver://` has its `jtds:` stripped under the `jdbc:` one. What follows is
Microsoft's `//[host[\instance][:port]][/database][;key=value]…` — properties after `;`, not a
path and a query — which is what DataGrip and DBeaver both store (the `/database` path is
jTDS's, and Prisma writes the same properties after `sqlserver://host:port`). **`parse_mssql_url`
reads every SQL Server-scheme URL**, the plain `mssql://u:p@h:1433/d?encrypt=true` node and
SQLAlchemy write included, its `?` query split at `&` and `;` alike. Only a JDBC URL or one
carrying a `;` used to come here; every other went to `parse_server_url`, which has no
`encrypt`, no `trustServerCertificate` and no driver default, so node-mssql's documented
`mssql://app:pw@db.corp:1433/shop?encrypt=true` — and a bare `mssql://…/shop` — imported at
`Prefer`, any certificate accepted, while the same URL with `;trustServerCertificate=false`
beside it imported at `VerifyFull`
(`a_sql_server_url_reads_its_tls_words_whatever_its_query_separator`). **The userinfo comes off
before the `;` split**, through
`split_mssql_userinfo`, as `parse_server_url` takes it off before the path: `parse_mssql_url`
used to cut the authority at the first `;` and never looked for an `@`, so
`sqlserver://sa:S3cret@db.example.com:1433;databaseName=app` imported with the host
`sa:S3cret@db.example.com` — into the suggested name, and into `connections.json` in plaintext,
`host` being no field the keyring rule ever sees — while `mssql://sa:pa;ss@h/app` was refused
with *"pa" is not a port number* under the paste field. The user and password are
percent-decoded, and node's and SQLAlchemy's `?encrypt=true` after the path is read as
properties rather than landing in the database name
(`a_sql_server_url_takes_its_userinfo_off_before_its_properties`). `split_mssql_props` splits
at `;` **except inside `{…}`**, which is
how the driver quotes a value holding one (`password={p;w=d}`, `}}` for a literal `}`), and the
authority wins over a property naming the same thing, as the query does for the other engines.
`encrypt` and `trustServerCertificate` are read as the Microsoft drivers mean the words, not as
they read: `false`/`no`/`optional` is `Disable` (TDS still encrypts the login, and nothing after
it); `strict` is `VerifyFull`; `true`/`yes`/`mandatory` is `VerifyFull` too, or `Require` with a
trusted certificate; and `trustServerCertificate=true` alone is `Require`, being the drivers'
encrypted default since 10.2. **A string that says neither takes its driver's default, not the
import's floor** (`apply_mssql_tls`'s `verifies_by_default`, `driver_verifies_by_default`):
the Microsoft drivers now encrypt and verify when told nothing — Microsoft.Data.SqlClient 4+
(an ADO.NET string with no `Driver`/`Provider`), mssql-jdbc 10.2+, Prisma's URL and node's
tedious, ODBC Driver 18, OLE DB Driver 19 (`MSOLEDBSQL19`) — so those land on `VerifyFull`;
jTDS, pymssql's FreeTDS (an `mssql+pymssql` scheme), ODBC 17 and older, the Native Client,
`SQLOLEDB` and OLE DB Driver 18 (`MSOLEDBSQL`) default off and keep the floor. A URL's `driver=`
— SQLAlchemy's `mssql+pyodbc://…?driver=ODBC+Driver+18+for+SQL+Server` — overrides what its
scheme implies, as a connection string's `Driver` does, so 18 verifies and 17 keeps the floor.
Reading every silent string as `Prefer` accepted any certificate where the
source had verified one. `System.Data.SqlClient` also defaulted off, and a bare ADO.NET string
does not say which client it was written for; the current one is the reading that fails loudly
against a self-signed server rather than silently against an attacker
(`sql_server_tls_words_land_on_the_ladder`,
`a_sql_server_string_silent_on_encryption_takes_its_drivers_default`).
**A named instance (`host\SQLEXPRESS`) is imported as one**, through `mssql_host`: kept as
`server\INSTANCE` where the source gave no port, so the connect asks SQL Server Browser for it
as the source's driver did (under `mssql.rs`), and reduced to the bare server where it gave one
— a port given is the instance's own, and the instance is reached directly at it, its name not
kept beside it. JDBC's three spellings — the authority's `db\SQLEXPRESS`, an `instanceName`
property, and `serverName=db\SQLEXPRESS`, which `parse_mssql_url` now splits too — arrive
alike and with no note (`a_sql_server_named_instance_is_imported_as_one`). **There used to be
one, `ImportNote::NamedInstance`**, `PortAssumed`'s honesty for an instance with no port:
Schemaic asked no Browser, so the row carried 1433 — which reaches the host's *default*
instance if it has one, not the named one — and the note said to set the port. With the lookup
in the connect nothing is left to set, and the note and its label are gone. It was the first
note a URL could warrant by itself, which is why `parse_url` is a wrapper over the private
`parse_url_noted`, and `parse_url_scan`, the DataGrip reader and the DBeaver reader call the
noted form; what a URL warrants now is a sign-in Schemaic does not have
(`ImportNote::ExternalLogin`, below). DBeaver also keeps the instance in its own `host` field
(`laptop\SQLEXPRESS`), so its reader splits it there through the same `mssql_host`, kept where
the entry names no port, and its generic-driver fallback strips jTDS's `jtds:` as `parse_url`
does (`dbeaver_reads_a_named_instance_and_a_jtds_source`); DataGrip's go through the URL
reader (`datagrip_reads_its_sql_server_data_sources`). **`parse_url_noted` refuses any parse
whose host still holds an `@`**, whatever the grammar — `Server=sa:pw@host` in an ADO.NET string
as much as a URL — with `UrlError::UserinfoInHost`, which carries nothing, since what it would
repeat is most likely a password. It is asked once there rather than in each parser, because
the SQL Server URL parser was the one that forgot; choosing the grammar moved beneath it into
the private `parse_url_any` (`a_host_still_holding_an_at_sign_is_refused_without_repeating_it`).
**A connection string with no scheme at all is SQL Server's too** — ADO.NET's
`Server=tcp:host\instance,port;Database=d;User Id=u;Password=p`, the shape `appsettings.json`
holds, and ODBC's and OLE DB's, which spell the same keywords around a `Driver` or `Provider`.
`looks_like_connection_string` decides on the first keyword — the server, the driver, the
provider or the database, which is what such strings open with — unless that first value holds
`://`, which is a variable named `DATABASE` holding a URL, **and on there being at least two
pairs**: one is a `.env` assignment (`SERVER=0.0.0.0`, `DATA_SOURCE=warehouse`), and *Choose a
file…* reads a `.env` line by line, so each became a preselected SQL Server row. It is asked **before**
`strip_env_assignment`, which would otherwise eat a `Server=` head as a variable name, and again
after it, for ASP.NET's `ConnectionStrings__Default="Server=…"`. `parse_connection_string`
**reads the driver rather than assuming it**: an ODBC string for MySQL is the same grammar, so a
`Driver`/`Provider` that `names_sql_server_driver` does not recognise is
`UrlError::UnknownScheme`, not a SQL Server row with a MySQL host. **With no driver named, the
string has to say it is SQL Server's** (`says_sql_server`), because MySQL's Connector/NET and
Npgsql write the same `Server=…;Database=…` grammar, and the canonical Connector/NET string
imported as SQL Server on 1433 with the MySQL password, its `Port=3306` dropped. A keyword
SqlClient lacks and those drivers have — `Port` above all (SqlClient writes `Server=h,1433`),
`Host`, `Username`, `SslMode`, `Search Path` — decides against; a keyword neither of them
accepts (`Trusted_Connection`, `MultipleActiveResultSets`, `Authentication`,
`ApplicationIntent`, `Failover Partner`, `AttachDbFilename`), an `Encrypt` *value* only
SqlClient writes (`Strict`, `Mandatory`, `Optional`) or a server only its grammar writes
(`tcp:`, `np:`, `lpc:`, `(localdb)`, `.`, `,port`, `\instance`) decides for; neither is
`UrlError::NotSqlServer`, which repeats nothing, rather than a guess —
`Server=h;Database=d;Uid=u;Pwd=p` is valid for both
(`a_connection_string_is_sql_servers_only_when_it_says_so`). **The *for* list is what neither
driver documents, not what SqlClient's strings usually say**: `User Id`, `Data Source`,
`Initial Catalog`, `Integrated Security`, `Encrypt` and `TrustServerCertificate` were on it, and
each is a synonym Connector/NET or Npgsql accepts (`normalize_key` makes Npgsql's
`Trust Server Certificate` the same key), so
`Server=mysql.internal;Database=shop;User Id=app;Password=…` imported as a preselected SQL
Server row on 1433 with its password — or, with `Integrated Security=yes`, with the user's
Windows identity (`a_keyword_the_mysql_and_postgres_drivers_share_is_no_evidence`). The cost is
that a genuine SqlClient string written only in those shared keywords —
`Data Source=h;Initial Catalog=d;User Id=u;Password=p` — is refused too. That is the loud
direction, and the refusal's message names the answer: write the server as `tcp:host`.
**A repeated keyword keeps the value
its driver would use** — the last in ADO.NET and OLE DB, SqlClient's documented rule for a base
string with an override a config transform appended, and the first under an ODBC `Driver`,
whose rule is the opposite — not the URL readers' first-wins, whose reason (the authority beats
a query parameter) a flat keyword list does not have
(`a_repeated_keyword_keeps_the_value_its_driver_uses`). It reads the transport too —
`np:` named pipes and `(localdb)` are `UrlError::Transport`, since TCP is all Schemaic speaks,
while `lpc:`, `.` and `(local)` are this machine and become `localhost` — `.\SQLEXPRESS` is
`localhost\SQLEXPRESS`. This reading is the importer's own, not
`connection::sql_server_address`, the Host field's reading of the same text, and the two are
pinned to agree
(`a_typed_host_and_an_imported_one_agree`, under `connection.rs`): a typed
`db\SQLEXPRESS,1500` once asked the Browser for an instance the imported one dialled at port
1500 (S5-L1-02). A named instance goes through `mssql_host` as the URL form's does
(`an_ado_net_string_notes_what_it_cannot_carry_over`), and the TLS words go through
`apply_mssql_tls`, now the one reading both grammars share. `split_mssql_props` gained a
`quotes` flag for ADO.NET's `"…"`/`'…'` (a doubled quote standing for one), off for JDBC, which
has no such quoting — a JDBC password that merely starts with `"` is left alone. **A login
handed to Windows or Entra is carried over where Schemaic has that sign-in, and noted where it
has not**: `Integrated Security`/`Trusted_Connection` true or `SSPI` becomes
`connection::AuthMode::Windows`, and `Authentication=Active Directory Default` becomes
`AuthMode::AzureCli` — that method is `DefaultAzureCredential`, whose chain tries the Azure CLI,
the one Entra sign-in Schemaic has. Only where `AuthMode::offered` has the mode, though: Windows
sign-in on a non-Windows build, and every other Entra method (`…Password`, `…Interactive`,
`…Integrated`), keep the row on a password and carry `ImportNote::ExternalLogin` — *"Signs in a
way Schemaic can't: set a SQL login"* — since the row cannot sign in until one is set. `Sql
Password` is a SQL login spelled out and changes nothing (as is JDBC's `NotSpecified`). **An Entra
method Schemaic lacks keeps no password**: it is the account's Entra password, which the driver
sends to Entra, and a SQL login would hand it to the server. **The reading is one, shared by both
readers** (`MssqlSignIn`): it lived in the ADO.NET/ODBC reader alone, so a DataGrip or DBeaver
`jdbc-url` saying `integratedSecurity=true` or `authentication=ActiveDirectoryDefault` — the only
place those tools state the sign-in — arrived as a clean SQL-login row with an empty user that
failed with *Login failed for user ''* (`an_ado_net_connection_string_is_read_by_its_keywords`,
`an_odbc_connection_string_is_read_when_its_driver_is_sql_server`,
`an_ado_net_string_notes_what_it_cannot_carry_over`,
`a_jdbc_urls_sign_in_is_read_as_an_ado_net_strings_is`). **Its keywords repeat by the rule
above** — the last, or the first under an ODBC `Driver` (`MssqlSignIn::new(first_wins)`), and
the last in a JDBC URL's properties. They were read first-to-set and never cleared, so a base
string's `Integrated Security=true` with `Integrated Security=false;User Id=u;Password=p`
appended imported as Windows sign-in, and `Authentication=ActiveDirectoryDefault` overridden by
`SqlPassword` as the Azure CLI's — a different principal from the SQL login the application
signs in as (`a_repeated_sign_in_keyword_keeps_the_value_its_driver_uses`). The two families
are held apart (`integrated`, `authentication`) and resolved only by `external`, since a false
`Integrated Security` says nothing about an `Authentication`: `Sql Password` is a SQL login, an
Entra method decides over `Integrated Security` (SqlClient refuses the two together), and only
with neither, or `NotSpecified`, does a true `Integrated Security` mean Windows.
**`redacted` hides a password for as far as the importer would read it, so it reads each shape with the importer's own grammar** —
a separator scan was the previous answer and kept being one character short of it. A keyword
string is found as `parse_url_any` would find it (`keyword_list`: `looks_like_connection_string`
on the text, or on what `strip_env_assignment` unwraps) or, for the whole entry only (`outer`),
as any text whose first `=` comes before a `:`, `/`, `?`, `@` or `#` — a Connector/NET string
opening `Uid=`, which the importer refuses as having no scheme. `redact_keywords` walks it with
`mssql_props`, the span-producing tokenizer `split_mssql_props` is now a map over, and so the
very grammar `parse_connection_string` reads with; each password value's whole written extent
(`MssqlProp::raw`, from its first non-blank character to its `;`, quotes and anything after the
closer included) becomes `…`. A value opened with `{` — or with ADO.NET's `"` or `'` — therefore
runs to the first closer that is not doubled, `;` and spaces included, so a SQL Server string
that fails to parse shows none of `password={p;w=d}` or `Password="p;w=d"` in the not-imported
list — split at each `;`, it showed `w=d}` (`a_braced_password_is_redacted_whole`,
`a_quoted_password_is_redacted_whole`). **A server value's userinfo is hidden as a URL's is**:
the importer refuses a host still holding an `@` (`UrlError::UserinfoInHost`) because what is
in front of it is most likely a password, and `Server=sa:Hunter2@db` is no `Pwd=` and no URL,
so the refused entry's own text showed it. A value under any key `is_server_key` names — the
set `parse_connection_string` reads the server from, asked by both — or a SQL Server URL's
`serverName` goes through `redact_server_userinfo` first, which keeps the login up to the first
`:` and hides the rest up to the last `@` (`a_server_values_userinfo_is_hidden_like_a_urls`).
Every other value is read again by `redact_text`, so a
URL or a `.env`-quoted string inside one is still found, to a nesting of `REDACT_DEPTH` (three):
a bound, so that no input's nesting makes the redaction anything but linear. What is not a
keyword string goes to `redact_url` — its userinfo through `split_userinfo` (below), where one
with no `:` is now read by `redact_params` rather than kept, because the ordinary delimiters
can sweep a SQL Server URL's `;password=` into a "userinfo"; and on a SQL Server scheme
(`is_mssql_scheme`) the tail is cut at its first `;` and the properties go through
`redact_keywords` with quoting off, as `parse_mssql_url` reads them. The separator scan leaked
on exactly the strings this module refuses: it ended a value at every `&`, so the canonical
Connector/NET `Pwd=My&Secret9` showed `&Secret9`, and it looked for an opening quote only where
the key shared a part with it, so `Password= "abc;Secret99"` — a space after the `=` — showed
`…;Secret99"`. `a_password_is_hidden_for_as_far_as_its_grammar_runs` is seventeen refused
inputs, every one of which leaked on that code, and a verbatim round-trip of names that hold no
secret.
**`split_userinfo` is where a URL comes apart, and the order is the whole point.** Everything
past `://` is cut at the **last `@`** — an email address is an ordinary username — and only then
is the remainder searched for a path, a `?` or a `#`. Both readers used to do it the other way
round, cutting at the first `/`, so `admin:sec/ret@db.example.com:5432/app` gave an authority of
`admin:sec`: `parse_server_url` read that as a host:port and `UrlError::BadPort` rendered *"sec"
is not a port number* — the first three characters of the password, under the paste field — while
`redacted` made the identical mistake on the identical string, found no userinfo to redact and
put the **whole URL** into the not-imported list, which `merge_skipped` then keeps across every
later scan. `/`, `?` and `#` are all legal in a password, and this module's own promise is to
accept more than a strict URL parser would, so one function answers for both readers: the entry a
parser refuses because of a `/` in the password is exactly the entry whose password would
otherwise be shown. The authority's delimiters are a parameter — `URL_DELIMS` (`/`, `?`, `#`)
for an ordinary URL, `MSSQL_URL_DELIMS` adding the `;` Microsoft's properties begin at — and
`redacted` keeps the ordinary set even on a SQL Server URL, because the one is a superset of the
other: a shorter authority can only find an `@` the longer one holds, and a shorter port
candidate only looks *more* like a port, so the ordinary rule hides at least what the SQL
Server rule takes as userinfo. `split_mssql_userinfo` decides the one case the fallback cannot,
a numeric password holding a `;` (`sa:1234;x@h`): `looks_like_userinfo` refuses it because
`1234` reads as a port, and the password's first half became the saved port. A property is
always `key=value`, so an `@` in a `;`-segment with no `=` before it ends a userinfo — but only
where nothing before the `@` is a `/`, `?` or `#`, so that `redacted` finds the same `@` inside
its own authority and hides the same password. **`SkipReason::message` is redacted for the
same reason `ImportScan::skip`
redacts the name** — it is the render-facing accessor, and every variant carrying text carries
text a *parser* chose out of the entry, so doing it at the one place both are rendered is what
stops a fourth variant reopening it (`raw_message` is the unredacted form, and this module's own
tests are its only caller). **The variants carry values, not finished English** — phase 4 of
*Interface translation* split what `Unreadable(String)` used to hold as sentences: a URL
`parse_url` refused is `BadUrl(UrlError)` (`parse_url_scan`'s pasted lines and a DataGrip
project's URLs, the two sites that built `Unreadable(e.message())`), a file that is not an
export is `NotAnExport` with the JSON reader's text, a newer export is `NewerExport` with its
format version, and `Unreadable(String)` is left for what is a library's or parser's own text —
DBeaver's and `docker inspect`'s JSON errors — which stays as written. `message` says all of
them byte for byte as before. **The interface's wording is redacted by the same function**:
the interface's `i18n::skip_reason_shown` words a reason from the catalogue and passes the whole
sentence through `redact_shown`, the public wrapper over the private `redacted`, because wording
a parser's text in another language does not make it any less a password — and redacting the
sentence rather than each argument is what keeps it the same question the English answers.
`english_says_every_import_refusal_as_core_does` fails without that call (on its `hunter2`
case). The parameter half of `redacted` — `redact_params`, now only for
what no grammar claims: a skip reason's prose, a URL's path and query, the text inside a
keyword string's value — also spans separators: ` ` is
one of them, so an ODBC-shaped `Pwd = hunter2` — which the paste field invites — put its key, its
`=` and its value in three different parts and never reached `split_once('=')` with the pair
together, while the unspaced `Password=hunter2` redacted correctly and hid that. **A redacted
bare value runs to the next `;` — or `&`, once a `?` has opened a query — spaces included**
(`in_value`): ADO.NET and ODBC values
are unquoted up to the `;`, so a space in a password is ordinary, and stopping at the first one
put `Password=… horse battery` in the not-imported list — for every `Transport`, `BadPort` or
unknown-driver refusal of such a string
(`a_password_holding_spaces_is_redacted_to_the_next_separator`). `&` separates a URL's
parameters and nothing else; anywhere else it is a password character. The arm that redacts a
value arriving in a part of its own — the space after the `=` is what puts it there — asks
`opening_quote` as the keyed arm does, since such a value is quoted as often as not.
A driver this app has no engine for is `Skipped` **by name** rather than bent onto the nearest
engine: a MySQL connection silently pointed at an Oracle server is a worse answer than an
honest omission, and the modal says how many were left behind. DBeaver names its engine twice
and only the *driver* distinguishes MariaDB (it ships under the `mysql` provider; SQL Server
under `sqlserver` or `mssql`); its rows are
sorted by name because the file is a JSON object keyed by internal ids, so "the order in the
file" is not an order anyone chose. **A connection's `folder` key carries over as its heading**,
verbatim through `overlay`, so a blank or missing one leaves the row ungrouped
(`dbeaver_carries_the_folder_across`). The file's top-level `folders` map is not read: a folder
here is only its name (`Connection::folder`), and the key already is that name. A nested DBeaver
folder arrives as one heading spelled however DBeaver writes its path in the key — this app has
no nesting to put it in — and trimming is left to `Connection::sanitized` on save. The
DataGrip reader is a narrow element scan
(`<data-source>`'s `name` and `uuid`, `<jdbc-url>`, `<user-name>`, `<driver-ref>`) rather than a
real XML parse, and if JetBrains ever moves that shape this reports zero connections rather than
wrong ones. It was written so while `schemaic-core` had no XML dependency; it has one now —
`quick-xml`, for SQL Server's query plans (`plan.rs`) — and this reader has not moved onto it,
so it is still defeated by one of those elements inside a comment or a CDATA section. **A project's file is only half of each
data source**: `.idea/dataSources.xml` is the part meant for version control — name, driver,
URL — and the user name is per-person, so it lives in the `dataSources.local.xml` beside it
under the same `uuid`. Reading only the first imported every project connection with a blank
user. `parse_datagrip_with_local` takes the second (`SourceFile::local`; `parse_datagrip` is it
with `None`) and matches an entry by `uuid`, falling back to `name` only where a uuid cannot
decide it — either side lacking one — because a local entry keyed by a uuid belongs to that
uuid and its name goes stale on a rename (`a_projects_user_names_come_from_its_local_file_by_uuid`
is the stale-name case). A user in the main file wins, and the local file never adds a row or a
skip of its own: an entry there with no counterpart has no server to connect to. `scan` is what
carries the local text down, and `scan_hands_a_datagrip_files_local_half_to_the_parser` pins
that composition rather than the parser alone. **`scan` also knows which project a
`.idea/dataSources.xml` is** — the directory holding that `.idea/`, off the file's own path
(`project_dir_of`) — so `$PROJECT_DIR$`, which DataGrip writes for every SQLite file in a
project, is expanded there instead of flagged `UnexpandedPath`; the flag, and the rule that it
is not guessed at, stand for a file whose project is unknown, the IDE's global one
(`a_project_files_macro_is_expanded_against_its_own_project`). `recent_project_dirs` is the other half of
finding a *project*, and exists so the app never has to walk the home directory for one: it
returns the paths an IDE wrote into its `options/recentProjects.xml` — the current
`additionalInfo` map's `<entry key>`s and the older `recentPaths` list — with `$USER_HOME$`
expanded, and `$APPLICATION_CONFIG_DIR$` against the product directory the caller is reading
(the older layout's default project lives there, and was dropped:
`a_project_in_the_config_directory_is_found_too`), keeping only strings that look like a path
(a macro, `/`, a drive letter) and are **not on a network share** — the app stats each one
serially with no deadline, and an unreachable `//server/…` waits out the SMB timeout, or on a
hard NFS mount blocks for good, so one such entry stalled the whole scan
(`a_project_on_a_network_share_is_not_listed`) — and each
once, in the file's order. Nothing else in that file is a project: `lastProjectLocation` is
where the *next* one would go, and the metadata's `value`s are build numbers
(`only_project_entries_count_as_recent_projects`). `.my.cnf`'s `[client]` is the base every
client group inherits, so a `[client_prod]` is read *layered on it* (what
`--defaults-group-suffix` does); `[mysqld]` is the server's own configuration and is not a
client at all.
**Docker's containers are the one source that is another program's answer rather than a
file.** `parse_docker_inspect` reads `docker inspect`'s JSON array, because `docker ps` alone
does not carry the environment and the environment is where the official images take their
credentials — plaintext on this machine already, set by whoever ran the container, so carried
the way `.my.cnf`'s are. The engine is read off the image's **repository**, never its tag
(`docker_engine`): a digest and tag go (a `:` before the last `/` being a registry's port, not a
tag), and the last path segment decides — `mysql`, `mysql-server`, `percona`/`percona-server`
and Oracle's `mysql/community-server` are MySQL, `mariadb`/`mariadb-server` MariaDB, `postgres`,
`postgresql` (bitnami's), `postgis`, `pgvector` and `timescaledb`/`timescaledb-ha` PostgreSQL,
with `mssql/server` and `azure-sql-edge` spelled out for SQL Server; `myapp:postgres` is nothing
(`the_engine_is_read_off_the_repository_not_the_tag_or_registry`). Anything else running — a
`redis`, an `adminer`, a `pgbouncer`, which listens on 5432 and is not the server — is **neither
offered nor counted as skipped**: it is not an entry at all
(`a_container_that_is_not_a_database_is_neither_offered_nor_counted`). A database container is
reached at the host port its engine's **fixed in-container port** is published on (3306, 5432,
1433 — `docker_container_port`, the official images' own), and one with nothing published there,
or a port out of range, is `SkipReason::NotPublished` *by name*: a server reachable only on
Docker's own network is not one the app can connect to
(`a_database_whose_port_is_not_published_is_skipped_by_name`). The trade is that an image told to
listen elsewhere inside its container reads as unpublished too. `docker_binding` reaches a
bind-to-everything (`0.0.0.0`, `::`, blank) at `127.0.0.1` and a specific address at that
address, preferring IPv4 because Docker lists one port once per family. The credentials:
PostgreSQL takes `POSTGRES_USER` (else `postgres`), `POSTGRES_PASSWORD` and `POSTGRES_DB`, with
bitnami's `POSTGRESQL_*` behind each, an unset database left for the server to default; SQL
Server signs in as `sa` with `MSSQL_SA_PASSWORD` or the older `SA_PASSWORD`; MySQL and MariaDB
take root with the root password, else the app user only where both its `*_USER` and
`*_PASSWORD` are set, else root with nothing — a generated root password
(`MYSQL_RANDOM_ROOT_PASSWORD`) or a `*_FILE` secret, which `scan`'s ordinary `NoPassword` note
then says (`a_generated_or_file_secret_root_password_arrives_without_one`). MariaDB reads its own
`MARIADB_*` names before the `MYSQL_*` aliases, as its image does
(`mariadb_reads_its_own_variables_before_the_mysql_ones`). An empty variable counts as unset and
a repeated one takes its last value. The row keeps the container's name; a stopped container
(`State.Running` false) offers nothing, and output that is not JSON is one `Unreadable` skip.
It goes through `scan` as every source does, so `dedupe`, `AlreadySaved` and `NoPassword` apply
— but the app's call hands `scan` Docker's answer alone, so no `~/.pgpass` sits beside it to lend
a password.
**Schemaic's own export is the other direction, and it carries no secret and no consent.**
`export_connections` turns a list of connections into a pretty-printed `ExportFile` — a
`schemaic_connections` version (1) and the connections in this app's own shape — from a copy in
which the database password, the SSH password and the SSH key's passphrase are blanked, the id
is 0 as every parser's is, and the two per-machine grants are back at their defaults:
`cli_access` off and `ai_data` unset, because letting a tool or an assistant read a database on
one machine is no grant on another. Everything else stays, the key, certificate and SQLite
paths included — a path is not a secret, and a team may well share it. **It is built from the
hydrated in-memory list, never from the on-disk `connections.json`**, and that is load-bearing:
the disk copy is the one that holds every secret in the clear wherever the keyring is
unavailable (`core/secrets.rs`), so serialising it would export exactly what this function
exists to leave out — the blanking is this function's own job, not the save path's
`sanitize_file` (`an_export_round_trips_through_the_import_without_its_secrets`). `parse_schemaic`
reads it back as `ImportSource::Schemaic` rows through the ordinary `scan`, so
`mark_existing` and the `NoPassword` note apply as to any source — **but `dedupe` never folds
one Schemaic row into another**: the fold is for two clients describing one server, and this
app's own list is one row per connection somebody made, so a same-endpoint pair there is
*Duplicate*'s read-only twin or a second folder, and folding it dropped the twin, flag and
all, unlisted (`an_export_keeps_two_connections_to_one_server_apart`). **Both directions go
through one `shareable`** — id 0, every `SecretKind::ALL` secret blank through
`secrets::clear_secrets` rather than a list of its own, so a secret kind added there is kept
out of an export by the same edit, both grants at their defaults — so a
hand-edited file can slip in neither a grant nor a password the next save would hand to the
keyring, and the export and the import cannot come to disagree about what a shared file may
carry (`a_hand_edited_export_is_stripped_and_a_newer_one_refused`). A version past
`EXPORT_VERSION` is refused as *exported by a newer Schemaic* (`SkipReason::NewerExport`, which
carries the version) rather than half-read. A file
that does not parse as the envelope — bad JSON, no `schemaic_connections` key — is one
`SkipReason::NotAnExport` saying it is *not a Schemaic connections export*, not an empty scan
(`a_file_that_is_not_an_export_is_unreadable`). Unlike every other source, the row arrives as
written rather than through `blank`, so read-only, the environment and the folder come across —
Schemaic's own guard rails, which the reason other sources blank them (guessing another tool's
is worse) does not reach — and `import_chosen`'s closing notice that they were *not carried
over* is said only when a row from another client was among those added. The import still
re-picks the colour and uniquifies the name, as it does for any row.
`EXPORT_FILE_SUFFIX` (`schemaic-connections.json`) is the end of an export's file name and the
only way the app's picker tells one from DBeaver's `.json` (`conn_sources::source_for_path`);
the save dialog puts it back on a name typed without it (`ExportPath::of`, which also answers
whether that name, one the dialog never showed, must be confirmed before it is replaced), so a
file renamed to plain `.json` afterwards is read as a list of URLs.
