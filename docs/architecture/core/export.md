# `core/export.rs`

`export.rs` — CSV/JSON/SQL/Markdown/HTML/Excel export (incl. CSV formula-injection guard;
Markdown pipe/backslash escaping; HTML entity escaping). **`sql_literal` writes a float far
from 1 with an exponent** (`float_literal`: outside `1e-5..1e16`, `{f:e}`, the shortest digits
that read back exactly): Rust prints an `f64` without one, so `1e300` was a 301-digit integer,
which T-SQL reads as a `numeric` and refuses past 38 digits (Msg 1007, Msg 103 past 128
characters) — a SQL Server dump holding one could not be restored. The exponent form is a
float literal on all four engines; an ordinary value keeps the plain form
(`a_float_far_from_one_is_written_with_an_exponent`). **Rows arrive through a pull source,
not as a `&ResultSet`**: `RowChunks::next_chunk` hands over one `RowChunk { rs, order }` at a
time and `ExportFormat::stream_to` renders from it, returning an `ExportTally`. Every entry
point took a whole result until the whole-table export landed, which made "export" mean
"export whatever the row cap fetched" — and materialising two million rows to render them
would hold the table twice over and run into `ResultSet`'s 512 MiB per-column arena ceiling,
which does not fail but blanks the cells past it, so the file would quietly have holes in it.
**Pull and not push, because JSON decides it**: its renderer holds a `serde_json` sequence
serializer open across the whole array, borrowing the writer, and that is not a value a push
API could store between calls — inverting it keeps the serializer a local in one function.
`OneChunk` is a source of exactly one chunk (what `render_to`, and through it the clipboard, is
built on) and `PullChunks` adapts a `FnMut() -> io::Result<Option<ResultSet>>` — the app's
`rx.blocking_recv()` —
materialising natural order into a **reused** `Vec<usize>`, one allocation for the whole
export against six renderers that would each have needed a second code path. `OneChunk`
yields **even for an empty result**, which is load-bearing rather than an edge case: CSV,
Markdown and HTML take their header from the first chunk, so a source that yielded nothing for
a zero-row result would write an empty file where the old code wrote a header. One empty chunk
and no chunk at all are deliberately different things
(`an_empty_chunk_carries_a_header_and_no_chunk_carries_nothing`).
**`SliceChunks` is `OneChunk`'s sibling for rows already in memory**, handed out `size` at a
time — what the app's `Fetched` export renders through, so that a save of rows already in hand
has a count to report and a Stop that lands between blocks. **Nothing is copied**: a chunk is
the same `&ResultSet` plus a *slice* of the same order vector, so it is a cursor over one
allocation rather than a partition of the rows. It follows `OneChunk` at exactly the place that
matters — **an empty order still yields one empty chunk, and only then `Ok(None)`** (that is
what the `started` flag is for). Five renderers write their header on the *first chunk they
see*, so ending the stream before any chunk writes no header at all: the rule that reads as
safe here is the one that lost the file, and a `SELECT` matching nothing exported a **0-byte
file** where `render_to` wrote `id,name\n`.
`an_empty_result_writes_the_same_bytes_through_either_path` is what holds the two paths
together, and it checks the equality is not vacuous by asserting CSV still carries its header.
A release-review pass measured the pair over an empty order for **all six** formats and found
them byte-identical — JSON `[]` 2 B, CSV header 8 B, SQL 0 B (its column list lives on each
`INSERT`, so no rows means no output), Markdown 28 B, HTML 108 B, Xlsx 5,218 B — while that
test and `chunking_a_fetched_result_cannot_change_the_bytes` both cover the five text formats
only, Excel's empty-source question being asked of a workbook in
`an_empty_xlsx_export_still_carries_its_header`. A source that ends *before* its first chunk is
still a different thing and still writes nothing; that is `PullChunks`' shape, not this one.
A `size` of `0` is clamped to the whole
slice rather than rejected: the figure comes from a tuning constant, and an export that spins
handing out empty chunks forever is a worse answer than one that writes a single block. Its one
hook is `SliceChunks::watching(f)`, `f: FnMut(u64) -> bool`, called as each chunk is handed out
with the **running row total** and returning `false` to stop — **one hook doing both jobs on
purpose**, because the caller reports to its progress channel and asks its cancellation token at
the same instant and out of the same closure, and two hooks would be two chances to ask them at
different moments, which is how a Stop comes to be noticed one chunk late. A `false` surfaces as
an `io::Error`, because that is the only way a `RowChunks` can end an export *without* it
looking finished: the renderers treat `Ok(None)` as end-of-stream and would publish a truncated
file as a complete one — the same reasoning, and the same failure, as `PullChunks`'
cancelled-read arm. Chunking a result that used to render whole must be invisible in the file,
and that is the whole risk of the change: `chunking_a_fetched_result_cannot_change_the_bytes`
renders every text format at every chunk size from 1 to len+1 and compares byte for byte
against `render_to` — the same review pass measured those boundaries for **Xlsx** as well and
found them byte-identical, which the test itself does not reach — and
`a_stopped_export_writes_nothing_through_the_renderer` asks the seam question of every format —
`stream_to` must propagate the refusal rather than treat it as a short read, since each writes a
header on the first chunk and a swallowed error would leave a header-only file looking valid.
Each `export_*_chunks` is
now the *only* implementation: every `*_to<W: io::Write>` form is that function over a
`OneChunk` and `ExportFormat::render_to` is `stream_to` over one, returning the same
`io::Result<ExportTally>` — so there is no buffered renderer and streamed renderer to keep in
agreement, with the `String` versions still thin wrappers for the clipboard. `render_to` is now
reached in production **only** through those wrappers: the app's `Fetched` export moved off it
onto `SliceChunks` when it gained a progress modal, so what is left of it is the one-chunk
spelling the clipboard and the tests render through. Two tests hold that together —
`streaming_render_matches_the_string_render_in_every_format` for the byte-for-byte agreement
per format, and `a_chunked_export_matches_the_same_rows_in_one_go` for the same rows split
into blocks rendering identically; add a new format by writing the `*_chunks` and wrapping it.
**Excel is the sixth format and the only binary one.** `export_xlsx_chunks` writes one worksheet
— named by `sheet_name` after the source table, because Excel *rejects* a workbook whose sheet
name breaks its rules rather than repairing it (at most 31 characters, none of `[ ] : * ? / \`,
no leading or trailing apostrophe, **not the reserved `History`** in any case, and `Result` when
the result is not a table's). `rust_xlsxwriter` enforces the first three and documents the
reserved name without checking it, so that one is ours — and `history` is an ordinary table name
on every engine, which makes it reachable rather than theoretical. **And so is every control
character, which is not an Excel rule but an XML one**: a sheet name is written into an
*attribute*, the escaper that runs there covers `& " < > \n` and nothing else, and a C0
character other than tab, LF and CR is not an XML 1.0 character at all — so a table named
`a<U+0001>b`, legal on MySQL and on PostgreSQL, produced an `xl/workbook.xml` that does not
parse, and Excel refuses such a workbook rather than repairing it: *Exported n rows* over a file
nothing opens. They map to `_` **before** the 31-character cut, so the ceiling is counted over
what survives. It is the same rule twice more elsewhere — `suggested_filename` already scrubs
them out of the *file* name derived from that same table name, and `erd_export::svg_text` exists
for it alone. The sheet has a frozen
bold header row and **typed cells**: `CellTag::Int`/`UInt`/`Float` become worksheet numbers and
everything else a string, so the application has nothing left to guess at, which is the whole
point of the format over a CSV that Excel then guesses at — the guess that turns `SET-1` into a
date and drops the leading zeros off a postcode. The exception is a number a worksheet could not
hold *exactly* — an `i64`/`u64` past 2^53, a non-finite float — which goes out as **text**,
because a worksheet number is an `f64` and a `BIGINT` key that comes back off by one is a silent
corruption. **The exception's exception is the fixed-point family**, which is `Str`-tagged for
that same exactness reason and is the commonest numeric column there is: sending a
`DECIMAL(10,2)` money column out as text is the CSV behaviour this format exists to replace —
`=SUM(B:B)` reads 0 and the column sorts as words. So `is_fixed_point` (`DECIMAL`/`DEC`/
`NUMERIC`/`FIXED`/`MONEY`/`SMALLMONEY`, matched on the leading token) plus `exact_decimal`
(the text, normalised the way `f64`'s own `Display` writes it, comparing equal to the parsed
value) makes it a number when — and only when — an `f64` holds it digit for digit. **The
column's type is what licenses it, never the shape of the text**: a `VARCHAR` postcode that
looks like a number is not one, which is the whole guess this format exists to stop.
`write_xlsx_cell` is that decision split out of the loop, and it answers **whether a cell
actually reached the sheet** — not the same as "was called", since an empty string writes
nothing. The caller needs that answer because **a row that wrote no cell would be no row at
all**: the writer emits a `<row>` only for a row that received one and the sheet's dimension
comes from the cells actually written, so an all-NULL (or all-withheld) row at the *end* of a
result vanished from the file while the tally still counted it, and an interior one survived
only by accident, stretched into existence by a later row. Such a row now gets one **formatted**
blank — `Format::new().set_unlocked()`, because this writer drops an *unformatted* blank exactly
as Excel does, and cell protection is the one property that changes nothing a reader can see on
a sheet that is never protected. A blank cell is the worksheet's way of saying "this row is here
and empty", which is what CSV's unconditional `\n` says. `rust_xlsxwriter`'s `constant_memory` feature is
load-bearing: it spills each finished row's XML to a library-managed temp file, which is what
holds an export to a bounded buffer at any row count — the bound `RowChunks` exists to give, and
the one an in-memory workbook would throw away at exactly the size where it matters. That makes
`export_xlsx_chunks` **the one function in `schemaic-core` whose tests touch the filesystem**
(see the pure-logic invariant). Excel's own ceilings are `XLSX_MAX_ROWS` (1,048,576),
`XLSX_MAX_COLS` (16,384) and `XLSX_MAX_CELL_CHARS` (32,767): a cell past the last is cut by
`fit_cell` and reported through `ExportTally::cut`, while a result taller than a
worksheet is **refused** — with an error naming CSV and JSON as the way out — rather than
truncated, stopping at row 1,048,576 of a 3M-row table being the one loss the file would carry no
trace of at all, and the `.part` dance means the refusal leaves the destination alone. **The cut
is counted in UTF-16 units**, which is what Excel counts, taken on a `char` boundary because
slicing a `&str` mid-codepoint panics; an all-ASCII value short enough in *bytes* cannot be too
long in units either, so the common case returns untouched without being walked twice. A result
wider than a worksheet is refused the same way: unreachable in practice, since no engine returns
16,384 columns, but stated rather than assumed because silently dropping the columns past it
would be the worse failure — and it is asked of **every chunk, not just the first**, since
`RowChunk` documents same-columns-per-chunk as a precondition rather than enforcing one, and the
`ci as u16` casts in the cell loop have no other bound. **The
known fidelity gap**: an empty *string* writes no cell, since `rust_xlsxwriter` emits nothing for
one, so it reads back as NULL — the same ambiguity CSV has, deliberately not fixed here.
And because the rendering is bytes rather than text, `ExportFormat::is_text` exists: the grid's
**Copy** menu renders through `render()` → `String`, where `to_string`'s `unwrap_or_default`
turns a binary rendering into the *empty* string, so an Excel entry there would silently clear
the clipboard and report success. That menu filters on the capability while the Download menu
still offers every format (*Data grid*) — the split `erd_export::ErdExportFormat::is_text`
already makes for PNG — and the capability is computed from the variant rather than stored, so a
seventh format has to answer it. **It is an exhaustive `match`, not `!matches!(self, Xlsx)`**,
and so are `writes_incrementally`, `erd_export::ErdExportFormat::{is_text, is_picture}` and
`import::ImportFormat::has_own_nulls`: a negated `matches!` compiles against a new variant and
silently answers `true` for it, which for `is_text` is an entry that clears the clipboard and
for `writes_incrementally` is a user sent to a `.part` sibling that is a truncated archive. The
census that was supposed to catch exactly that could not, because
`every_format_is_covered_by_a_parity_test` partitions **by `is_text()`** — a seventh binary
format defaulting to `true` lands in the text half and the census passes. The gate's needle was
the predicate the defect was in. `every_variant_is_in_all` is the one that does not have that
shape — but it used to *be* that state rather than the guard against it: it summed `1` over `ALL`
and asserted the total was `6`, then asserted `ALL.len() == n`, which is `6 == 6` twice over,
both terms read off the same `[ExportFormat; 6]`. A seventh variant left out of `ALL` compiles
the moment the exhaustive `match`es have their arms, and the whole module goes on passing by not
looking at it. Nothing in the type system relates the enum to the array, so the check is **this
module's own source**: the variant names the enum declares against the names the `ALL` literal
lists, with both sides held to the real `ALL`'s length so a parser that stops matching fails
rather than comparing an empty set with an empty set. (Reading the repository's own source is the
sanctioned exception to the no-filesystem rule, and it is `doc_coverage.rs`'s shape for the same
kind of question.) It checks `ALL` for duplicates as well — two of one format would make the
counts agree for the wrong reason — while the sibling half, that a new variant must *answer*
`is_text` and `writes_incrementally` rather than defaulting to `true`, stays a compile error. **The filter is `clipboard_formats()`, not `.filter(|f|
f.is_text())` written at the call site**: the predicate had a test and its application did not,
so deleting the filter left the whole suite green and shipped an Excel entry that clears the
clipboard. The composition is the part that can regress, so the composition is what has a name
and a test. **`writes_incrementally()` is the second capability**, and the answer to a question
the export plumbing was written before anything could answer `false` to: the five text formats
reach the sink as they go, so a failure halfway leaves a `.part` sibling holding the rows that
arrived, while `Xlsx` is a ZIP and nothing reaches the sink until `save_to_writer` — so every
pre-save failure left a **zero-byte** file the message pointed the user at. `export_cancel_note`
and `export_failure_note` therefore take that answer rather than `true`, and the app sweeps the
empty sibling instead of leaving it as litter (*Data grid*). `stream_to`/`render_to` carry a `W: Send` bound for this
renderer alone: `rust_xlsxwriter` hands the writer to the thread that assembles the workbook's
ZIP, and every sink an export already writes to (`BufWriter<File>`, `Vec<u8>`) satisfies it.
**The formats anything reads
back must not pass a raw-bytes cell straight through.** Such a cell is
`model::binary_display`'s `<n bytes>` (a `Value` has no bytes variant to hold the real thing),
and emitting that produces a file which silently stores the *placeholder* as the column's data
on re-import. `dropped_binary_columns` finds those cells in a pre-pass — requiring a *column*
signal **and** the cell's text to agree, since either alone is wrong in a way that loses
data — and `withheld_binary` is the one per-cell test every withholding emitter shares, so the
two-signals rule cannot be spelled differently in one of them. **The column signal is
`edit::holds_bytes`** — `Column::is_binary` OR `ResultSet::binary_columns`, asked through the
one function that defines the pair rather than spelled here as well — the second being the backend's own
per-value assertion that the column handed over raw bytes (`ResultBuilder::mark_binary`, set from
`ValueRef::Blob` in SQLite's `fetch_query`). It is there because on SQLite a type name cannot
always answer: `declares_bytes` already covers every `…BLOB…` spelling, the `BINARY` family
**and** the untyped column (`schema::sqlite_affinity("")` is `Blob`), but it cannot see a blob
living in a column declared `TEXT`, and a column with no `origin` at all — an expression, a join,
a CTE — has no declared type to ask. In both of those the two-signal rule degenerated to "never
withhold" and the `<n bytes>` placeholder went into CSV, JSON and SQL as though it were the data.
The consequence to know: in a column that has handed over raw bytes, a cell whose text reads
exactly `<n bytes>` is now withheld — the same answer a declared `BLOB` column always gave.
**Which emitters withhold is decided by whether anything reads the format back**, and that is
four of the six: the SQL export (`NULL`, plus a `-- NOTE:` heading the script — a comment
rather than a refusal, since the script still runs and the one thing it may not do is pretend
the placeholder was the data) and **CSV, JSON and Excel**, which are exactly
`import::ImportFormat`, including the single-column "copy this column" forms the first two have
(a worksheet has none). Markdown and HTML keep it, deliberately:
nothing reads those back, and there the placeholder is the *useful* rendering — blanking it
would make a 4 MB blob indistinguishable from an empty cell and from NULL, which is less than
what the grid itself shows. This was the SQL export alone at first, and the commit that made it
so named CSV in its own account of the bug.
**None of this changed when the bytes became reachable.** `core::blob`'s panel re-reads one
cell on demand and can write *that* cell to a file; every emitter here still withholds the
column exactly as described, because a stream has no bytes to carry — the placeholder is all
the `ResultSet` ever held.
`binary_mask` is the same answer in the shape the cell loop needs — one `bool` per column,
indexed by `ci`. Two shapes for one fact because the `-- NOTE:` wants the ordered `Vec<usize>`
while the loop asks per *cell*, where `Vec::contains` is a linear scan over the answer (12M of
them on a 200k × 60 result); the same hoist `Db::convert_row` and `pg_cell` make for
`Column::is_binary`.
**Both are computed per chunk, and the `-- NOTE:` line is the one thing a stream genuinely
cannot know up front.** Which columns were withheld is a fact about the rows in hand — it needs
the column signal *and* a `binary_display` placeholder in the data to agree — and a stream never
holds them all, so a column can carry real bytes for a million rows and meet a placeholder in
the next block. The note is therefore emitted the first time a column is actually withheld,
naming only the newly discovered ones and tracked in a `noted: Vec<bool>`; over a single chunk
that collapses to exactly the old behaviour, one note before the first `INSERT` naming every
withheld column (`the_binary_note_finds_a_column_that_only_drops_later`).
**That line's column names go through `comment_text` as well as `ident_sql`, and the pair is the
point**: quoting an identifier and making it safe on a comment line are different guarantees, and
this note is the case that proves the first is not the second. It always quoted, and a column
name holding a newline still ended the `--` there and left the rest of the name on the next line
of the file as a statement — `ident_sql` doubles a backtick or a double quote and does nothing
about `\n`. `comment_text(raw)` is the answer, placed here beside the quoters so the grep the
"one identifier quoter" rule relies on finds it too; the dump's four header sites are its other
callers, and the invariant is written up under *Architecture invariants*.
**A literal only means what it was written to mean on a session that reads it that way**, which
is what `MYSQL_LITERAL_MODE_SQL` and its dialect wrapper `literal_mode_sql` are for.
`sql_literal` doubles a backslash for the MySQL family because that is what the server does with
one *by default*; under `NO_BACKSLASH_ESCAPES` the doubled literal means two backslashes, so a
value `a'b\c` written `'a''b\\c'` comes back `a'b\\c` — one character longer, no error, measured
on MariaDB 10.11.14 and MySQL 8.4.11. The statement **takes that one flag out of the mode rather
than overwriting it**, so whatever strictness the server was configured with survives: this is
about what the literals mean, not about what the statements around them may do. The flag has to
be named to be on (`ANSI` does not imply it, checked on MariaDB 10.11.14), which is what makes
the comma-wrapped `REPLACE` + `TRIM` surgery exact rather than a guess. PostgreSQL and SQLite
answer `None`, which is the same split `sql_literal`'s own arms take — but for two different
reasons, and **PostgreSQL's is a claim about the *connection*, not about the engine.** The
reason this doc used to give was wrong: it said PostgreSQL announces the other setting with a
`WARNING` at every occurrence rather than silently, which is true of the server and false of
this app, since `tokio_postgres` delivers `NoticeResponse` only on the connection's
`AsyncMessage` stream and `AsyncMessage`, `poll_message` and `Notice` appear nowhere in
`schemaic-db` — the announcement is discarded before anything could show it, and a `WARNING` is
not an error anyway. Had the app rested on that argument, a cluster with
`standard_conforming_strings = off` would have stored a password containing a `\` as a
*different* password than the preview showed, silently, and that GUC is settable per role, per
database and in `postgresql.conf` with every new connection inheriting it (measured on PG 16.15
through `psql`: `ALTER ROLE zz PASSWORD 'p\ass''q'` stores `pass'q`, the typed value is then
refused and the mangled one accepted). It rests on the startup pin instead:
`pg::connect_probe` sends `-c standard_conforming_strings=on` on the **startup packet**, so
every connection this app opens parses literals the way `sql_literal` wrote them — in force
before the first statement and not undoable by anything the app runs, which is why there is no
statement to return and why `pg::run_ddl` issues nothing where `mysql::run_ddl` issues
`MYSQL_LITERAL_MODE_SQL`. SQLite's `None` is a third reason rather than either: no backslash
escape at all, and no setting that would give it one. **The pin was one line of connection
config with nothing testing it**, which is the shape that gets deleted as dead configuration, so
two tests hold it now, both watched failing with the option replaced by an `application_name`:
`only_mysql_needs_a_statement_to_settle_what_a_literal_means` reads the option out of `pg.rs`
over **code only** — the comment block above that option explains the setting by name, and the
first version of this gate stayed green over the deletion because of it — and the live tier's
`a_password_with_a_backslash_is_stored_as_it_was_typed` turns the GUC off on the role the tier
connects as and checks that a plan's connection reports `on` anyway. **Beside it on the startup
packet, the three settings that decide how a value the app reads is spelt** —
`extra_float_digits=3`, `DateStyle=ISO`, `IntervalStyle=postgres`, as `pg_dump` pins them for
its own read: every PostgreSQL cell is the server's text, which a dump writes and a copy
inserts as it came, so a role's `extra_float_digits = 0` (PostgreSQL 11's default) copied
`0.1 + 0.2` as `0.3`, its `DateStyle = 'SQL, DMY'` dumped 2 January as `02/01/2026` — 1 February
to a default session — and `IntervalStyle = sql_standard` an interval the default reads with
the opposite sign on its hours (`a_role_cannot_change_how_values_are_spelt`, which sets all
three on the tier's role). The init SQL may name none of them, since it runs after the packet
(`init_sql`, below). Three kinds of
caller: a **file**, where `dump::literal_mode_guard_sql` wraps it in a save/restore pair, a
**live plan**, where `Db::run_ddl` and `Db::run_server_ddl` run the bare statement best-effort
beside `lock_wait_sql` — scoped to the plan, never pinned at connect time, because a user who
sets that mode means it for the SQL they type — and **an import's or a copy's write**, whose
session sends `export::import_mode_sql`: the flag off *and* `NO_AUTO_VALUE_ON_ZERO` on, so an
`AUTO_INCREMENT` key of 0 is written as 0 rather than as the counter's next value (a copy
renumbered such a row), and a server-wide `NO_BACKSLASH_ESCAPES` no longer doubles every
backslash a copy or import writes — which it did, the path never having sent the statement
(`a_mysql_copy_keeps_floats_backslashes_and_a_zero_key`, red on MariaDB 11.8 with the global mode
set). Not best-effort there: a session that kept the flag would store other values. The file's
guard takes the same mode, and with it `export::rows_session_sql`'s `+00:00` zone — see
`literal_mode_guard_sql` under `core/dump.rs`.
**`ExportTally` is what a renderer returns, because a row count was never the whole result.**
`rows`, plus three losses the file itself shows no trace of: `withheld` (those binary columns,
named — empty for Markdown and HTML), `blanked` (`ResultSet::capped_columns`, the cells past a
column's 512 MiB arena, which read back as the empty string) and `cut` (columns cut to
`XLSX_MAX_CELL_CHARS`, which only the Excel renderer can produce). Nothing had ever read that
second flag, so a streamed chunk that overran the arena wrote a file with holes in it and
reported a full row count: the grid surfaces the arena cap with a note of its own, but a streamed
chunk is never mounted in a grid. **`cut` is a third category rather than an arm of
`blanked`** because the two say different things to someone deciding how much of the file to
trust: a blanked cell is empty and looks like a NULL, a cut one *looks complete* — and it
is the only one of the three the user can act on, by exporting that column as CSV or JSON, which
have no cell ceiling. It is spelled `cut` and not `truncated`, the word it wanted, because
`ResultSet::truncated` already means the *row cap* was hit and both are read in the same export
and grid paths — `grid.rs` asks `rs.truncated` for the export-scope split a few lines from where
it receives this tally, and two fields of one name meaning different losses is how a caveat ends
up reporting the wrong one. `export_note` says it, `absorb` folds it and `has_caveat` counts it, on the
rules the other two already follow. `ExportTally::note` folds one
chunk's losses in without repeating a name, since a streamed export meets the same column in
every chunk and a caveat that named `body` two hundred times would say less than one that names
it once. `ExportTally::absorb` is that same question one level up — folding **another table's**
tally in, rows summed and a column name kept once in first-seen order — because a dump writes
many tables into one file and reports one sentence about it. It lives here rather than in the
dump's writer loop for the reason `dump_verdict` does: written out at the call site it is a fold
nothing can test, the caller needing a `Db`, a runtime handle and two channels to reach.
None of the three is an error — the file is written and the rows in it are real — so the one
thing that must not happen is a *surprise* going unsaid, which is `export_said(tally, name,
streaming)`'s job: silent for a clean non-streamed save, and **never silent about any of the
three losses**. A blanked or cut column is a surprise anywhere — the value was there, the file
holds less of it than the screen does, and nothing on screen says so.
**A withheld binary column used to be exempt, and the exemption no longer has a term to hang
on.** The argument for it was that a *save of the grid* already renders that cell as
`<7 bytes>`, so *"a text export cannot hold raw bytes"* restated what the user could see, at a
length that painted past the export modal it lands in. That is true of the grid-mounted export
(`SliceChunks`) and false of every path that reaches `withheld` without one — All rows, the
dump, the folder export, all `PullChunks`. Those rows were never rendered, so nothing showed
anyone `<7 bytes>` for them; the file has an empty field where the bytes were, which is also how
every text format writes NULL, and re-importing it writes empty over the blobs.
Gating the clause on `streaming` looks like the fix and is not, which is the part worth knowing
before touching it: **every production caller that writes a file passes `true`** — the grid's,
the dump's and the folder export's alike (the last through `dump_view::files_done_note`'s
`export_said(…, true)`; core's `files_note` passes `true` too, but is now only the English
`english_says_every_folder_outcome_as_core_does` holds that wording to; only the grid's
clipboard copy passes `false`, having no
modal to confirm it), because the grid export reports into a modal that stays up until it is
dismissed and a confirmation saying nothing about a clean save would be a dialog reporting
silence. So the flag no longer means "no grid was mounted", and a clause hung on it would read
as exempting the grid save while exempting nothing. The clause is said unconditionally instead,
at a length the modal can hold — the other half of the original objection, and the half that was
fixable. The silence gate at the top of the function is `!streaming && !t.has_caveat()`, so the
flag now decides only whether a **clean** export speaks; and the `" — "` / `"; "` seam is
computed per clause from **what has already been said**, not from a sibling category — the `cut`
clause read `blanked` alone, which is the shape that opens with `; ` the day it is the first
thing in the sentence. **The tally itself is unchanged** — `withheld` was always populated,
`has_caveat` has always counted it and the SQL writer's `-- NOTE: binary column … exported as
NULL` has always read it; it is only the sentence that moved.
**The decision is a value and the sentence is its English.** `export_said` answers an
`ExportNote` — `rows`, the destination's `name` and the three column lists — and
`ExportNote::caveats()` yields each non-empty loss as a `(Caveat, &[String])` in the order it
is said, `Withheld`, `Blanked`, `Cut`. Its `Display` is the old sentence, ` — ` before the
first caveat and `; ` between, and `export_note` is now only `export_said(…).map(to_string)`,
kept for core's tests and the CLI's dump report, which hands it to `dump::done_note`. The
interface words the value itself (`i18n::export_note`, under *The string catalogue*), so the
rules about *what* is said stay here, tested, and only the words move.
`export_failure_note(message, partial)` is the same rule on the other side, and what it has to
say changed with `part_path`: the destination is no longer opened until the export succeeds, so
the sentence is not "your file is a fragment" but `— <name> was not changed; the rows that were
written are in <name>.part` — the reassurance and where the partial went, in one line — while a
`partial` of `None`, an export refused before the write started, still says nothing extra.
`export_cancel_note(name, partial)` is the same two facts in the cancel arm's voice, a note
rather than an error because stopping was the user's own doing — and it takes a `bool` for the
reason `export_failure_note` takes an `Option`: a buffered format's sibling holds nothing, so
pointing at it directs the user to an empty file. Both are pinned across every format by
`neither_note_points_at_an_empty_sibling`, which asserts the `.part` sentence appears exactly
where `writes_incrementally` says and that the *destination survived* half is said either way.
Neither was given a typed value: their inputs — a message, a name, a flag — are already plain,
so the interface words the same arguments (`i18n::export_failure_note`/`export_cancel_note`)
and these two stay the English.
**`part_file(path)` is `part_path` over a whole path** — the sibling with the suffix on its file
name, a path with no file name coming back unchanged for the create to refuse — lifted from
`dump::part_of`, then the app's, so that `schemaic export`, a crate with no access to the app,
builds its file under the same spelling the app does; `part_of` (now `cli/dump.rs`'s) delegates
to it
(`a_file_is_built_beside_itself_under_part`). `ExportFormat::from_name` and `for_path` are the
other half of that command's needs: a format named by any of its `extensions()` or its
`label()`, case-insensitively and with a leading dot ignored — so `excel` and `htm` both resolve
— and the one an output path's extension names, `None` for either when nothing matches
(`a_format_is_named_by_its_extension_or_label`).
**The schema tree's folder export has three sentences of its own beside those**, and they are
here rather than in the modal's callback for the reason `absorb` is here: a report with arms
(nothing finished, some finished, a table ticked and never found) is a decision, and one written
inside a `create_ext_action` is a decision the suite cannot reach. `files_note(files, tally,
folder, missing, replaced)` **delegates the caveat half to `export_note`** rather than restating
it — a
folder loses exactly what a single file does, and the wording of a loss belongs in one place, so
it inherits that function's rules about a withheld binary column along with everything it says —
and asks it with `streaming: true`, since a folder has no other way to say how much arrived and
`export_note` is otherwise silent on a clean write. It then **terminates that sentence itself**:
`export_note` does not end its own, its single-file caller being the last clause of one, and here
it is not — `missing_clause` follows, and the two ran together into *"…Exported 115k rows to
sakila-csv 1 ticked table not found and was no file: ghost."* What it adds is the count, a
folder's completeness not being visible at a glance, and the `missing` tables named last for the
reason `DumpPlan::missing` is named at all. **`replaced` is the third clause and all three arms
carry it**, through the shared `replaced_clause`: `select_directories()` has no overwrite prompt,
so the guard a single-file export gets free from the save dialog's "replace?" had no equivalent
here and nothing named the files afterwards either — they were replaced in silence. It names them
rather than counting them, the count answering "did I lose anything" and only the names answering
"what", and a stopped or failed export is *more* likely to be inspected than a finished one,
which is why it is not the happy arm's alone. `files_cancel_note(files, folder, missing,
replaced)` parts company with
`export_cancel_note` on purpose: a stopped single-file export has nothing but a fragment to talk
about, while this one leaves **whole, published files** behind — each renamed into place only
once its table was complete — so it leads with what the user kept and mentions the unwritten
table second, or not at all when no file finished. `files_failure_note(message, files, folder,
missing, replaced)`
follows `export_failure_note`'s `None` rule through `files == 0`: an export refused before the
first file lands must not send the user to a folder nothing was written to. The arms are
decided and tested here, and the three stay their English: the modal words the same arguments
itself (`dump_view.rs`'s `files_*_note`), held equal to these by
`english_says_every_folder_outcome_as_core_does`.
**`export_file_names(tables, format)` is what names those files**, and what it adds over
`suggested_filename` — whose sanitizing rule it reuses rather than repeating — is the part a
single file never needed: **uniqueness**. Two distinct tables can sanitize to one name (`a:b` and
`a*b` both become `a_b`), and Windows and macOS fold case, so `Orders` and `orders` are one file:
a folder export that trusted the names would write the second table over the first and still
report both. A later duplicate gets `_2`, `_3` before the extension, and the suffix is checked
against the set too, so a real `a_b_2` standing beside two tables that both want `a_b` is not
clobbered by the deduplicator's own choice
(`export_file_names_suffix_does_not_collide_with_a_real_table`). **Every base name is reserved
before any is handed out**, which is what makes that independent of the order the tables arrive
in: checked against the names issued *so far*, it worked only while the real `a_b_2` came first —
and the picker sorts, where `*` (42) and `:` (58) both precede `_` (95), so it never did. `a*b`,
`a:b`, `a_b_2` put `a:b`'s rows in `a_b_2.csv`, which is the file someone would open expecting
`a_b_2`'s. Names come back in the selection's order, one per table, so the caller can zip the two.
**`unique_column_keys` is the same rule for a JSON export's object keys**, and it had exactly
the bug the paragraph above is about: a bare occurrence counter turned
`SELECT 1 AS a, 2 AS a, 3 AS a_2` into the keys `["a", "a_2", "a_2"]`, and since every key is
written and every JSON parser keeps the last duplicate, the **middle** column's value was gone
on the way back in — including through this app's own JSON import, which reads the format. So
every bare name is reserved before any key is handed out, which makes the answer independent of
the order the columns arrive in, and only a *generated* suffix steps around the reservation: a
column always keeps its own name.
**`export_jsonl`/`export_jsonl_to` are here rather than in the CLI that asked for them**, because
the row is the thing being shared and not the envelope: `RowObject`, `unique_column_keys` and the
withheld-binary rule are the same three, so a duplicate column name gets the same `_2` suffix and
a blob placeholder is `null` whether a row left through the export menu or down a pipe. Only the
envelope differs — no brackets, no commas, compact rather than pretty, a `\n` after every row, so
a consumer can parse the first row before the last has arrived — and an empty result is the empty
string rather than `[]`, there being no envelope to emit. It has **no `ExportFormat` variant** on
purpose: this is `schemaic query --format=jsonl`, not a format offered in the Download menu.
**`export_csv_plain` is the CLI's CSV, and it differs from the file export in one place: no
formula guard.** `csv_field_plain` is `csv_field`'s RFC 4180 quoting without the `'` prefix,
which is right for a file a person opens in a spreadsheet and wrong for a program reading a
pipe, where it rewrote `+15551234` as `'+15551234` — a phone number with a stray apostrophe in
every tool that loads it, disagreeing with the JSON of the same cell and said nowhere
(`plain_csv_writes_a_leading_plus_as_the_value`, which asserts the guarded export still
prefixes it). The two share one private emitter, `csv_chunks_with(w, src, field_fn)`, which
`export_csv_chunks` calls with `csv_field`, so the header rule and the withheld-binary rule
cannot drift between them — only how a field is written is a parameter. **`csv_field_plain`
writes a non-null empty string as `""`**, since the emitter writes nothing for a NULL — the
`COPY … CSV` convention, so a program reading the pipe can tell the two apart; the guarded
`csv_field` still writes both as an empty field. Like `export_jsonl` it has no `ExportFormat`
variant. **`export_markdown_null_as(rs, order, null, header)` is the same split
for Markdown**: the CLI's `table` passes `"NULL"`, the GUI's `export_markdown` keeps the empty
cell, and both go through the private `markdown_chunks_with(w, src, null, header)`, which
`export_markdown_chunks` calls with `""` — so only the NULL cell's spelling is a parameter.
**Both CLI emitters take a `header: bool`, and both private emitters with them**, for
`--no-header`; `export_csv_chunks` and `export_markdown_chunks` pass `true`, so the GUI's
exports cannot lose a header through it. Without one, the CSV has no name row — so an empty
result is `""` rather than the name row alone — and the Markdown has neither the name row nor
the `---` row under it, which leaves it no longer a Markdown table, and it is not meant to be
one: it is rows for `while read` and `cut`, which take a header line for a record
(`plain_csv_without_a_header_is_the_rows_alone`,
`a_markdown_table_without_a_header_is_the_rows_alone`).
**`export_vertical(rs, order, null)` is the `mysql` client's `\G`**, for a row too wide for a
table to be readable: a numbered `*** N. row ***` banner per row, then one `name: value` line
per column with the names right-aligned so the values start in one column. It is written for a
person, so a blob keeps its `<n bytes>` placeholder as the CLI's table does — nothing reads it
back. A multi-line value continues under its own first line (width + 2 spaces) rather than at
the margin, where a continuation would read as a field of its own, and a trailing `\r` goes with
the line ending it belongs to (`export_vertical_indents_a_multi_line_value_under_itself`).
**Every other control character is shown, not written** (`visible_controls`: C0 and DEL as
their control pictures, `␍` `␛`, C1 as a `\x9b` escape), and so it is in the table's `md_cell`:
both go to a terminal, and a value is whatever anyone could write into a table — a mid-line CR
printed a spoofed `is_admin: 1` over the field it sat in, and an ESC sequence can hide text or
write the clipboard (`a_reader_format_shows_control_characters_rather_than_obeying_them`).
A tab stays. `md_cell` is the GUI's Markdown copy too, where a raw ESC was never wanted either.
Alignment counts `char`s, not bytes — `é` is two bytes and one column — and carries no
unicode-width table, so a CJK name is off by its double width; that is the trade, taken for the
names a schema actually has (`export_vertical_aligns_names_by_character`). No rows is `""`, not
a banner over nothing. Like `export_jsonl` it has no `ExportFormat` variant — it is
`schemaic --format vertical`, not a Download-menu entry — so
`every_format_is_covered_by_a_parity_test` does not see it. **`withheld_columns(rs, order)` is `ExportTally::withheld` for a
caller that renders to a string** and so never sees a tally: the names of the columns a JSON or
CSV rendering writes as `null` / an empty field, off the same `dropped_binary_columns` the
emitters use, so the caveat cannot name a different set than the file actually lost
(`withheld_columns_names_the_blob_and_nothing_else`). Its callers are the CLI's
`format::withheld_warning` and the grid's clipboard renderer `render_order` — Copy ▸ JSON/CSV
and the gutter's *Copy as* of those — which puts the caveat on `commit_note` through
`export_said(…, "the clipboard", false)` (the destination `export-dest-clipboard` in the
interface's language); the clipboard used to write `null` or an empty field
for a withheld blob and say nothing, and a JSON `null` reads as a NULL.
**`export_column_in_list(rs, order, ci, dialect)` and `export_values(rs, order, dialect)` are
two clipboard shapes with no `ExportFormat` variant**, like `export_jsonl`: the enum is matched
exhaustively by the Download and streaming paths, and these are copies, never files. The first
is one column as the list an `IN` takes — `(2, 1)` — each distinct value once, in `order`, as
`script_literal` writes it for the column's type, so a SQL Server `datetime` keeps its day under
a day-first login, a string is `N'…'` there and MySQL's backslash doubles; *distinct* is judged
on the rendered literal. **A NULL is left out, not written**: `x IN (NULL)` matches nothing, and
a NULL in a `NOT IN` list makes it match nothing at all. A withheld binary cell is left out too.
With nothing left the answer is the empty string, because `IN ()` is a syntax error, and the
caller then leaves the clipboard alone (`a_column_copies_as_an_in_list`). The second is the
rows as `VALUES` tuples — what *Copy as SQL INSERT* writes, without the statement around it —
one per line and comma-separated, a NULL and a withheld binary cell both `NULL`
(`rows_copy_as_values_tuples`). **Both return a `SqlCopy` — the text and the binary columns it
withheld — rather than a bare string**, because a fragment pasted after a `VALUES` or into an
`IN (…)` cannot carry the `-- NOTE:` line the INSERT export writes, and every engine renders
every binary value as the placeholder: a `BINARY(16)` key's tuples came out all `NULL` and its
IN list came out empty, leaving the clipboard as it was, both without a word. `SqlCopy::note`
is the sentence for the bar, worded by what happened to the cells — written as NULL, left out,
or *Nothing copied* when nothing was left — and the grid's `sql_copy_text` puts it on
`commit_note` even when the text is empty
(`a_binary_columns_in_list_withholds_its_values_and_names_the_column`,
`values_tuples_name_the_binary_columns_they_write_as_null`). The decision is `SqlCopy::said`,
which answers a `SqlCopyNote` (`AsNull`, `NothingCopied`, `LeftOut`) with the columns; `note`
is `said().map(sentence)`, `SqlCopyNote::sentence` the English, and the grid words the value
itself (`sql_copy_note`). **Both take `script_literal`, as *Copy as SQL INSERT* does**:
a copy lands wherever it is pasted, a `sqlcmd` script included, so a SQL Server value holding
`$(` is cut there as it is in the `INSERT`
(`the_in_list_and_values_copies_keep_sqlcmd_out_of_a_value`). **And a tuple cannot leave a column
out**: a pending ＋Row's unset cell reaches it from `exported_rows_at` as `NULL` — the rendering
`insert_blocks_at` exists to avoid — so such a row, pasted after an `INSERT … VALUES`, writes
NULL over the column's default. Neither puts a withheld-column caveat on `commit_note`; nor do
the column's CSV and JSON copies.
**`insert_shape(rs, tab)` is where an `INSERT` export lands, and it reads the result rather than
the tab.** A grid tab's `source` is the table it was *opened* on and is kept however its text is
edited, so a tab opened on `users` and rerun as `SELECT … FROM customers` exported
`INSERT INTO users` — customer rows, landing silently wherever the columns lined up. The
columns' `ColumnOrigin`s are what the server said the rows are: every column from one table
names that table, as a `TableRef` (an owned `(database, namespace, table)`), and renames each
column to `origin.column`, so `id AS user_id` inserts into `id` rather than into a column the
table has not got; no column with an origin at all falls back to `tab`, which then has nothing
to contradict it; anything else — a join, a computed column beside real ones — is `None`, the
`table` placeholder, for the user to decide rather than a guess. It reshapes the input and
leaves `export_inserts_chunks` alone, so a dump, which names its own tables, is untouched. The
grid's clipboard (`render_order`) and its `.sql` file (`save_export`) both call it, so the two
cannot name different targets; the four `insert_shape_*` tests pin the three arms and the
rename. **`result_table(rs, tab)` is that target without the renamed copy, and everything else
that acts on "this result's table" asks it rather than the tab's `source`** — the RESULTS title
bar's Properties and Live Monitor, and the chat attachment's label, each of which described or
watched `users` under a grid of `SELECT * FROM orders` while they read `source`. `insert_shape`
delegates its target to it, so the export and those buttons cannot disagree about which table a
result is. **`column_table(rs, ci, tab)` is the per-column answer** — the column's own origin,
else `tab` when no column has one, else `None` — because a question about one column (the grid's
AI Summary) has an answer even in a join, where the result as a whole has none
(`result_table_answers_what_insert_shape_targets`, `column_table_is_the_columns_own_origin`).
**The origin is what the server says, and for a view it is not always the view**: measured on
MariaDB 10.11, a `MERGE` view's columns name the view but an `ALGORITHM=TEMPTABLE` one's name its
base table, while PostgreSQL 16 names the view. A tab opened on a temptable view therefore
resolves to the table underneath — where the rows are, and what an `INSERT` export targets —
and its Properties and Live Monitor act on that table, not on the view.
**`export_insert_blocks(w, blocks, dialect, chunk_rows, watch)` is the one sanctioned
way round `RowChunk`'s rule that every chunk of an export carries the same columns** — a rule
that exists because the header is written from the first chunk and never revisited. A file of
`INSERT`s has no header, and a pending ＋Row's `INSERT` must name only the columns it set (see
`core::edit`'s `insert_blocks_all`: an unset cell written `NULL` overrides the server's default,
and `DEFAULT` in `VALUES` is refused by SQLite), so the grid's `.sql` file cannot be one stream.
Each `InsertBlock { rs, order, target }` goes through `export_inserts_chunks` in its own
`SliceChunks` of `chunk_rows`, naming its own column list and target, and the tallies fold
through `ExportTally::absorb`. `watch` is `SliceChunks::watching`'s hook over the **whole** file
— each block's source adds the rows of the blocks before it, so the progress count runs across
block edges rather than restarting — and a `false` is still an `io::Error`, for the reason above
(`a_saved_insert_file_omits_a_pending_rows_unset_cells_too`, which also checks the running total,
and `a_stop_between_insert_blocks_is_an_error`, both in `edit.rs`). It is for `INSERT` blocks
only: the other five formats have a header and still need one column set.
And
`all_rows_label(size, sorted, manual_tx, staged)` is the Download menu's `All rows` entry, four
disclosures made at the point of choice in place of an untested `match` in the view (*Data grid*).
`staged` is the newest and reads *"without your staged edits"*: the **fetched** scope now renders
the rows in hand through `edit::GridCells` exactly as Ctrl+C does, so it carries the green edits
and the pending rows, while this scope asks the server again and the server has never been told.
It became worth saying at precisely the moment the other scope stopped diverging — before that it
was covered by "an export is of what was fetched".
