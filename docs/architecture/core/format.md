# `core/format.rs`

`core/format.rs` — per-column display formatters (`ColumnFormat`/`apply`: epoch→datetime, bytes,
bool). Display-only; edit/copy stay raw. Persisted to `format.json`. `apply(fmt, v, marks)`
writes *Grouped* and *Bytes* in the marks it is handed — the interface's, from every view —
through `text::group_number` and `text::remark` (`a_format_writes_its_figure_in_the_marks_given`;
*Figures in the interface's marks*).
**Named crate-qualified because the basename is no longer unique**: `schemaic-cli` has a
`format.rs` of its own (the `--format` surface), and `doc_coverage`'s bare-basename match would
let either entry answer for both files.
**The Boolean one reads the word through `celledit::read_bool`, the workspace's one reader of
it.** `bool_glyph` was a `matches!` over eight hand-listed spellings and PostgreSQL's text
protocol hands `false` back as the single letter `f`, which was in none of them — so every
false row in a `boolean` column painted **`true`** the moment the format was chosen, while that
same column's editor held false. `read_bool` is the edit path's reader, is case-insensitive
throughout, and says in its own doc that it is deliberately wide *because* PostgreSQL says
`t`/`f`; the two had simply never met. A text it cannot read now falls back to the raw display
rather than to `true`, which is this module's rule at `apply` — a formatter can never hide or
corrupt data — and the old `_ => true` broke it, printing the word over every name in a column
the format was mis-clicked onto. The numeric arms stay: `1`/`0` is how MySQL and SQLite store
one (`every_engines_boolean_spelling_reads_the_same_way_it_edits`).
**`rule_key(conn_id, rs, ci)` is the whole identity a saved formatter is stored under**, and it
is one function because the read and the write are one key. The grid seeded a column's formatter
from the connection the result was *loaded on* and saved one against the tab's *live* `conn_id`,
which moves when a tab is rebound while its result stays on screen — so setting a format upserted
the rule against a server the rows never came from: absent where the user set it, and silently
applied to a real, different table on the other connection. `grid.rs`'s private `format_key` was
the half that only the seed called; both `GridState::new` and `set_format` come through here now,
and both read `conn_at_load`. The table part is the name the UI shows the table by
(`schema.table` outside PostgreSQL's `public`), so a rule on `sales.orders` cannot leak onto
`public.orders`, and the identity is the column's **own** provenance rather than the tab's source
table — keying on the source meant a hand-written query in a table-opened tab read and wrote
rules under a table its columns never came from. An expression column answers `None`: it belongs
to no table, so there is nothing to persist against, and it still formats for the life of the
result. `human_bytes` is `pub`
because the binary-cell panel says the same sentence about a blob that `ColumnFormat::Bytes`
says about a number in a cell, and two spellings of `1.5 MB` in one application is the kind of
drift only a screenshot ever catches.
**`contrasting_bytes(a, b)` is the same function for a sentence whose whole content is that the
two numbers differ.** `human_bytes` keeps one decimal, which is right for a size shown on its own
and wrong here: a 65,536-byte file refused by a MySQL `BLOB` (65,535) read *"That file is
64.0 KB — this column holds at most 64.0 KB."*, and the same collision spans a ~51 KB window at
the 64 MiB load cap. Where the rounded forms collide it falls back to exact grouped bytes on
**both** sides, and where they don't it keeps the readable form; it never checks which argument
is larger, because the caller's sentence says that and a pair that is genuinely equal is a
caller's bug this must not hide. Both over-cap refusals use it — the binary panel's column cap
and the app's `LOAD_CAP`.
