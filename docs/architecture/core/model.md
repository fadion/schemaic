# `core/model.rs`

`model.rs` — the shared result-set model everything else is phrased in. `Value` holds text-
protocol cells parsed into compact numeric variants *only where lossless* (`DECIMAL`/dates/JSON
stay `Str`, so nothing is rounded or reformatted); `ResultSet` stores them columnar, so
`CellRef` reads a cell without allocating, and **each column sits behind its own `Arc`** — the
grid's `rs` signal and the shown panel's canonical `QueryState::Loaded` hold the *same*
`Arc<ResultSet>` on purpose, so the post-commit splice mutates through `Arc::make_mut` at a
strong count of 2; with the columns inline that deep-copied every arena (30 ms and ~160 MB at
200k×50, on the UI thread, on the one path built to avoid a rebuild). `splice_rows` replaces
only the column `Arc`s whose values actually changed, so an untouched column is never copied
(29.6 ms → 1.8 ms at 200k×50, measured).
**`splice_cells` is its per-cell sibling, and the difference is not convenience.** The grid's
staged edits are keyed by cell, so a row with one edited cell would have to supply the other
forty-nine to `splice_rows` — read back out with `CellRef::to_value` and pushed again, which
*re-formats* every numeric it round-trips: a `FLOAT` stored as the text `1.0` comes back as
`Value::Float(1.0)` and is written `1`. Here an unlisted cell is copied verbatim, tag **and**
text, under the same copy-on-write rule, so nothing but the listed cells can differ.
`append_rows` is the other half of the same caller's need — the grid's *pending* rows, which the
server has never seen, padded with NULL exactly as `from_rows` pads. Both exist for
`edit::GridCells::exported`. `project_columns(keep)` is the third reader's need: the same rows
with only `keep`'s columns, **in `keep`'s order**, for a reader that takes a block rather than
the whole result — the gutter's *Copy as* over a highlighted range, in drawn order, and each
pending-row group of an `INSERT` copy (`edit::GridCells::insert_blocks_at`). It clones the kept
columns' `Arc`s rather than their arenas, and it **renumbers `capped_columns` and
`binary_columns`** onto the new positions, because every withholding rule downstream reads them
by index: carried over as they were, they would name whichever column now sits at the old
index. The private `clone_meta` supplies every other field. No test here pins the renumbering;
it is reached only through `edit`'s projection tests. `retained_bytes` sums what a result costs to *hold* —
each column's arena plus its packed offset word per cell, as allocated — which is what the
results strip shows on a kept result, and the reason a result of nothing but NULLs is still
megabytes: the cost is per cell, not per character.
**`ResultBuilder::take_chunk(capacity)` is the one way a load produces more than one result**:
it closes off the rows pushed so far as a `ResultSet` and starts a fresh builder over the same
columns, which is what lets an uncapped export reach the disk in blocks instead of holding the
table. `finish` still *consumes*, because an ordinary load produces exactly one result; a load
being written to a file as it arrives produces a sequence of them. The chunk's `elapsed_ms` and
`truncated` are deliberately zeroed — both are facts about the whole load, and an uncapped
stream is never truncated — so the caller stamps the total on whatever it reports. Four tests
here pin the chunking directly, where it had only the indirect coverage of the db crate's
streaming tests: the columns carrying forward with the builder left empty but *typed*
(`take_chunk_cuts_the_rows_so_far_and_carries_the_columns_on`), `truncated`/`elapsed_ms` zeroed
on the emitted chunk and not inherited by its successor
(`a_chunk_claims_neither_truncation_nor_the_loads_elapsed_time`), the empty chunk a load whose
rows divide evenly by the block size ends on
(`taking_a_chunk_with_nothing_in_it_still_yields_the_columns`), and `capped_columns` not carried
into the next block, which would report blank cells in a chunk whose cells are all there
(`capped_columns_are_not_inherited_by_the_next_chunk`). A fifth pins the **opposite** choice
beside it, which nothing had asserted: `binary` *is* inherited, `capped_columns` being a
property of the chunk's arena while this is a property of the column — a blob seen in block 3
says the column holds bytes, and the `<n bytes>` placeholder a later block writes has to be
withheld on the same grounds. Deleting the carry-forward left the workspace suite green, and a
streamed export of a table whose blob column is NULL for the first chunk's rows wrote the
placeholder into CSV, JSON **and** SQL as though it were the value
(`binary_columns_are_inherited_by_the_next_chunk`). Note that
the **grid's** result is still materialised whole up to the row cap; it is the *export* that
streams. `Column`/`ColumnOrigin`/`ColumnFlags` carry the
write-back provenance the wire reports per column, and a binary column is read-only **as
text**: `<n bytes>` is a placeholder rather than a value, so typing or pasting one back would
write the ASCII of its own size into the column (`edit::EditModel::text_editable`). The bytes
themselves are writable, through the blob panel and nothing else. `Column::is_nullable` is
whether the result column may be NULL **as the engine reports it** —
`origin.is_some_and(|o| !o.flags.not_null)` — and so `false` for a column with no origin: an
expression, aggregate or literal carries no answer, and calling it nullable would be a guess
(`only_a_sourced_column_without_not_null_is_nullable`). The engines report from different
places, the result on MySQL and the catalog on PostgreSQL, so an outer join or a view reads
differently on each (the grid's *Nullable markers* entry has both). `flags.not_null` is filled on every engine — MySQL's wire
`NOT_NULL_FLAG`, a catalog query on PostgreSQL, `table_info` on SQLite — so the grid's header
marker, its cell menu's *Set to NULL* and the row editor's NULL toggle (`row_colspecs`) all
read it. On SQLite the rowid alias (`INTEGER PRIMARY KEY`) is `not_null` although the pragma
says `notnull = 0`: it cannot hold NULL, and taking the pragma at its word put the nullable
mark on nearly every table's key and offered a *Set to NULL* that failed with `datatype
mismatch`. `ColumnInfo::nullable` keeps the pragma's answer, which the designer's DDL needs.
`Column::takes_default` is its sibling beside it, read by the cell menu's *Set to DEFAULT*:
whether the server has a default to give — an origin, and neither `no_default` (the user must
supply the value) nor `auto_increment`, because an auto-increment or identity column's
"default" draws the next number from its counter, which re-keys the row rather than putting a
value back. No origin is `false` for the same reason as above — an expression has no column to
default (`only_a_sourced_column_with_a_default_to_give_takes_one`). It is deliberately **not**
`is_nullable`'s complement or subset: a `NOT NULL` column with a declared default is the common
case it exists for. **Its `no_default` half is only as good as the engine's flag**, and SQL
Server's `described_column` sets that flag `false` for every column, so there a `NOT NULL`
column with no default is offered *Set to DEFAULT* — T-SQL's `DEFAULT` on a column declaring
none is `NULL`, which that column refuses, so the batch rolls back with the server's error
rather than writing anything. Not exercised by a test.
**A raw-bytes cell has exactly one rendering, and
it lives here:** `binary_display(len)` → `<n bytes>`, with `is_binary_display` as its
recognizer and `type_is_binary` / `Column::is_binary` as the question "is this column bytes at
all". The three engines each used to answer differently — SQLite showed the size, MySQL
`from_utf8_lossy`'d the bytes into mojibake, PostgreSQL handed over the text protocol's `\x…` —
and the mojibake was a data bug rather than a cosmetic one: it *looks* like data, so a CSV or
`INSERT` export wrote the replacement characters as the value and re-imported as the wrong
bytes. SQLite's was the honest answer and is now everyone's. `type_is_binary` names SQL
Server's `image`, `hierarchyid`, `geography` and `udt` (the wire's name for any CLR type) too:
they arrive as bytes and so as the placeholder, and unlisted, an export wrote `N'<22 bytes>'` as
the value where every other blob is withheld and noted
(`type_is_binary_covers_sql_servers_byte_carried_types`). A dump does better than withholding
them — see `dump::literal_select`.
**The placeholder is the whole of what the grid holds** — the bytes are dropped at the wire on
every engine and never enter a `ResultSet`, which is a deliberate bound rather than an
oversight: a 200k-row result with a `LONGBLOB` column would otherwise be the whole table in RAM
to render `<n bytes>`. That is why looking at a blob is a **second, targeted query**
(`core::blob`) rather than a lookup in the result on screen.
**What the wire carries is not what a type is, and `BIT` is the case that proves it**: MySQL
hands a bit-field over as bytes, which put it on `type_is_binary`'s list, and a `BIT(8)` holding
10 then rendered as `<1 bytes>`, was *withheld* from the CSV and JSON exports — the formats
this app reads back — and was read-only into the bargain. A bit-field has a lossless text form:
its number, which is also what MySQL accepts back. So it is off that list, `type_is_bit` names
it, and `bit_display` reads the bytes big-endian the way the server wrote them (`convert_row`
hoists that per column beside the binary mask, since only the column's type can say those bytes
are a number). PostgreSQL's `bit` never arrived as bytes at all — its text protocol sends
`00001010`, which was being reported as a byte count.
**`bit_cell(bytes)` is what the MySQL loader stores for a `BIT` column, and the variant is the
load-bearing part**: it is a `Value::UInt`, because a `Value::Str` reached `export::sql_literal`
as the quoted `'10'`, and `'10'` assigned to a MySQL `BIT` column is not the number ten —
`Field_bit::store(const char*, …)` takes the raw bits of the *bytes*, so it is `0x3132` = 12594
on a `BIT(16)` and "Data too long" on a `BIT(8)`. A named function rather than two words in the
loader's `match`, so the seam that broke is reachable from a test: `bit_value` was right and had
a table of tests all along, and what was wrong was the variant its answer got wrapped in on the
way to an exporter.
`Column::is_binary` reads **two**
inputs because neither covers every result: `ColumnOrigin::binary` is the authoritative wire
flag but exists only for a table-backed column, so a `bytea` expression with no catalog
provenance reached every caller as ordinary text until the type name was consulted too.
Conversely nothing may act on the type name *alone* — a SQLite `BLOB` column is an affinity,
not a promise, and may hold ordinary text — which is why every decision that discards a value
(`export::dropped_binary_columns`, `pg::pg_cell`) requires the type and the value to agree.
**`ResultSet::binary_columns` is a third input, and it is not a type at all** — it is the
backend's own record that a value in this column *arrived* as raw bytes, set through
`ResultBuilder::mark_binary` (SQLite's `fetch_query`, from `ValueRef::Blob`, which is an
assertion rather than a heuristic). `export::dropped_binary_columns` ORs it into the type signal
because on SQLite there are two shapes with no type signal to find: a blob living in a column
declared `TEXT`, and any column with no `origin` at all — an expression, a join, a CTE.
(The untyped column was already covered: `db::sqlite::declares_bytes` asks
`schema::sqlite_affinity("")`, which is `Blob`.) Recorded per value and reported per *column*,
since that is the grain everything downstream asks at, and a column marked here is still tested
per cell against `is_binary_display` before anything is withheld — so a row of real text in a
column that held a blob elsewhere is untouched.
**The flag is computed once per result, never per cell.** `Column::is_binary` splits a type
name and walks a keyword list; the read loops run up to the row cap times the column count, so
both backends hoist it out — MySQL into a `Vec<bool>` before `ResultBuilder::new`, PostgreSQL
into `pg::cell_kinds`, which now carries the binary flag and the numeric `NumKind` **together**,
one `CellKind` per column, since both halves were derived from the same type name. Asked
per cell it would be tens of millions of string splits for an answer that cannot change between
rows.
`ColumnOrigin::implicit_key` is the one field no
wire reports: it marks a result column that identifies a row but is no column of the table —
SQLite's explicitly projected `rowid` — asserted by the backend on the same trust
`ColumnFlags::primary_key` already carries, and `false` on MySQL and PostgreSQL. It is a key
that is never *editable*, which is why it is a flag and not simply another key column. The
write path's shared decisions live here so the
three engines can't drift: `GridWrite::plan` (the deletes → updates → inserts order),
`one_row_verdict` (the 1-row safety net's verdict, a miss answered as a `OneRowMiss`),
`Rollback`/`engine_is_transactional` (what a rollback actually achieved — see the invariant
below), `WriteFailure` (what a failed write reports) and `drop_committed` (which staged edits
a commit un-stages).
**`WriteFailure { cause, undone }` is a write path's failure as a value rather than a
sentence** — the grid's Commit, an import, a copy — so the interface can say it in its own
language. `WriteCause` is what went wrong: `Server` (the server's or the driver's own words),
`Row` (the rows being written could not be read on — an `import::RowError`, the file's or the
copy's read's, which travelled as text — a bare `Query`, or `Server` where MySQL and SQL Server added the rollback — until it became a value of its own), `OneRow` (the net's `OneRowMiss` — its `action`, a `WriteAction` where
`WriteStep::action` used to answer the English *delete on*/*update on*/*insert into*, the
unquoted `table` and the count), `Batch { rows, wrote, affected }` (an import batch that wrote
another number, `affected` where the engine counts what a statement affected rather than what
it inserted), `CommitCancelled`, `ImportCancelled`, and `Refused` (a refusal of the database
layer's own, made before a batch was sent — a `DbRefusal`, below); `undone` is the `Rollback`, `None` where the path
reports the cause alone. `message()` is the English the errors displayed before it existed,
byte for byte: the cause, then `Rollback::note` — unchanged, still the English clause with
its own leading separator — and `query failed: ` in front exactly when
`WriteCause::is_statements` (`Server`, `Row`, `OneRow`, `Batch`), never over a Stop or Schemaic's own
refusal. That one predicate also decides the error modal's source for `DbError::Write` (under
`schemaic-db`'s `lib.rs`), so the prefix and the offer of *Explain* cannot disagree.
`a_write_failure_says_what_its_error_used_to` and
`the_note_reads_as_one_sentence_with_the_verdict` pin the English; the interface's wording is
held to it by `english_says_every_write_failure_as_core_does` (*The string catalogue*).
**`DbRefusal` is `schemaic-db`'s own sentence as a value** — what `DbError::Refused` and
`WriteCause::Refused` carry, one variant per thing that crate refuses or will not do: the init
SQL's `InitSql(InitRefusal)`, a sign-in's `Transport(AuthMode)`, SQL Server's `BlankValue` and
`CodePage` (each with the file's `row` where an import names one, and `CodePage` the `lost`
character and its code page where it is known), SQLite's `open_target` refusals and worker
failure, MySQL's quote-safety and read-only plan refusals, a read-only PostgreSQL session's
`ShadowedBuiltin { schema, name, operator }` — a schema on its path holding what PostgreSQL could
pick in a builtin's place, its two wordings `db-refused-shadowed-function` and
`db-refused-shadowed-operator` in `i18n::db_refusal_text` — and the rest under `schemaic-db`'s
`DbError` entry. Both carried a finished English `String` until now, so every one of them reached
the window in English whatever the interface's language — the app's `db_error_shown` passed
`Refused(m)` through as written. `message()` is each producer's old English **byte for byte**,
and `Display` is it, so the CLI, the MCP server and the logs, which keep `to_string()`, read
what they read before; the interface's wording is held to it by
`english_says_every_db_refusal_as_core_does` (*The string catalogue*). The names, types and
values inside are the user's or the server's, and a `SqliteWorker`'s text is the runtime's.
**`ConnectFailure` is the same move for the sentences `schemaic-db` composes *around* a
driver's, the SSH library's or the operating system's text at connect** — what
`DbError::ConnectFailed` carries: `TimedOut`, `InitFailed { statement, error }`,
`SessionRetired`, `BeginFailed`, `PrepareSession`, `ReadOnlySetup`, `GuardSession`,
`PlanSetup`, `NoMaintenanceDb`, and `Ssh(SshFailure)` and `Tls(TlsFailure)`, a variant for each
of `ssh.rs`'s and `tls.rs`'s failures (under `schemaic-db`), and `SignIn(SignInFailure)` for a
cloud IAM or Entra sign-in that got no token (`cloud_iam.rs`, `entra.rs`, both halves).
`SshFailure::NoAnswer` carries an `SshPhase` — `Connect` or `Authentication` — where
`ssh::within` used to splice the word into its string, the two host-key refusals carry the host, the fingerprints and the store's file
name rather than the paragraphs — and `OpenSshHostKeyChanged`, the refusal on the user's own
`~/.ssh/known_hosts`, the host and the offered fingerprint, and no line (russh's count skips
comments) — and `Route` holds the `ssh_config::RouteRefusal` itself.
`SignInFailure` names its CLI as a `TokenCli` — `Aws`, `Gcloud`, `Azure` — whose `name()` is
the product as a sentence says it ("AWS CLI (aws)"), passed through in every language, and a
host or user `launch::cli_flag_value` refuses as `BadValue { field, problem }` (`SignInField`,
`ValueProblem`) rather than a sentence with the word spliced in; the CLI's own first stderr
line, or the system's error where it could not be started, rides inside as it came. Each
was a `DbError::Connect(String)` until now, so only the *connection failed:* in front reached
the window in the interface's language and the rest of it in English. `message()` is each
producer's old English **byte for byte**, without that prefix, and `Display` is it, so the
CLI, the MCP server and the logs read what they read before; the driver's error inside, the
hosts, paths and fingerprints pass through as they came. The interface's wording is held to it
by `english_says_every_connect_failure_as_core_does` (*The string catalogue*).
**`QueryFailure` is the same move for the `Query` sentences `schemaic-db` writes itself** —
what `DbError::QueryFailed` carries: `ExportStopped`, `NoRowsToExport`, `NoServerSettings`,
`NoStatusList`, `NoImportColumns`, `ReadStopped(error)`, `BatchContinued` (SQL Server's text,
with the note that the batch ran on past it), `SqliteLocked(error)` (SQLite's text, with the
advice that names the way out) and `RollbackFailedToo { error, rollback }`. `message()` is each
producer's old English **byte for byte**, after the *query failed:* the variant puts in front,
and `Display` is it; the server's, SQLite's or the runtime's text inside passes through. The
interface's wording is held to it by `english_says_every_query_failure_as_core_does` (*The
string catalogue*).
**`CellEdit` is what one staged cell holds** — `Null | Text | Bytes | Default` — and it replaced
the `Option<String>` that had room for exactly two of the first three. `Text` is what was typed
or pasted, bound as a string parameter and coerced by the server to whatever the column is;
`Bytes` is raw octets and is the only shape that can write a binary column, a `BLOB` having no lossless text
form. **It holds an `Arc<[u8]>` rather than a `Vec<u8>`**, because a staged cell is cloned far
more often than it is written: the grid painter clones the value out of the dirty map on every
repaint of that cell, the row panel's `blob_value` on every change to the map, and `build_edits`
again per commit — each one a full copy of the buffer while it is a `Vec`, so a 60 MB file is
60 MB of memcpy per repaint, on the UI thread. The copy that remains is the one at bind time,
where MySQL's and SQLite's `cell_param` want an owned `Vec` (`b.to_vec()`) and nothing can avoid
it; PostgreSQL renders a hex literal instead and never takes one.
`CellEdit::bytes(impl Into<Arc<[u8]>>)` is the constructor, so a `Vec` off a file read and a
slice in a test both build one without a call site spelling the conversion.
`as_text` answers `None` for `Bytes` **as well as** for `Null`, which is the point rather
than an omission: every caller of it is a text path — the re-fetch key, the paste round trip, the
clipboard — and a byte string decoded lossily into one of those is the mojibake `binary_display`
exists to have stopped. `display` renders `Bytes` as that same `<n bytes>`, so a cell a file has
just been loaded into reads the way it will read after the commit and is no more copyable back
into the column than a stored blob is. `to_value` is that same question answered for a reader
that speaks **cells** rather than text — the export, which hands a `ResultSet` on and never asks
the painter — and it parts company with `display` on two variants: `Null` becomes
`Value::Null`, not the word, and so does `Default` (below). A sentinel is the point where a cell is *drawn*; a file is written
by a format that already knows what a null is, so the string `NULL` would land inside a JSON
string and in an SQL `INSERT` where `null` belongs. Only the blob panel produces `Bytes`;
`from_opt` is the
widening for the text paths that still speak `Option<String>`. One type the whole way down —
`StagedEdits`, `RowEdit::set`, `RowInsert::cols`, `edit::DirtyCells` and the grid's `dirty` /
`new_rows` all carry it — so there is no seam where bytes would have to be re-encoded to cross.
**`Default` is the column's default, and it is the one variant that is never a parameter** — the
grid's *Set to DEFAULT*, staged by `GridState::stage_default` and nothing else. No bind can carry
`DEFAULT`, so each writer spells it into the statement (`db`'s write-back paragraphs have the four
spellings, SQLite's being a substitution rather than the keyword). What it will hold is the
server's to decide and unknown until the post-commit re-read brings it back, which sets every
reader's answer: `as_text` is `None`, `display` is `STAGED_DEFAULT` — the keyword `DEFAULT`,
since that is what the statement will say — and `to_value` is `Value::Null`, the same blank a
pending row's unset cell exports. **That last is a blank and not a claim**, so a CSV or JSON
export reads it as a null; the two copies that write *statements* must not, and ask the dirty map
instead of the exported value: *Copy as ▸ SQL UPDATE* (`GridCells::updates_at`) writes
`c = DEFAULT`, as `edit::staged_sql` does, and *Copy as ▸ SQL INSERT* (`insert_blocks`) leaves the
column out of that stored row's `INSERT` — grouping stored rows by the columns they keep, as it
already grouped pending rows — which is what Commit's own `INSERT` does with one
(`copy_as_update_writes_a_staged_default_as_the_keyword`,
`an_insert_of_a_stored_row_omits_a_column_staged_to_its_default`, both red against the copies that
wrote `NULL`).
**`ErrorSource`, `ModalError` and `ErrorModalContent` decide what the shared error modal
offers**, and sit beside `QueryState` because the modal's fallback is that type's `Failed`. A
surface hands the modal a `ModalError` — the text and whose words it is, `Statement`, `Server`
or `App` — and `ErrorModalContent::resolve(override, tab_error)` answers what it shows and which
of *Explain* and *AI fix* it offers, each carrying the text it acts on: a statement's failure
gets both, the server's words about anything else (a failed `COMMIT`, a refused connection)
*Explain* alone, and Schemaic's own notices — a recovered `connections.json`, a failed `.sql`
save, a keyring notice, "Not connected to X" — neither. The modal used to offer *Explain* on
all of those, and there was nothing in them for a model to explain. The recovery notice's source
is pinned where it is chosen:
`diagram_layout_gate::a_recovery_notice_reaches_the_modal_as_schemaics_own` reads
`report_recoveries`' body for `ModalError::app`, and fails with `::server` in its place. With
no override it falls back to the active tab's run error, and **that fallback carries its own
source**: `QueryState::Failed` holds a `ModalError`, `resolve` takes `over.or(tab_error)`, and
both go through the one `match`, so the fallback and an override cannot drift apart. It used to
be `Failed(String)`, resolved *as* a `ModalError::statement` — a statement's failure by
construction, which it was not: a run refused before anything was sent (`db_for`'s "connection
no longer exists", its no-tunnel refusal — "SSH tunnel is not established yet", or the last
load's failure, `connection::TunnelFailures::refusal` — `session_for`'s refusals), a timeout, a
cancel or not-sent note, a failed `BEGIN` and a refused connection all land in the tab as its
error, and every one of them was offered an AI fix and an explanation
(`a_tab_refused_before_sending_offers_neither_action`). The run paths in `app/main.rs` now say
which at the point of failure: a `DbError` through `DbError::modal_source` and then
`core::tx::run_error_source`, the not-sent, timeout and cancel notes and the
`db_for`/`session_for` refusals `App`, a failed `BEGIN` `Server` — the server's words, but not
about a statement of the user's. The schema tree's per-database Refresh raises its `db_for`
refusal as `ModalError::app` too, rather than writing it into the node's `SchemaState::Failed`,
whose row labelled every failure the server's (it held a bare `String`, fed by the fetch's error
as well) and so offered *Explain* on "SSH tunnel is not established yet". `Failed` holds a
`ModalError` now — the fetch's failure alone, `Server`, its English as `text` and
`db_error_shown`'s wording as `shown` — which `failed_row` hands the modal as it is. **That makes the `DbError` variant the provenance, so
`schemaic-db` has to write the right one**: it wrote its own refusals and its own set-up
statements' failures as `Query`/`Connect`, so the single run folded a failed lazy `BEGIN` into
the statement's result and the bar offered *AI fix* over a healthy `SELECT` — while Run All,
which hard-coded `ModalError::server` for the same failure, did not. Both now take the source
`Session::ensure_tx` gives (`ConnectFailed`, `Server`, for a refused `BEGIN`), and opening a Manual tab's session
carries `Session::open_enforced`'s through `DbError::modal_about_another_statement` rather than
calling every failure the server's. `DbError::Refused` and the
connect-failure rule for set-up failures are under the `DbError` entry. The `App` notes' constructor
is a call-site choice the type forces and nothing else pinned — swapping one for `::statement`
re-offered a fix for "the statement was not sent" with every other test green — so
`app_tests::a_runs_own_notes_reach_the_bar_as_schemaics` reads `main.rs` for it: every
`QueryState::Failed(` built from `not_sent_message(`/`timeout_message(` opens with
`ModalError::app(`, and every `i18n::run_cancelled_message(` is followed within 900 bytes by
`QueryState::Failed(ModalError::app(en).shown_as(m))` — core's English the text, the wording
shown — with floors on both counts. `ModalError::new(text, source)` is the
constructor for a producer already holding a source. The editor's error bar asks the same
`resolve_kept(None, Some(err), kept)` for its own *Explain* and *AI fix* as the modal's
fallback does, so bar and modal cannot offer different ones — including on a **kept** (pinned)
failure, which earns neither: both actions resolve against the current buffer and a pin's
message is a snapshot of a statement that may no longer be in it. The bar used to withhold them
itself while its *View* opened a modal that asked plain `resolve` and offered both again
(`a_kept_failure_offers_no_fix_from_the_modal`). The statement *Explain* attaches is
`ErrorModalContent::explain_range` on both surfaces — `error_fix_range`'s choice, and only when
the error earns a fix — where each view had spelled that rule on its own
(`explain_attaches_a_statement_only_when_there_is_one_to_fix`). **`ModalError::about_another_statement` rewrites a source after the fact**,
for an error raised by SQL that is not the editor's: a grid filter/sort re-run's failure goes
into `view_err` with `Statement` turned to `Server`, because that SQL is one Schemaic built and
the modal's *AI fix* would act on the editor's buffer; `Server` and `App` pass through unchanged
(`about_another_statement_keeps_the_words_and_drops_the_fix`). A grid Commit and a Manual tab's
`COMMIT`/`ROLLBACK` (`end_tx`) reach it through `DbError::modal_about_another_statement`, which
is `modal_source` composed with it — the commit arm had restated that as "`Cancelled` is `App`,
everything else `Server`", and `end_tx` hard-coded `ModalError::server`, so both offered
*Explain* on Schemaic's own sentences. **The
source rides with the text in one value because it used to be a separate flag**
(`error_modal_fixable`): every surface had to remember to set it and the modal to clear it on
close, or the next open inherited the last one's answer; now a surface cannot supply one
without the other. **An override's own source alone decides**, and the tab's error never
stands in for an action the override withheld — the modal is showing the notice, and an
*Explain* would be about something else (`schemaics_own_notice_offers_neither_action`). The
actions are a `match` rather than `!= App`, so a fourth source has to decide its own rather
than land on whichever side of a comparison it happens to fall.
**`ModalError::worth_viewing(fits_chars)` is the grid bar's *View* condition**: offered when
one-lining to `fits_chars` hid something (`text::hides_detail`) *or* when the modal has an
action the bar does not — and the second half is asked of `resolve` rather than restated, so
the bar and the modal cannot disagree about what the modal offers. It used to be `hides_detail`
alone, so a one-line commit failure — the server's words, which the modal offers *Explain* on —
had no way into the modal at all; a short notice of Schemaic's own still gets no *View*, since
it would open a modal repeating the bar with nothing to do about it
(`a_short_notice_of_schemaics_own_is_not_worth_viewing`). **`CommitDone::Failed` carries a
`ModalError`, not a `String`**, because the one channel carries both the server's refusal and
Schemaic's own ("Not connected — the commit was not attempted") and only the producer knows
which; `settle_after_switch` passes it through untouched, source and all
(`a_failure_survives_a_switch_unchanged` checks both).
**A `ModalError`'s `text` is English wherever it can reach a model or a log; a translation goes
in `shown`**, which is for the eye alone. That is every `Statement` and `Server` source, and
every run path's `App` note — the not-sent, timeout and Stop notes that `ran_of` records, held
to `ModalError::app(en).shown_as(m)` by `a_runs_own_notes_reach_the_bar_as_schemaics` (under
`app/main.rs`). It is *not* every `ModalError`: an `App` notice that is only ever shown — the
recovery report (`report_recoveries`, under `ui/lib.rs`) and the keyring's save notice, both
through `i18n::notice_text`; the AI panel's two run refusals; the connection gate's
connection-changed, not-connected and tab-moved-on notes in `app/main.rs`; the grid's
nothing-to-fill and staged re-query notes — is built as `ModalError::app(tr!(…))` (or an
`i18n::` wording) with the wording in `text`. That is harmless only because `App` offers
neither *Explain* nor *AI fix* and none of them is a `QueryState::Failed` the write log reads;
one moved onto a path that logs or reaches a model has to move to `shown_as` with it, since no
test would notice. `displayed()` — the wording, else the text — is what the editor's error bar and the
grid's bottom bar draw and what `worth_viewing` measures, and `resolve_kept` hands it on as
`ErrorModalContent::shown`, while `explain` and `fix` go on carrying `text`. Everything that
*acts* on the error reads `text`: *Explain* and *AI fix* send it to the model, the statement
*Explain* attaches is located in it (`explain_range` reads `fix`), and the write log records it
(`ran_of` on a `QueryState::Failed`). The two used to be one field, and when the grid Commit's
failure began to be worded from the catalogue (`modal_error_shown`, under `app/main.rs`) the
wording went into `text` — so under a non-English interface *Explain* sent the model a
translation. A producer with a wording sets it with `shown_as`, which leaves `text` alone;
`about_another_statement` changes the source and keeps `shown`
(`the_actions_act_on_the_english_whatever_is_shown`), and drops `at`, the server's position,
which indexes SQL Schemaic built rather than the buffer
(`about_another_statement_keeps_the_words_and_drops_the_fix`). It carried `at` along once,
and only the squiggle's own `Statement` gate stood between that position and the editor. The editor's run paths had the same fault
over their transaction notes and their own not-sent, timeout and Stop notices, and keep the
two apart the same way now (under `core::tx`).
