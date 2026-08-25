//! Result-set model shared across the app.
//!
//! Cells arrive over the MySQL *text protocol* (every value as a string) and are
//! parsed into [`Value`]'s compact numeric variants where lossless; `DECIMAL`,
//! dates, JSON, and anything else MySQL sends as exact text stay a `Str` so
//! nothing is rounded or reformatted. Column provenance ([`ColumnOrigin`]) drives
//! the write-back editing system.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// A single result cell.
///
/// M2 parses the wire text into compact numeric variants (for tighter memory on
/// large results and right-aligned display); everything else — including
/// `DECIMAL` and dates, which MySQL already sends as exact text — stays a
/// `Str`, so nothing is rounded or reformatted lossily.
/// `PartialEq` (not `Eq` — `Float` rules that out) so tests can compare a parsed
/// or coerced cell directly against the value it should be.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Text to render in a grid cell (NULLs render as the literal `NULL`,
    /// styled dim by the UI).
    pub fn display(&self) -> String {
        match self {
            Value::Null => "NULL".to_string(),
            Value::Int(v) => v.to_string(),
            Value::UInt(v) => v.to_string(),
            Value::Float(v) => v.to_string(),
            Value::Str(s) => s.clone(),
        }
    }
}

/// Compact per-cell type tag for the columnar [`ResultSet`] storage. The cell's
/// text lives in the owning column's arena; this tag says how to interpret it
/// (and whether it is SQL `NULL`). Mirrors the [`Value`] variants. Stored as the
/// top 3 bits of each cell's packed offset word (see [`ColumnData`]) — no
/// per-cell byte of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellTag {
    Null,
    Int,
    UInt,
    Float,
    Str,
}

impl CellTag {
    /// 3-bit encoding packed into the high bits of a column's offset word.
    fn to_bits(self) -> u32 {
        match self {
            CellTag::Null => 0,
            CellTag::Int => 1,
            CellTag::UInt => 2,
            CellTag::Float => 3,
            CellTag::Str => 4,
        }
    }

    /// Inverse of [`CellTag::to_bits`]. Any unused bit pattern maps to `Str` —
    /// the safe default (raw text), so a stray value can never mis-parse.
    fn from_bits(bits: u32) -> CellTag {
        match bits {
            0 => CellTag::Null,
            1 => CellTag::Int,
            2 => CellTag::UInt,
            3 => CellTag::Float,
            _ => CellTag::Str,
        }
    }
}

/// A borrowed view of a single result cell: its type tag plus its stored text
/// (empty for NULL). Cheap to copy, no allocation — this is what the grid reads
/// on the hot render/sort/scan paths. Reconstruct an owned, typed [`Value`] with
/// [`CellRef::to_value`] only where one is actually needed (edit keys, JSON).
#[derive(Clone, Copy, Debug)]
pub struct CellRef<'a> {
    pub tag: CellTag,
    text: &'a str,
}

impl<'a> CellRef<'a> {
    pub fn is_null(&self) -> bool {
        self.tag == CellTag::Null
    }

    /// The text to render in a grid cell: NULL renders as the literal `NULL`
    /// (matching [`Value::display`]); every other cell renders its stored text.
    /// Borrowed — no allocation.
    pub fn display(&self) -> &'a str {
        if self.is_null() { "NULL" } else { self.text }
    }

    /// The raw stored text — empty for NULL, the canonical value text otherwise.
    /// Used where NULL must render blank rather than as `NULL` (CSV/JSON/HTML).
    pub fn text(&self) -> &'a str {
        self.text
    }

    /// Does this stored cell already hold `v`? Answers exactly "would pushing `v`
    /// produce this cell", by tag and canonical text — the numeric text was
    /// written with `Display`, so it parses back to the same value. Allocation-free,
    /// because it runs per replacement cell on the post-commit splice path. An
    /// unparseable numeric or a NaN compares unequal, which only costs a rebuild
    /// that wasn't needed.
    pub fn matches(&self, v: &Value) -> bool {
        match (self.tag, v) {
            (CellTag::Null, Value::Null) => true,
            (CellTag::Str, Value::Str(s)) => self.text == s,
            (CellTag::Int, Value::Int(n)) => self.text.parse::<i64>() == Ok(*n),
            (CellTag::UInt, Value::UInt(n)) => self.text.parse::<u64>() == Ok(*n),
            (CellTag::Float, Value::Float(f)) => self.text.parse::<f64>() == Ok(*f),
            _ => false,
        }
    }

    /// Reconstruct the owned, typed [`Value`] by parsing the stored text per the
    /// tag. A numeric cell whose text unexpectedly fails to parse degrades to a
    /// `Str` (defensive — the tag is set from a real parsed value at build time).
    pub fn to_value(&self) -> Value {
        match self.tag {
            CellTag::Null => Value::Null,
            CellTag::Int => self
                .text
                .parse()
                .map(Value::Int)
                .unwrap_or_else(|_| Value::Str(self.text.to_string())),
            CellTag::UInt => self
                .text
                .parse()
                .map(Value::UInt)
                .unwrap_or_else(|_| Value::Str(self.text.to_string())),
            CellTag::Float => self
                .text
                .parse()
                .map(Value::Float)
                .unwrap_or_else(|_| Value::Str(self.text.to_string())),
            CellTag::Str => Value::Str(self.text.to_string()),
        }
    }
}

/// Where a result column really came from — the MySQL wire protocol reports
/// this per column, even through aliases and joins. `None` (see [`Column`])
/// means the column has no single base column (an expression, aggregate, or
/// literal) and so cannot be edited.
#[derive(Clone, Debug)]
pub struct ColumnOrigin {
    /// Real schema (database) the column belongs to.
    pub database: String,
    /// PostgreSQL namespace the table lives in (`public`, `sales`, …), from the
    /// prepared column's `table_oid`. `None` on MySQL, which has no level between
    /// database and table. Part of the table's identity: without it, same-named
    /// tables in two schemas collapse into one and an `UPDATE` could hit the
    /// wrong one.
    pub schema: Option<String>,
    /// Real table (`org_table`), not the query alias.
    pub table: String,
    /// Real column name (`org_name`), not the query alias.
    pub column: String,
    /// Key/nullability flags carried on the column definition.
    pub flags: ColumnFlags,
    /// Raw-bytes column (BLOB/BINARY/VARBINARY/BIT — binary charset). Such a
    /// value can't round-trip through the text protocol without loss, so the
    /// editing system treats it as read-only and refuses it as a WHERE key.
    pub binary: bool,
    /// This column is the table's **implicit** row key — a value that identifies
    /// a row uniquely but is no column of the table (SQLite's `rowid`, projected
    /// explicitly because `SELECT *` does not return it). `false` everywhere
    /// else, including a SQLite `WITHOUT ROWID` table, which has none.
    ///
    /// It makes a keyless table writable, and it is asserted by the backend
    /// rather than derived from the schema — the same trust
    /// [`ColumnFlags::primary_key`] already carries. Two consequences follow, and
    /// both are why this is a flag and not just another key column:
    /// [`crate::edit::analyze_edit`] falls back to it only after a primary key
    /// and a unique index have both failed, and the column itself is never
    /// **editable** — it is the handle the write holds the row by, not data the
    /// table has, and a newly inserted row has no value to offer for it.
    pub implicit_key: bool,
}

/// Per-column key/nullability flags from the wire column definition. Used by the
/// editing system to decide updatability and build a safe `WHERE`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ColumnFlags {
    pub primary_key: bool,
    pub unique_key: bool,
    pub not_null: bool,
    pub auto_increment: bool,
    /// The column has **no** default value (MySQL `NO_DEFAULT_VALUE_FLAG`): a new
    /// row must supply it, or the `INSERT` errors ("Field 'x' doesn't have a
    /// default value"). Nullable columns have an implicit `NULL` default, so this
    /// is only set for NOT-NULL, non-auto-increment columns without a `DEFAULT`.
    pub no_default: bool,
}

/// Column metadata from a result.
#[derive(Clone, Debug)]
pub struct Column {
    pub name: String,
    /// SQL type name as reported by the driver (e.g. `VARCHAR`, `INT`).
    pub type_name: String,
    /// Provenance: the real base column this maps to, if any. `None` for
    /// expressions/aggregates/literals (not editable).
    pub origin: Option<ColumnOrigin>,
}

/// Type names that render right-aligned, in both engines' spellings (including
/// the `intN`/`floatN` internal names). Compared against a declared type's
/// **leading token**, so `DOUBLE PRECISION` and `INT(11) UNSIGNED` are covered by
/// `DOUBLE` and `INT`.
const NUMERIC_TYPES: &[&str] = &[
    "TINYINT",
    "SMALLINT",
    "MEDIUMINT",
    "INT",
    "INTEGER",
    "BIGINT",
    "INT2",
    "INT4",
    "INT8",
    "DECIMAL",
    "DEC",
    "NUMERIC",
    "FIXED",
    "FLOAT",
    "FLOAT4",
    "FLOAT8",
    "DOUBLE",
    "REAL",
    "YEAR",
    "BIT",
];

/// Leading type tokens that mean **raw bytes**, in all three engines'
/// spellings. Compared as the leading token so `VARBINARY(255)` and `BLOB(16)`
/// are covered.
///
/// **`BIT` is not here, and that is the point of the list.** MySQL hands a
/// `BIT(M)` over as bytes, so it looked like one — but a bit-field *has* a
/// lossless text form (its number, which is also what MySQL accepts back), and
/// calling it binary meant the grid showed `<1 bytes>` for a `BIT(8)` holding 10
/// and, worse, the CSV and JSON exports **withheld the column entirely**, in the
/// two formats this app reads back. PostgreSQL's `bit` never even arrived as
/// bytes: its text protocol sends `00001010`, which was then reported as a byte
/// count. What the wire happens to carry is not what a type *is*; see
/// [`bit_display`] for the conversion that made the difference visible.
///
/// This is the *type-name* half of the question only. MySQL asks it of a
/// resolved wire type, PostgreSQL of `BYTEA`, SQLite of a *declared* type —
/// which is an affinity rather than a promise, so a SQLite `BLOB` column can
/// still hold text. Nothing here may therefore be used to discard a value on
/// its own; [`is_binary_display`] is the second signal every such decision
/// pairs it with.
const BINARY_TYPES: &[&str] = &[
    "BINARY",
    "VARBINARY",
    "TINYBLOB",
    "BLOB",
    "MEDIUMBLOB",
    "LONGBLOB",
    "BYTEA",
    "GEOMETRY",
];

/// Does this type name name a **bit-field** — MySQL's `BIT(M)`, PostgreSQL's
/// `BIT`/`BIT VARYING`?
///
/// Leading token, like [`type_is_binary`]. It exists because MySQL sends a
/// bit-field as raw bytes and there is nothing in the value itself to say those
/// bytes are a number rather than a string: only the column's type knows.
pub fn type_is_bit(type_name: &str) -> bool {
    let head = type_name
        .split(|c: char| c == '(' || c.is_whitespace())
        .next()
        .unwrap_or_default();
    head.eq_ignore_ascii_case("BIT") || head.eq_ignore_ascii_case("VARBIT")
}

/// A MySQL bit-field's bytes as the number they are: **big-endian, unsigned**,
/// which is the byte order MySQL sends and the form it takes back
/// (`b = 10` assigns a `BIT(8)`).
///
/// `BIT(M)` is 1–64 bits, so at most eight bytes arrive; anything longer than
/// that is not a bit-field this can be sure about, and the *last* eight bytes are
/// taken because that is the low half of a big-endian number — dropping the high
/// bytes of a value too wide to represent, rather than dropping the value.
///
/// An empty run of bytes is `0`: MySQL has no zero-width bit-field, and a NULL is
/// a NULL long before it reaches here, so the only caller of this with nothing to
/// read is a server that sent nothing.
pub fn bit_display(bytes: &[u8]) -> String {
    bit_value(bytes).to_string()
}

/// [`bit_display`]'s answer **before** it becomes text — the number itself.
///
/// The distinction is not cosmetic. A `Value::Str("10")` and a `Value::UInt(10)`
/// look identical in the grid and are different data everywhere else: the SQL
/// export quotes every `Value::Str`, and `'10'` assigned to a MySQL `BIT`
/// column is not the number 10 — `Field_bit::store(const char*, …)` takes the
/// raw bits of the *bytes*, so `'10'` is `0x3132` = 12594 on a `BIT(16)` and
/// "Data too long" on a `BIT(8)`. The claim in this module's own doc — that a
/// bit-field has a lossless text form, "the form it takes back (`b = 10`
/// assigns a `BIT(8)`)" — is true of the bare token `10` and false of `'10'`,
/// which was the only thing an export could emit for a string.
///
/// So the conversion belongs at the wire, where the type is known, and the
/// number carries from there: `Value::UInt` reaches `sql_literal` as a bare
/// number, JSON as a number, and the grid as the same digits `bit_display`
/// wrote.
pub fn bit_value(bytes: &[u8]) -> u64 {
    let tail = if bytes.len() > 8 {
        &bytes[bytes.len() - 8..]
    } else {
        bytes
    };
    tail.iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b))
}

/// The [`Value`] a bit-field's bytes become — what the MySQL loader stores for a
/// `BIT` column.
///
/// A named function rather than two words inside the loader's `match`, so the
/// seam that broke is reachable from a test: the fault was never in
/// [`bit_value`], which was right and had a table of tests, but in the *variant*
/// its answer was wrapped in on the way to an exporter.
pub fn bit_cell(bytes: &[u8]) -> Value {
    Value::UInt(bit_value(bytes))
}

/// Does this SQL type name name a raw-bytes column, in any of the three engines?
///
/// Matches the **leading type token** for the same reason [`Column::is_numeric`]
/// does: a parameter list or a trailing modifier is not part of the name.
pub fn type_is_binary(type_name: &str) -> bool {
    let head = type_name
        .split(|c: char| c == '(' || c.is_whitespace())
        .next()
        .unwrap_or_default();
    BINARY_TYPES.iter().any(|k| head.eq_ignore_ascii_case(k))
}

/// **The one rendering of a raw-bytes cell**, for every backend.
///
/// A BLOB has no lossless text form and [`Value`] has no bytes variant, so the
/// three engines each invented their own answer and all three were different:
/// SQLite showed the size, MySQL `from_utf8_lossy`'d the bytes into mojibake,
/// PostgreSQL handed over the text protocol's `\x…`. The mojibake was the
/// dangerous one — it looks like data, so a CSV or `INSERT` export wrote it
/// *as* the data and re-imported as wrong bytes.
///
/// SQLite's was the honest answer and is now everyone's: it says what it is,
/// it costs one short string for a 40 MB blob rather than a hex dump long
/// enough to hang the grid, and the editing system independently refuses to
/// write a binary column, so nothing round-trips it back into the database.
pub fn binary_display(len: usize) -> String {
    format!("<{len} bytes>")
}

/// Is this cell text one that [`binary_display`] produced?
///
/// The recognizer exists because the placeholder is the only signal that a
/// cell's real bytes were *dropped* rather than rendered — the export paths
/// need to tell "this text is the value" from "this text stands in for a value
/// I do not have". Deliberately exact: a leading `<`, decimal digits, ` bytes>`
/// and nothing else, so a user's prose about byte counts is not mistaken for
/// one.
pub fn is_binary_display(s: &str) -> bool {
    let Some(inner) = s.strip_prefix('<').and_then(|s| s.strip_suffix(" bytes>")) else {
        return false;
    };
    !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit())
}

impl Column {
    /// Does this column hold raw bytes?
    ///
    /// Two inputs, because neither alone covers the result sets that exist.
    /// [`ColumnOrigin::binary`] is the authoritative wire answer but is only
    /// present for a *table-backed* column, so a `SELECT some_bytea_expr` — or
    /// any PostgreSQL result whose catalog lookup found no provenance — reached
    /// it as `None` and was treated as text. The type name covers those.
    pub fn is_binary(&self) -> bool {
        self.origin.as_ref().is_some_and(|o| o.binary) || type_is_binary(&self.type_name)
    }

    /// Is this a numeric column? Display-only — it decides right alignment.
    ///
    /// Matches the **leading type token** (the name up to the first `(` or space)
    /// rather than any substring: `t.contains("INT")` made PostgreSQL's `interval`
    /// and both engines' `point` / `multipoint` numeric, so an ordinary
    /// `now() - created_at` column right-aligned as though it were a number.
    /// Parameter lists and `UNSIGNED` / `ZEROFILL` / `PRECISION` suffixes fall
    /// outside the token and are ignored.
    pub fn is_numeric(&self) -> bool {
        let head = self
            .type_name
            .split(|c: char| c == '(' || c.is_whitespace())
            .next()
            .unwrap_or_default();
        NUMERIC_TYPES.iter().any(|k| head.eq_ignore_ascii_case(k))
    }
}

/// Width of the offset field in a packed cell word; the remaining top 3 bits
/// carry the [`CellTag`]. 29 bits caps a column's arena at 512 MiB of text.
const OFFSET_BITS: u32 = 29;
/// Mask for the offset field of a packed cell word.
const OFFSET_MASK: u32 = (1 << OFFSET_BITS) - 1;
/// Max text bytes per column arena (the largest representable offset).
const MAX_ARENA: usize = OFFSET_MASK as usize;

/// One column's cells in the compact **columnar** layout: a single `u32` per
/// cell (its [`CellTag`] packed into the top 3 bits, its text's end offset into
/// the arena in the low 29 bits) plus one `arena` holding every cell's canonical
/// text back-to-back. Cell `i` spans `arena[start..end]`, where `end` is the low
/// 29 bits of `ends[i]` and `start` those of `ends[i - 1]` (or `0` for the first
/// cell); a NULL cell has an empty span and a `Null` tag, and a numeric cell
/// stores its canonical display text — so a cell's rendered text is a zero-copy
/// slice of the arena.
///
/// This replaces the old row-major `Vec<Value>` (a 32-byte `Value` per cell plus
/// a heap allocation per string): **4 bytes** of fixed overhead per cell plus the
/// text bytes, and one arena allocation per column instead of one per string.
#[derive(Clone, Debug, Default)]
struct ColumnData {
    /// Per cell: `(tag << OFFSET_BITS) | end_offset`. The tag rides in the top 3
    /// bits so there's no separate tag byte; the offset (low 29 bits) reaches
    /// [`MAX_ARENA`] — 512 MiB for the *whole column*, i.e. ~2.7 KB per row at
    /// the 200k-row cap, which a text column can reach.
    ends: Vec<u32>,
    arena: String,
    /// The arena hit [`MAX_ARENA`] and was truncated, so every cell from that
    /// point on is blank. Reported, not silent — see
    /// [`ColumnData::finish_cell`].
    capped: bool,
}

impl ColumnData {
    fn with_capacity(rows: usize) -> Self {
        ColumnData {
            ends: Vec::with_capacity(rows),
            arena: String::new(),
            capped: false,
        }
    }

    /// Finalize the cell whose text has just been appended to `arena`, recording
    /// its tag + end offset as one packed word.
    ///
    /// The 29-bit offset caps a column's arena at 512 MiB **across all its rows**
    /// — not per cell, which is what the comment here used to imply while calling
    /// it "unreachable within the row cap". At the 200,000-row default that is
    /// only ~2.7 KB per row, which a `TEXT` or `JSON` column clears comfortably.
    ///
    /// Past the cap the arena is truncated at a char boundary, so an offset can
    /// never collide with the tag bits — graceful capping, never corruption. But
    /// every *later* cell then has `start == end == MAX_ARENA` and renders blank
    /// while keeping its tag, so `capped` is set and carried out to
    /// [`ResultSet::capped_columns`]: rows going blank partway down a result is
    /// exactly the kind of thing a user must not have to guess at.
    fn finish_cell(&mut self, tag: CellTag) {
        self.finish_cell_within(tag, MAX_ARENA);
    }

    /// [`ColumnData::finish_cell`] with the ceiling passed in, so the capping
    /// path can be tested without building a 512 MiB result.
    fn finish_cell_within(&mut self, tag: CellTag, max_arena: usize) {
        if self.arena.len() > max_arena {
            let mut cut = max_arena;
            while !self.arena.is_char_boundary(cut) {
                cut -= 1;
            }
            self.arena.truncate(cut);
            self.capped = true;
        }
        let end = self.arena.len() as u32;
        self.ends.push((tag.to_bits() << OFFSET_BITS) | end);
    }

    /// Append one value: write its canonical text into the arena (nothing for
    /// NULL) and record its tag. Numerics are written via `Display` — identical
    /// to [`Value::display`] — so the stored text is exactly what the grid shows.
    fn push(&mut self, v: &Value) {
        use std::fmt::Write as _;
        let tag = match v {
            Value::Null => CellTag::Null,
            Value::Int(n) => {
                let _ = write!(self.arena, "{n}");
                CellTag::Int
            }
            Value::UInt(n) => {
                let _ = write!(self.arena, "{n}");
                CellTag::UInt
            }
            Value::Float(f) => {
                let _ = write!(self.arena, "{f}");
                CellTag::Float
            }
            Value::Str(s) => {
                self.arena.push_str(s);
                CellTag::Str
            }
        };
        self.finish_cell(tag);
    }

    /// Append a borrowed cell verbatim (tag + text) — copies a cell from one
    /// column buffer into another (used when rebuilding a column on splice).
    fn push_ref(&mut self, c: CellRef<'_>) {
        self.arena.push_str(c.text);
        self.finish_cell(c.tag);
    }

    fn cell(&self, row: usize) -> Option<CellRef<'_>> {
        let packed = *self.ends.get(row)?;
        let tag = CellTag::from_bits(packed >> OFFSET_BITS);
        let end = (packed & OFFSET_MASK) as usize;
        let start = if row == 0 {
            0
        } else {
            (self.ends[row - 1] & OFFSET_MASK) as usize
        };
        Some(CellRef {
            tag,
            text: &self.arena[start..end],
        })
    }
}

/// A fully materialized result set, stored **columnar** (one [`ColumnData`] per
/// column) rather than row-major. `schemaic_db::Db` loads rows into memory up to
/// a caller-supplied row cap (`truncated` flags when more exist) via
/// [`ResultBuilder`].
///
/// **A grid's result is still materialised whole; an export is not.** The cap is
/// what makes materialising one safe — nobody scrolls two million rows — and
/// `Db::stream_query` escapes it by cutting the load into a sequence of these
/// with [`ResultBuilder::take_chunk`], so a whole-table export is bounded by one
/// chunk rather than by the table.
///
/// Cells are read through [`ResultSet::cell`] (a borrowed [`CellRef`]); the field
/// is private so the columnar invariant (each column's packed `ends` words stay
/// in lock-step with its `arena`) can't be violated from outside.
///
/// **Each column is behind its own `Arc`, which makes cloning a `ResultSet`
/// cheap** — a `Vec<Column>` plus one refcount bump per column, not the data.
/// That matters because the grid's `rs` signal and the tab's canonical
/// `QueryState::Loaded` deliberately hold the *same* `Arc<ResultSet>`, so the
/// post-commit splice mutates through `Arc::make_mut` with a strong count of 2:
/// with the columns inline that deep-copied every arena in the result — seconds
/// and hundreds of megabytes at the 200k×50 target, on the UI thread, on the one
/// path built to avoid a rebuild. Now the outer clone is trivial and
/// [`ResultSet::splice_rows`] replaces only the column `Arc`s it actually
/// changes, so an untouched column is never copied at all.
#[derive(Clone, Debug, Default)]
pub struct ResultSet {
    pub columns: Vec<Column>,
    cols: Vec<Arc<ColumnData>>,
    n_rows: usize,
    pub elapsed_ms: u128,
    /// True if the fetch stopped at the row cap (more rows may exist).
    pub truncated: bool,
    /// Indices of columns whose text arena hit its 512 MiB ceiling, so their
    /// cells past that point are blank. Separate from `truncated`, which is
    /// about *rows* and says nothing about this: a user whose cells go empty at
    /// row 180,000 otherwise has no way to learn why.
    pub capped_columns: Vec<usize>,
    /// Indices of columns the **backend saw raw bytes in**, whatever the column
    /// claimed to be.
    ///
    /// The backend's assertion, not a guess from a type name, and it exists
    /// because on one engine the type name cannot answer. A SQLite result column
    /// carries `origin: None` and `decl_type()` — an *affinity*, or nothing at
    /// all: `CREATE TABLE files(id INTEGER PRIMARY KEY, data)` is legal and is
    /// the ordinary shape for a blob store, and a computed column
    /// (`SELECT zeroblob(4)`) has no declared type either. So
    /// [`Column::is_binary`] is `false` for it, the export's two-signal rule
    /// degenerates to "never withhold", and the `<n bytes>` placeholder went into
    /// CSV, JSON **and** SQL as though it were the data — the one place the
    /// placeholder is certainly not a user's prose is exactly where the guard
    /// refused to act.
    ///
    /// Set per value (`ValueRef::Blob` is not a heuristic) and reported per
    /// *column*, since that is the grain everything downstream asks at. A column
    /// marked here is still tested per cell against
    /// [`is_binary_display`] before anything is withheld, so a row of real text
    /// in a column that held a blob elsewhere is untouched.
    pub binary_columns: Vec<usize>,
    /// For a statement that returns no result set (UPDATE/INSERT/DELETE/DDL),
    /// the number of rows the server reports affected. `None` for a row-
    /// returning result (a SELECT grid), so the UI can tell the two apart.
    pub affected: Option<u64>,
    /// The database this statement **ran against** — what the grid's stats line
    /// reports, so a result says where it came from.
    ///
    /// It lives on the result, not on the tab, and that is the whole point. A
    /// tab's database selection moves: the moment someone changes it, the grid
    /// still shows rows fetched under the old one, so a label read from the tab
    /// would be wrong in exactly the situation it exists to catch. Stored here it
    /// is a snapshot by construction, and it survives a commit splice for free
    /// (which mutates the columns in place and leaves this alone).
    ///
    /// `None` when the connection had no default database, and on a result no
    /// query produced — a test fixture, or the temporary set a re-fetch splices
    /// from.
    ///
    /// It names the **scope**, not the origin of every row: a statement is free
    /// to qualify another database (`SELECT … FROM world.country` while scoped to
    /// `sakila`), and this still reports the scope the connection ran under.
    pub database: Option<String>,
}

impl ResultSet {
    /// Build a row-returning result from row-major data (columns + rows). Each
    /// inner `Vec<Value>` is one row in column order; a row shorter than
    /// `columns` is padded with NULL and extra cells are ignored. Used by tests
    /// and the splice/refetch paths — the DB loader uses [`ResultBuilder`]
    /// directly to avoid ever holding a row-major copy.
    pub fn from_rows(columns: Vec<Column>, rows: Vec<Vec<Value>>) -> Self {
        let mut b = ResultBuilder::with_capacity(columns, rows.len());
        for row in &rows {
            b.push_row(row);
        }
        b.finish()
    }

    /// Build a no-result-set outcome (UPDATE/INSERT/DELETE/DDL): the server's
    /// reported affected-row count, no grid.
    pub fn affected_rows(columns: Vec<Column>, n: u64) -> Self {
        ResultSet {
            columns,
            affected: Some(n),
            ..Default::default()
        }
    }

    /// Builder-style setter for the query's elapsed time.
    pub fn with_elapsed(mut self, ms: u128) -> Self {
        self.elapsed_ms = ms;
        self
    }

    /// Builder-style setter for the truncated (hit-the-row-cap) flag.
    pub fn with_truncated(mut self, truncated: bool) -> Self {
        self.truncated = truncated;
        self
    }

    pub fn row_count(&self) -> usize {
        self.n_rows
    }
    pub fn col_count(&self) -> usize {
        self.columns.len()
    }

    /// Borrowed view of the cell at `(row, col)` in **data** (unsorted) order, or
    /// `None` if either index is out of range.
    pub fn cell(&self, row: usize, col: usize) -> Option<CellRef<'_>> {
        self.cols.get(col)?.cell(row)
    }

    /// For one base table, the *real* column name → result-column index, matched
    /// on the wire **provenance** each column carries rather than on the name it
    /// is displayed under.
    ///
    /// This is the identity rule for anything a result inherits from a table:
    /// key icons, saved column formatters, FK "Follow". A name match is not that
    /// rule — a tab opened from `customers` keeps its source when the user types
    /// a different query into it, so `SELECT o.customerNumber FROM orders o`
    /// would take `customers`' primary-key icon, and `SELECT 1 AS customerNumber`
    /// would take it on a literal.
    ///
    /// `schema` is the PostgreSQL namespace and is **part of the identity**:
    /// without it, a result spanning `public.orders` and `sales.orders` maps the
    /// wrong side. An expression column (no origin) is in no table and is
    /// deliberately absent. First occurrence wins, so a column selected twice
    /// resolves to its leftmost appearance.
    pub fn origin_columns(
        &self,
        database: &str,
        schema: Option<&str>,
        table: &str,
    ) -> std::collections::HashMap<&str, usize> {
        let mut map = std::collections::HashMap::new();
        for (ci, col) in self.columns.iter().enumerate() {
            if let Some(o) = &col.origin
                && o.table == table
                && o.database == database
                && o.schema.as_deref() == schema
            {
                map.entry(o.column.as_str()).or_insert(ci);
            }
        }
        map
    }

    /// Replace whole data rows in place — `(data_row, new cells)` with cells
    /// aligned to the columns — rebuilding each column buffer with the
    /// substitutions applied. Used by the grid's in-place edit splice (post-commit
    /// re-fetch), so scroll/selection survive without a full query re-run. Rows
    /// not listed keep their existing cells; a replacement shorter than `columns`
    /// leaves the missing columns' cells unchanged.
    ///
    /// A column is rebuilt only when a replacement actually **changes** one of its
    /// cells. The post-commit re-fetch hands back whole rows, so for the ordinary
    /// one-cell edit 49 of 50 columns arrive identical to what is stored — and
    /// rebuilding them meant a fresh arena and a `push_ref` per cell for the whole
    /// result (10 million of them at the project's 200k × 50 target, on the UI
    /// thread, immediately after a write).
    pub fn splice_rows(&mut self, rows: &[(usize, Vec<Value>)]) {
        if rows.is_empty() {
            return;
        }
        let repl: std::collections::HashMap<usize, &[Value]> =
            rows.iter().map(|(di, v)| (*di, v.as_slice())).collect();
        let n = self.n_rows;
        for (ci, cd) in self.cols.iter_mut().enumerate() {
            let changed = rows.iter().any(|(di, cells)| match cells.get(ci) {
                Some(v) => !cd.cell(*di).is_some_and(|c| c.matches(v)),
                None => false, // this row supplies no cell for this column
            });
            if !changed {
                // Not even a refcount touched — whoever else holds this column
                // keeps sharing it.
                continue;
            }
            let mut nb = ColumnData::with_capacity(n);
            // The new arena is within a cell's length of the old one; reserving it
            // avoids growing from zero by repeated reallocate-and-copy.
            nb.arena.reserve(cd.arena.len());
            for r in 0..n {
                match repl.get(&r).and_then(|cells| cells.get(ci)) {
                    Some(v) => nb.push(v),
                    None => match cd.cell(r) {
                        Some(c) => nb.push_ref(c),
                        None => nb.push(&Value::Null),
                    },
                }
            }
            // A fresh column replaces the shared one — copy-on-write, so any
            // other holder of the old `ResultSet` still sees its old values.
            *cd = Arc::new(nb);
        }
    }
}

/// Assembles a columnar [`ResultSet`] one row at a time, so a large result never
/// exists as a row-major `Vec<Vec<Value>>` in memory: the DB loader converts each
/// wire row and pushes it straight into the per-column buffers.
pub struct ResultBuilder {
    columns: Vec<Column>,
    cols: Vec<ColumnData>,
    n_rows: usize,
    elapsed_ms: u128,
    truncated: bool,
    /// Per column: has the backend put raw bytes in it? See
    /// [`ResultSet::binary_columns`].
    binary: Vec<bool>,
}

impl ResultBuilder {
    pub fn new(columns: Vec<Column>) -> Self {
        Self::with_capacity(columns, 0)
    }

    pub fn with_capacity(columns: Vec<Column>, rows: usize) -> Self {
        let cols = (0..columns.len())
            .map(|_| ColumnData::with_capacity(rows))
            .collect();
        ResultBuilder {
            binary: vec![false; columns.len()],
            columns,
            cols,
            n_rows: 0,
            elapsed_ms: 0,
            truncated: false,
        }
    }

    /// Record that column `ci` carried raw bytes — the backend's own assertion,
    /// made where the wire value is still in hand. See
    /// [`ResultSet::binary_columns`] for why a type name cannot answer this on
    /// every engine.
    pub fn mark_binary(&mut self, ci: usize) {
        if let Some(slot) = self.binary.get_mut(ci) {
            *slot = true;
        }
    }

    /// The columns being built — the loader passes these to `convert_row`.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// Rows pushed so far (the loader compares this against the row cap).
    pub fn row_count(&self) -> usize {
        self.n_rows
    }

    /// Bytes of cell **text** held so far, across every column.
    ///
    /// The figure a streamed export flushes on when the rows are wide: a row
    /// count is a budget in the wrong unit, since nothing bounds a row's width,
    /// and this is the same arena length the 512 MiB per-column ceiling is
    /// measured against. Cheap — one `len()` per column, no walk — because the
    /// caller asks it once per row.
    pub fn text_bytes(&self) -> usize {
        self.cols.iter().map(|c| c.arena.len()).sum()
    }

    /// Append one row of cells (column order). A row shorter than the column
    /// count is padded with NULL; extra cells are ignored.
    pub fn push_row(&mut self, cells: &[Value]) {
        for (ci, cd) in self.cols.iter_mut().enumerate() {
            match cells.get(ci) {
                Some(v) => cd.push(v),
                None => cd.push(&Value::Null),
            }
        }
        self.n_rows += 1;
    }

    /// Close off the rows pushed so far as one [`ResultSet`] and start a fresh,
    /// empty builder over the **same columns** — the streaming export's flush.
    ///
    /// This is what keeps an uncapped export bounded in memory. `finish` consumes
    /// the builder because a normal load produces exactly one result; a load that
    /// is being written to a file as it arrives produces a sequence of them, and
    /// without this the loader would have to hold every row to the end — the very
    /// thing the export exists to avoid, and the thing the 512 MiB per-column
    /// arena ceiling ([`ResultSet::capped_columns`]) would silently truncate.
    ///
    /// `capacity` sizes the new chunk's per-column buffers, so a loader flushing
    /// every N rows pays one allocation per column per chunk rather than growing
    /// from empty each time.
    pub fn take_chunk(&mut self, capacity: usize) -> ResultSet {
        let mut next = ResultBuilder::with_capacity(self.columns.clone(), capacity);
        // **Carried forward, unlike `capped_columns`.** That one is a property of
        // the chunk's own arena; this one is a property of the *column* — a blob
        // in block 3 says the column holds bytes, and the placeholder text a
        // later block writes for it must be withheld on the same grounds. The
        // blocks already flushed keep their own answer, which is exactly how the
        // SQL emitter treats a column that only starts dropping later.
        next.binary = self.binary.clone();
        let mut done = std::mem::replace(self, next);
        // The elapsed time belongs to the whole load, not to a chunk of it; a
        // per-chunk figure would be meaningless and the caller stamps the total
        // on whatever it reports. `truncated` is likewise the load's, and an
        // uncapped stream is never truncated.
        done.elapsed_ms = 0;
        done.truncated = false;
        done.finish()
    }

    pub fn set_elapsed(&mut self, ms: u128) {
        self.elapsed_ms = ms;
    }

    pub fn set_truncated(&mut self, truncated: bool) {
        self.truncated = truncated;
    }

    pub fn finish(self) -> ResultSet {
        let capped_columns = self
            .cols
            .iter()
            .enumerate()
            .filter(|(_, c)| c.capped)
            .map(|(i, _)| i)
            .collect();
        let binary_columns = self
            .binary
            .iter()
            .enumerate()
            .filter(|(_, seen)| **seen)
            .map(|(i, _)| i)
            .collect();
        ResultSet {
            capped_columns,
            binary_columns,
            columns: self.columns,
            // One `Arc` per column, allocated once here at the end of the load —
            // see [`ResultSet`] for why the columns are shared individually.
            cols: self.cols.into_iter().map(Arc::new).collect(),
            n_rows: self.n_rows,
            elapsed_ms: self.elapsed_ms,
            truncated: self.truncated,
            affected: None,
            // Stamped by the loader that knows the scope — see the field's doc.
            database: None,
        }
    }
}

/// One row's staged edits, ready to execute as a single `UPDATE`. Built by the
/// grid's editing system from the result's per-column provenance: `database` /
/// `table` are the real base table, `set` are the columns to change (new text,
/// bound as a parameter), and `key` is the WHERE identity (columns + their
/// *original* typed values). The executor runs each of these in one transaction
/// and requires every statement to affect exactly one row.
#[derive(Clone, Debug)]
pub struct RowEdit {
    pub database: String,
    /// PostgreSQL namespace of `table` (`None` on MySQL). The executor qualifies
    /// with it **unconditionally** — this statement is never shown to the user,
    /// so it must not depend on `search_path`.
    pub schema: Option<String>,
    pub table: String,
    /// Columns to set → new value. `Some(text)` is bound as a string param (the
    /// server coerces to the column type); `None` sets SQL `NULL`.
    pub set: Vec<(String, Option<String>)>,
    /// WHERE identity: key columns → their original typed values.
    pub key: Vec<(String, Value)>,
}

/// One new row staged for `INSERT`. Built by the grid from the result's single
/// base table: `database` / `table` are that table, and `cols` are the columns
/// the user set → value (`Some(text)` bound as a string param; `None` = SQL
/// `NULL`). Columns *omitted* from `cols` take their DB default (auto-increment,
/// `DEFAULT`, or `NULL`). The executor runs each in the same transaction as the
/// updates and requires it to affect exactly one row.
#[derive(Clone, Debug)]
pub struct RowInsert {
    pub database: String,
    /// PostgreSQL namespace of `table` — see [`RowEdit::schema`].
    pub schema: Option<String>,
    pub table: String,
    pub cols: Vec<(String, Option<String>)>,
}

/// One row staged for `DELETE`, identified by its WHERE key (columns + their
/// original typed values) — the same row-identity model as [`RowEdit::key`]. The
/// executor runs it in the shared transaction and requires it to affect exactly
/// one row.
#[derive(Clone, Debug)]
pub struct RowDelete {
    pub database: String,
    /// PostgreSQL namespace of `table` — see [`RowEdit::schema`].
    pub schema: Option<String>,
    pub table: String,
    pub key: Vec<(String, Value)>,
}

/// A batch of staged grid mutations committed together in one transaction:
/// cell-edit `UPDATE`s, new-row `INSERT`s, and row `DELETE`s.
#[derive(Clone, Debug, Default)]
pub struct GridWrite {
    pub updates: Vec<RowEdit>,
    pub inserts: Vec<RowInsert>,
    pub deletes: Vec<RowDelete>,
}

impl GridWrite {
    /// No staged changes at all.
    pub fn is_empty(&self) -> bool {
        self.updates.is_empty() && self.inserts.is_empty() && self.deletes.is_empty()
    }

    /// The batch's statements in the order they must execute: **deletes →
    /// updates → inserts**.
    ///
    /// Deletes run first so "delete a row, then insert one carrying the same
    /// unique key" works. Every engine's executor iterates this one plan, so the
    /// order can't drift between them — and it is assertable without a server.
    pub fn plan(&self) -> Vec<WriteStep<'_>> {
        let dels = self.deletes.iter().map(WriteStep::Delete);
        let upds = self.updates.iter().map(WriteStep::Update);
        let inss = self.inserts.iter().map(WriteStep::Insert);
        dels.chain(upds).chain(inss).collect()
    }
}

/// One statement of a [`GridWrite`], as [`GridWrite::plan`] orders them.
#[derive(Clone, Copy, Debug)]
pub enum WriteStep<'a> {
    Delete(&'a RowDelete),
    Update(&'a RowEdit),
    Insert(&'a RowInsert),
}

impl WriteStep<'_> {
    /// How the statement is named in the 1-row guard's error message.
    pub fn action(&self) -> &'static str {
        match self {
            WriteStep::Delete(_) => "delete on",
            WriteStep::Update(_) => "update on",
            WriteStep::Insert(_) => "insert into",
        }
    }

    pub fn database(&self) -> &str {
        match self {
            WriteStep::Delete(d) => &d.database,
            WriteStep::Update(e) => &e.database,
            WriteStep::Insert(i) => &i.database,
        }
    }

    pub fn schema(&self) -> Option<&str> {
        match self {
            WriteStep::Delete(d) => d.schema.as_deref(),
            WriteStep::Update(e) => e.schema.as_deref(),
            WriteStep::Insert(i) => i.schema.as_deref(),
        }
    }

    pub fn table(&self) -> &str {
        match self {
            WriteStep::Delete(d) => &d.table,
            WriteStep::Update(e) => &e.table,
            WriteStep::Insert(i) => &i.table,
        }
    }

    /// `db.table`, or `db.schema.table` on an engine that has namespaces — for
    /// the error message only, so it is deliberately unquoted.
    fn qualified(&self) -> String {
        match self.schema() {
            Some(s) => format!("{}.{}.{}", self.database(), s, self.table()),
            None => format!("{}.{}", self.database(), self.table()),
        }
    }
}

/// The 1-row write-back safety net, as a decision: a staged statement must
/// affect **exactly one** row, and anything else fails the whole batch.
///
/// This is what stands between an over-optimistic [`crate::edit::analyze_edit`]
/// and a corrupted table. `0` means the WHERE key matched nothing (the row moved
/// or was already gone); `2` or more means the key wasn't unique, and applying
/// the edit would have silently rewritten rows the user never saw. Both roll the
/// batch back.
///
/// Pure so both engines share one verdict *and* one message — it used to be
/// written inline in each executor, with two divergent wordings and no test.
/// `Err` carries the message; the caller wraps it in its own error type.
/// The message states what the guard saw and stops there: it is reached
/// *before* the rollback runs, so it can't know what the rollback achieved.
/// The caller appends [`Rollback::note`] once it does.
pub fn one_row_verdict(step: WriteStep<'_>, affected: u64) -> Result<(), String> {
    if affected == 1 {
        return Ok(());
    }
    Err(format!(
        "{} {} affected {affected} rows (expected exactly 1)",
        step.action(),
        step.qualified(),
    ))
}

/// What a write path's rollback actually achieved.
///
/// The write paths open a transaction and roll it back on any failure, and their
/// errors said so unconditionally. On MySQL that is a promise the engine may not
/// keep: `MyISAM`, `MEMORY`, `ARCHIVE` and `CSV` ignore `BEGIN`/`ROLLBACK`
/// entirely. `ROLLBACK` still *succeeds* — it raises warning 1196, *"Some
/// non-transactional changed tables couldn't be rolled back"* — so a failed
/// import of 100k rows reported "rolled back the whole import" over 50k rows
/// that are permanently in the table. The user re-runs it and now has 50k
/// duplicates, or a key collision that aborts the retry too, with the table in
/// a state neither run describes.
///
/// PostgreSQL is fully transactional, so it is always [`Rollback::Complete`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rollback {
    /// Every statement in the batch was undone.
    Complete,
    /// The server reported it couldn't undo everything (MySQL warning 1196), or
    /// the rollback itself failed — either way, what was written is still there.
    Incomplete,
}

impl Rollback {
    /// The clause appended to a write-path error, describing what survived.
    /// Starts with its own separator so it reads as one sentence after a
    /// [`one_row_verdict`] message.
    pub fn note(self) -> &'static str {
        match self {
            Rollback::Complete => " — rolled back all changes",
            Rollback::Incomplete => {
                " — the rollback did NOT undo them: this table's storage engine is \
                 not transactional, so the rows already written remain. Check the \
                 table before retrying."
            }
        }
    }
}

/// Can a table on this MySQL storage engine honour a `ROLLBACK`?
///
/// Only used to decide whether atomicity may be *promised*, so an engine this
/// doesn't recognise — including an empty string, which is what an unread
/// catalogue gives — resolves to `false`. Same rule as `ddl::pg_replaceable`:
/// uncertainty goes to the side that doesn't claim more than it knows.
///
/// MariaDB's `Aria` is deliberately absent: it is crash-safe, not
/// transactional, and only its (non-default) transactional variant rolls back.
pub fn engine_is_transactional(engine: &str) -> bool {
    matches!(
        engine.trim().to_ascii_lowercase().as_str(),
        "innodb" | "ndbcluster" | "ndb" | "rocksdb" | "tokudb" | "myrocks"
    )
}

/// The grid's staged (green) cell edits: `(data row, result column)` → the new
/// value, `None` meaning SQL `NULL`. Staged is *not* written — a commit is what
/// turns these into [`RowEdit`]s.
pub type StagedEdits = HashMap<(usize, usize), Option<String>>;

/// Drop exactly the staged edits a completed commit covered, leaving every other
/// one staged.
///
/// The grid used to clear the whole map after any commit. That is right for the
/// staged batch (which *is* the whole map) and wrong for the row panel, which
/// commits on its own path — saving one row there discarded every green cell edit
/// elsewhere in the grid, unwritten and unannounced, with the change counter
/// dropping to zero as if they had been committed. It also swallowed an edit
/// staged while a commit was in flight.
///
/// `committed` is the key set the write was assembled from, so an edit that
/// arrived after that snapshot survives — which is the whole point. Clearing too
/// *little* would leave a written cell painted green, so callers must pass every
/// key their write covered, not only the ones that produced SQL.
pub fn drop_committed(staged: &mut StagedEdits, committed: &HashSet<(usize, usize)>) {
    staged.retain(|k, _| !committed.contains(k));
}

/// Resolve what a person typed into the grid's **go to row** box against a grid
/// currently showing `total` rows, as a 0-based *display* index — or `None` when
/// there is nothing to go to.
///
/// The counterpart of [`crate::text_ops::offset_of_line`], which does this for the
/// editor's go-to-line popup — but it deliberately **does not** share that one's
/// contract. A number outside the grid clamps to the nearest end rather than
/// resolving to nothing: past the last row goes to the last row, and `0` goes to
/// the first. A row of 9s is how people ask for the bottom of a long result, and
/// a silent no-op there is indistinguishable from a feature that doesn't work.
/// Overshooting is cheap to recover from — the gutter number and the row
/// highlight say plainly where you landed — where a jump that does nothing tells
/// you nothing at all.
///
/// `None` is left for the two cases where no row can be meant: an empty grid, and
/// input that isn't a number.
///
/// Display index, not data row: the grid numbers its gutter by display position,
/// so "row 40" means the fortieth row *as sorted on screen*, which is the only
/// reading that matches what the user is looking at. `total` is therefore the
/// count of **numbered** rows: a pending unsaved row's gutter reads `*`, not a
/// number, so counting them in made "row 101" land on a row showing no number,
/// and made the clamp stop one short of the last one that does.
///
/// The forms it accepts are exactly the forms the grid **prints**, so a number
/// read off the screen and typed back in works:
///
/// - plain digits, `148203`;
/// - the compact counts of the stats line — `200k`, `1.25k`, `3m`, `1b` — which
///   is the whole reason this isn't a `parse::<usize>()`. `200k` is what the
///   grid says a big result holds, and typing it back returned `None`: nothing
///   moved and nothing was said, which is indistinguishable from a broken
///   feature. A decimal point means something only against one of those
///   suffixes; a bare `1.25` is not a row number;
/// - digit-group separators (`148,203`, `148 203`, `148_203`), which the app
///   itself never writes — the stats line is compact, the gutter and the find
///   readout unformatted — but which come in from anywhere a count is copied.
///
/// Nothing else is stripped: a stray letter is still a miss rather than being
/// quietly filtered into a number the user never asked for.
pub fn goto_row_index(input: &str, total: usize) -> Option<usize> {
    if total == 0 {
        return None;
    }
    let mut s = String::with_capacity(input.len());
    for c in input.trim().chars() {
        match c {
            ',' | '_' | ' ' | '\u{202f}' | '\u{a0}' => continue,
            _ => s.push(c.to_ascii_lowercase()),
        }
    }
    let (digits, mult): (&str, u128) = match s.as_bytes().last() {
        Some(b'k') => (&s[..s.len() - 1], 1_000),
        Some(b'm') => (&s[..s.len() - 1], 1_000_000),
        Some(b'b') => (&s[..s.len() - 1], 1_000_000_000),
        _ => (&s[..], 1),
    };
    let (int_part, frac_part) = match digits.split_once('.') {
        // Only a suffix gives a fraction a meaning: `1.25k` is 1250 rows, a bare
        // `1.25` is not a row number at all.
        Some(_) if mult == 1 => return None,
        Some((i, f)) => (i, f),
        None => (digits, ""),
    };
    if (int_part.is_empty() && frac_part.is_empty())
        || !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    // Fixed-point rather than an `f64`, so `1.25k` is exactly 1250 and a long
    // fraction truncates rather than rounding to a row the user didn't name.
    let scale = 10u128.checked_pow(frac_part.len() as u32)?;
    // All digits but too wide for the machine is still just "past the end" — it
    // must clamp with every other overshoot, not fall through to the miss above.
    let int = int_part.parse::<u128>().unwrap_or(u128::MAX);
    let frac = frac_part.parse::<u128>().unwrap_or(0);
    let n = int
        .saturating_mul(mult)
        .saturating_add(frac.saturating_mul(mult) / scale);
    let n = usize::try_from(n).unwrap_or(usize::MAX);
    Some(n.clamp(1, total) - 1)
}

/// The selection a **whole-row** gesture makes: anchor at column 0, active at the
/// last column, in display coordinates.
///
/// Shared by the grid's gutter click and the Ctrl+G jump, which have to stay
/// bug-for-bug identical — the jump exists to land you where a click would, so
/// the row lights up the way the user already knows. It also decides how the
/// **aggregates bar** reads the result: its whole-row arm keys on a span covering
/// every column, so a divergence here would quietly start summing the id column.
///
/// A result with no columns at all saturates rather than underflowing into a
/// huge index.
pub fn row_selection(row: usize, ncols: usize) -> ((usize, usize), (usize, usize)) {
    ((row, 0), (row, ncols.saturating_sub(1)))
}

/// The selection a whole-row **drag** makes: every column, from the row the drag
/// started on to the row under the pointer.
///
/// The anchor keeps the starting row even when the drag runs upwards — the
/// rectangle is normalised downstream (`bounds`), and keeping the anchor where
/// the gesture began is what lets a drag reverse without the selection jumping.
pub fn row_range_selection(
    anchor_row: usize,
    active_row: usize,
    ncols: usize,
) -> ((usize, usize), (usize, usize)) {
    ((anchor_row, 0), (active_row, ncols.saturating_sub(1)))
}

/// How a grid selection names itself in the "Attach … to chat" action — the
/// phrase between `Attach` and `to chat`.
///
/// The label is the consent notice for a gesture that sends data off the
/// machine, so it has to describe what is actually going: a lone cell is one
/// *column* of one row, not "1 row" (which reads as the whole row), and a
/// selection covering every column **is** whole rows, where naming the columns
/// as well is noise. Between those, both halves matter and both are said.
pub fn attach_scope_label(rows: usize, cols: usize, ncols: usize) -> String {
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    // Every column selected → whole rows, however many.
    if ncols > 0 && cols >= ncols {
        return plural(rows, "row");
    }
    if rows == 1 {
        return plural(cols, "column");
    }
    format!("{} and {}", plural(rows, "row"), plural(cols, "column"))
}

/// Where a Go-to-row jump lands: the selection to make, and the column to scroll
/// to. `None` when the input names no row — see [`goto_row_index`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GotoTarget {
    pub anchor: (usize, usize),
    pub active: (usize, usize),
    /// **0, not the active cell's column.** Following the active cell would fling
    /// the viewport to the far right of a wide result on every jump — the one
    /// line that stops a row jump from also being a horizontal one.
    pub scroll_col: usize,
}

/// Resolve the grid's **go to row** box: what to select and where to scroll.
///
/// `total` is the count of rows the gutter *numbers*, and `ncols` the result's
/// column count. The three decisions the grid used to make inline around
/// [`goto_row_index`] — which row, the landing gesture, and the scroll column —
/// are here together so they can be tested as one and so `grid_view` is a
/// wrapper rather than a place where any of them can quietly change.
pub fn goto_target(input: &str, total: usize, ncols: usize) -> Option<GotoTarget> {
    let row = goto_row_index(input, total)?;
    let (anchor, active) = row_selection(row, ncols);
    Some(GotoTarget {
        anchor,
        active,
        scroll_col: 0,
    })
}

/// A template for re-`SELECT`ing just-edited rows so the grid can splice DB
/// truth back in without re-running the whole query (built by
/// [`crate::edit::refetch_template`]). Only produced when the result is a single
/// base table with every column having a real origin, so `SELECT <real cols> …`
/// reproduces the row 1:1.
#[derive(Clone, Debug)]
pub struct RefetchTemplate {
    pub database: String,
    /// PostgreSQL namespace of `table` — see [`RowEdit::schema`].
    pub schema: Option<String>,
    pub table: String,
    /// Real column name for every result column, in result-column order.
    pub columns: Vec<String>,
    /// Indices into `columns` forming the row-identity `WHERE` key.
    pub key_cols: Vec<usize>,
}

/// One row to re-fetch: the grid data-row to splice back into, plus that row's
/// *post-edit* key values (aligned to [`RefetchTemplate::key_cols`]).
#[derive(Clone, Debug)]
pub struct RefetchRow {
    pub data_row: usize,
    pub key: Vec<Value>,
}

/// A full re-fetch request handed to the commit path alongside the edits.
#[derive(Clone, Debug)]
pub struct RefetchRequest {
    pub template: RefetchTemplate,
    pub rows: Vec<RefetchRow>,
}

/// Outcome of a commit, delivered back to the grid on the UI thread.
#[derive(Clone, Debug)]
pub enum CommitDone {
    /// Splice these fresh rows in place — `(data_row, new cell values)`, the
    /// values aligned to the result columns. The grid overwrites those rows and
    /// clears its staged edits, preserving scroll/selection.
    Spliced(Vec<(usize, Vec<Value>)>),
    /// The whole query was re-run instead (not spliceable, or the re-fetch
    /// failed) — the grid is being rebuilt from fresh results, so it does nothing.
    FullReran,
    /// The commit failed; the message is shown and the staged edits are kept.
    Failed(String),
}

/// UI-facing lifecycle of a query in a tab. Shared between the app (writer)
/// and the UI (reader) through a Floem signal.
#[derive(Clone, Debug)]
pub enum QueryState {
    /// No query has run in this tab yet.
    Idle,
    Running,
    Loaded(Arc<ResultSet>),
    Failed(String),
    /// The query was cancelled by the user.
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(type_name: &str) -> Column {
        Column {
            name: "c".to_string(),
            type_name: type_name.to_string(),
            origin: None,
        }
    }

    // ── Chunked loads (the streamed export's builder) ──

    fn named(name: &str, type_name: &str) -> Column {
        Column {
            name: name.to_string(),
            type_name: type_name.to_string(),
            origin: None,
        }
    }

    /// The happy path: a chunk carries the rows pushed since the last one, and
    /// the builder keeps going over the **same columns** — which is what lets an
    /// export take its header off whichever block arrives first.
    #[test]
    fn take_chunk_cuts_the_rows_so_far_and_carries_the_columns_on() {
        let cols = vec![named("id", "INT"), named("name", "TEXT")];
        let mut b = ResultBuilder::with_capacity(cols, 2);
        b.push_row(&[Value::Int(1), Value::Str("a".into())]);
        b.push_row(&[Value::Int(2), Value::Str("b".into())]);

        let first = b.take_chunk(2);
        assert_eq!(first.row_count(), 2);
        assert!(
            first
                .columns
                .iter()
                .map(|c| c.name.as_str())
                .eq(["id", "name"]),
            "the chunk must name its columns"
        );
        assert_eq!(first.cell(0, 1).unwrap().display(), "a");

        // The builder is empty but still typed — not a fresh untyped one.
        assert_eq!(b.row_count(), 0);
        assert!(
            b.columns()
                .iter()
                .map(|c| c.name.as_str())
                .eq(["id", "name"])
        );

        b.push_row(&[Value::Int(3), Value::Str("c".into())]);
        let second = b.take_chunk(0);
        assert_eq!(second.row_count(), 1);
        assert_eq!(second.cell(0, 0).unwrap().display(), "3");
        assert!(
            second
                .columns
                .iter()
                .map(|c| c.name.as_str())
                .eq(["id", "name"])
        );
    }

    /// **A chunk is never truncated and carries no elapsed time**, because
    /// neither is a fact about it. `truncated` says a row cap cut the read short,
    /// and a stream has no cap; a per-chunk duration would be a fraction of the
    /// load presented as the whole of one. A chunk that inherited `truncated`
    /// would put "showing N of ~M" on an export that is complete.
    #[test]
    fn a_chunk_claims_neither_truncation_nor_the_loads_elapsed_time() {
        let mut b = ResultBuilder::new(vec![named("id", "INT")]);
        b.set_truncated(true);
        b.set_elapsed(1234);
        b.push_row(&[Value::Int(1)]);

        let chunk = b.take_chunk(0);
        assert!(!chunk.truncated, "a chunk of a stream is not truncated");
        assert_eq!(
            chunk.elapsed_ms, 0,
            "the duration belongs to the whole load"
        );

        // And the flags do not survive into the successor either — the next
        // chunk starts as clean as this one ended.
        b.push_row(&[Value::Int(2)]);
        let next = b.take_chunk(0);
        assert!(!next.truncated);
        assert_eq!(next.elapsed_ms, 0);
    }

    /// The edge the export actually hits: a load whose rows divide evenly by the
    /// chunk size ends on an **empty** chunk, and that chunk still has to be a
    /// real result with the columns on it.
    #[test]
    fn taking_a_chunk_with_nothing_in_it_still_yields_the_columns() {
        let mut b = ResultBuilder::new(vec![named("id", "INT"), named("name", "TEXT")]);
        let empty = b.take_chunk(0);
        assert_eq!(empty.row_count(), 0);
        assert!(
            empty
                .columns
                .iter()
                .map(|c| c.name.as_str())
                .eq(["id", "name"])
        );
        assert!(
            empty.affected.is_none(),
            "a chunk is a grid, not a DML outcome"
        );
        assert!(empty.capped_columns.is_empty());
    }

    /// `capped_columns` is per chunk, like every other property of one: a column
    /// that hit its arena ceiling in an earlier block must not mark a later block
    /// that never came near it, or the UI would report blank cells in a chunk
    /// whose cells are all there.
    #[test]
    fn capped_columns_are_not_inherited_by_the_next_chunk() {
        let mut b = ResultBuilder::new(vec![named("id", "INT")]);
        b.push_row(&[Value::Int(1)]);
        let first = b.take_chunk(0);
        assert!(first.capped_columns.is_empty());
        b.push_row(&[Value::Int(2)]);
        assert!(b.take_chunk(0).capped_columns.is_empty());
    }

    // ── Raw-bytes columns ──

    #[test]
    fn type_is_binary_covers_all_three_engines_spellings() {
        for t in [
            "BLOB",
            "TINYBLOB",
            "MEDIUMBLOB",
            "LONGBLOB",
            "BINARY",
            "VARBINARY",
            "BYTEA",
            "GEOMETRY",
        ] {
            assert!(type_is_binary(t), "{t} should be binary");
        }
    }

    /// The leading-token rule, for the same reason `is_numeric` has it: a
    /// parameter list is not part of the type's name, and a substring match
    /// would make `BLOB` out of nothing at all.
    #[test]
    fn type_is_binary_reads_the_leading_token_and_ignores_case() {
        assert!(type_is_binary("varbinary(255)"));
        assert!(type_is_binary("blob"));
    }

    /// **A bit-field is not a blob**, however it arrives. MySQL sends `BIT(M)` as
    /// raw bytes, which is what put it on the binary list — and being on it meant
    /// a `BIT(8)` holding 10 rendered as `<1 bytes>` and was *withheld* from the
    /// CSV and JSON exports, the two formats this app reads back. It has a
    /// lossless text form, so it is carried like any other value.
    #[test]
    fn a_bit_field_is_not_a_binary_column() {
        for t in ["BIT", "bit(1)", "BIT(64)", "bit varying(8)", "VARBIT"] {
            assert!(!type_is_binary(t), "{t} is a bit-field, not raw bytes");
            assert!(type_is_bit(t), "{t} should read as a bit-field");
        }
        // And the name is a leading token here too: nothing else starts with it.
        assert!(!type_is_bit("BIGINT"));
        assert!(!type_is_bit("bitcoin_balance"));
    }

    /// The number MySQL sent, in the byte order it sent it, which is the form it
    /// takes back (`SET b = 10` assigns a `BIT(8)`).
    #[test]
    fn a_bit_fields_bytes_read_as_a_big_endian_number() {
        assert_eq!(bit_display(&[0x0A]), "10");
        assert_eq!(bit_display(&[0x01, 0x00]), "256", "big-endian, not little");
        assert_eq!(bit_display(&[]), "0");
        assert_eq!(bit_display(&[0x00]), "0");
        // A full 64-bit field, the widest `BIT(M)` there is.
        assert_eq!(bit_display(&[0xFF; 8]), u64::MAX.to_string());
        // Wider than any bit-field: the **low** eight bytes, so a value too wide
        // to represent loses its high end rather than the whole answer to an
        // overflow. Nine bytes here, and the leading `0x01` is the one dropped.
        assert_eq!(
            bit_display(&[0x01, 0x02, 0, 0, 0, 0, 0, 0, 0x03]),
            0x0200_0000_0000_0003_u64.to_string()
        );
    }

    #[test]
    fn type_is_binary_rejects_text_and_numeric_columns() {
        for t in [
            "VARCHAR(255)",
            "TEXT",
            "LONGTEXT",
            "INT",
            "JSON",
            "",
            "BITS",
        ] {
            assert!(!type_is_binary(t), "{t} should not be binary");
        }
    }

    /// The placeholder and its recognizer are a pair — a change to one that
    /// does not change the other silently stops every export from noticing a
    /// dropped blob.
    #[test]
    fn a_binary_display_is_recognised_as_one() {
        for len in [0usize, 1, 4096, 40_000_000] {
            let text = binary_display(len);
            assert!(is_binary_display(&text), "{text}");
        }
    }

    /// The recognizer decides whether an export replaces a cell with NULL, so
    /// a false positive is data loss. Nothing but the exact shape passes.
    #[test]
    fn text_that_merely_resembles_the_placeholder_is_not_one() {
        for s in [
            "",
            "<bytes>",
            "< bytes>",
            "<4096 bytes",
            "4096 bytes>",
            "<4 kb>",
            "<4096 bytes> and more",
            "about <4096 bytes>",
            "<-1 bytes>",
            "<4 096 bytes>",
        ] {
            assert!(
                !is_binary_display(s),
                "{s:?} should not read as a placeholder"
            );
        }
    }

    /// The gap this closed: `ColumnOrigin::binary` is the authoritative answer
    /// but only exists for a table-backed column, so a PostgreSQL `bytea` whose
    /// catalog lookup found no provenance reached every caller as ordinary text.
    #[test]
    fn a_binary_column_without_provenance_is_still_binary() {
        assert!(col("BYTEA").is_binary());
        assert!(col("BLOB").is_binary());
        assert!(!col("TEXT").is_binary());
    }

    /// And the other direction: the wire flag wins even where the type name is
    /// one this crate does not recognise (a MySQL `VECTOR`, a custom domain).
    #[test]
    fn the_wire_flag_alone_makes_a_column_binary() {
        let mut c = sourced("v", "db", None, "t", "v");
        c.type_name = "SOMETHING_NEW".to_string();
        assert!(!c.is_binary());
        if let Some(o) = c.origin.as_mut() {
            o.binary = true;
        }
        assert!(c.is_binary());
    }

    // ── Result-column provenance (`origin_columns`) ──

    /// A result column displayed as `shown`, really `db[.schema].table.real`.
    fn sourced(shown: &str, db: &str, schema: Option<&str>, table: &str, real: &str) -> Column {
        Column {
            name: shown.to_string(),
            type_name: "int".to_string(),
            origin: Some(ColumnOrigin {
                database: db.to_string(),
                schema: schema.map(str::to_string),
                table: table.to_string(),
                column: real.to_string(),
                flags: ColumnFlags::default(),
                binary: false,
                implicit_key: false,
            }),
        }
    }

    fn rs_of(columns: Vec<Column>) -> ResultSet {
        ResultSet::from_rows(columns, Vec::new())
    }

    /// The bug: a tab opened from `customers` keeps its source when the user runs
    /// a different query in it, so matching by name gave `orders.customerNumber`
    /// the customers primary-key icon and its saved formatter.
    #[test]
    fn origin_columns_ignores_a_same_named_column_from_another_table() {
        let rs = rs_of(vec![
            sourced("orderNumber", "shop", None, "orders", "orderNumber"),
            sourced("customerNumber", "shop", None, "orders", "customerNumber"),
        ]);
        assert!(
            rs.origin_columns("shop", None, "customers").is_empty(),
            "nothing in this result came from customers"
        );
        assert_eq!(rs.origin_columns("shop", None, "orders").len(), 2);
    }

    #[test]
    fn origin_columns_sees_through_an_alias() {
        let rs = rs_of(vec![sourced("ts", "shop", None, "customers", "created_at")]);
        let map = rs.origin_columns("shop", None, "customers");
        assert_eq!(map.get("created_at"), Some(&0));
        assert_eq!(map.get("ts"), None, "the display name is not the identity");
    }

    /// An expression column has no origin at all: `SELECT 1 AS customerNumber`
    /// used to earn the gold key.
    #[test]
    fn origin_columns_skips_a_column_with_no_provenance() {
        let rs = rs_of(vec![col("int"), sourced("id", "shop", None, "t", "id")]);
        let map = rs.origin_columns("shop", None, "t");
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("id"), Some(&1));
    }

    /// The namespace is part of the table's identity — the case `build_follow_specs`
    /// already guarded and the icon map didn't.
    #[test]
    fn origin_columns_separates_two_schemas_with_the_same_table_name() {
        let rs = rs_of(vec![
            sourced("id", "app", Some("public"), "orders", "id"),
            sourced("sales_id", "app", Some("sales"), "orders", "id"),
        ]);
        assert_eq!(
            rs.origin_columns("app", Some("public"), "orders").get("id"),
            Some(&0)
        );
        assert_eq!(
            rs.origin_columns("app", Some("sales"), "orders").get("id"),
            Some(&1)
        );
        assert!(rs.origin_columns("app", None, "orders").is_empty());
    }

    #[test]
    fn origin_columns_takes_the_leftmost_of_a_repeated_column() {
        let rs = rs_of(vec![
            sourced("a", "shop", None, "t", "id"),
            sourced("b", "shop", None, "t", "id"),
        ]);
        assert_eq!(rs.origin_columns("shop", None, "t").get("id"), Some(&0));
    }

    #[test]
    fn origin_columns_separates_two_databases() {
        let rs = rs_of(vec![sourced("id", "shop", None, "t", "id")]);
        assert!(rs.origin_columns("other", None, "t").is_empty());
    }

    // ── Clearing the staged map after a commit (`drop_committed`) ──

    fn staged(keys: &[(usize, usize)]) -> StagedEdits {
        keys.iter()
            .map(|&k| (k, Some(format!("v{}{}", k.0, k.1))))
            .collect()
    }

    #[test]
    fn a_commit_drops_only_the_keys_it_covered() {
        // Row 5 has two green edits; the row panel saves row 2 on its own path.
        let mut s = staged(&[(5, 0), (5, 1), (2, 3)]);
        drop_committed(&mut s, &[(2, 3)].into_iter().collect());
        assert_eq!(s.len(), 2, "row 5's staged edits are still unwritten");
        assert!(s.contains_key(&(5, 0)));
        assert!(s.contains_key(&(5, 1)));
        assert!(!s.contains_key(&(2, 3)));
    }

    #[test]
    fn committing_the_whole_staged_batch_empties_it() {
        let mut s = staged(&[(0, 0), (1, 1), (2, 2)]);
        let all: HashSet<_> = s.keys().copied().collect();
        drop_committed(&mut s, &all);
        assert!(s.is_empty(), "no committed cell may stay painted green");
    }

    #[test]
    fn an_edit_staged_after_the_write_was_assembled_survives_it() {
        // The commit snapshotted {(1,1)}; the user staged (4,2) during the
        // round-trip. Clearing wholesale sent it nowhere and said nothing.
        let committed: HashSet<_> = [(1, 1)].into_iter().collect();
        let mut s = staged(&[(1, 1), (4, 2)]);
        drop_committed(&mut s, &committed);
        assert_eq!(s.keys().copied().collect::<Vec<_>>(), vec![(4, 2)]);
    }

    #[test]
    fn dropping_nothing_and_dropping_from_nothing_are_both_no_ops() {
        let mut s = staged(&[(0, 0)]);
        drop_committed(&mut s, &HashSet::new());
        assert_eq!(s.len(), 1);

        let mut empty = StagedEdits::new();
        drop_committed(&mut empty, &[(0, 0)].into_iter().collect());
        assert!(empty.is_empty());
    }

    #[test]
    fn value_is_null_only_for_null() {
        assert!(Value::Null.is_null());
        assert!(!Value::Int(0).is_null());
        assert!(!Value::UInt(0).is_null());
        assert!(!Value::Float(0.0).is_null());
        assert!(!Value::Str(String::new()).is_null());
    }

    #[test]
    fn value_display_covers_every_variant() {
        assert_eq!(Value::Null.display(), "NULL");
        assert_eq!(Value::Int(-42).display(), "-42");
        assert_eq!(Value::UInt(42).display(), "42");
        assert_eq!(Value::Str("hi".to_string()).display(), "hi");
        // Float uses f64::to_string — integral floats print without a fraction.
        assert_eq!(Value::Float(1.5).display(), "1.5");
        assert_eq!(Value::Float(2.0).display(), "2");
    }

    #[test]
    fn is_numeric_matches_numeric_types_case_insensitively() {
        for t in [
            "INT",
            "int",
            "BIGINT",
            "tinyint(1)",
            "DECIMAL(10,2)",
            "numeric",
            "FLOAT",
            "double",
            "YEAR",
            "BIT",
            "MEDIUMINT UNSIGNED",
            // PostgreSQL spellings, and MySQL's parameter/modifier suffixes.
            "integer",
            "double precision",
            "real",
            "int(11) unsigned zerofill",
            "numeric(10,2)",
        ] {
            assert!(col(t).is_numeric(), "{t} should be numeric");
        }
    }

    #[test]
    fn is_numeric_rejects_non_numeric_types() {
        for t in [
            "VARCHAR(255)",
            "TEXT",
            "DATETIME",
            "JSON",
            "BLOB",
            "ENUM('a')",
            // The adversarial cases: each of these *contains* a numeric keyword,
            // which is what the old substring match matched on. The previous
            // negative list held six types none of which could collide, so it
            // could not fail for the reason the function was wrong.
            "POINT",
            "MULTIPOINT",
            "INTERVAL",
            "GEOMETRY",
            "TIMESTAMP",
            "integer[]",
        ] {
            assert!(!col(t).is_numeric(), "{t} should not be numeric");
        }
    }

    #[test]
    fn gridwrite_is_empty_tracks_all_three_buckets() {
        let mut w = GridWrite::default();
        assert!(w.is_empty());
        w.updates.push(RowEdit {
            database: "d".to_string(),
            schema: None,
            table: "t".to_string(),
            set: vec![],
            key: vec![],
        });
        assert!(!w.is_empty());

        let mut w = GridWrite::default();
        w.inserts.push(RowInsert {
            database: "d".to_string(),
            schema: None,
            table: "t".to_string(),
            cols: vec![],
        });
        assert!(!w.is_empty());

        let mut w = GridWrite::default();
        w.deletes.push(RowDelete {
            database: "d".to_string(),
            schema: None,
            table: "t".to_string(),
            key: vec![],
        });
        assert!(!w.is_empty());
    }

    #[test]
    fn resultset_counts_reflect_dimensions() {
        let rs = ResultSet::from_rows(
            vec![col("INT"), col("TEXT")],
            vec![
                vec![Value::Int(1), Value::Str("a".to_string())],
                vec![Value::Int(2), Value::Str("b".to_string())],
                vec![Value::Int(3), Value::Str("c".to_string())],
            ],
        );
        assert_eq!(rs.row_count(), 3);
        assert_eq!(rs.col_count(), 2);

        let empty = ResultSet::default();
        assert_eq!(empty.row_count(), 0);
        assert_eq!(empty.col_count(), 0);
    }

    #[test]
    fn columnar_cell_roundtrips_every_variant() {
        let rs = ResultSet::from_rows(
            vec![col("INT"), col("BIGINT"), col("DOUBLE"), col("TEXT")],
            vec![
                vec![
                    Value::Int(-42),
                    Value::UInt(18_446_744_073_709_551_615),
                    Value::Float(1.5),
                    Value::Str("héllo".to_string()),
                ],
                vec![Value::Null, Value::Null, Value::Null, Value::Null],
            ],
        );
        // Tags survive; text is the canonical display; to_value round-trips.
        let c = rs.cell(0, 0).unwrap();
        assert_eq!(c.tag, CellTag::Int);
        assert_eq!(c.display(), "-42");
        assert!(matches!(c.to_value(), Value::Int(-42)));

        let c = rs.cell(0, 1).unwrap();
        assert_eq!(c.tag, CellTag::UInt);
        assert!(matches!(
            c.to_value(),
            Value::UInt(18_446_744_073_709_551_615)
        ));

        let c = rs.cell(0, 2).unwrap();
        assert_eq!(c.tag, CellTag::Float);
        assert!(matches!(c.to_value(), Value::Float(f) if f == 1.5));

        // Multi-byte UTF-8 slices at the right arena boundary.
        let c = rs.cell(0, 3).unwrap();
        assert_eq!(c.tag, CellTag::Str);
        assert_eq!(c.display(), "héllo");
        assert!(matches!(c.to_value(), Value::Str(s) if s == "héllo"));

        // NULL: dim `NULL` for display, empty raw text, `Value::Null` typed.
        let c = rs.cell(1, 0).unwrap();
        assert!(c.is_null());
        assert_eq!(c.display(), "NULL");
        assert_eq!(c.text(), "");
        assert!(c.to_value().is_null());
    }

    #[test]
    fn packed_tag_does_not_corrupt_offsets() {
        // Many cells of differing lengths: the tag packed into the high bits must
        // not disturb the low-29-bit offsets, so every cell slices back exactly —
        // including across a tag change (Str → Int) mid-column.
        let rs = ResultSet::from_rows(
            vec![col("MIXED")],
            vec![
                vec![Value::Str("a".to_string())],
                vec![Value::Str("bbbbbbbbbb".to_string())],
                vec![Value::Null],
                vec![Value::Str(String::new())],
                vec![Value::Int(1234567890)],
                vec![Value::Str("tail".to_string())],
            ],
        );
        assert_eq!(rs.cell(0, 0).unwrap().display(), "a");
        assert_eq!(rs.cell(1, 0).unwrap().display(), "bbbbbbbbbb");
        assert!(rs.cell(2, 0).unwrap().is_null());
        assert_eq!(rs.cell(3, 0).unwrap().display(), "");
        assert!(!rs.cell(3, 0).unwrap().is_null());
        assert_eq!(rs.cell(4, 0).unwrap().display(), "1234567890");
        assert_eq!(rs.cell(4, 0).unwrap().tag, CellTag::Int);
        assert_eq!(rs.cell(5, 0).unwrap().display(), "tail");
    }

    #[test]
    fn columnar_cell_out_of_range_is_none() {
        let rs = ResultSet::from_rows(vec![col("INT")], vec![vec![Value::Int(1)]]);
        assert!(rs.cell(0, 0).is_some());
        assert!(rs.cell(1, 0).is_none()); // row past end
        assert!(rs.cell(0, 1).is_none()); // col past end
    }

    #[test]
    fn empty_string_cell_is_distinct_from_null() {
        // Both have an empty arena span; the tag keeps them apart.
        let rs = ResultSet::from_rows(
            vec![col("TEXT"), col("TEXT")],
            vec![vec![Value::Str(String::new()), Value::Null]],
        );
        let empty = rs.cell(0, 0).unwrap();
        assert!(!empty.is_null());
        assert_eq!(empty.display(), "");
        let null = rs.cell(0, 1).unwrap();
        assert!(null.is_null());
        assert_eq!(null.display(), "NULL");
    }

    #[test]
    fn short_row_is_padded_with_null() {
        let rs = ResultSet::from_rows(
            vec![col("INT"), col("TEXT")],
            vec![vec![Value::Int(7)]], // only one cell for a two-column result
        );
        assert!(matches!(rs.cell(0, 0).unwrap().to_value(), Value::Int(7)));
        assert!(rs.cell(0, 1).unwrap().is_null());
    }

    #[test]
    fn splice_rows_leaves_an_unchanged_column_untouched() {
        // The post-commit re-fetch returns the *whole* row, so a one-cell edit
        // hands back 49 identical columns and one changed one. Rebuilding the
        // identical ones is the cost this skips — asserted by the buffer's
        // identity (its allocation is not replaced), not just its contents.
        let mut rs = ResultSet::from_rows(
            vec![col("INT"), col("TEXT")],
            vec![
                vec![Value::Int(1), Value::Str("a".to_string())],
                vec![Value::Int(2), Value::Str("b".to_string())],
            ],
        );
        let untouched_before = rs.cols[0].arena.as_ptr();
        let changed_before = rs.cols[1].arena.as_ptr();
        // Column 0's value is unchanged; only the text column moves.
        rs.splice_rows(&[(1, vec![Value::Int(2), Value::Str("B".to_string())])]);
        assert_eq!(
            rs.cols[0].arena.as_ptr(),
            untouched_before,
            "an unchanged column must not be rebuilt"
        );
        assert_ne!(
            rs.cols[1].arena.as_ptr(),
            changed_before,
            "the changed column is rebuilt"
        );
        // …and the data is still right.
        assert!(matches!(rs.cell(1, 0).unwrap().to_value(), Value::Int(2)));
        assert_eq!(rs.cell(1, 1).unwrap().display(), "B");
        assert_eq!(rs.cell(0, 1).unwrap().display(), "a");
    }

    #[test]
    fn splicing_a_shared_result_set_does_not_copy_the_untouched_columns() {
        // The grid's `rs` signal and the tab's canonical `QueryState::Loaded`
        // hold the *same* `Arc`, so the post-commit splice goes through
        // `Arc::make_mut` with a strong count of 2. That used to deep-copy every
        // column — the whole result set — on the UI thread, on the path built
        // specifically to avoid a rebuild. Columns are behind their own `Arc`s
        // so the outer clone is a handful of refcount bumps.
        let rs = Arc::new(ResultSet::from_rows(
            vec![col("INT"), col("TEXT")],
            vec![
                vec![Value::Int(1), Value::Str("a".to_string())],
                vec![Value::Int(2), Value::Str("b".to_string())],
            ],
        ));
        let canonical = Arc::clone(&rs); // the tab still holds it
        let untouched_before = rs.cols[0].arena.as_ptr();

        let mut spliced = rs;
        assert!(
            Arc::strong_count(&spliced) > 1,
            "the shared-Arc precondition this test is about"
        );
        Arc::make_mut(&mut spliced)
            .splice_rows(&[(1, vec![Value::Int(2), Value::Str("B".to_string())])]);

        assert_eq!(
            spliced.cols[0].arena.as_ptr(),
            untouched_before,
            "the untouched column must still share storage with the original"
        );
        assert_eq!(
            canonical.cols[0].arena.as_ptr(),
            untouched_before,
            "…and the original must be unharmed by the splice"
        );
        // The changed column is genuinely copy-on-write: the original keeps its
        // old value, the spliced one has the new.
        assert_eq!(spliced.cell(1, 1).unwrap().display(), "B");
        assert_eq!(canonical.cell(1, 1).unwrap().display(), "b");
    }

    #[test]
    fn cell_matches_compares_by_tag_and_canonical_text() {
        let rs = ResultSet::from_rows(
            vec![col("INT"), col("TEXT"), col("INT")],
            vec![vec![Value::Int(-7), Value::Str("hi".into()), Value::Null]],
        );
        assert!(rs.cell(0, 0).unwrap().matches(&Value::Int(-7)));
        assert!(!rs.cell(0, 0).unwrap().matches(&Value::Int(7)));
        assert!(rs.cell(0, 1).unwrap().matches(&Value::Str("hi".into())));
        assert!(!rs.cell(0, 1).unwrap().matches(&Value::Str("HI".into())));
        assert!(rs.cell(0, 2).unwrap().matches(&Value::Null));
        assert!(!rs.cell(0, 2).unwrap().matches(&Value::Str(String::new())));
        // A different tag never matches, even when the text would.
        assert!(!rs.cell(0, 0).unwrap().matches(&Value::Str("-7".into())));
        assert!(!rs.cell(0, 0).unwrap().matches(&Value::UInt(7)));
    }

    #[test]
    fn splice_rows_replaces_only_listed_rows() {
        let mut rs = ResultSet::from_rows(
            vec![col("INT"), col("TEXT")],
            vec![
                vec![Value::Int(1), Value::Str("a".to_string())],
                vec![Value::Int(2), Value::Str("b".to_string())],
                vec![Value::Int(3), Value::Str("c".to_string())],
            ],
        );
        rs.splice_rows(&[(1, vec![Value::Int(20), Value::Str("B".to_string())])]);
        // Row 1 replaced; rows 0 and 2 untouched.
        assert_eq!(rs.cell(0, 1).unwrap().display(), "a");
        assert!(matches!(rs.cell(1, 0).unwrap().to_value(), Value::Int(20)));
        assert_eq!(rs.cell(1, 1).unwrap().display(), "B");
        assert_eq!(rs.cell(2, 1).unwrap().display(), "c");
        assert_eq!(rs.row_count(), 3);
    }

    #[test]
    fn builder_matches_from_rows_and_carries_flags() {
        let mut b = ResultBuilder::new(vec![col("INT")]);
        assert_eq!(b.row_count(), 0);
        assert_eq!(b.columns().len(), 1);
        b.push_row(&[Value::Int(1)]);
        b.push_row(&[Value::Null]);
        b.set_elapsed(12);
        b.set_truncated(true);
        let rs = b.finish();
        assert_eq!(rs.row_count(), 2);
        assert_eq!(rs.elapsed_ms, 12);
        assert!(rs.truncated);
        assert!(rs.affected.is_none());
    }

    // ── The 1-row write-back safety net ──
    //
    // The guard between an over-optimistic `analyze_edit` and a corrupted table.
    // Both engines call this one verdict; before the extraction it was written
    // inline in each executor and neither copy had a test.

    fn del(table: &str) -> RowDelete {
        RowDelete {
            database: "db".into(),
            schema: None,
            table: table.into(),
            key: vec![("id".into(), Value::Int(1))],
        }
    }

    fn upd(table: &str) -> RowEdit {
        RowEdit {
            database: "db".into(),
            schema: None,
            table: table.into(),
            set: vec![("name".into(), Some("x".into()))],
            key: vec![("id".into(), Value::Int(1))],
        }
    }

    fn ins(table: &str) -> RowInsert {
        RowInsert {
            database: "db".into(),
            schema: None,
            table: table.into(),
            cols: vec![("name".into(), Some("x".into()))],
        }
    }

    #[test]
    fn one_row_verdict_accepts_exactly_one_row() {
        assert!(one_row_verdict(WriteStep::Update(&upd("t")), 1).is_ok());
        assert!(one_row_verdict(WriteStep::Delete(&del("t")), 1).is_ok());
        assert!(one_row_verdict(WriteStep::Insert(&ins("t")), 1).is_ok());
    }

    #[test]
    fn one_row_verdict_rejects_a_key_that_matched_nothing() {
        // The row moved or was already gone — applying the rest of the batch on
        // top of that would commit a half-understood edit.
        let e = one_row_verdict(WriteStep::Update(&upd("city")), 0).unwrap_err();
        assert!(e.contains("update on db.city"), "{e}");
        assert!(e.contains("affected 0 rows"), "{e}");
    }

    #[test]
    fn the_verdict_itself_claims_nothing_about_the_rollback() {
        // It runs *before* the rollback, so it can't know. The claim used to be
        // baked into this message and was false on a non-transactional table.
        let e = one_row_verdict(WriteStep::Update(&upd("city")), 0).unwrap_err();
        assert!(!e.contains("rolled back"), "{e}");
    }

    // ── What the rollback actually achieved (`Rollback`) ──

    #[test]
    fn a_complete_rollback_says_everything_was_undone() {
        let msg = Rollback::Complete.note();
        assert!(msg.contains("rolled back all changes"), "{msg}");
    }

    #[test]
    fn an_incomplete_rollback_says_the_changes_remain() {
        // MyISAM/MEMORY/ARCHIVE/CSV ignore BEGIN and ROLLBACK: the statements
        // that already ran are permanent. Telling the user they were undone is
        // how one failed import becomes 50k duplicates on the retry.
        let msg = Rollback::Incomplete.note();
        assert!(!msg.contains("rolled back all changes"), "{msg}");
        assert!(msg.contains("not transactional"), "{msg}");
        assert!(msg.contains("remain"), "{msg}");
    }

    #[test]
    fn the_note_reads_as_one_sentence_with_the_verdict() {
        let verdict = one_row_verdict(WriteStep::Update(&upd("city")), 2).unwrap_err();
        let full = format!("{verdict}{}", Rollback::Complete.note());
        assert_eq!(
            full,
            "update on db.city affected 2 rows (expected exactly 1) — rolled back all changes"
        );
    }

    // ── Which storage engines can honour a rollback ──

    #[test]
    fn innodb_is_transactional_however_the_catalogue_spells_it() {
        assert!(engine_is_transactional("InnoDB"));
        assert!(engine_is_transactional("innodb"));
        assert!(engine_is_transactional("  INNODB  "));
    }

    #[test]
    fn the_non_transactional_engines_are_not() {
        for e in [
            "MyISAM",
            "MEMORY",
            "ARCHIVE",
            "CSV",
            "MRG_MyISAM",
            "BLACKHOLE",
        ] {
            assert!(!engine_is_transactional(e), "{e} is not transactional");
        }
    }

    #[test]
    fn an_unknown_engine_does_not_get_the_benefit_of_the_doubt() {
        // Same rule as `ddl::pg_replaceable`: uncertainty resolves to the side
        // that doesn't promise something the server may not deliver.
        assert!(!engine_is_transactional(""));
        assert!(!engine_is_transactional("   "));
        assert!(!engine_is_transactional("SomeFutureEngine"));
    }

    #[test]
    fn one_row_verdict_rejects_a_key_that_was_not_unique() {
        // The corruption case: two rows matched, so committing would silently
        // rewrite a row the user never saw.
        let e = one_row_verdict(WriteStep::Delete(&del("city")), 2).unwrap_err();
        assert!(e.contains("delete on db.city"), "{e}");
        assert!(e.contains("affected 2 rows"), "{e}");
        assert!(one_row_verdict(WriteStep::Insert(&ins("city")), 7).is_err());
    }

    #[test]
    fn one_row_verdict_names_the_namespace_when_there_is_one() {
        let mut e = upd("city");
        e.schema = Some("public".into());
        let msg = one_row_verdict(WriteStep::Update(&e), 0).unwrap_err();
        assert!(msg.contains("db.public.city"), "{msg}");
    }

    #[test]
    fn write_plan_orders_deletes_then_updates_then_inserts() {
        // Deletes first is load-bearing: "delete a row, then insert one carrying
        // the same unique key" must work. Both engines iterate this plan.
        let w = GridWrite {
            updates: vec![upd("u1"), upd("u2")],
            inserts: vec![ins("i1")],
            deletes: vec![del("d1")],
        };
        let plan = w.plan();
        let names: Vec<(&str, &str)> = plan.iter().map(|s| (s.action(), s.table())).collect();
        assert_eq!(
            names,
            vec![
                ("delete on", "d1"),
                ("update on", "u1"),
                ("update on", "u2"),
                ("insert into", "i1"),
            ]
        );
    }

    #[test]
    fn write_plan_of_an_empty_batch_is_empty() {
        let w = GridWrite::default();
        assert!(w.is_empty());
        assert!(w.plan().is_empty());
    }

    // ── The column arena's ceiling is reported, not just applied ──────────

    #[test]
    fn a_column_under_its_ceiling_reports_nothing() {
        let mut c = ColumnData::with_capacity(2);
        c.arena.push_str("abc");
        c.finish_cell_within(CellTag::Str, 64);
        assert!(!c.capped);
    }

    #[test]
    fn a_column_that_hits_its_ceiling_says_so_and_caps_at_a_char_boundary() {
        // The real ceiling is 512 MiB *per column across all rows* — ~2.7 KB a
        // row at the 200k cap, which a TEXT column clears — so this path is
        // reachable and its consequence (every later cell blank) is invisible
        // without the flag. Tested at a lowered ceiling because the honest
        // version would need 600 MB of RAM.
        let mut c = ColumnData::with_capacity(2);
        c.arena.push_str("aaaaaaaa");
        c.finish_cell_within(CellTag::Str, 4);
        assert!(c.capped);
        assert_eq!(c.arena.len(), 4);

        // Multi-byte: the cut lands on a boundary, never mid-character.
        let mut c = ColumnData::with_capacity(1);
        c.arena.push_str("aé€"); // 1 + 2 + 3 bytes
        c.finish_cell_within(CellTag::Str, 4);
        assert!(c.capped);
        assert_eq!(c.arena, "aé");
    }

    #[test]
    fn every_later_cell_of_a_capped_column_is_empty() {
        // Why the flag matters: the cells after the cap keep their tag and read
        // as blank, so an `Int` column silently becomes a column of "".
        let mut c = ColumnData::with_capacity(3);
        c.arena.push_str("aaaa");
        c.finish_cell_within(CellTag::Str, 3);
        c.arena.push_str("bbbb");
        c.finish_cell_within(CellTag::Str, 3);
        assert!(c.capped);
        assert_eq!(c.cell(1).map(|r| r.text().to_string()).as_deref(), Some(""));
    }

    #[test]
    fn an_ordinary_result_caps_nothing() {
        let rs = ResultSet::from_rows(vec![col("text")], vec![vec![Value::Str("x".into())]]);
        assert!(rs.capped_columns.is_empty());
    }

    /// A pure-UPDATE commit splices in place instead of re-running, and the
    /// spliced result must still say where it came from — otherwise the label
    /// disappears the first time you edit a cell, which is the one moment you are
    /// definitely writing to whatever it names.
    #[test]
    fn a_commit_splice_keeps_the_result_database() {
        let mut rs = ResultSet::from_rows(
            vec![col("INT"), col("VARCHAR")],
            vec![vec![Value::Int(1), Value::Str("a".into())]],
        );
        rs.database = Some("world".into());
        rs.splice_rows(&[(0, vec![Value::Int(1), Value::Str("b".into())])]);
        assert_eq!(rs.database.as_deref(), Some("world"));
        assert_eq!(rs.cell(0, 1).map(|c| c.display()), Some("b"));
    }

    /// A result nothing scoped — a fixture, or the temporary set a re-fetch
    /// splices from — says nothing rather than guessing.
    #[test]
    fn a_result_no_query_produced_names_no_database() {
        let rs = ResultSet::from_rows(vec![col("INT")], vec![vec![Value::Int(1)]]);
        assert_eq!(rs.database, None);
    }

    #[test]
    fn goto_row_is_one_based_and_returns_a_zero_based_index() {
        assert_eq!(goto_row_index("1", 100), Some(0));
        assert_eq!(goto_row_index("2", 100), Some(1));
        assert_eq!(
            goto_row_index("100", 100),
            Some(99),
            "the last row is valid"
        );
    }

    /// A number past the end goes to the end, and one below the start goes to the
    /// start. Both ends clamp, so any number at all lands somewhere.
    #[test]
    fn goto_row_clamps_a_number_outside_the_grid_to_the_nearest_end() {
        assert_eq!(goto_row_index("101", 100), Some(99));
        assert_eq!(goto_row_index("900", 100), Some(99));
        assert_eq!(
            goto_row_index("0", 100),
            Some(0),
            "0 is before the first row"
        );
    }

    /// The one range case that stays `None`: there is no row to land on.
    #[test]
    fn goto_row_finds_nothing_in_an_empty_grid() {
        assert_eq!(goto_row_index("1", 0), None);
        assert_eq!(goto_row_index("0", 0), None);
    }

    #[test]
    fn goto_row_rejects_what_is_not_a_row_number() {
        for s in ["", "   ", "abc", "1abc", "-1", "1.5", "1e3", "+"] {
            assert_eq!(goto_row_index(s, 100), None, "{s:?}");
        }
    }

    #[test]
    fn goto_row_ignores_surrounding_whitespace() {
        assert_eq!(goto_row_index("  42  ", 100), Some(41));
        assert_eq!(goto_row_index("\t7\n", 100), Some(6));
    }

    /// **The form the grid actually prints.** The stats line says `200k rows`,
    /// and typing that back returned `None`: nothing moved and nothing was said,
    /// which is indistinguishable from a feature that doesn't work.
    #[test]
    fn goto_row_accepts_the_compact_counts_the_grid_prints() {
        assert_eq!(goto_row_index("200k", 200_000), Some(199_999));
        assert_eq!(goto_row_index("1.25k", 200_000), Some(1_249));
        assert_eq!(goto_row_index("3m", 5_000_000), Some(2_999_999));
        assert_eq!(goto_row_index("1b", 100), Some(99), "past the end clamps");
        // Case and spacing come off a copy-paste as often as not.
        assert_eq!(goto_row_index(" 200K ", 200_000), Some(199_999));
    }

    /// A fraction means something only against a suffix — `1.25k` is 1250 rows,
    /// a bare `1.25` is not a row number — and it is fixed-point, so a long one
    /// truncates rather than rounding to a row nobody named.
    #[test]
    fn goto_row_reads_a_fraction_only_against_a_suffix() {
        assert_eq!(goto_row_index("1.25", 200_000), None);
        assert_eq!(goto_row_index("1.2345k", 200_000), Some(1_233));
        assert_eq!(
            goto_row_index("k", 200_000),
            None,
            "a suffix with no number"
        );
        assert_eq!(goto_row_index("1.k", 200_000), Some(999));
    }

    /// Separators are accepted for anything pasted in from elsewhere — the app
    /// itself never writes one (the stats line is compact, the gutter and the
    /// find readout unformatted).
    #[test]
    fn goto_row_accepts_digit_group_separators() {
        assert_eq!(goto_row_index("148,203", 200_000), Some(148_202));
        assert_eq!(goto_row_index("148 203", 200_000), Some(148_202));
        assert_eq!(goto_row_index("148_203", 200_000), Some(148_202));
        // A narrow no-break space is what some locales actually render.
        assert_eq!(goto_row_index("148\u{202f}203", 200_000), Some(148_202));
    }

    /// Stripping separators must not turn a typo into a different valid row.
    #[test]
    fn goto_row_still_rejects_a_stray_letter_among_the_digits() {
        assert_eq!(goto_row_index("14x8", 200_000), None);
        assert_eq!(goto_row_index("1,4x8", 200_000), None);
    }

    /// A number too large for the machine is still just "past the end" — a row of
    /// 9s is how someone asks for the bottom, and it must not fall through to the
    /// garbage path and do nothing.
    #[test]
    fn goto_row_clamps_a_number_wider_than_usize() {
        assert_eq!(goto_row_index("99999999999999999999999999", 100), Some(99));
    }

    #[test]
    fn a_row_drag_covers_every_column_of_every_row_it_crossed() {
        assert_eq!(row_range_selection(2, 5, 4), ((2, 0), (5, 3)));
        // Dragging upwards keeps the anchor where the gesture began, so
        // reversing the drag doesn't move the far end of the selection.
        assert_eq!(row_range_selection(5, 2, 4), ((5, 0), (2, 3)));
        // One row is the same shape a click makes.
        assert_eq!(row_range_selection(3, 3, 4), row_selection(3, 4));
        // No columns saturates rather than underflowing.
        assert_eq!(row_range_selection(1, 2, 0), ((1, 0), (2, 0)));
    }

    // ── The attach action's label ──

    #[test]
    fn a_lone_cell_is_one_column_not_one_row() {
        // "1 row" would read as the whole row, which is not what is sent.
        assert_eq!(attach_scope_label(1, 1, 5), "1 column");
        assert_eq!(attach_scope_label(1, 3, 5), "3 columns");
    }

    #[test]
    fn covering_every_column_is_whole_rows_and_says_only_that() {
        assert_eq!(attach_scope_label(3, 5, 5), "3 rows");
        assert_eq!(attach_scope_label(1, 5, 5), "1 row");
        // A single-column result: covering the one column is covering the row.
        assert_eq!(attach_scope_label(4, 1, 1), "4 rows");
    }

    #[test]
    fn a_block_names_both_halves() {
        assert_eq!(attach_scope_label(3, 1, 5), "3 rows and 1 column");
        assert_eq!(attach_scope_label(3, 2, 5), "3 rows and 2 columns");
    }

    #[test]
    fn a_result_with_no_columns_still_labels_its_rows() {
        // `ncols == 0` can't mean "every column is selected" — with nothing to
        // select the honest phrase is still about rows.
        assert_eq!(attach_scope_label(2, 0, 0), "2 rows and 0 columns");
    }

    /// The gutter click and the Ctrl+G jump make the **same** gesture, which is
    /// why it is one function: the jump exists to land you where a click would,
    /// so the row lights up the way the user already knows.
    #[test]
    fn a_row_gesture_anchors_at_column_zero_and_ends_at_the_last() {
        assert_eq!(row_selection(4, 5), ((4, 0), (4, 4)));
        assert_eq!(row_selection(0, 1), ((0, 0), (0, 0)));
    }

    /// A result with no columns at all can't underflow into a huge index.
    #[test]
    fn a_row_gesture_with_no_columns_does_not_underflow() {
        assert_eq!(row_selection(3, 0), ((3, 0), (3, 0)));
    }

    /// The jump's landing, as one decision: which row, the gesture, and the
    /// scroll column. All three used to be inline in the grid's effect.
    #[test]
    fn a_jump_lands_on_the_row_as_a_whole_row_selection() {
        let t = goto_target("40", 100, 6).unwrap();
        assert_eq!(t.anchor, (39, 0), "1-based in, 0-based out");
        assert_eq!(t.active, (39, 5));
    }

    /// **Column 0, not the active cell's column.** Following the active cell
    /// would fling the viewport to the far right of a wide result on every jump
    /// — this is the one line that stops a row jump from also being a horizontal
    /// one.
    #[test]
    fn a_jump_scrolls_at_column_zero_however_wide_the_result() {
        for ncols in [1, 6, 50] {
            assert_eq!(goto_target("40", 100, ncols).unwrap().scroll_col, 0);
        }
    }

    /// It inherits `goto_row_index` whole — the clamp, the compact counts, and
    /// the two cases that mean no row.
    #[test]
    fn a_jump_inherits_what_the_box_can_read() {
        assert_eq!(goto_target("999999", 100, 3).unwrap().anchor.0, 99);
        assert_eq!(goto_target("200k", 200_000, 3).unwrap().anchor.0, 199_999);
        assert_eq!(goto_target("abc", 100, 3), None);
        assert_eq!(goto_target("1", 0, 3), None, "an empty grid");
    }
}
