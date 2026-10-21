# `ui/database_editor.rs`

`database_editor.rs` — the **container** form: a database, or one of PostgreSQL's or SQL
Server's namespaces inside one, over `core::ddl`'s `DatabaseDraft` — and PostgreSQL's
**Create extension** picker, the same form (below). The smallest of the schema
editors and the only one that **only ever creates** — there is no `current`, no diff and
no change count, because a container is dropped from its own row's menu and a rename is
offered on no engine — MySQL and PostgreSQL have none that is safe to perform (MySQL withdrew
`RENAME DATABASE` in 5.1.23; PostgreSQL's needs every session off the database), and no reason
is recorded for SQL Server's. The footer therefore says what will be
made rather than counting changes. The panel has a **fixed** height (`PANEL_H`), like every
other modal here, though it is two to four rows tall: sized to its content, the body's
`flex_grow` scroll resolved to zero and the modal rendered a title sitting on its footer.
**Three homes.** The schema tree's **Create ▸ Database / Schema**, the SCHEMA gear's
**Create database**, and a right-click on the tree's **empty space** — which is the one
people reach for first, since every file manager trains it. The gear is not a duplicate of
that third: a connection whose tree already fills the panel has no blank space left to
right-click, which is exactly the connection on which a new database is worth making.
`create_children`, `schema_tree::blank_space_entries` and `overlays::gear_entries` are the
three places that decide which entries exist, each with its own test. The gear's list got
that treatment last and had most to gain from it: two of its then five entries were conditional
and two of those were conditionally dimmed, and it stayed a 130-line inline builder in a file whose
test module could not reach it, while its sibling had already been split out after an ordering
inversion no test could see. `GearEntry` carries a `GearKind` discriminant rather than a bare
label, for `BlankKind`'s reason: routing on the string with a catch-all arm makes a sixth entry
render as a row that does nothing. It now carries no label at all, and neither do
`CreateEntry` and `BlankEntry`: `GearKind::label()`, `CreateKind::label()` and
`BlankKind::label()` draw it from the kind, so an entry
holds no text a test or a route could key on, and the tests read `e.kind.label()`. **Whether a row is *inert* is a second question with a
different lifetime, and that is why it is `GearKind::disabled_when(read_only, down)` rather than
a field `gear_entries` fills.** Which rows exist is settled when the menu opens — the engine's
capabilities and whether a connection is saved cannot change under a standing menu — but whether
a row acts cannot be: the health poll writes `ConnStatus::Disconnected` with no user interaction
at all, and the gear menu is deliberately not rebuilt while it stands. Frozen into a per-row
`bool` at build time, `Create database` and `Users and privileges` stayed lit on a server that
had since stopped, and `Create database` opened the form, previewed a real `CREATE DATABASE` and
failed at Apply with the connect error the header was already showing — verbatim the outcome
their own comments say the dim exists to prevent. `read_only` is not the live half (it changes
only on a status-bar click, and that press dismisses the menu) but it is asked here too, so
there is one answer per row rather than two half-fresh ones.
The two menus deliberately **do not agree entry for entry**
— the gear also carries `Collapse all` and the size toggle, which are about the panel rather
than the server — so it is a second list and not a call to the first; what they must agree on is
the *gating*, which is why both ask `users::supports_users`, `server_vars::supports_server_vars`
and `ddl::supports_database_editing`. **The read-only refusal is inside
`open_for_new` itself**, not at the four launch sites: the invariant is that a launch guards
itself in the same step that launches it, and written per site that is four copies of one
`if` — of which `create_submenu`'s arm had already been left out, relying on
`MenuEntry::disabled` alone. One refusal at the door is the only version that survives a
fifth home. The entries stay dimmed, because that is what *says* the action is unavailable.
The option fields are per-engine and **absent** rather than dimmed where the engine has
none — asked as `ddl::supports_owners` / `supports_database_charset` /
`supports_database_collation` rather than as `dialect ==` tests, which is what the first two
were written as first, and the last two of a *database* only, a namespace being a name in a
catalogue rather than a store. MySQL gets `Character set` / `Collation`, PostgreSQL gets
`Owner`, SQL Server's database gets `Collation` alone — T-SQL takes no character set, its
collation naming the code page, which is why the collation is a predicate of its own rather
than the charset's other half — and SQL Server's schema a name and nothing else,
`supports_owners` being false there and the other two asked of a database only. PostgreSQL's
`ENCODING` is deliberately not
offered at all: the server refuses it unless the locale clauses and `TEMPLATE template0`
agree, so a field for it would mostly produce *"new encoding is incompatible with the
encoding of the template database"*.
All three are **free text with a `suggest_chevron` beside them** — built by
`table_designer::suggest_field`, so each also ghosts the first listed value the typed text
begins and takes it on Tab — the designer's column-type pattern, because every value they take is per-server and per-version — a
collation only MariaDB has, a role created five minutes ago. The MySQL lists are the
constants `ddl::MYSQL_CHARSETS`/`MYSQL_COLLATIONS`, and the Collation row is offered
`ddl::database_collations(dialect)`, which is the latter on MySQL and `MSSQL_COLLATIONS` on SQL
Server — free text there too, though `DatabaseDraft::validate` refuses a collation that is not
a bare name, since T-SQL can write no other (under `ddl.rs`); the collations are deliberately **not**
filtered by the chosen character set, since filtering would make it a picker that has to be
*right*. Owner's list is `Db::roles` (`left(rolname, 3) <> 'pg_'`, **not** a `NOT LIKE
'pg\_%'` whose escape only survives while `standard_conforming_strings` is on), fetched on
open like the trigger editor's functions — which is why `suggest_chevron` takes its options
as a **closure read at press time**: the reply lands after the form is built, and rebuilding
the row to deliver it would tear down a field the user may be typing in. The ghost reads the
same closure on every keystroke, untracked, for the same reason. That closure is also
why the chevron now **refuses to open on an empty list**: `popup_menu_overlay` builds a panel
for any `Some`, and an empty entry vec still draws — height collapsing to the padding and
border, width clamping to the 170px floor — so a press before the fetch landed painted an
empty bordered box that then ate the click meant to dismiss it. Unreachable while every
caller passed a constant.
**The footer says nothing while the form is valid**, unlike every peer modal. Theirs show a
change count, which is a diff the reader cannot otherwise see; here there is no diff, so
the only sentence available restated the title and the name field — and a status line that
only ever agrees with the screen teaches the eye to skip the place errors appear.
`change_of` is the pure mapping from kind to `Change`, and it is unit-tested because a
swapped arm is invisible in a rendered form: the two kinds are different statements at
**different levels** — `CreateDatabase` is server-level and `CreateSchema` is not.
`ddl_preview::preview_container` is the one exit, and it reads that level off the change
(`ddl::is_server_level`) rather than taking a caller's word for it.
**Off `whole_ui_gate`'s list, 3 to zero, and this one paid twice over.** `open_for_new(ConnUi,
SchemaUi, DdlUi, &RolesFn, kind, database)` is the doors' rule — a door names the fetch it ends
in — with `container_names(SchemaUi, …)` and `fetch_roles(DdlUi, &RolesFn, …)` behind it. It
was also **the single callee blocking two of `overlays.rs`'s entries**:
`schema_settings_overlay` reached the root bundle for `open_for_new(&Ui, …)` and nothing else,
and `create_submenu` for that and `table_designer::open_for_new` beside it, so neither could be
narrower than this file. One thing the narrowing was *not* checked against: `fetch_roles`' round
trip went unexercised by hand, because MySQL has no owners and `ddl::supports_owners` returns
before the fetch fires — only a PostgreSQL **Create database** reaches it.
**PostgreSQL's Create extension picker is this form too** (`ContainerKind::Extension`), because
what it needs is exactly this form's shape: a name, a chevron, and an exit that only ever
creates. `open_for_extension(ConnUi, SchemaUi, DdlUi, &ExtensionsFn, database)` is its door,
with the read-only refusal inside it for `open_for_new`'s reason — it has two homes, the
Create ▸ submenu's *Extension* and the Extensions folder's *Create extension* (under
`overlays.rs`). **It opens on a blank name, not a proposal**: an extension's name is a control
file the server ships, so a `new_extension` would be a name no server has, and the empty-name
refusal keeps Preview SQL off until one is picked or typed. The Name row is `optional_field`'s
free text with a chevron of installable names — free text for the reason the other fields are,
so a name the fetch missed is still reachable — reading *No extensions left to install* when
the list is empty, which it is until the fetch lands and for good once the server's every
extension is in. `fetch_extensions` clears `DdlUi::extensions`, asks only where
`ddl::supports_extensions` says yes, and drops a reply whose `DdlUi::session` stamp is stale,
as `fetch_roles` does; the reply is narrowed by `schema::installable_extensions` against the
tree's loaded list as well as the view's own flag. `SchemaActions::extensions` is the
`ExtensionsFn` (`Rc<dyn Fn(u64, String, ExtensionsDoneFn)>`) behind it, wired in the app beside
`roles` and failing as silently. No owner, character-set or collation row: an extension
belongs to whoever installs it. The title is *Create extension in {database}*; `change_of`
maps it to `Change::CreateExtension { name }`, the name alone
(`the_extension_picker_asks_for_create_extension_in_its_database`, through
`container_preview`); and it leaves through
`preview_container` like a schema — `is_server_level` is false for it, so it runs inside the
database through `Db::run_ddl`.
**Its words are the catalogue's** (`database-editor-*`) — sections, fields, the
*server default* and *you* placeholders and each chevron's empty note, with Cancel and Preview
SQL the shared `dialog-cancel`/`editor-preview-sql`. **The title is one message per kind and
placement** — `database-editor-title-{database,schema,extension}` and the `-schema-in`/
`-extension-in` pair that take `$database` — where it had been `format!("Create {}",
ContainerKind::label())` beside a separate *Create schema in {db}*: a sentence assembled from an
English noun cannot be translated. `optional_field` takes its placeholder and note as `String`
for the same reason. The identifier placeholders (`database_name`, `extension_name`, …) stay.
The status line and Preview's readiness read `DatabaseDraft::problems`, worded by
`database_problem_text` from `database-problem-*` (the editors' refusals, under `i18n.rs`),
held by `english_says_every_database_problem_as_core_does`.
