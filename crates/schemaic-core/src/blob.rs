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
    /// The header parsed and declares more than [`PREVIEW_PIXEL_CAP`] pixels.
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

/// The [`BlobRef`] for the binary cell at `(di, ci)`, or `None` if its bytes
/// cannot be fetched safely.
///
/// **Why this cannot ask [`EditModel::table_index`].** C2 keeps a binary column out
/// of `col_table` — it is not writable as text — so the write model has no
/// table for the very column this is about. The table itself *is* in the model
/// whenever [`crate::edit::analyze_edit`] resolved a key for it, so the lookup
/// is by the column's own provenance instead, and the key comes from
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
    if !col.is_binary() && !rs.binary_columns.contains(&ci) {
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
        // A 24-megapixel photo is large and still legitimate.
        assert_eq!(preview_verdict(Some((6000, 4000))), PreviewVerdict::Show);
    }

    #[test]
    fn the_cap_is_the_boundary_and_not_one_past_it() {
        // Exactly the cap is allowed; one pixel more is not.
        assert_eq!(
            preview_verdict(Some((PREVIEW_PIXEL_CAP as u32, 1))),
            PreviewVerdict::Show
        );
        assert_eq!(
            preview_verdict(Some((PREVIEW_PIXEL_CAP as u32 + 1, 1))),
            PreviewVerdict::TooLarge {
                width: PREVIEW_PIXEL_CAP as u32 + 1,
                height: 1
            }
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

    /// **The composition C2 breaks, not the predicate.** `analyze_edit` keeps a
    /// binary column out of `col_table`, so `table_index` answers `None` for the
    /// very column being fetched — a `blob_source` written against the write
    /// model's own lookup returns `None` here and the feature never offers
    /// itself. This asserts both halves in one place: the write model still
    /// refuses the column, and the fetch still finds its table.
    #[test]
    fn a_binary_column_c2_made_read_only_still_resolves_a_source() {
        let rs = staff_rs();
        let model = crate::edit::analyze_edit(&rs, staff_schema);
        assert!(!model.editable(1), "C2 should still bar the binary column");
        assert_eq!(model.table_index(1), None, "and leave it out of col_table");

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

    #[test]
    fn a_non_binary_column_has_no_blob_source() {
        let rs = staff_rs();
        let model = crate::edit::analyze_edit(&rs, staff_schema);
        assert_eq!(blob_source(&model, &rs, 0, 0), None);
    }

    #[test]
    fn a_column_index_past_the_end_answers_none() {
        let rs = staff_rs();
        let model = crate::edit::analyze_edit(&rs, staff_schema);
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
        let model = crate::edit::analyze_edit(&rs, |_d: &str, _s: Option<&str>, t: &str| {
            (t == "t").then(|| table_info("t", &["id"], &[("id", "int")]))
        });
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
        let model = crate::edit::analyze_edit(&rs, |_d: &str, _s: Option<&str>, t: &str| {
            (t == "staff")
                .then(|| table_info("staff", &[], &[("staff_id", "int"), ("picture", "blob")]))
        });
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
        let model = crate::edit::analyze_edit(&rs, |_d: &str, _s: Option<&str>, t: &str| {
            (t == "staff").then(|| {
                table_info(
                    "staff",
                    &["staff_id"],
                    &[("staff_id", "int"), ("picture", "bytea")],
                )
            })
        });
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
        let model = crate::edit::analyze_edit(&rs, |_d: &str, _s: Option<&str>, t: &str| {
            (t == "t").then(|| table_info("t", &["id"], &[("id", "int")]))
        });
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
        let model = crate::edit::analyze_edit(&rs, |_d: &str, _s: Option<&str>, t: &str| {
            (t == "t").then(|| table_info("t", &["id"], &[("id", "int")]))
        });
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
        let model = crate::edit::analyze_edit(&rs, |_d: &str, _s: Option<&str>, t: &str| {
            (t == "t").then(|| table_info("t", &["id"], &[("id", "int")]))
        });
        assert_eq!(blob_source(&model, &rs, 0, 1), None);
    }

    /// A data row past the end answers `None` rather than panicking.
    #[test]
    fn a_row_index_past_the_end_answers_none() {
        let rs = staff_rs();
        let model = crate::edit::analyze_edit(&rs, staff_schema);
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
        let model = crate::edit::analyze_edit(&rs, |_d: &str, _s: Option<&str>, t: &str| {
            (t == "t").then(|| table_info("t", &["id"], &[("id", "int")]))
        });
        let got = blob_source(&model, &rs, 0, 1).expect("dynamic signal should be honoured");
        assert_eq!(got.column, "payload");
        assert_eq!(got.key, vec![("id".to_string(), Value::UInt(7))]);
    }
}
