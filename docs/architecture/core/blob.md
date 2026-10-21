# `core/blob.rs`

`blob.rs` — **one raw-bytes cell, fetched on demand**: the pure half of the grid's binary-cell
panel. Nothing here writes a blob back — the file goes to the grid's dirty map as a
`CellEdit::Bytes` (see `blob_view.rs`), never through here — though the panel it serves is a
write surface now, and the two questions a write has to settle before staging anything *are*
here: `column_byte_cap` and `LOAD_CAP`, below.
Because the bytes never reach the grid (see `model.rs` above),
opening a `<n bytes>` cell is a second query, and what aims it is
`blob_source(model, rs, di, ci) -> Option<BlobRef>`: the column's own wire provenance —
`database`/`schema`/`table` and the **real** column name, not the query's alias — plus
`edit::row_key`'s WHERE identity for the row it sits in. **It is aimed by the same key an
`UPDATE` of that row would carry**, so a fetch can only be offered where a write could have
been: no key, no fetch, and never a `SELECT … LIMIT 1` over an ambiguous row, which would show
bytes from a row the user did not click (`a_table_with_no_usable_key_has_no_source`).
**Two signals, and the second is per *value*** — the pairing `export::dropped_binary_columns`
already makes, sharper here. A column answers "is this bytes at all"; only the cell answers "did
*this* value arrive as bytes", and on SQLite the two genuinely differ, since a declared `BLOB`
is an affinity and one row of that column can hold text. Such a row would go through SQLite's
`length()`/`substr`, which count **characters** on text — a size reported in the wrong unit and
a cap that is not the cap. So the cell's own text must be one `binary_display` wrote, the only
evidence a `ResultSet` keeps that a value's bytes were dropped rather than rendered
(`a_text_value_in_a_binary_column_is_not_fetchable`). A NULL cell fails the same check, for the
different and simpler reason that there are no bytes.
**It cannot ask `EditModel::table_index`, and that composition is the load-bearing part.** The
rule that binary columns are never editable used to keep the column out of `col_table`, so the
write model answered `None` for the very column being fetched;
`EditModel::table_for_origin` finds the keyed base table by the provenance triple instead
(`a_same_named_table_in_another_schema_is_not_the_source`). C2 has since narrowed to *text*, so
the column is in the map like any other and this lookup is merely the one that cannot go stale
on that question again — which is what
`a_binary_column_resolves_a_source_and_a_bytes_only_write` pins from both sides in one test: the
column takes a write but not a text one (`editable` yes, `text_editable` no), and the fetch
still finds its table. Binary columns remain barred as WHERE keys. The binary signal read
here is `edit::holds_bytes` — the one definition of the pair `Column::is_binary` **or**
`ResultSet::binary_columns`, which this and `export::dropped_binary_columns` used to spell
themselves — so a SQLite blob in a column declared `TEXT` is fetchable
(`a_dynamically_asserted_binary_column_resolves_a_source`); an implicit key is not, being no
column of the table to `SELECT`.
`FETCH_CAP` is 64 MiB, and it is enforced **in the `SELECT`** — a `SUBSTRING` of that many
bytes taken beside the whole value's `OCTET_LENGTH`, in one row of one statement. On MySQL it is
not the only cap in that `SUBSTRING`: the server's own `max_allowed_packet` is read into the
same expression and the smaller wins, for a reason that is under `schemaic-db`. `BlobValue`
keeps `bytes` and `len` apart because they disagree exactly when it matters, and `truncated()`
is read off the server's length rather than off the buffer: a value that happens to be exactly
the cap is otherwise indistinguishable from a cut one, and a truncated value refuses a save,
which would write a file that is the front of a blob and looks like the blob.
`BlobKind`/`sniff` identify content from its magic bytes and never from the column's type
name — a `BLOB` is an affinity, not a promise — and **the variant list is a claim about the
renderer**: `is_image()` is `!= Opaque`, and floem's `img()` draws *nothing* for bytes it cannot
decode, so a kind that can be named but not shown is a caption over an empty box. Adding a
format is therefore two edits — the enum, and floem's `image-*` features in the root
`Cargo.toml` — and `the_renderable_kinds_are_exactly_the_ones_floem_is_built_to_decode` is what
sends the author to the second one. `hex_line`/`hex_row_count`/`HEX_COLS` are a classic hex
dump built one line at a time, because a capped fetch is still four million lines; a row past
the end is blank rather than a panic, the virtual list and the buffer being two signals that can
be one frame apart. `BlobRef::title()` is `staff.picture`, and `save_stem()` puts the **key
values** in the offered file name (`staff_picture_1`): a stem built from the column alone offers
every row of an avatar table the same name, so the second save proposes to overwrite the first.
Everything that is not a plain filename character becomes `_`, because a key value is server
data on its way to a save dialog — a separator, a `..` or an NTFS stream colon must not steer
where the file lands (`the_save_stem_cannot_escape_into_a_path`).
**The sanitizer is `save_stem_from(parts)`, on the operation rather than on one of its
callers.** `BlobRef::save_stem` is only *one* producer of a save name and it was the only one
that sanitized: the other is the panel opened on a cell with no committed row behind it — a
pending row, or a `NULL` cell — which has no key to name and borrows the table and column
straight out of the result set instead. Those are server-supplied identifiers, PostgreSQL and
SQLite restrict neither and MySQL restricts `/ \ .` in a *table* name but not in a column name,
so a `bytea` column called `..\..\..\Startup\payload` reached the save dialog's name box as a
relative path out of the folder it was showing. `BlobTarget::stem` is a private field behind
`BlobTarget::new` for the same reason — a field anyone can write is a third producer waiting to
forget — and `grid.rs`'s `blob_launch` sends both arms through the constructor, the keyed one
included, so the two are one rule rather than two that agree today.
**Its last step is `export::device_safe_stem`, which this side had no answer of its own for.**
`CON`, `NUL`, `COM1`… name a device whatever extension follows them, in every directory, so a
keyless cell on a column called `NUL` offered `NUL.png` and the atomic save's `rename` came back
as an OS message about a name the user never chose. `export::suggested_filename` has carried
that list since it was written and `save_stem_from` carried none — two save-name sanitizers
answering one question differently — so the list, the comparison and the `_` prefix moved into
`device_safe_stem` and both call it, the way there is one identifier quoter. A prefix rather
than a suffix, because the escape has to survive an extension being appended.
**`column_byte_cap(dialect, type_name)` is the write's one pure question: how much does this
column actually hold?** It exists so an oversized file is refused where the user picked it. The other
place it gets refused is MySQL's `ERROR 1406: Data too long`, which arrives at the *commit*,
names the column rather than the file, and rolls back the whole staged batch — every unrelated
edit in it included. The four MySQL blob families answer from the family alone (TINYBLOB 255,
BLOB 65535, MEDIUMBLOB 16777215, LONGBLOB 4294967295), since the bound there is the family and
not a parameter — MySQL resolves a `BLOB(M)` declaration to the smallest family that holds `M`
and reports what it resolved to, so no length ever survives to be read (measured on MySQL 8.4:
`BLOB(70000)` introspects as `mediumblob` and `BLOB(200)` as `tinyblob`, and
`information_schema.CHARACTER_MAXIMUM_LENGTH` agrees with all four numbers) — while
`BINARY(n)`/`VARBINARY(n)` take theirs from the parameter.
**It reads the *declared* type (`ColumnInfo::type_name`) and never a result column's wire type
name**, which is the load-bearing part rather than a preference: MySQL reports all four sizes as
`MYSQL_TYPE_BLOB`, so the wire says `BLOB` for a `LONGBLOB` and strips the length off a
`VARBINARY` — only the schema knows, and a cap read off the wire would refuse a 1 MB file the
`MEDIUMBLOB` behind it was waiting for. Each family name is matched whole and case-insensitively
on the head word, which is what keeps `LONGBLOB` from answering as a `BLOB`
(`a_longer_family_name_is_not_read_as_a_shorter_one`, beside
`each_blob_family_carries_its_own_cap` and
`a_fixed_width_binary_column_takes_its_cap_from_its_parameter`).
**The dialect is asked first, and it is not decoration**: the table above is MySQL's, and a type
name alone cannot say whose it is — `BLOB` and `VARBINARY(16)` are legal SQLite declarations too,
where the parameter is ignored outright and the family means nothing. Read for all three engines
it capped a SQLite `BLOB` at 65,535 bytes and a `VARBINARY(16)` at **sixteen**, so *Load from
file* refused files that engine would have stored without complaint, at the one gesture the cap
exists to make pleasant. The question is the capability `ddl::enforces_declared_byte_length` and
never a `dialect == MySql` standing in for one, and `None` comes back before the type name is
even split. Fourteen unit tests had covered this function and every one of them named a MySQL
type, so nothing ever asked whose table was being read — which is why the pin is the composed
one, a schema and a dialect through `analyze_edit`'s cap fill into `byte_cap`.
**`None` is "no answer", never "no limit"**, and every caller must treat it so: `bytea`, SQLite,
an unknown type name, an unloaded schema and a bare `VARBINARY` all give it
(`an_unbounded_or_unreadable_type_answers_no_cap`), and after one the server is the authority it
always was.
**`LOAD_CAP` bounds a loaded file whatever the column says**, and it is `FETCH_CAP` — 64 MiB —
deliberately. The read half has always been bounded and the write half was
not: a `SELECT` can hand back only `FETCH_CAP`, but a file picker hands over whatever is on disk,
`fs::read` allocates all of it and the staged edit holds it, while a `LONGBLOB`'s own cap is 4 GB
and the column whose schema has not loaded reports no cap at all. The same number as the read cap
rather than a larger one, because a value above it comes back truncated, refuses to save and
displays as a prefix of itself — writing one in would create cells this app can never render or
export whole again. `load_too_large(len)` compares with `>`, so a file of exactly the cap fits,
that being the largest the read half can return whole
(`a_file_of_exactly_the_load_cap_still_fits`, which pins the boundary and the two constants'
equality together). The app's `load_blob` asks it of `fs::metadata` **before** the read — refusing
a 4 GB file by inspecting the `Vec` it produced means allocating it first — and the read then
bounds itself; missing metadata is not itself a refusal, the read below reporting the real error.
**That second half was a claim the code did not keep, and `read_capped(reader, cap)` is it.** The
site's own comment said "the read still bounds itself below" while calling `std::fs::read`, which
sizes its buffer from the same `metadata` hint and then reads to EOF — and on Linux `st_size` is
**0** for a character or block device, so `/dev/zero`, a FIFO or a `/proc` file passed the
pre-check and grew a `Vec` until the allocator or the OOM killer stopped it, taking the window,
every tab's uncommitted grid edits and any open manual transaction with it. `LOAD_CAP`'s only
enforcement was the allocation it exists to prevent. `read_capped` asks the reader for `cap + 1`
bytes and answers `Ok(None)` when it gets them all, which is what keeps "exactly the cap" and
"more than the cap" apart on the boundary `load_too_large` draws; it takes a **reader** rather
than a path so it can be tested against `io::repeat(0)`, an infinite one, which is exactly the
shape of the bug and something no real file in a test could be
(`read_capped_stops_on_a_reader_that_never_ends`, `read_capped_keeps_the_caps_own_boundary`).
`load_blob` opens the file and reads through it; over the cap, the honest size is the `metadata`
hint where there is one and `cap + 1` — "more than this" — where there is not, since the rest of
the file was never read. Its sentence contrasts the two sizes through
`format::contrasting_bytes`, like the column cap's: every size in a ~51 KB window above 64 MiB
otherwise read *"That file is 64.0 MB — the most that can be loaded is 64.0 MB."*
