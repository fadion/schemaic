# `core/import.rs`

`import.rs` — the inverse of `export.rs`: CSV/TSV, JSON (array *or* NDJSON) and Excel
`.xlsx` → table. Format
inference, delimiter/header `sniff` (by *consistency*, quote-aware), `auto_map` (name-match with
a header, positional without), per-column `coerce` (only the families a wrong answer would
definitely break — int/uint/float/exact/bool; everything else passes through, the **server stays
the type authority**), and a two-pass contract:
What a write may touch is read off the model, never guessed from the type text:
`insert_columns` — the single authority `validate`/`row_iter`/`build_insert` all funnel through —
drops any `ColumnInfo::is_server_assigned` column, so a file Schemaic exported can be imported
back (a generated column matched by name used to pass validation and then fail the whole
transaction); and `missing_required` warns on `!nullable && !auto_increment && default.is_none()
&& generated.is_none()` rather than on "integer primary key", which was silent on a natural
`INT` key and noisy about every defaulted column. `validate` reads the whole file and returns
*every* `Issue` with its line before anything is written, then `row_iter` streams coerced rows
for the load. **A row the stream cannot hand over is a `RowError`, not a sentence** —
`RowIter`'s item is `Result<Vec<Value>, RowError>`, and so is `db::RowSource`'s, which a copy's
read feeds too: `Issue` is `RowCtx::row`'s backstop for a file that changed underneath the
import since `validate` passed it, carrying only the record's first problem since the load is
all-or-nothing and the list has already been shown; `Read` is the CSV parser's own words, or a
copy's read's error; `Copy` is a `copy_table::CopyStop`. It was an `Err(String)` the db layer
carried as text, so why a load stopped on its own rows could only ever be said in English.
`message()` is that string byte for byte — *line N, column C: `IssueKind::message` (text)*, the
text `empty` for a blank field, and `Read`'s and `Copy`'s words as they are
(`a_row_error_says_what_it_used_to`; `row_iter_reports_the_first_bad_row_and_says_where` asserts
the exact English).
**A shape check still has to refuse a number with no SQL literal.** `ColKind::Exact` parses for
shape only — the text is what gets inserted, so nothing is rounded on the way through, which is
the whole reason it is not `Float` — and `NaN`, `inf`, `Infinity` and `1e400` all parse as
`f64`, so they passed the check and went into the `INSERT` verbatim with `validate` reporting
zero issues, from the pass whose entire purpose is one list of everything wrong before anything
is written. One file, two outcomes: MariaDB 10.11.14 answers `ERROR 1366: Incorrect decimal
value: 'NaN'` and rolls back whichever batch it landed in, while PostgreSQL 16.15 **accepts**
`'NaN'::numeric`, so the file imports clean and leaves a NaN in an exact column, where it
changes every `SUM`, `AVG` and comparison over it from then on. The arm now carries `is_finite`,
the term the `Float` arm beside it had. **The `Float` arm admits a non-finite where the engine
keeps one** (`keeps_non_finite`, exhaustive): PostgreSQL's `float8` stores NaN and ±Infinity,
SQLite's `REAL` ±infinity but no NaN — `non_finite_literal` writes `NULL` for one, right for a
copy that read a `NULL` and a dropped value for a file that says `NaN` — and MySQL and SQL
Server neither. Admitted only when *spelled* as one (no digit in the text), since `1e400` is an
overflowing numeral PostgreSQL refuses as out of range, not an infinity; what is admitted is
written through `import_literal` as the engine's own literal. It refused all of them on every
engine, while a copy already wrote them (`a_postgres_float_takes_nan_and_infinity`,
`an_overflowing_numeral_is_still_refused_on_postgres`,
`non_finite_floats_import_only_where_the_engine_keeps_them`).
**The problem list has two caps, and the line under it names both** (`issue_tail(shown, total,
capped)`): `validate` stops *collecting* at `max_issues` (200) and the modal stops *rendering*
at `ISSUE_LINES` (50, in `ui/import_view.rs`). The disclosure was wired to the first only, so a
file with 57 problems rendered 20 lines under a heading reading *57 problems in the file* and
said nothing about the other 37 — the user fixed twenty, re-imported, was told 37, and repeated,
at one whole file pass each. Both caps can be in force at once and the sentence has to say so:
at exactly `max_issues` the old wording put *…and more.* over 20 of 200, which is the same gap
one notch worse. A unit test on the helper alone guards none of this — it was correct in
isolation the moment it was written and the defect was entirely in *which* of the two numbers
the view asked about — so the pin is a source gate in the view
(`the_problem_list_discloses_its_own_cap_and_not_only_cores`). The decision is
`issue_tail_of`, answering an `IssueTail` (`Cut`, `Uncounted`, `CutAndUncounted`) whose
`message` is the English; `issue_tail` is now only that English over it, and the view words the
value itself, so the gate asks for `issue_tail_of(` (under `import_view.rs`).
`build_insert` reuses `export::ident_sql`/`sql_literal`, so quoting can't drift
from the SQL export; it is the statement every engine's load runs, SQL Server's included, which
renders its batches as literals rather than binding them for a parameter ceiling (under
`db::mssql`). It is now a thin wrapper over **`build_insert_as`**, which every engine's writer
reaches through `db::ImportTarget::insert_sql`: that takes a `CellForm` per column (empty for a
file, whose cells are all `Value`) and `overriding`, which puts `overriding_clause` —
`OVERRIDING SYSTEM VALUE`, PostgreSQL's alone — before `VALUES`. The other forms are what
*Copy to* reads a raw-bytes cell as (under `copy_table.rs`): `Hex` is written as `X'…'` on
MySQL and SQLite and as `decode('…', 'hex')` on PostgreSQL — not `'\x…'`, which reads
differently under `standard_conforming_strings` — `Server` through `export::server_literal`, and
`Quoted` as-is. **Each is checked to be its form's shape first** — even-length hex digits,
the dump's own checks, one of the shapes SQLite's `quote()` writes — and a cell that is not
makes the whole call an `Err` (`copy_table::CopyStop::NotItsForm`, naming the column), so the
batch is never written and the load's transaction rolls back: these texts go into the statement as SQL, nothing else may, and a wrong shape written
would be a wrong value rather than a refusal
(`a_carried_cell_is_written_as_its_form_and_checked_first`,
`overriding_puts_the_clause_before_values_where_the_engine_has_one`). **A `Value` cell goes
through `import_literal`**, which is `sql_literal` except for a non-finite float: the export
writes that `NULL`, a stated rule for a file and a silent loss in a table being reproduced, so
here `non_finite_literal` writes the engine's own — `'NaN'::float8`, `'Infinity'::float8`,
`'-Infinity'::float8` on PostgreSQL; `9e999`/`-9e999` on SQLite, and `NULL` for its NaN, which
SQLite stores as `NULL` anyway — while MySQL and SQL Server, which cannot hold one, keep the
`NULL` (`a_non_finite_float_is_written_as_the_engines_own_literal`; SQLite's
`a_copy_keeps_an_infinite_real`). **Two capabilities decide what reaches it, and both are exhaustive `match`es.**
`supports_import` is the table menu's *Import* entry, so an engine without a load has the entry
absent rather than a wizard whose last step is a refusal; it answers yes for all four since
`db::mssql::import_rows` was written. `bool_literal_is_integer` is whether a coerced boolean
goes in as the integer `1`/`0` or the quoted `'true'`/`'false'`, and it was `MySql => integer,
_ => quoted`, written when the default arm meant PostgreSQL — which sorted SQLite onto the
wrong side: its `BOOLEAN` is a declared type with NUMERIC affinity, so `'true'` is kept as TEXT,
and TEXT in a boolean context is 0, so every row imported as true was invisible to `WHERE flag`
and returned by `WHERE NOT flag`. PostgreSQL, which rejects the integer for a real boolean, is
the exception rather than the default; SQL Server's `bit` takes `1`/`0`. With the second a
`match` too, this file has no entry left in `engine_comparison_gate`'s budget. Note JSON columns are the union of every object's keys and the mapping is
built from a *sample* — `trim_to_mapping` drops keys that first appear past it (CSV keeps the
field-count check, where a mismatch means a stray delimiter). An array and JSON Lines read
through one path: `ArrayUnwrap` blanks the wrapping brackets + top-level commas as bytes
stream past, so a sample really stops at its limit instead of deserializing the file first
(a whole-file walk still buffers, for the key union). CSV's NULL tokens match the *trimmed*
field, except the empty one, which is exact — a blank field is data, and `trim` is the setting
that says otherwise. `read_sample` is bounded at `SAMPLE_MAX_BYTES` for the two streamable
formats, because a **record count is not a byte bound**: the CSV reader sets no field-size
limit, so one stray `"` makes the whole
remainder of a file a single unterminated field and a sample "of 200 records" reads to EOF —
from a file the user only meant to look at.
**The JSON reader had to learn the difference between that cap arriving and a broken file.**
`json_records` takes `bounded: Option<u64>` — the cap itself: under `read_sample`'s `Read::take`,
a `serde_json` EOF *inside* a value is the byte cap, so the walk stops with `more = true`, while
a whole-file walk passes `None` and a genuinely truncated file still fails there rather than
importing a prefix in silence. CSV degrades gracefully under truncation — the reader simply yields fewer records —
and JSON does not: the preview reached the user as `Couldn't read the file: EOF while parsing a
string at line 1 column 8388608`, which reads as file corruption, on a file `validate` then
walks end to end without complaint. The trigger is the first `limit` records exceeding the cap,
i.e. an average record over ~42 KiB, which one text column reaches easily — a JSON export of a
hundred support tickets is inside it — and `read_sample`'s own doc had been promising that a
truncated read "can only make the preview *shorter*".
**It carries the cap rather than a flag because "did anything parse" was the wrong question.**
With a *first* record over the cap — one object holding a large text or base64 column —
`objects` is still empty when EOF arrives, so an arm that only asked whether something had
parsed did not fire and the corruption message came back on a perfectly valid file. The reader
knows the fact the deserializer does not, so a `Counting` wrapper carries the byte count out
(`read_sample` builds the `Read::take` and discards the handle, and `StreamDeserializer` never
gives its reader back): with nothing parsed and `seen >= cap`, the file is blamed on the cap in
its own sentence — *its first JSON record is larger than the N MiB the preview reads* — rather
than on corruption. Below the cap with nothing parsed it is still a short or broken file, and
still reported as one.
**And saying so was still a dead end, which is why that case is a variant rather than a
message.** `ImportError::PreviewRecordTooLarge { cap }` is not a read failure: the file is valid
and the whole-file walk reads it end to end — only the preview cannot be built from a prefix.
Spelled as a `Read(String)` it reached the modal through the same channel as "this file is
corrupt", the sample stayed `None`, and Next is gated on the sample, so a valid JSON file whose
first record holds one long text or base64 column could not be imported **at all**.
`read_json_columns` is the answer: it reads that first record alone, unbounded in bytes and
bounded in memory, deserializing every value as `serde::de::IgnoredAny` — which parses and
discards without allocating, so a two-gigabyte record costs its key names — and hands back the
keys with no rows. The mapping step needs a column list and nothing else to be usable. `Sample`
carries `values_withheld` to say that is what happened, a flag rather than an inference from
`rows.is_empty()`, because a file with a header and no records has exactly that shape and the two
deserve different notes. **`probe_sample(open, format, cfg, limit)` is the composition**, and it
lives in core rather than in the probe's closure for the usual reason: neither half was wrong —
`read_sample` correctly refuses to invent a preview and the modal correctly gates Next on having
one — and a decision written into a view closure is a decision no test can reach, which is how it
stayed that way. It reopens the file (the first reader is spent by then, and the failure is not
knowable until it has been) and falls back for that one variant only; a truncated or malformed
file still fails, on the same bytes, through the second reader.
**`Read(String)` now carries only a library's own text** — calamine's, `serde_json`'s, the CSV
reader's, an io error's OS text — and every sentence Schemaic writes itself is
`ImportError::Refused(ReadRefusal)`: `NotObjects`, `Oversize`, `Inflated`, `NoSuchSheet`,
`NoSheets` and `TooWide`, values rather than prose so the modal can say them in the interface's
language. `ReadRefusal::message` is the English, byte for byte the sentences `Read` used to be
built from, and `ImportError`'s `Display` still prefixes *Couldn't read the file:* to it; neither
`ReadRefusal` nor `MemoryWarning` nor `IssueTail` has a `Display` of its own. The interface's
words are `i18n::read_refusal_text` and `import_error_text` (under *The string catalogue*).
**Excel is the third format, and a workbook is a file with several tables in it.**
`infer_format` takes `xlsx`/`xlsm` and deliberately **not** `.xls`/`.xlsb` — different formats
this reader cannot open, where guessing would trade a clear "unsupported file" for a parse error
on a file that was never going to work. `ReadConfig::sheet` picks the worksheet (`None` = the
first, which is what a one-sheet workbook wants), and a name that no longer matches any sheet is
an **error rather than a fall back to the first**: a workbook edited between the preview and the
load would otherwise import a different table than the one the user reviewed, which is the
failure worth being loud about. `open_xlsx` opens the file and `xlsx_records` is the single
reader the preview, the validation pass and the load all walk, as `json_records` is for JSON;
`read_workbook_sample` is the probe's entry point, returning the preview **and** the sheet names
from one parse, because a second call to list the sheets meant opening and inflating the whole
workbook again on every settings change.
**`xlsx_rows` streams the sheet and never materialises it.** It walks
`worksheet_cells_reader` and assembles one row at a time — cells arrive in sheet order and an
empty cell is not emitted at all, so a row is complete when a cell for a later row shows up.
What it replaced was `worksheet_range`, which builds the **dense** bounding rectangle of every
cell present: a legal 5,461-byte workbook holding two cells at opposite corners cost 262 MB —
48,099× the file — and the worst legal sheet 550 GB, which is not an error but
`handle_alloc_error`, i.e. the process. Measured on this project's own 200k × 50 target, the
preview went from **1,085 MB / 2.8 s to 64 MB / 21 ms**. Two properties of that walk are
load-bearing and neither is visible in an `A1`-anchored fixture: rows carry the **worksheet**
row number, the one in Excel's own margin, since a sheet with a title block above its header
starts partway down (enumerating from zero sent the user to look at a blank row), and a
**wholly empty row is not a record**, since a blank spacer row would otherwise insert a row of
NULLs nobody typed — which the streaming walk gets for free rather than by a filter, an empty
row producing no cell to end the previous one. **The row's width comes from the sheet's
*declared* extent** (`sheet_width` over `Dimensions`), capped at `XLSX_MAX_COLS` (16,384) and
refused above it: the rows are streamed, so a sheet's height costs nothing to skip past, and the
row buffer is the one allocation whose size the file controls. A cell is placed relative to the
used range's **left edge** — the same correction `range.start()` used to make — and one *left* of
that settled origin is dropped, since the slots are indexed relative to it and moving it after
the first row would shift every value already placed onto the wrong column.
**A cell to the *right* of the settled width widens that row, and only that row.** It used to be
dropped there too, and the drop had no reporter: `trim_to_mapping`'s doc promised that "a
worksheet's columns are fixed by its used range, so every row is already the same width and a
count mismatch is a real one worth reporting" — true of what arrives, and that was the defect,
because padding and truncating every row to the settled width made `coerce_record`'s
`IssueKind::FieldCount` branch unreachable **by construction** for Excel. A two-column header
over three-column data — a streaming writer with no `<dimension>`, or a stale one — previewed
two columns, said nothing in `missing_required` when the lost column was nullable, and reported
the right number of rows having written two-thirds of the data. CSV refuses the identical file.
Widening per row is what makes the count differ, which is the path and the message a ragged CSV
row already takes; `XLSX_MAX_COLS` still bounds it, the cells being unable to raise the ceiling
after the geometry closed just as they could not raise it while it was open.
`xlsx_size_refusal` is still asked of the file's *stat* before any of it
is read, because it can say so before the file is touched and it names the size — but the
ceiling is now **enforced in `open_xlsx`**, which reads through a `std::io::Read::take` at
`XLSX_MAX_BYTES` (512 MiB) so all three readers inherit it. A guard the *launcher* has to
remember is a guard the two load-path opens did not have; they went straight to `read_to_end`.
`ReadRefusal::Oversize` is the one refusal both raise — `xlsx_size_refusal` answers it bare, the
read inside `ImportError::Refused` — its `file_bytes` the size where the caller knows it and
`None` where a read through the cap learned only that it was over.
**`XLSX_MAX_BYTES` bounds the wrong quantity, though, which is why there is a second ceiling.**
Both of its enforcement points measure *compressed* bytes — the size on disk and the bytes read
through the `take` — and `xlsx_memory_warning` is derived from the same figure, while what is
actually held is the **inflated** archive: calamine materialises the shared-strings table and
the sheet XML with no ceiling of its own. A 4 MiB workbook whose `xl/sharedStrings.xml` is a few
hundred megabytes of one repeated byte passed every compressed-size check, was given a "this is
a small workbook" disclosure, and took the process out — at *preview* time, on a probe the
import modal re-fires on every settings change, on a file the user only meant to glance at. So
`open_xlsx` now asks `inflated_refusal` between the read and the inflate, against
`XLSX_MAX_INFLATED_BYTES` (2 GiB), summing the entry sizes from the ZIP **central directory**
(`by_index_raw` decompresses nothing) — which cost `schemaic-core` a promotion of `zip` from a
dev-dependency to a dependency. It is the same shape as the declared sheet width: a figure the
workbook supplies, trusted to describe what the workbook costs. Two limits are deliberate and
written into the constant's own doc. An archive that *understates* its entries is not caught —
the `zip` reader does not stop a decompressor at the declared size, so closing that would mean
inflating the whole archive once ourselves and throwing it away, doubling the cost of every
preview of every legitimate workbook — and anything that is not a readable archive answers
`None`, because calamine is about to open the same bytes and a size complaint would replace its
real diagnosis.
**An entry the scan cannot size is skipped, not an exit** — a third limit the first spelling got
wrong by extending the second one's `?` to a case it does not cover. The walk was
`by_index_raw(i).ok()?`, and `by_index_raw` is not a directory read: it seeks to the entry's
declared `local_header_offset` and checks the local-file-header magic, which `ZipArchive::new`
never does. So a four-byte tamper to the central-directory record of one entry no workbook reader
opens abandoned the whole scan and answered `None`, which `open_xlsx_within` reads as "no size
problem" — while the archive still opened, listed and read perfectly, calamine reaching every
part `by_name`. Only the ceiling was skipped. Skipping the single entry instead is safe in the
direction that matters: the sum is a lower bound and the message says *unpacks to at least*.
**And the refusal no longer promises a memory ceiling.** The sum measures *declared uncompressed
XML bytes*; what calamine then holds is the parsed shared-strings `Vec<String>`, filled eagerly
before any sheet is touched — measured at **1.48×** the counted quantity against calamine 0.36.1,
a 64 MiB `xl/sharedStrings.xml` of minimal `<si><t>a</t></si>` entries costing 99 MB live. That
is a *floor*: the counting allocator charges `Layout::size()`, so a one-character `String` counts
one byte where a real allocator's minimum chunk is 16–32, and `<si><t/></si>` reaches ~1.85× with
no rounding at all. "The most this can hold in memory" was a promise in the wrong unit,
understated by about half, and the sentence now says what the bound is actually about.
The ceiling is a parameter of `open_xlsx_within`/`inflated_refusal` so the
decision is testable without a two-gigabyte fixture; what the test pins is that opening a
workbook consults the bound at all, which is the seam rather than the predicate.
`cell_text` is one cell → `Field`, and its conversions are chosen
so a value that came *out* of a database survives the trip back in: an empty cell → null; a
number in `f64`'s shortest round-trip form, so `7.0` is `7` (an `INT` column rejects the latter,
and Excel has no integer type to tell them apart); a date or time as ISO 8601 rather than the
serial number Excel actually stores, since a `DATE` column otherwise receives `45292`; a bool as
**`1`/`0`, not `true`/`false`** — both spellings are in `ColKind::Bool`'s accepted list, so this
is not a trade between engines but the one that also works where the column is *not* classified
`Bool`: MySQL and MariaDB report a `BOOLEAN` as `tinyint(1)`, so `classify` gives `ColKind::Int`
and `coerce("true", Int)` is `NotAnInteger`, and every row of an ordinary spreadsheet's
TRUE/FALSE column was refused while the same workbook imported fine on PostgreSQL and SQLite;
a **duration** through `duration_hms` as
`H:MM:SS` with hours deliberately unwrapped past 24, since a `[h]:mm:ss` cell holds a fraction of
a day and emitting it as decimal hours put a timesheet's 8h30m into a MySQL `TIME` column as
`8.500000` — eight and a half *seconds*, wrong by 3600× and silent, because the value coerces
perfectly well; and a formula error in **Excel's own spelling** (`#REF!`, `#DIV/0!`, from
calamine's `Display` rather than its `Debug`, which gives a `Div0` that appears nowhere in the
application the user is looking at) as its own text rather than a null, so a cell the sheet
itself could not evaluate surfaces as an `Issue` naming the row. **That `Issue` is
`IssueKind::CellError`, raised ahead of the type dispatch and not by it**: a cell the sheet
could not evaluate is wrong for *every* column type, and `ColKind::Other` — text, date, JSON,
blob, enum — has no dispatch to catch it with, so a `#N/A` went into a `VARCHAR` column as the
literal text. **Which cells those are is carried from the reader, not re-derived from the
text.** `Record` — the private type the traversal hands over (`for_each_record`,
`xlsx_records`, `xlsx_rows`, `RowSourceIter::Buffered`) — pairs the `fields` with
`sheet_errors`, the indices of the cells calamine reported as `Data::Error`, and
`coerce_record` takes that slice where it used to take the `ImportFormat`. It was a table of
the ten spellings gated on `ImportFormat::Xlsx`, and that cannot work: `cell_text` renders a
formula error in Excel's own words, so a broken cell and a *string* cell whose whole content is
`#N/A` are byte-identical by the time the coercer sees them — and `#N/A` is pandas' first
default `na_values` entry and what people type by hand for "not applicable", so an ordinary
workbook with one in a `status` column was refused **whole** (the first issue makes `row_iter`
return `Err` and rolls the transaction back). No format gate is needed any more, because only a
worksheet ever fills the list, and nothing trims the indices alongside a trimmed field list
(`trim_to_mapping`) because an index past the end matches no field that is still there. The
worksheet header goes through
`text::strip_bom` for the same reason the CSV path does: a BOM that survived a round trip
through a BOM'd CSV lands *inside* the first header name and silently breaks name-matching on
the very first column. **`ImportFormat::has_own_nulls` is the capability the null rules ask** — true for JSON
and Excel, false for CSV — because a worksheet's empty cell is a real null and `NullRule`'s token
list must not apply to it; it replaced two separate `match format` expressions, in `validate` and
in `row_iter`, that had to agree with each other.
**`preview_field` is the third asker, and it exists because the mapping step could not render a
CSV NULL at all.** The view decided nullness from the `Field` alone, and a `Field` is `None` only
for a *format-level* null — `read_csv_sample` builds every field as `Some(..)` and never reads
`cfg.nulls`, because for CSV whether empty text means NULL is `NullRule`'s call and that call is
made at coercion, which the preview does not run. So `id,note` / `1,NULL` / `2,` with the defaults
previewed row 1's `note` as the plain text `NULL` and row 2's as an empty cell, both in the
ordinary colour and both indistinguishable from the string values `'NULL'` and `''` — while the
import stored NULL in both; and turning *Empty field is NULL* off left the preview byte-identical
while row 2 now stored `''`, so the control whose entire reason to exist is that distinction
changed nothing on screen, at any setting, for any value. The one arm that did draw the faint
italic was reachable only for a record shorter than the header, which for CSV is an
`IssueKind::FieldCount` rather than a null. `preview_field(field, nulls, format) -> PreviewCell`
is that decision, in core beside the rule it asks and asking `has_own_nulls` exactly as `validate`
and `row_iter` do, so a JSON empty string is not swept up with it. Its tests run from
`read_sample` rather than from a hand-built `Field`: a hand-built `None` would have hidden the
defect entirely, and `NullRule::matches` passed its own tests from the day it was written — the
bug was the composition. `RowSourceIter::Json` is now
`RowSourceIter::Buffered` and carries both: the same buffering for two different causes, JSON not
knowing its columns before EOF and an `.xlsx` not being readable as a prefix at all.
`trim_to_mapping` puts Excel on **CSV's** side rather than JSON's — the header fixes the
columns, so a row that does not match that count is a mismatch to report rather than keys to
drop, which is now true of the reader as well as of this rule (see `xlsx_rows` above). **`read_sample` bypasses `SAMPLE_MAX_BYTES` for a workbook** for that same prefix reason:
a ZIP's central directory is at the end of the file, so a truncated read does not open at all and
the bound would turn "preview a 9 MB workbook" into "this file is corrupt". What is left to
disclose is the memory that costs: `xlsx_load_estimate` is `XLSX_MEMORY_FACTOR` × the file size.
That factor was a **guess of 25**, on the reasoning that an `.xlsx` is a ZIP of XML that deflates
well — right about the file and wrong about what was actually held, which was the dense `Range`
the old reader built. Measured against `read_sample` on the same counting allocator
`JSON_MEMORY_FACTOR` came off, it fell to 4: 2.0× on the 200k × 50 target, 1.5× on 120k × 50 of
distinct 60-char strings.
**But the warning stands in front of the *load*, and `read_sample` is the preview.** The two
paths do not cost the same thing — `row_iter`'s `Xlsx` arm materialises every row of the sheet
into a `Vec<(Record, u64)>` before it hands out the first, so its cost is linear in *cells* where
the preview's is linear in rows read, and quoting the preview's figure for the load was an
under-estimate on both. Re-measured with a counting global allocator over
`schemaic_core::import` itself, `--release`: a 23.3 MB workbook of 100k × 50 peaks at
**309.5 MB on `row_iter` (13.27×)** against 128.0 MB on `read_sample` (5.49×), and the ratio
holds at 20k × 50 (13.53×) because the buffer is linear in cells. The factor is **14**, the
larger measurement rounded up, which is the over-estimate a warning needs.
**`XLSX_WARN_BYTES` is derived from it rather than set beside it.** `XLSX_DISCLOSURE_BUDGET`
(1 GiB) is the estimate the disclosure must already have preceded — "where an estimate stops
being something a machine absorbs without noticing" as a figure instead of prose — and the
threshold is that budget divided by the factor, so raising either one alone fails
`a_large_workbook_is_disclosed_before_it_costs` instead of silently moving the sentence past the
cost. Sharing `JSON_WARN_BYTES` is what let it drift: at 14× a 200 MiB workbook costs about
2.8 GB, and nothing had been said yet. **The structural half is untouched and still owed** — the
buffer exists only because `RowIter` is an `Iterator` rather than an internal-iteration walk, as
the CSV arm is, and `xlsx_records` already streams through a callback.
The modal asks **`memory_warning(format, bytes)`** and never `json_memory_warning`
directly: one call site, so a format that needs a warning cannot be given one nothing asks for.
All three answer a `MemoryWarning` — `Json` or `Xlsx`, each the file size and the estimate —
rather than the sentence, whose English is `message` and whose interface words are the view's.
**The reader is `calamine`, and an imported `.xlsx` is the first untrusted XML this app has ever
parsed** — a ZIP of it, from wherever the user got the file. `calamine` depends on quick-xml
**0.41**, the version that fixes `RUSTSEC-2026-0194`/`0195`, and that is now the only quick-xml
in the tree: `cargo tree -i quick-xml` returns exactly one, 0.41.0, under calamine — and under
`schemaic-core` itself, which now asks the same version directly to read SQL Server's plan
documents (`core::plan`), which arrive from the server rather than with the build. There were
two for a while, the second a vulnerable 0.39 whose **only** consumer was `wayland-scanner`, a
Linux-only build-time proc macro over Wayland's vendored protocol XML — not usvg/resvg, which
parses SVG with roxmltree and never depended on either version, an attribution `deny.toml` had
to correct once already and which the bump has now settled by demonstration: resvg and usvg sat
still at 0.43 while 0.39 disappeared. That XML shipped with the build rather than arriving from
a user, so `deny.toml` ignored the two advisories on that reachability argument, on the stated
condition that the ignores go once the duplicate did.
`wayland-scanner` 0.31.11 moved to quick-xml 0.41 and took the last 0.39 with it, so they are
gone (they had begun emitting `advisory-not-detected`) and `RUSTSEC-2023-0071` — `rsa`'s Marvin
timing sidechannel, via russh — is the only ignore left. **The argument outlives the
duplicate**: if a second quick-xml ever comes back it must not be deduplicated *downwards*,
which would put the untrusted parser back on the vulnerable code, and `deny.toml` carries that
warning in prose where the ignores used to be.
`target_verdict` over `DbNodeView` is the modal's other half: whether a schema change means the
table it is open on has *gone*. `has_table` is an
`Option<bool>` because "I looked and it wasn't there" and "I haven't looked" are the same
`false` to a bool and a refresh empties what there is to look at, and the caller passes
`same_connection` because `db_nodes` holds only the **active** connection's databases — nothing
compared them, so switching connection discarded a hand-built mapping. Pure + unit-tested.
