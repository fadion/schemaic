//! **Raw-bytes cells, fetched on demand.**
//!
//! A binary cell's bytes never reach the grid: all three backends replace them
//! with [`crate::model::binary_display`]'s `<n bytes>` at the wire, because a
//! [`Value`] has no bytes variant and a columnar [`ResultSet`] stores text in
//! per-column arenas. That is a deliberate bound, not an oversight — a 200k-row
//! result with a `LONGBLOB` column would otherwise be the whole table in RAM to
//! render a placeholder — and it is why looking at a blob is a **second query**
//! rather than a lookup.
//!
//! This module is the pure half of that: what identifies the one cell to
//! re-read ([`BlobRef`], built by [`blob_source`]), and how the bytes are
//! presented once they arrive ([`sniff`], [`hex_line`]). The SQL that fetches
//! them is *not* here — it is built per backend beside `build_update`, for the
//! same reason that one is: MySQL and SQLite bind the WHERE key as parameters
//! and PostgreSQL inlines it, and a fourth spelling of the WHERE identity is
//! exactly the drift [`crate::edit::row_key`] exists to prevent.
//!
//! **The identity is the write path's identity.** A blob is re-read by the same
//! key an `UPDATE` of that row would carry, so a fetch can only be offered
//! where a write could have been aimed — no key, no fetch, and never a
//! `LIMIT 1` over an ambiguous row.

use crate::edit::{EditModel, row_key};
use crate::intel::SqlDialect;
use crate::model::{ResultSet, Value};

/// Most bytes read into memory for one cell.
///
/// A `LONGBLOB` holds up to 4 GiB and this app renders in-process, so the fetch
/// is capped rather than trusted. 64 MiB is far above any image, document or
/// serialized payload a cell realistically holds, and far below what makes the
/// process fail.
///
/// The cap is enforced in the `SELECT` — a `SUBSTRING` of this many bytes,
/// **beside the whole value's `OCTET_LENGTH`**. Reading the length separately is
/// what makes truncation a fact rather than a suspicion: a buffer that comes
/// back exactly `FETCH_CAP` long is otherwise indistinguishable from a value
/// that happens to be that size, and [`BlobValue::truncated`] has to be right
/// about that to refuse a save rather than write a corrupt file.
pub const FETCH_CAP: usize = 64 * 1024 * 1024;

/// Most bytes a file loaded *into* a cell may carry — the write half's
/// [`FETCH_CAP`], and deliberately the same number.
///
/// **The read half has always been bounded and the write half was not.** A
/// `SELECT` can only hand back `FETCH_CAP`, but a file picker will hand over
/// whatever is on disk: `fs::read` allocates all of it, the staged edit holds
/// it, and the statement binds it. A `LONGBLOB` column's own cap is 4 GB, and a
/// column whose schema has not loaded reports no cap at all — so without this
/// the only thing between a mis-clicked disk image and the process is how much
/// memory the machine has.
///
/// The same number as the read cap because the panel would otherwise promise
/// what it cannot show: a value larger than this comes back truncated, refuses
/// to save, and displays as a prefix of itself. Writing one in would create
/// cells this app can never again render or export whole.
pub const LOAD_CAP: usize = FETCH_CAP;

/// Is a file of `len` bytes too large to load into a cell?
///
/// A `>` rather than a `>=`: a file of exactly [`LOAD_CAP`] is the largest one
/// the read half can hand back whole, so it is a value this app can still show,
/// save and export — the boundary belongs inside.
pub fn load_too_large(len: u64) -> bool {
    len > LOAD_CAP as u64
}

/// Most pixels a preview will decode: 32 megapixels.
///
/// **[`FETCH_CAP`] does not bound this, which is the whole reason it exists.**
/// The renderer decodes to RGBA, so what a preview costs is width × height × 4
/// — a function of the image's *dimensions*, not of the bytes it arrived in. A
/// 40 KB PNG can legitimately declare 30000 × 30000 and expand to 3.6 GB, and
/// the bytes here came out of a database rather than off this machine, so the
/// dimensions are input like any other and are not to be trusted on the way to
/// an allocation.
///
/// 32 megapixels is ~128 MB of RGBA. Above any image a cell realistically
/// holds — an avatar, a logo, a scan — and below what a desktop cannot absorb.
pub const PREVIEW_PIXEL_CAP: u64 = 32_000_000;

/// Most pixels a preview may measure along **either** edge: 4096.
///
/// **[`PREVIEW_PIXEL_CAP`] does not bound this either, and the renderer's real
/// constraint is this one.** floem's images live in the same atlas as its
/// glyphs, and that atlas grows to `2 × max(width, height)` with no clamp of its
/// own — so the number that has to be inside the GPU's limit is a *dimension*,
/// not an area. wgpu's default `max_texture_dimension_2d` is 8192, and there is
/// no `on_uncaptured_error` handler anywhere in the floem crates, so a
/// `create_texture` that fails validation ends the process rather than the
/// preview.
///
/// An ordinary 6000 × 4000 camera photograph is 24 megapixels — comfortably
/// *inside* the pixel cap, and 12000 past the atlas limit. So the two caps are
/// not one cap stated twice: an image can pass either and fail the other, and
/// both are checked.
///
/// 4096 rather than 8192, which is the number measured: the atlas doubles the
/// larger edge, so 4096 is the largest edge that fits a default-limit adapter,
/// and adapters reporting *less* than the default exist. Above any image a cell
/// realistically holds, and the refusal keeps the bytes readable as hex or
/// saveable to a file.
pub const PREVIEW_EDGE_CAP: u32 = 4096;

/// Whether a preview may be built, given what the header says it would decode
/// to.
///
/// The **decision** lives here, pure and tested; the **measurement** does not,
/// because reading dimensions means a decoder. The caller hands over what it
/// managed to read and this says what to do with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewVerdict {
    /// Within budget — decode and draw it.
    Show,
    /// The header parsed and declares more than [`PREVIEW_PIXEL_CAP`] pixels,
    /// or more than [`PREVIEW_EDGE_CAP`] along either edge. One arm for two
    /// caps because the panel says the same sentence either way — it names the
    /// dimensions, which is what the reader can act on.
    TooLarge { width: u32, height: u32 },
    /// No dimensions could be read.
    ///
    /// **Not the same as "small enough to try".** Nothing bounds the decode
    /// here, and the reason the header did not parse is very likely that the
    /// bytes are not the image [`sniff`] took them for — in which case the
    /// renderer draws *nothing at all*, silently, and the panel becomes a
    /// caption over an empty box. Saying so is strictly better than showing it.
    Unmeasurable,
}

/// [`PreviewVerdict`] for the dimensions a caller managed to read.
///
/// Multiplied as `u64`, deliberately: `u32 * u32` overflows at a little over
/// four gigapixels, and a header declaring 65536 × 65536 would otherwise wrap
/// to zero and pass a cap it exceeds by two orders of magnitude.
pub fn preview_verdict(dims: Option<(u32, u32)>) -> PreviewVerdict {
    let Some((width, height)) = dims else {
        return PreviewVerdict::Unmeasurable;
    };
    // A zero dimension decodes to nothing and draws nothing — it is a header
    // this cannot use, not an image within budget.
    if width == 0 || height == 0 {
        return PreviewVerdict::Unmeasurable;
    }
    // Two caps, two different quantities — see `PREVIEW_EDGE_CAP`. The area
    // bounds what the decode allocates; the edge bounds what the renderer's
    // texture atlas can be asked for, and exceeding *that* is a process exit
    // rather than a refused preview.
    if width > PREVIEW_EDGE_CAP || height > PREVIEW_EDGE_CAP {
        return PreviewVerdict::TooLarge { width, height };
    }
    if u64::from(width) * u64::from(height) > PREVIEW_PIXEL_CAP {
        return PreviewVerdict::TooLarge { width, height };
    }
    PreviewVerdict::Show
}

/// Which cell's bytes to fetch, and how to find its row again.
///
/// The `(database, schema, table, column)` quartet is the column's own wire
/// provenance — the real base column, not the query's alias — and `key` is
/// [`row_key`]'s output for the row it sits in.
#[derive(Clone, Debug, PartialEq)]
pub struct BlobRef {
    pub database: String,
    /// PostgreSQL namespace of `table` (`None` on MySQL/SQLite). Qualified
    /// unconditionally by the backends, like [`crate::model::RowEdit::schema`]:
    /// the statement is never shown to the user, so it must not depend on
    /// `search_path`.
    pub schema: Option<String>,
    pub table: String,
    /// The **real** column name on `table`.
    pub column: String,
    /// WHERE identity: key columns → the row's original values.
    pub key: Vec<(String, Value)>,
}

impl BlobRef {
    /// What the panel's title bar says: `staff.picture`.
    pub fn title(&self) -> String {
        format!("{}.{}", self.table, self.column)
    }

    /// The file name a save offers, without an extension: `staff_picture_1`.
    ///
    /// **The key values are in it, and that is the point.** Two rows' blobs from
    /// one column are the overwhelmingly common case — a table of avatars is
    /// nothing else — and a stem built from the column alone offers every one of
    /// them the same name, so the second save silently proposes to overwrite the
    /// first. Anything that is not a plain filename character becomes `_`, so a
    /// key holding a path separator or a `:` cannot escape the name into a
    /// directory or an NTFS alternate data stream.
    pub fn save_stem(&self) -> String {
        let mut parts = vec![self.table.clone(), self.column.clone()];
        parts.extend(self.key.iter().map(|(_, v)| v.display()));
        let raw = parts.join("_");
        let mut out: String = raw
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        // A name of nothing but separators is no name; and a very long key must
        // not push the whole thing past what a filesystem will take.
        out.truncate(120);
        if out.trim_matches('_').is_empty() {
            "blob".to_string()
        } else {
            out
        }
    }
}

/// One cell's bytes as fetched, with the length the server reported.
///
/// The two are separate because they disagree exactly when it matters: `bytes`
/// is capped at [`FETCH_CAP`] and `len` is the whole value, so a blob larger
/// than the cap arrives complete enough to look at and incomplete for anything
/// that writes it back out.
#[derive(Clone, Debug, PartialEq)]
pub struct BlobValue {
    pub bytes: Vec<u8>,
    /// Octet length of the **whole** value on the server.
    pub len: u64,
}

impl BlobValue {
    /// Did [`FETCH_CAP`] cut this value short?
    ///
    /// Asked of the server's own length rather than of the buffer, so it stays
    /// right for a blob that happens to be exactly the cap: a save of a
    /// truncated value would write a file that is not the data, so this is what
    /// the panel refuses on.
    pub fn truncated(&self) -> bool {
        self.len > self.bytes.len() as u64
    }
}

/// What the leading bytes say this blob is.
///
/// Sniffed from the content, never from the column — a `BLOB` column's type
/// name promises nothing about what was stored in it, which is the same reason
/// [`crate::model::type_is_binary`]'s own doc gives for never deciding on a
/// type name alone.
///
/// **Every variant here must be one the renderer can decode**, which is why the
/// list is short and why the workspace's `floem` dependency names
/// `image-gif`/`image-bmp` on top of its default formats. `img()` draws nothing
/// at all for bytes it cannot read, so a kind this can *name* but the renderer
/// cannot *show* becomes a panel captioned "GIF image" over an empty box —
/// worse than the hex dump it replaced. Adding a format is therefore two edits,
/// here and in the root `Cargo.toml`, and [`BlobKind::is_image`] is the
/// assertion that they stayed together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlobKind {
    Png,
    Jpeg,
    Gif,
    Bmp,
    Webp,
    Ico,
    /// Not a format this recognizes — the hex view is all there is to show.
    Opaque,
}

impl BlobKind {
    /// Can this be handed to an image view?
    ///
    /// Everything except [`BlobKind::Opaque`] — see the type's own doc for why
    /// that equivalence is a requirement on the *variant list* rather than a
    /// shortcut taken here.
    pub fn is_image(self) -> bool {
        self != BlobKind::Opaque
    }

    /// The short name shown beside the byte count (`PNG image`).
    pub fn label(self) -> &'static str {
        match self {
            BlobKind::Png => "PNG image",
            BlobKind::Jpeg => "JPEG image",
            BlobKind::Gif => "GIF image",
            BlobKind::Bmp => "BMP image",
            BlobKind::Webp => "WebP image",
            BlobKind::Ico => "Icon",
            BlobKind::Opaque => "Binary data",
        }
    }

    /// The extension a save-to-file dialog should suggest, without the dot.
    pub fn extension(self) -> &'static str {
        self.extensions()[0]
    }

    /// Every extension the save dialog should accept for this kind, the first
    /// being the one [`BlobKind::extension`] suggests.
    ///
    /// A list rather than the single suggestion because the file picker filters
    /// on it, and a JPEG saved as `.jpeg` is the same file — offering only
    /// `.jpg` would hide the user's own existing files from the dialog they are
    /// saving into.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            BlobKind::Png => &["png"],
            BlobKind::Jpeg => &["jpg", "jpeg"],
            BlobKind::Gif => &["gif"],
            BlobKind::Bmp => &["bmp"],
            BlobKind::Webp => &["webp"],
            BlobKind::Ico => &["ico"],
            BlobKind::Opaque => &["bin", "dat"],
        }
    }
}

/// Identify a blob by its magic bytes.
///
/// Only the leading bytes are read, so this costs the same for a 40 MB value as
/// for a 40-byte one — it runs on every fetch, including the ones that turn out
/// to be opaque.
pub fn sniff(bytes: &[u8]) -> BlobKind {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    const GIF87: &[u8] = b"GIF87a";
    const GIF89: &[u8] = b"GIF89a";
    if bytes.starts_with(PNG) {
        return BlobKind::Png;
    }
    // JPEG: SOI marker. The third byte is the first marker of the segment that
    // follows and varies by encoder (JFIF `\xe0`, Exif `\xe1`, raw `\xdb`), so
    // only the two-byte SOI is matched.
    if bytes.starts_with(b"\xff\xd8\xff") {
        return BlobKind::Jpeg;
    }
    if bytes.starts_with(GIF87) || bytes.starts_with(GIF89) {
        return BlobKind::Gif;
    }
    if bytes.starts_with(b"BM") && bytes.len() >= 14 {
        return BlobKind::Bmp;
    }
    // RIFF container, with the form type at offset 8 — `RIFF????WEBP`. The
    // length field between them is not checked: a truncated fetch has the right
    // header and a length describing bytes we did not ask for.
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return BlobKind::Webp;
    }
    // `00 00 01 00` — an icon directory. The neighbouring `00 00 02 00` is a
    // *cursor*, which shares the format and is not matched: floem decodes ICO.
    if bytes.starts_with(b"\x00\x00\x01\x00") {
        return BlobKind::Ico;
    }
    BlobKind::Opaque
}

/// Bytes shown per line of the hex dump.
pub const HEX_COLS: usize = 16;

/// How many lines [`hex_line`] can produce for a buffer of `len` bytes.
///
/// The hex view is virtualized — a 64 MiB blob is four million lines, and
/// building them eagerly would cost more than the fetch did — so the UI asks
/// for this count and then for the lines it can actually see.
pub fn hex_row_count(len: usize) -> usize {
    len.div_ceil(HEX_COLS)
}

/// One line of a classic hex dump: offset, [`HEX_COLS`] bytes in two groups,
/// then the printable ASCII.
///
/// A `row` past the end returns an empty string rather than panicking: the
/// virtual list and the buffer are two signals that can be one frame apart, and
/// a blank line is a better answer to that than a crash.
///
/// The trailing line of a buffer that does not fill [`HEX_COLS`] is **padded in
/// the hex columns and not in the ASCII** — so the ASCII panel stays aligned
/// under the same column it would be for a full line, which is the only thing
/// the padding is for.
pub fn hex_line(bytes: &[u8], row: usize) -> String {
    let start = row * HEX_COLS;
    if start >= bytes.len() {
        return String::new();
    }
    let end = (start + HEX_COLS).min(bytes.len());
    let chunk = &bytes[start..end];
    let mut out = format!("{start:08x}  ");
    for i in 0..HEX_COLS {
        match chunk.get(i) {
            // `write!` into the buffer, not `push_str(&format!(..))`: this runs
            // per byte of every visible line on every scroll frame, and the
            // inner `format!` would allocate and drop a two-character `String`
            // each time. The macro cannot fail on a `String`.
            Some(b) => {
                use std::fmt::Write as _;
                let _ = write!(out, "{b:02x} ");
            }
            None => out.push_str("   "),
        }
        // Split the row into two groups of eight, the way `xxd -g1` and every
        // hex editor do — the eye counts to eight, not to sixteen.
        if i == HEX_COLS / 2 - 1 {
            out.push(' ');
        }
    }
    out.push('|');
    for &b in chunk {
        out.push(if (0x20..0x7f).contains(&b) {
            b as char
        } else {
            '.'
        });
    }
    out.push('|');
    out
}

/// The most bytes a column of this declared type can hold, or `None` where the
/// type sets no bound worth enforcing here.
///
/// **The point is to refuse an oversized file where the user chose it**, rather
/// than at the commit — MySQL answers a `BLOB` overrun with
/// `ERROR 1406: Data too long`, which arrives after the modal has closed, names
/// the column rather than the file, and rolls the whole staged batch back with
/// it.
///
/// It reads the **declared** type (`ColumnInfo::type_name`, the full text with
/// its parameters — `mediumblob`, `varbinary(4)`), not the wire type name a
/// result column carries. That is not a preference: MySQL reports all four blob
/// sizes as `MYSQL_TYPE_BLOB` on the wire, so `Column::type_name` says `BLOB`
/// for a `LONGBLOB` and `VARBINARY` with no length at all. Only the schema knows.
///
/// `None` is "no answer", never "no limit", and the caller must treat it that
/// way: PostgreSQL's `bytea` and SQLite's `BLOB` genuinely have no bound anyone
/// hits from a file picker (1 GB and `SQLITE_MAX_LENGTH`), and an unloaded
/// schema or an unknown type name gives the same `None` — after which the server
/// is still the authority it always was.
///
/// **The dialect is asked first, and it is not decoration.** The table below is
/// MySQL's, and a type name alone cannot say whose it is: `BLOB` and
/// `VARBINARY(16)` are legal SQLite declarations too, where the parameter is
/// ignored and the family means nothing. Applied there, this function capped a
/// `BLOB` at 65,535 bytes and a `VARBINARY(16)` at **sixteen** — so *Load from
/// file* refused files the engine would have stored without complaint. The
/// question is a capability, [`crate::ddl::enforces_declared_byte_length`], and
/// never `dialect == MySql` in place of one.
pub fn column_byte_cap(dialect: SqlDialect, type_name: &str) -> Option<u64> {
    if !crate::ddl::enforces_declared_byte_length(dialect) {
        return None;
    }
    let head = type_name
        .split(|c: char| c == '(' || c.is_whitespace())
        .next()
        .unwrap_or_default();
    // The four MySQL blob families, whose bound is the family and not a
    // parameter: MySQL promotes a `BLOB(M)` *declaration* to the smallest family
    // that holds M and reports what it promoted to, so a length never survives
    // to be read here. Measured on MySQL 8.4 rather than assumed —
    // `BLOB(70000)` introspects as `mediumblob`, `BLOB(200)` as `tinyblob`, and
    // `information_schema.CHARACTER_MAXIMUM_LENGTH` agrees with all four numbers
    // below.
    for (name, cap) in [
        ("TINYBLOB", 255u64),
        ("BLOB", 65_535),
        ("MEDIUMBLOB", 16_777_215),
        ("LONGBLOB", 4_294_967_295),
    ] {
        if head.eq_ignore_ascii_case(name) {
            return Some(cap);
        }
    }
    // `BINARY(n)` / `VARBINARY(n)`, whose bound *is* the parameter. Without one
    // the answer is unknown rather than zero: a bare `BINARY` is `BINARY(1)` to
    // the server, but a bare one reaching here means the caller handed over a
    // wire type name this function has already said it cannot read.
    if head.eq_ignore_ascii_case("BINARY") || head.eq_ignore_ascii_case("VARBINARY") {
        return type_name
            .split_once('(')
            .and_then(|(_, rest)| rest.split_once(')'))
            .and_then(|(n, _)| n.trim().parse::<u64>().ok());
    }
    None
}

/// The [`BlobRef`] for the binary cell at `(di, ci)`, or `None` if its bytes
/// cannot be fetched safely.
///
/// **Why this does not ask [`EditModel::table_index`].** It once *could not*:
/// C2 kept a binary column out of `col_table` outright, so the write model had
/// no table for the very column this is about, and a `blob_source` written
/// against that lookup returned `None` and never offered the feature. C2 has
/// since narrowed to *text* — the column is in the map like every other one —
/// so the lookup would work now, and it is still made by the column's own
/// provenance ([`crate::edit::EditModel::table_for_origin`]) because that is the
/// question actually being asked: a *read* is aimed at a keyed base table,
/// whether or not anything may be written to the column. The key comes from
/// [`row_key`] unchanged. A result whose binary column has no keyed base table
/// answers `None`, and the fetch is never offered: `SELECT … LIMIT 1` over an
/// ambiguous row would show bytes from a row the user did not click.
///
/// **Two signals, per *value* and not only per column** — the same pairing
/// `export::dropped_binary_columns` makes, and for a sharper reason
/// here. A column answers "is this bytes at all"; only the cell answers "did
/// *this* value arrive as bytes", and on SQLite the two genuinely differ: a
/// declared `BLOB` is an affinity, not a promise, so one row of that column can
/// hold text. Fetching such a row would put it through `length()` and `substr`,
/// which count **characters** on text and octets on a blob — a size reported in
/// the wrong unit, and a `SUBSTRING` cap that is not the cap. So the cell's own
/// text must be one [`crate::model::binary_display`] wrote, which is the only
/// evidence a `ResultSet` keeps that a value's bytes were dropped rather than
/// rendered.
pub fn blob_source(model: &EditModel, rs: &ResultSet, di: usize, ci: usize) -> Option<BlobRef> {
    let col = rs.columns.get(ci)?;
    if !crate::edit::holds_bytes(rs, ci) {
        return None;
    }
    // The per-value half. A NULL cell fails it too — its stored text is empty —
    // which is the right answer for a different reason: there are no bytes.
    if !crate::model::is_binary_display(rs.cell(di, ci)?.text()) {
        return None;
    }
    let origin = col.origin.as_ref()?;
    // An implicit key (SQLite's `rowid`) is no column of the table, so there is
    // nothing to select — the same reason it is never editable.
    if origin.implicit_key || origin.column.is_empty() {
        return None;
    }
    let tbl = model.table_for_origin(&origin.database, origin.schema.as_deref(), &origin.table)?;
    Some(BlobRef {
        database: tbl.database.clone(),
        schema: tbl.schema.clone(),
        table: tbl.table.clone(),
        column: origin.column.clone(),
        key: row_key(rs, tbl, di),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, ColumnFlags, ColumnOrigin};
    use crate::schema::{ColumnInfo, TableInfo};

    // ---- sniff -------------------------------------------------------------

    #[test]
    fn png_magic_is_recognized() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR"), BlobKind::Png);
    }

    #[test]
    fn jpeg_magic_is_recognized_for_every_segment_marker() {
        // JFIF, Exif, and a raw quantization table — the third byte differs and
        // must not be part of the match.
        assert_eq!(sniff(b"\xff\xd8\xff\xe0"), BlobKind::Jpeg);
        assert_eq!(sniff(b"\xff\xd8\xff\xe1"), BlobKind::Jpeg);
        assert_eq!(sniff(b"\xff\xd8\xff\xdb"), BlobKind::Jpeg);
    }

    #[test]
    fn both_gif_versions_are_recognized() {
        assert_eq!(sniff(b"GIF87a\x01\x00"), BlobKind::Gif);
        assert_eq!(sniff(b"GIF89a\x01\x00"), BlobKind::Gif);
    }

    #[test]
    fn webp_needs_the_form_type_not_just_riff() {
        assert_eq!(sniff(b"RIFF\x24\x00\x00\x00WEBPVP8 "), BlobKind::Webp);
        // A WAV is also a RIFF container, and is not an image.
        assert_eq!(sniff(b"RIFF\x24\x00\x00\x00WAVEfmt "), BlobKind::Opaque);
    }

    #[test]
    fn a_short_buffer_never_panics_and_is_opaque() {
        assert_eq!(sniff(b""), BlobKind::Opaque);
        assert_eq!(sniff(b"\x89"), BlobKind::Opaque);
        assert_eq!(sniff(b"RIFF"), BlobKind::Opaque);
        assert_eq!(sniff(b"BM"), BlobKind::Opaque);
    }

    #[test]
    fn plain_bytes_are_opaque() {
        assert_eq!(sniff(b"hello world"), BlobKind::Opaque);
        assert_eq!(sniff(&[0u8; 64]), BlobKind::Opaque);
    }

    #[test]
    fn an_icon_directory_is_recognized_and_a_cursor_is_not() {
        assert_eq!(sniff(b"\x00\x00\x01\x00\x01\x00\x10\x10"), BlobKind::Ico);
        assert_eq!(sniff(b"\x00\x00\x02\x00\x01\x00\x10\x10"), BlobKind::Opaque);
    }

    #[test]
    fn every_image_kind_reports_itself_as_one_and_opaque_does_not() {
        for k in [
            BlobKind::Png,
            BlobKind::Jpeg,
            BlobKind::Gif,
            BlobKind::Bmp,
            BlobKind::Webp,
            BlobKind::Ico,
        ] {
            assert!(k.is_image(), "{k:?} should be an image");
            assert_ne!(k.extension(), "bin", "{k:?} should suggest its own type");
            assert_ne!(k.label(), "Binary data", "{k:?} should name its format");
        }
        assert!(!BlobKind::Opaque.is_image());
        assert_eq!(BlobKind::Opaque.extension(), "bin");
    }

    /// **The variant list and the renderer's format list are one list.**
    ///
    /// `is_image` is `!= Opaque`, so a variant added here without the matching
    /// `image-*` feature in the root `Cargo.toml` becomes a caption over an
    /// empty box rather than a hex dump. Nothing in a Rust test can read
    /// floem's enabled features, so this pins the half that can be checked: the
    /// kinds this claims are renderable, spelled out, so adding one to the enum
    /// fails here and sends the author to the dependency.
    #[test]
    fn the_renderable_kinds_are_exactly_the_ones_floem_is_built_to_decode() {
        // PNG, JPEG, ICO and WebP come from floem's `default-image-formats`;
        // GIF and BMP are the two the workspace adds explicitly.
        let renderable = [
            BlobKind::Png,
            BlobKind::Jpeg,
            BlobKind::Ico,
            BlobKind::Webp,
            BlobKind::Gif,
            BlobKind::Bmp,
        ];
        let all = [
            BlobKind::Png,
            BlobKind::Jpeg,
            BlobKind::Gif,
            BlobKind::Bmp,
            BlobKind::Webp,
            BlobKind::Ico,
            BlobKind::Opaque,
        ];
        for k in all {
            assert_eq!(
                k.is_image(),
                renderable.contains(&k),
                "{k:?} claims a preview floem may not be built to draw — add the \
                 matching `image-*` feature to floem in the root Cargo.toml, or \
                 stop calling it renderable"
            );
        }
    }

    // ---- preview budget ----------------------------------------------------

    #[test]
    fn an_ordinary_image_is_shown() {
        assert_eq!(preview_verdict(Some((1920, 1080))), PreviewVerdict::Show);
        assert_eq!(preview_verdict(Some((1, 1))), PreviewVerdict::Show);
        // The largest image that can be drawn: square at the edge cap.
        assert_eq!(
            preview_verdict(Some((PREVIEW_EDGE_CAP, PREVIEW_EDGE_CAP))),
            PreviewVerdict::Show
        );
    }

    /// **An ordinary camera photograph was inside the cap and killed the
    /// process.** 6000 × 4000 is 24 megapixels — well under
    /// [`PREVIEW_PIXEL_CAP`], and this test used to assert `Show` on exactly
    /// those numbers with the comment "large and still legitimate". floem's
    /// images share the glyph atlas, which grows to `2 × max(w, h)` with no
    /// clamp; wgpu's default `max_texture_dimension_2d` is 8192 and no floem
    /// crate installs an `on_uncaptured_error` handler, so the failed
    /// `create_texture` ended the window and every tab's uncommitted edits.
    ///
    /// The gate bounded the area. The renderer bounds an edge.
    #[test]
    fn a_photograph_wider_than_the_atlas_is_refused_though_its_area_is_fine() {
        let (w, h) = (6000, 4000);
        assert!(
            u64::from(w) * u64::from(h) < PREVIEW_PIXEL_CAP,
            "the point of this test is that the area cap does not catch it"
        );
        assert_eq!(
            preview_verdict(Some((w, h))),
            PreviewVerdict::TooLarge {
                width: w,
                height: h
            }
        );
        // Either edge, not just the first.
        assert_eq!(
            preview_verdict(Some((h, w))),
            PreviewVerdict::TooLarge {
                width: h,
                height: w
            }
        );
    }

    #[test]
    fn the_cap_is_the_boundary_and_not_one_past_it() {
        // Exactly the edge cap is allowed; one pixel more is not.
        assert_eq!(
            preview_verdict(Some((PREVIEW_EDGE_CAP, 1))),
            PreviewVerdict::Show
        );
        assert_eq!(
            preview_verdict(Some((PREVIEW_EDGE_CAP + 1, 1))),
            PreviewVerdict::TooLarge {
                width: PREVIEW_EDGE_CAP + 1,
                height: 1
            }
        );
        assert_eq!(
            preview_verdict(Some((1, PREVIEW_EDGE_CAP + 1))),
            PreviewVerdict::TooLarge {
                width: 1,
                height: PREVIEW_EDGE_CAP + 1
            }
        );
    }

    /// **The two caps bound two different things, and today one hides the
    /// other.** The edge cap is the renderer's texture limit and the pixel cap
    /// is the decode's RAM budget; at 4096 the largest image that clears the
    /// edge cap is 16.7 MP, which is inside the 32 MP area cap — so nothing
    /// currently reaches the second check.
    ///
    /// That is worth an assert rather than a comment: `PREVIEW_EDGE_CAP` is a
    /// number a hand-check against a real adapter may raise, and raising it past
    /// 5657 makes the area cap live again. This fails then, which is the moment
    /// to re-read both.
    #[test]
    fn the_area_cap_sits_behind_the_edge_cap_at_these_numbers() {
        let widest = u64::from(PREVIEW_EDGE_CAP) * u64::from(PREVIEW_EDGE_CAP);
        assert!(
            widest <= PREVIEW_PIXEL_CAP,
            "the edge cap no longer subsumes the area cap ({widest} > {PREVIEW_PIXEL_CAP}) \
             — the area check is live again, and both caps need re-reading"
        );
    }

    /// **The bomb this exists for.** A small, entirely valid PNG can declare
    /// enormous dimensions; what it costs to draw is width × height × 4, and
    /// `FETCH_CAP` bounds neither term.
    #[test]
    fn a_decompression_bomb_is_refused_by_its_dimensions() {
        assert_eq!(
            preview_verdict(Some((30_000, 30_000))),
            PreviewVerdict::TooLarge {
                width: 30_000,
                height: 30_000
            }
        );
    }

    /// **The multiply must not wrap.** `u32 * u32` overflows a little past four
    /// gigapixels, so a header declaring 65536 × 65536 would come out as zero
    /// in 32 bits and pass a cap it exceeds enormously — in release, silently.
    #[test]
    fn dimensions_that_overflow_u32_still_exceed_the_cap() {
        let (w, h) = (65_536u32, 65_536u32);
        assert_eq!(w.wrapping_mul(h), 0, "the premise: this wraps in 32 bits");
        assert_eq!(
            preview_verdict(Some((w, h))),
            PreviewVerdict::TooLarge {
                width: w,
                height: h
            }
        );
        assert_eq!(
            preview_verdict(Some((u32::MAX, u32::MAX))),
            PreviewVerdict::TooLarge {
                width: u32::MAX,
                height: u32::MAX
            }
        );
    }

    #[test]
    fn unreadable_dimensions_are_not_treated_as_permission() {
        assert_eq!(preview_verdict(None), PreviewVerdict::Unmeasurable);
        // A zero dimension draws nothing; it is a header this cannot use.
        assert_eq!(
            preview_verdict(Some((0, 100))),
            PreviewVerdict::Unmeasurable
        );
        assert_eq!(
            preview_verdict(Some((100, 0))),
            PreviewVerdict::Unmeasurable
        );
    }

    // ---- hex dump ----------------------------------------------------------

    #[test]
    fn a_full_hex_line_has_offset_two_groups_and_ascii() {
        let bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        assert_eq!(
            hex_line(bytes, 0),
            "00000000  89 50 4e 47 0d 0a 1a 0a  00 00 00 0d 49 48 44 52 |.PNG........IHDR|"
        );
    }

    #[test]
    fn a_short_final_line_pads_the_hex_and_not_the_ascii() {
        let line = hex_line(b"AB", 0);
        // The ASCII panel opens at the same column a full line would put it.
        let full = hex_line(&[0u8; 16], 0);
        assert_eq!(
            line.find('|'),
            full.find('|'),
            "short line misaligns the ASCII panel:\n{line}\n{full}"
        );
        assert!(line.ends_with("|AB|"), "{line}");
    }

    #[test]
    fn nonprintable_bytes_become_dots_and_the_boundaries_are_exact() {
        // 0x20 (space) and 0x7e (~) are printable; 0x1f and 0x7f are not.
        let line = hex_line(b"\x1f\x20\x7e\x7f", 0);
        assert!(line.ends_with("|. ~.|"), "{line}");
    }

    #[test]
    fn hex_offsets_advance_by_the_column_count() {
        let bytes: Vec<u8> = (0..48).collect();
        assert!(hex_line(&bytes, 0).starts_with("00000000  00 01"));
        assert!(hex_line(&bytes, 1).starts_with("00000010  10 11"));
        assert!(hex_line(&bytes, 2).starts_with("00000020  20 21"));
    }

    #[test]
    fn a_row_past_the_end_is_blank_rather_than_a_panic() {
        assert_eq!(hex_line(b"AB", 1), "");
        assert_eq!(hex_line(b"", 0), "");
        assert_eq!(hex_line(b"AB", usize::MAX / 32), "");
    }

    #[test]
    fn the_row_count_covers_every_byte_and_no_more() {
        assert_eq!(hex_row_count(0), 0);
        assert_eq!(hex_row_count(1), 1);
        assert_eq!(hex_row_count(16), 1);
        assert_eq!(hex_row_count(17), 2);
        // Every line the count promises renders something.
        let bytes: Vec<u8> = (0..17).collect();
        for row in 0..hex_row_count(bytes.len()) {
            assert!(!hex_line(&bytes, row).is_empty(), "row {row} was blank");
        }
        assert!(hex_line(&bytes, hex_row_count(bytes.len())).is_empty());
    }

    // ---- truncation --------------------------------------------------------

    #[test]
    fn truncation_is_read_from_the_servers_length_not_the_buffer() {
        let whole = BlobValue {
            bytes: vec![0; 16],
            len: 16,
        };
        assert!(!whole.truncated());
        let cut = BlobValue {
            bytes: vec![0; 16],
            len: 4096,
        };
        assert!(cut.truncated());
    }

    #[test]
    fn an_empty_blob_is_not_truncated() {
        // A zero-length BLOB is a real value and distinct from NULL; reporting
        // it as truncated would refuse a save of the empty file it really is.
        let empty = BlobValue {
            bytes: Vec::new(),
            len: 0,
        };
        assert!(!empty.truncated());
    }

    // ---- naming ------------------------------------------------------------

    fn a_ref(table: &str, column: &str, key: &[(&str, Value)]) -> BlobRef {
        BlobRef {
            database: "db".to_string(),
            schema: None,
            table: table.to_string(),
            column: column.to_string(),
            key: key
                .iter()
                .map(|(c, v)| (c.to_string(), v.clone()))
                .collect(),
        }
    }

    #[test]
    fn the_title_names_the_table_and_the_column() {
        assert_eq!(
            a_ref("staff", "picture", &[("staff_id", Value::UInt(1))]).title(),
            "staff.picture"
        );
    }

    /// **Two rows of one column must not offer the same file name.** The stem
    /// is what the save dialog proposes, so a name built from the column alone
    /// makes the second save of an avatar table propose to overwrite the first.
    #[test]
    fn the_save_stem_distinguishes_two_rows_of_one_column() {
        let a = a_ref("staff", "picture", &[("staff_id", Value::UInt(1))]).save_stem();
        let b = a_ref("staff", "picture", &[("staff_id", Value::UInt(2))]).save_stem();
        assert_eq!(a, "staff_picture_1");
        assert_ne!(a, b);
    }

    /// A composite key contributes every part, in key order.
    #[test]
    fn the_save_stem_carries_a_composite_key() {
        let s = a_ref(
            "t",
            "c",
            &[("a", Value::Int(7)), ("b", Value::Str("x".into()))],
        )
        .save_stem();
        assert_eq!(s, "t_c_7_x");
    }

    /// **A key value cannot steer where the file lands.** It is server data, and
    /// it reaches a save dialog's default name — so a separator, a `..`, or an
    /// NTFS stream colon in it must become an ordinary character rather than a
    /// path.
    #[test]
    fn the_save_stem_cannot_escape_into_a_path() {
        let s = a_ref("t", "c", &[("k", Value::Str("../../etc/passwd".into()))]).save_stem();
        assert!(!s.contains('/'), "{s}");
        assert!(!s.contains('.'), "{s}");
        let s = a_ref("t", "c", &[("k", Value::Str("a:$DATA".into()))]).save_stem();
        assert!(!s.contains(':'), "{s}");
        assert!(!s.contains('$'), "{s}");
    }

    #[test]
    fn a_stem_with_nothing_usable_in_it_still_names_a_file() {
        let s = a_ref("...", "///", &[("k", Value::Str("!!!".into()))]).save_stem();
        assert_eq!(s, "blob");
    }

    #[test]
    fn a_very_long_key_is_cut_to_a_name_a_filesystem_takes() {
        let s = a_ref("t", "c", &[("k", Value::Str("x".repeat(500)))]).save_stem();
        assert!(s.len() <= 120, "{} chars", s.len());
    }

    /// A NULL in a key is part of the identity like any other value, and names
    /// itself rather than vanishing — two rows keyed `(1, NULL)` and `(1, 2)`
    /// would otherwise collide back into one name.
    #[test]
    fn a_null_key_value_still_contributes_to_the_stem() {
        let a = a_ref("t", "c", &[("x", Value::Int(1)), ("y", Value::Null)]).save_stem();
        let b = a_ref("t", "c", &[("x", Value::Int(1)), ("y", Value::Int(2))]).save_stem();
        assert_ne!(a, b);
        assert_eq!(a, "t_c_1_NULL");
    }

    // ---- blob_source -------------------------------------------------------

    fn col(name: &str, ty: &str, table: &str, pk: bool, binary: bool) -> Column {
        Column {
            name: name.to_string(),
            type_name: ty.to_string(),
            origin: Some(ColumnOrigin {
                database: "db".to_string(),
                schema: None,
                table: table.to_string(),
                column: name.to_string(),
                flags: ColumnFlags {
                    primary_key: pk,
                    not_null: pk,
                    ..Default::default()
                },
                binary,
                implicit_key: false,
            }),
        }
    }

    /// An expression column: no provenance at all.
    fn expr_col(name: &str, ty: &str) -> Column {
        Column {
            name: name.to_string(),
            type_name: ty.to_string(),
            origin: None,
        }
    }

    fn table_info(table: &str, pk: &[&str], cols: &[(&str, &str)]) -> TableInfo {
        TableInfo {
            schema: None,
            name: table.to_string(),
            columns: cols
                .iter()
                .map(|(n, ty)| ColumnInfo {
                    name: n.to_string(),
                    type_name: ty.to_string(),
                    nullable: !pk.contains(n),
                    primary_key: pk.contains(n),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    /// `SELECT staff_id, picture FROM staff` with one row — the shape the whole
    /// feature is aimed at.
    fn staff_rs() -> ResultSet {
        ResultSet::from_rows(
            vec![
                col("staff_id", "INT", "staff", true, false),
                col("picture", "BLOB", "staff", false, true),
            ],
            vec![vec![
                Value::UInt(1),
                Value::Str("<36365 bytes>".to_string()),
            ]],
        )
    }

    fn staff_schema(_db: &str, _s: Option<&str>, t: &str) -> Option<TableInfo> {
        (t == "staff").then(|| {
            table_info(
                "staff",
                &["staff_id"],
                &[("staff_id", "int"), ("picture", "blob")],
            )
        })
    }

    /// **The composition, which is where this used to break.** `analyze_edit`
    /// once kept a binary column out of `col_table` (C2 in its blanket form), so
    /// `table_index` answered `None` for the very column being fetched and a
    /// `blob_source` written against the write model's own lookup found nothing.
    /// C2 has since narrowed to text, so the column *is* in `col_table` — and
    /// `table_for_origin` still has to answer, because this lookup is the one
    /// that cannot go stale on that question again. Both halves in one place:
    /// the column is writable but not as text, and the fetch finds its table.
    #[test]
    fn a_binary_column_resolves_a_source_and_a_bytes_only_write() {
        let rs = staff_rs();
        let model = crate::edit::analyze_edit(&rs, SqlDialect::MySql, staff_schema);
        assert!(model.editable(1), "the picture column takes a write");
        assert!(
            !model.text_editable(1),
            "but not a text one — its cell is a placeholder"
        );
        assert_eq!(model.table_index(1), Some(0), "it maps to `staff`");

        let got = blob_source(&model, &rs, 0, 1).expect("picture should be fetchable");
        assert_eq!(
            got,
            BlobRef {
                database: "db".to_string(),
                schema: None,
                table: "staff".to_string(),
                column: "picture".to_string(),
                key: vec![("staff_id".to_string(), Value::UInt(1))],
            }
        );
    }

    /// The load ceiling's boundary, which is the only part of a `>` worth
    /// pinning: a file of exactly [`LOAD_CAP`] is the largest the read half can
    /// return whole, so refusing it would refuse a value the app can otherwise
    /// handle end to end.
    #[test]
    fn a_file_of_exactly_the_load_cap_still_fits() {
        assert!(!load_too_large(LOAD_CAP as u64));
        assert!(load_too_large(LOAD_CAP as u64 + 1));
        assert!(!load_too_large(0));
        // The write ceiling is the read ceiling, deliberately: a larger value
        // comes back truncated and refuses to save, so writing one in would
        // create a cell this app can never render or export whole again.
        assert_eq!(LOAD_CAP, FETCH_CAP);
    }

    /// **The four blob families are four different columns**, and MySQL's wire
    /// type name calls all of them `BLOB` — which is why this reads the declared
    /// type and why getting it wrong is silent: a `LONGBLOB` capped at 64 KiB
    /// refuses files the column would take, and a `TINYBLOB` treated as `BLOB`
    /// lets through 65 KiB the server rejects at commit.
    #[test]
    fn each_blob_family_carries_its_own_cap() {
        assert_eq!(column_byte_cap(SqlDialect::MySql, "tinyblob"), Some(255));
        assert_eq!(column_byte_cap(SqlDialect::MySql, "blob"), Some(65_535));
        assert_eq!(
            column_byte_cap(SqlDialect::MySql, "mediumblob"),
            Some(16_777_215)
        );
        assert_eq!(
            column_byte_cap(SqlDialect::MySql, "longblob"),
            Some(4_294_967_295)
        );
        // Case is the server's business, not ours — MySQL's information_schema
        // reports lower-case, the DDL a user typed may not.
        assert_eq!(
            column_byte_cap(SqlDialect::MySql, "MEDIUMBLOB"),
            Some(16_777_215)
        );
        assert_eq!(
            column_byte_cap(SqlDialect::MySql, "MediumBlob"),
            Some(16_777_215)
        );
    }

    /// The leading token decides, so `longblob` is never read as `blob` with a
    /// prefix — the same rule [`crate::model::type_is_binary`] states, and the
    /// reason both split on `(` and whitespace rather than comparing substrings.
    #[test]
    fn a_longer_family_name_is_not_read_as_a_shorter_one() {
        assert_ne!(
            column_byte_cap(SqlDialect::MySql, "longblob"),
            column_byte_cap(SqlDialect::MySql, "blob")
        );
        assert_ne!(
            column_byte_cap(SqlDialect::MySql, "tinyblob"),
            column_byte_cap(SqlDialect::MySql, "blob")
        );
    }

    /// `BINARY(n)`/`VARBINARY(n)` take their bound from the parameter, which is
    /// the whole reason the *declared* type is what this reads: the wire name is
    /// `VARBINARY` with the length stripped off.
    #[test]
    fn a_fixed_width_binary_column_takes_its_cap_from_its_parameter() {
        assert_eq!(column_byte_cap(SqlDialect::MySql, "varbinary(4)"), Some(4));
        assert_eq!(column_byte_cap(SqlDialect::MySql, "binary(16)"), Some(16));
        assert_eq!(
            column_byte_cap(SqlDialect::MySql, "varbinary(65535)"),
            Some(65_535)
        );
        assert_eq!(
            column_byte_cap(SqlDialect::MySql, "VARBINARY( 8 )"),
            Some(8)
        );
    }

    /// **`None` means "no answer", and the two sources of it are different
    /// facts.** `bytea` and SQLite's `BLOB` have no bound a file picker reaches;
    /// a wire type name with its length stripped, or a type nobody wrote down,
    /// is simply unknown. Both leave the server the authority — which is the
    /// only safe way for this to be wrong.
    #[test]
    fn an_unbounded_or_unreadable_type_answers_no_cap() {
        assert_eq!(column_byte_cap(SqlDialect::MySql, "bytea"), None);
        assert_eq!(column_byte_cap(SqlDialect::MySql, "BYTEA"), None);
        // SQLite's declared types, including the untyped column.
        assert_eq!(column_byte_cap(SqlDialect::MySql, ""), None);
        assert_eq!(column_byte_cap(SqlDialect::MySql, "text"), None);
        // A bare `VARBINARY` is the wire name, not a declaration — unknown, not
        // `VARBINARY(1)`, because guessing 1 would refuse every file.
        assert_eq!(column_byte_cap(SqlDialect::MySql, "varbinary"), None);
        assert_eq!(column_byte_cap(SqlDialect::MySql, "binary"), None);
        // And a parameter that is not a number is not a cap.
        assert_eq!(column_byte_cap(SqlDialect::MySql, "varbinary(max)"), None);
    }

    /// **A type name cannot say whose type it is, and this is the whole seam
    /// `R1-L2-02` lived at.** `BLOB` and `VARBINARY(16)` are legal SQLite
    /// declarations, where the family means nothing and the parameter is
    /// ignored outright — that engine types *values*, not columns, and stores a
    /// megabyte in either. Read as MySQL's, they capped the cell at 65,535 and
    /// **16** bytes, so *Load from file* refused files SQLite would have taken.
    #[test]
    fn a_sqlite_column_declares_no_byte_cap_whatever_it_is_called() {
        for ty in [
            "BLOB",
            "blob",
            "tinyblob",
            "mediumblob",
            "longblob",
            "varbinary(16)",
            "binary(4)",
        ] {
            assert_eq!(
                column_byte_cap(SqlDialect::Sqlite, ty),
                None,
                "SQLite's `{ty}` is an affinity, not a bound"
            );
        }
    }

    /// PostgreSQL reaches the same answer from the other side: `bytea` declares
    /// no length at all, so there is nothing to read even before the capability
    /// is asked. The two are not the same fact and both are `None`.
    #[test]
    fn postgres_declares_no_byte_cap_either() {
        assert_eq!(column_byte_cap(SqlDialect::Postgres, "bytea"), None);
        assert_eq!(column_byte_cap(SqlDialect::Postgres, "blob"), None);
    }

    /// **The seam, composed: `analyze_edit` -> `col_cap` -> `byte_cap`.**
    ///
    /// `column_byte_cap` had fourteen unit tests and every one of them named a
    /// MySQL type, so nothing ever asked which engine's table was being read —
    /// and the answer only becomes wrong once a *schema* on a *dialect* flows
    /// through `analyze_edit`'s cap fill into the single consumer, the blob
    /// panel's over-cap refusal. Both legs here, on one fixture, because a test
    /// of either alone is what let this ship.
    #[test]
    fn the_declared_cap_reaches_the_edit_model_only_where_the_engine_keeps_it() {
        let rs = ResultSet::from_rows(
            vec![
                col("id", "INT", "files", true, false),
                col("data", "BLOB", "files", false, true),
            ],
            vec![vec![
                Value::UInt(1),
                Value::Str(crate::model::binary_display(9)),
            ]],
        );
        let schema = |_d: &str, _s: Option<&str>, t: &str| {
            (t == "files")
                .then(|| table_info("files", &["id"], &[("id", "int"), ("data", "mediumblob")]))
        };
        assert_eq!(
            crate::edit::analyze_edit(&rs, SqlDialect::MySql, schema).byte_cap(1),
            Some(16_777_215),
            "MySQL answers an overrun with ERROR 1406, so the cap is worth having"
        );
        assert_eq!(
            crate::edit::analyze_edit(&rs, SqlDialect::Sqlite, schema).byte_cap(1),
            None,
            "SQLite would have stored the file; refusing it is our bug, not its"
        );
    }

    #[test]
    fn a_non_binary_column_has_no_blob_source() {
        let rs = staff_rs();
        let model = crate::edit::analyze_edit(&rs, SqlDialect::MySql, staff_schema);
        assert_eq!(blob_source(&model, &rs, 0, 0), None);
    }

    #[test]
    fn a_column_index_past_the_end_answers_none() {
        let rs = staff_rs();
        let model = crate::edit::analyze_edit(&rs, SqlDialect::MySql, staff_schema);
        assert_eq!(blob_source(&model, &rs, 0, 99), None);
    }

    /// A binary **expression** — `SELECT compress(x)` — has no base column to
    /// re-read, so there is nothing to aim a fetch at.
    #[test]
    fn a_binary_expression_column_has_no_source() {
        let rs = ResultSet::from_rows(
            vec![
                col("id", "INT", "t", true, false),
                expr_col("blob_expr", "BLOB"),
            ],
            vec![vec![Value::UInt(1), Value::Str("<8 bytes>".to_string())]],
        );
        let model = crate::edit::analyze_edit(
            &rs,
            SqlDialect::MySql,
            |_d: &str, _s: Option<&str>, t: &str| {
                (t == "t").then(|| table_info("t", &["id"], &[("id", "int")]))
            },
        );
        assert!(rs.columns[1].is_binary(), "fixture must be a binary column");
        assert_eq!(blob_source(&model, &rs, 0, 1), None);
    }

    /// **No key, no fetch.** A keyless table resolves no `EditTable`, so
    /// `SELECT picture FROM staff WHERE <nothing> LIMIT 1` — which would show
    /// bytes from whichever row the server happened to return — is never built.
    #[test]
    fn a_table_with_no_usable_key_has_no_source() {
        let rs = staff_rs();
        // Same result set, but the schema reports no primary key.
        let model = crate::edit::analyze_edit(
            &rs,
            SqlDialect::MySql,
            |_d: &str, _s: Option<&str>, t: &str| {
                (t == "staff")
                    .then(|| table_info("staff", &[], &[("staff_id", "int"), ("picture", "blob")]))
            },
        );
        assert_eq!(blob_source(&model, &rs, 0, 1), None);
    }

    /// Two same-named tables in different namespaces must not answer for each
    /// other — the reason `table_for_origin` matches the whole triple.
    #[test]
    fn a_same_named_table_in_another_schema_is_not_the_source() {
        let mut picture = col("picture", "BYTEA", "staff", false, true);
        // The column's provenance says `archive`; the model only knows `public`.
        picture.origin.as_mut().unwrap().schema = Some("archive".to_string());
        let mut id = col("staff_id", "INT", "staff", true, false);
        id.origin.as_mut().unwrap().schema = Some("public".to_string());
        let rs = ResultSet::from_rows(
            vec![id, picture],
            vec![vec![Value::UInt(1), Value::Str("<9 bytes>".to_string())]],
        );
        let model = crate::edit::analyze_edit(
            &rs,
            SqlDialect::MySql,
            |_d: &str, _s: Option<&str>, t: &str| {
                (t == "staff").then(|| {
                    table_info(
                        "staff",
                        &["staff_id"],
                        &[("staff_id", "int"), ("picture", "bytea")],
                    )
                })
            },
        );
        assert_eq!(
            blob_source(&model, &rs, 0, 1),
            None,
            "archive.staff must not be keyed by public.staff's row"
        );
    }

    /// SQLite's `rowid` arrives as a binary-ish implicit key on some shapes; it
    /// is no column of the table, so there is nothing to `SELECT`.
    #[test]
    fn an_implicit_key_column_is_never_a_blob_source() {
        let mut c = col("rowid", "BLOB", "t", false, true);
        c.origin.as_mut().unwrap().implicit_key = true;
        let rs = ResultSet::from_rows(
            vec![col("id", "INT", "t", true, false), c],
            vec![vec![Value::UInt(1), Value::Str("<4 bytes>".to_string())]],
        );
        let model = crate::edit::analyze_edit(
            &rs,
            SqlDialect::MySql,
            |_d: &str, _s: Option<&str>, t: &str| {
                (t == "t").then(|| table_info("t", &["id"], &[("id", "int")]))
            },
        );
        assert_eq!(blob_source(&model, &rs, 0, 1), None);
    }

    /// **A text value in a raw-bytes column is not fetchable.**
    ///
    /// SQLite's `BLOB` is an affinity, so one row of such a column can hold
    /// text while another holds bytes — and the fetch would then run `length()`
    /// and `substr` over characters, reporting a character count as octets. The
    /// column-level signal cannot see the difference; the cell's own text can,
    /// because a value whose bytes were dropped is the only one wearing
    /// `binary_display`'s placeholder.
    #[test]
    fn a_text_value_in_a_binary_column_is_not_fetchable() {
        let rs = ResultSet::from_rows(
            vec![
                col("id", "INT", "t", true, false),
                col("payload", "BLOB", "t", false, true),
            ],
            vec![
                // Row 0 really is bytes; row 1 is text in the same column.
                vec![Value::UInt(1), Value::Str("<4 bytes>".to_string())],
                vec![Value::UInt(2), Value::Str("hello".to_string())],
            ],
        );
        let model = crate::edit::analyze_edit(
            &rs,
            SqlDialect::MySql,
            |_d: &str, _s: Option<&str>, t: &str| {
                (t == "t").then(|| table_info("t", &["id"], &[("id", "int")]))
            },
        );
        assert!(
            blob_source(&model, &rs, 0, 1).is_some(),
            "the row that really is bytes must still be fetchable"
        );
        assert_eq!(
            blob_source(&model, &rs, 1, 1),
            None,
            "a text value must not be fetched as though it were bytes"
        );
    }

    /// A NULL cell has no bytes, and fails the per-value signal for that reason
    /// rather than needing its own arm.
    #[test]
    fn a_null_cell_resolves_no_source() {
        let rs = ResultSet::from_rows(
            vec![
                col("id", "INT", "t", true, false),
                col("payload", "BLOB", "t", false, true),
            ],
            vec![vec![Value::UInt(1), Value::Null]],
        );
        let model = crate::edit::analyze_edit(
            &rs,
            SqlDialect::MySql,
            |_d: &str, _s: Option<&str>, t: &str| {
                (t == "t").then(|| table_info("t", &["id"], &[("id", "int")]))
            },
        );
        assert_eq!(blob_source(&model, &rs, 0, 1), None);
    }

    /// A data row past the end answers `None` rather than panicking.
    #[test]
    fn a_row_index_past_the_end_answers_none() {
        let rs = staff_rs();
        let model = crate::edit::analyze_edit(&rs, SqlDialect::MySql, staff_schema);
        assert_eq!(blob_source(&model, &rs, 99, 1), None);
    }

    /// SQLite declares affinities rather than types, so a `BLOB`-affinity column
    /// can report no static binary type — the backend asserts it per value in
    /// [`ResultSet::binary_columns`] instead, and the fetch must follow that
    /// signal as `dropped_binary_columns` already does.
    #[test]
    fn a_dynamically_asserted_binary_column_resolves_a_source() {
        let rs = ResultSet::from_rows(
            vec![
                col("id", "INT", "t", true, false),
                // Neither the type name nor the origin flag says binary.
                col("payload", "", "t", false, false),
            ],
            vec![vec![Value::UInt(7), Value::Str("<4 bytes>".to_string())]],
        );
        assert!(!rs.columns[1].is_binary(), "fixture: no static signal");
        let mut rs = rs;
        rs.binary_columns = vec![1];
        let model = crate::edit::analyze_edit(
            &rs,
            SqlDialect::MySql,
            |_d: &str, _s: Option<&str>, t: &str| {
                (t == "t").then(|| table_info("t", &["id"], &[("id", "int")]))
            },
        );
        let got = blob_source(&model, &rs, 0, 1).expect("dynamic signal should be honoured");
        assert_eq!(got.column, "payload");
        assert_eq!(got.key, vec![("id".to_string(), Value::UInt(7))]);
    }
}
