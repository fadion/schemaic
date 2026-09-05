//! Result-set export — pure over [`ResultSet`] + a display order, no UI.
//!
//! `order` is the display→data-row permutation (post-sort); callers pass the
//! grid's live order so exports match what's on screen.
//!
//! [`ExportFormat`] is the single value the grid's two menus dispatch on — Copy
//! (to the clipboard) and Download (to a file) — so the label, extension,
//! suggested file name and rendering can't drift between them.
//!
//! Every renderer comes in two shapes. The `*_to` functions write into any
//! [`std::io::Write`], which is what the file export uses: a 200k-row result
//! rendered into a `String` first is a second full copy of the data — hundreds of
//! megabytes on a wide result — held only to hand it to `fs::write`. Streaming it
//! into a `BufWriter` keeps that cost to the buffer. The `String`-returning
//! versions are thin wrappers over them, kept for the clipboard, which has no
//! streaming API to target. Both share one implementation, so the two paths can't
//! drift — a test asserts they agree byte-for-byte in every format.

use std::io::{self, Write};

use crate::intel::SqlDialect;
use crate::model::{ResultSet, Value};

/// Run a `*_to` renderer into a `String`. Writing into a `Vec<u8>` can't fail and
/// every renderer emits `&str`, so both the io error and the UTF-8 check are
/// unreachable — `unwrap_or_default` keeps that from becoming a panic path.
fn to_string(f: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> String {
    let mut buf = Vec::new();
    match f(&mut buf) {
        Ok(()) => String::from_utf8(buf).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// The formats the results grid can export a result set to — one value driving
/// the menu label, the file extension, the suggested file name, and the rendering,
/// so "copy to clipboard" and "save to file" can't drift apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Csv,
    /// `INSERT` statements, in the connection's dialect.
    Sql,
    Markdown,
    Html,
    /// An Excel workbook — **the only binary format**, which is why
    /// [`ExportFormat::is_text`] exists.
    Xlsx,
}

impl ExportFormat {
    /// Every format, in the order the grid's menus list them.
    pub const ALL: [ExportFormat; 6] = [
        ExportFormat::Json,
        ExportFormat::Csv,
        ExportFormat::Sql,
        ExportFormat::Markdown,
        ExportFormat::Html,
        ExportFormat::Xlsx,
    ];

    /// The menu label.
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Json => "JSON",
            ExportFormat::Csv => "CSV",
            ExportFormat::Sql => "SQL",
            ExportFormat::Markdown => "Markdown",
            ExportFormat::Html => "HTML",
            // The application, not the extension: this is the word a user scans
            // a menu for, and `.xlsx` is already on the file the dialog names.
            ExportFormat::Xlsx => "Excel",
        }
    }

    /// Can this format's output be put on the clipboard?
    ///
    /// **A capability, not a variant test, and the Copy menu is the reason.** That
    /// menu renders every format through [`Self::render`], which returns a
    /// `String` — [`to_string`]'s `unwrap_or_default` turns a rendering that is
    /// not UTF-8 into the *empty* string, so a binary format listed there would
    /// silently clear the clipboard and report nothing. The menu filters on this
    /// instead, exactly as the ER diagram's does with
    /// [`crate::erd_export::ErdExportFormat::is_text`] for PNG — the same problem,
    /// already solved once.
    ///
    /// Computed from the variant rather than stored, so a seventh format has to
    /// answer it.
    pub fn is_text(self) -> bool {
        !matches!(self, ExportFormat::Xlsx)
    }

    /// Does this format reach the sink **as it goes**, or only at the end?
    ///
    /// **A capability, beside [`Self::is_text`], and the answer to a question
    /// the export plumbing was written before anything could answer `false`
    /// to.** The five text formats write each row as they see it, so a failure
    /// halfway leaves a `.part` sibling holding the rows that arrived and
    /// `export_failure_note` can honestly point at it. `Xlsx` is a ZIP: nothing
    /// reaches the sink until `save_to_writer`, so *every* pre-save failure —
    /// both ceilings, a dropped connection, a query error mid-stream — left a
    /// **zero-byte** file the user was directed to, and a failure during the
    /// save leaves a truncated archive, which is worse than empty because it
    /// looks like a workbook and will not open.
    ///
    /// Computed from the variant rather than stored, so a seventh format has to
    /// answer it.
    pub fn writes_incrementally(self) -> bool {
        !matches!(self, ExportFormat::Xlsx)
    }

    /// The formats a **Copy** menu may offer, in menu order — [`Self::ALL`]
    /// filtered by [`Self::is_text`].
    ///
    /// A function rather than the filter written inline at the call site,
    /// because the predicate and its application are two different things and
    /// only one of them was pinned: `is_text` had a test and the menu's
    /// `.filter(|f| f.is_text())` did not, so deleting the filter left the whole
    /// suite green and shipped an Excel entry that clears the clipboard. The
    /// composition is the part that can regress, so the composition is what has
    /// a name and a test.
    ///
    /// Download menus keep offering [`Self::ALL`] — a file can hold bytes.
    pub fn clipboard_formats() -> impl Iterator<Item = ExportFormat> {
        Self::ALL.into_iter().filter(|f| f.is_text())
    }

    /// The file extension, without the leading dot.
    pub fn extension(self) -> &'static str {
        self.extensions()[0]
    }

    /// The extensions as a `'static` slice, for a file dialog's type filter.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            ExportFormat::Json => &["json"],
            ExportFormat::Csv => &["csv"],
            ExportFormat::Sql => &["sql"],
            ExportFormat::Markdown => &["md", "markdown"],
            ExportFormat::Html => &["html", "htm"],
            ExportFormat::Xlsx => &["xlsx"],
        }
    }

    /// Render `rs` (in display `order`) in this format. `source` is the result's
    /// real `(database, namespace, table)` when known — only [`ExportFormat::Sql`]
    /// uses it, to name the `INSERT` target.
    ///
    /// **Only meaningful for a format [`Self::is_text`] accepts**, and it
    /// answers a binary one with the empty string *without rendering it* — the
    /// alternative was building the whole workbook, temp file and ZIP included,
    /// to throw the bytes away at the UTF-8 check. The clipboard menu filters
    /// the format list so this is never reached in the app, but the method is
    /// public and the cost would be invisible at the call site.
    pub fn render(
        self,
        rs: &ResultSet,
        order: &[usize],
        source: Option<(&str, Option<&str>, &str)>,
        dialect: SqlDialect,
    ) -> String {
        if !self.is_text() {
            return String::new();
        }
        to_string(|w| self.render_to(w, rs, order, source, dialect).map(|_| ()))
    }

    /// Render into `w` in **one chunk**, so a large result never exists twice in
    /// memory. Identical output to [`Self::render`], returning what was written
    /// and what could not be carried ([`ExportTally`]).
    ///
    /// **No file export goes through this any more.** It was the file path, and
    /// the app's `Fetched` export moved off it onto [`SliceChunks`] when that
    /// export gained a progress modal — a single chunk offers no moment at which
    /// to report a count or notice a Stop. What is left is the one-chunk
    /// spelling: [`Self::render`] (the clipboard) is built on it, and so are the
    /// tests. `chunking_a_fetched_result_cannot_change_the_bytes` is what holds
    /// the two paths byte-identical.
    ///
    /// Errors are the writer's own (a full disk, a revoked permission). They must
    /// reach the user: unlike [`Self::render`], which either produces the whole
    /// text or nothing, a failure here leaves a **truncated file** that looks
    /// complete.
    pub fn render_to<W: Write + Send>(
        self,
        w: &mut W,
        rs: &ResultSet,
        order: &[usize],
        source: Option<(&str, Option<&str>, &str)>,
        dialect: SqlDialect,
    ) -> io::Result<ExportTally> {
        self.stream_to(w, &mut OneChunk::new(rs, order), source, dialect)
    }

    /// Stream this rendering from a [`RowChunks`] source, returning what it wrote
    /// — what an export of a whole table uses, where the rows arrive from the
    /// server in blocks and must reach the file as they come rather than be
    /// gathered first.
    ///
    /// [`Self::render_to`] is this over a source of one chunk, so the two cannot
    /// drift: there is one renderer per format, not a buffered one and a streamed
    /// one that have to be kept in agreement.
    /// `W: Send` is [`export_xlsx_chunks`]'s requirement, not this dispatch's:
    /// `rust_xlsxwriter` hands the writer to the thread that assembles the
    /// workbook's ZIP. Every sink an export already writes to — a
    /// `BufWriter<File>` and a `Vec<u8>` — satisfies it.
    pub fn stream_to<W: Write + Send>(
        self,
        w: &mut W,
        src: &mut dyn RowChunks,
        source: Option<(&str, Option<&str>, &str)>,
        dialect: SqlDialect,
    ) -> io::Result<ExportTally> {
        match self {
            ExportFormat::Json => export_json_chunks(w, src),
            ExportFormat::Csv => export_csv_chunks(w, src),
            ExportFormat::Sql => export_inserts_chunks(w, src, source, dialect),
            ExportFormat::Markdown => export_markdown_chunks(w, src),
            ExportFormat::Html => export_html_chunks(w, src),
            ExportFormat::Xlsx => export_xlsx_chunks(w, src, source),
        }
    }
}

/// What an export wrote, and what it **could not carry**.
///
/// The row count was the whole of an export's result until it turned out that
/// two kinds of loss are invisible in the file itself:
///
/// - **Withheld bytes.** A raw-bytes cell has no `Value` variant to hold it, so
///   it arrives as [`crate::model::binary_display`]'s `<n bytes>` placeholder,
///   and the formats Schemaic reads back must not write that text as data (see
///   [`dropped_binary_columns`]). The SQL emitter says so in a `-- NOTE:` line;
///   CSV and JSON have no comment syntax, so an empty field was the only trace
///   — and an empty field is what a NULL looks like. Those two formats are
///   exactly `import::ImportFormat`, so the export a user reaches for as a
///   portable copy was the one that silently dropped a column.
/// - **Blanked cells.** A column's text arena stops at 512 MiB and does not
///   fail: every cell past the ceiling reads back as the empty string
///   ([`ResultSet::capped_columns`]). The grid
///   surfaces that with its own note, but a streamed chunk is never mounted in a
///   grid, so a whole-table export of a wide-text column could write a file with
///   holes in it and report a full row count. `RowChunks`' own doc names this
///   hazard as the reason the trait exists; nothing read the flag.
///
/// Both are *columns*, named, because that is what a user needs to know which
/// part of the file to distrust. Neither is an error: the file is written and
/// the rows in it are real. What must not happen is the caveat going unsaid,
/// which is [`export_note`]'s job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportTally {
    /// Rows written to the file.
    pub rows: u64,
    /// Columns whose bytes could not be carried, in column order. Empty for
    /// Markdown and HTML, which keep the placeholder deliberately — nothing
    /// reads those back, and there the placeholder is the useful rendering.
    pub withheld: Vec<String>,
    /// Columns whose cells past the arena ceiling were written **blank**.
    pub blanked: Vec<String>,
    /// Columns whose text was **cut** to fit a cell of the target format, in
    /// column order. Only [`ExportFormat::Xlsx`] can produce this: a worksheet
    /// cell holds at most [`XLSX_MAX_CELL_CHARS`] characters, and a `TEXT` or
    /// `JSON` column routinely holds more.
    ///
    /// A third category rather than folding into [`Self::blanked`] because the
    /// two say different things to the person deciding whether to trust the file:
    /// a blanked cell is empty and looks like a NULL, a cut one *looks
    /// complete*. It is also the only one of the three the user can act on —
    /// export that column as CSV or JSON instead, which have no cell ceiling.
    ///
    /// **`cut` and not `truncated`**, which is the word this wanted:
    /// [`ResultSet::truncated`](crate::model::ResultSet::truncated) already
    /// means something else — the *row cap* was hit — and both are read in the
    /// same export and grid paths, `grid.rs` asking `rs.truncated` for the
    /// export-scope split a few lines from where it receives this tally. Two
    /// fields of the same name meaning different losses is how a caveat ends up
    /// reporting the wrong one. Same reason the arena-cap category is `blanked`
    /// rather than `capped`.
    pub cut: Vec<String>,
}

impl ExportTally {
    /// Fold one chunk's losses in, keeping column order and never repeating a
    /// name: a streamed export sees the same column in every chunk, and a
    /// caveat that named `body` two hundred times would say less than one that
    /// names it once.
    fn note(&mut self, rs: &ResultSet, withheld: &[usize]) {
        for &ci in withheld {
            if let Some(c) = rs.columns.get(ci)
                && !self.withheld.contains(&c.name)
            {
                self.withheld.push(c.name.clone());
            }
        }
        for &ci in &rs.capped_columns {
            if let Some(c) = rs.columns.get(ci)
                && !self.blanked.contains(&c.name)
            {
                self.blanked.push(c.name.clone());
            }
        }
    }

    /// Record that column `ci`'s text did not fit a cell and was cut. Named
    /// once, however many rows it happened on — [`Self::note`]'s rule.
    fn note_cut(&mut self, rs: &ResultSet, ci: usize) {
        if let Some(c) = rs.columns.get(ci)
            && !self.cut.contains(&c.name)
        {
            self.cut.push(c.name.clone());
        }
    }

    /// Did anything about this export need saying beyond the row count?
    pub fn has_caveat(&self) -> bool {
        !self.withheld.is_empty() || !self.blanked.is_empty() || !self.cut.is_empty()
    }

    /// Fold **another table's** tally into this one — a dump writes many tables
    /// into one file and reports one sentence about it.
    ///
    /// Rows sum; a column name is kept **once**, in first-seen order. Two tables
    /// that both have a `body` column too wide to carry say `body` once, and two
    /// tables with different withheld columns keep both — which is the whole
    /// content of a caveat that has to name what to distrust.
    ///
    /// It lives here rather than in the dump's writer loop because it is the same
    /// question [`ExportTally::note`] answers one level down, and a fold written
    /// out at the call site is one nothing can test: the caller needs a `Db`, a
    /// runtime handle and two channels to reach.
    pub fn absorb(&mut self, other: ExportTally) {
        self.rows += other.rows;
        for c in other.withheld {
            if !self.withheld.contains(&c) {
                self.withheld.push(c);
            }
        }
        for c in other.blanked {
            if !self.blanked.contains(&c) {
                self.blanked.push(c);
            }
        }
        for c in other.cut {
            if !self.cut.contains(&c) {
                self.cut.push(c);
            }
        }
    }
}

/// One block of rows on its way out — a [`ResultSet`] and the rows to take from
/// it, as display indices into it.
///
/// Every chunk of one export must carry the **same columns**: the header is
/// written from the first chunk and never revisited, so a source that changed
/// them mid-stream would produce a file whose header describes only its opening
/// rows.
pub struct RowChunk<'a> {
    pub rs: &'a ResultSet,
    pub order: &'a [usize],
}

/// A pull source of rows for an export.
///
/// **The reason exports take one of these rather than a `&ResultSet`.** A result
/// the grid is showing is bounded by the row cap and already in memory, so
/// handing it over whole costs nothing; a *table* export is neither. Materialising
/// 2 million rows to render them would hold the table twice over and run into the
/// 512 MiB per-column arena ceiling, which does not fail — it blanks the cells
/// past it ([`ResultSet::capped_columns`]) — so the export would quietly write a
/// file with holes in it. Pulling chunks bounds the memory at one chunk and lets
/// the rows go to disk as they come off the wire.
///
/// **Pull and not push**, because JSON decides it. Its renderer holds a
/// `serde_json` sequence serializer open across the whole array, borrowing the
/// writer; a push API would have to store that across calls and it is not a type
/// that can be stored. Inverting it keeps the serializer a local in one function,
/// which is also what keeps the five formats to one implementation each — the
/// `export_*_to` entry points are this same code over a source of exactly one
/// chunk ([`OneChunk`]), not a second copy of it.
///
/// The lifetime on the returned chunk ties it to the borrow of `self`, so a
/// source may hand out a view of a buffer it reuses: only one chunk is alive at
/// a time, by construction.
pub trait RowChunks {
    /// The next block of rows, or `None` when the source is exhausted.
    ///
    /// An error aborts the export. It is an [`io::Error`] because that is what
    /// the writer already returns and what the caller must report either way — a
    /// source that fails for its own reasons (a dropped connection, a cancelled
    /// export) wraps that reason in one.
    fn next_chunk(&mut self) -> io::Result<Option<RowChunk<'_>>>;
}

/// A [`RowChunks`] over one already-materialised result — what the grid's export
/// uses.
///
/// It yields exactly one chunk, **even when the result has no rows**, which is
/// load-bearing rather than an edge case: CSV, Markdown and HTML take their
/// header from the first chunk, so a source that yielded nothing for an empty
/// result would write an empty file where the old code wrote a header.
pub struct OneChunk<'a> {
    rs: &'a ResultSet,
    order: &'a [usize],
    done: bool,
}

impl<'a> OneChunk<'a> {
    pub fn new(rs: &'a ResultSet, order: &'a [usize]) -> Self {
        OneChunk {
            rs,
            order,
            done: false,
        }
    }
}

impl RowChunks for OneChunk<'_> {
    fn next_chunk(&mut self) -> io::Result<Option<RowChunk<'_>>> {
        if self.done {
            return Ok(None);
        }
        self.done = true;
        Ok(Some(RowChunk {
            rs: self.rs,
            order: self.order,
        }))
    }
}

/// A [`RowChunks`] over rows **already in memory**, handed out `size` at a time.
///
/// [`OneChunk`]'s sibling, and the difference is the only thing it exists for:
/// progress. A fetched export has every row in hand, so it *could* be rendered
/// in one block — and was — but then there is no moment between "started" and
/// "finished" at which anything can be reported, and a large export to a slow
/// format looks identical to a hung one.
///
/// **Nothing is copied.** A chunk is the same `&ResultSet` and a *slice* of the
/// same order vector, so this is a cursor over one allocation rather than a
/// partition of the rows. That is also why it borrows rather than owning: the
/// caller already holds both for the length of the export.
///
/// A `size` of `0` is clamped to the whole slice rather than rejected — the
/// figure comes from a tuning constant, and an export that spins handing out
/// empty chunks forever is a worse answer than one that writes a single block.
pub struct SliceChunks<'a> {
    rs: &'a ResultSet,
    order: &'a [usize],
    at: usize,
    size: usize,
    /// Has a chunk been handed out yet? What makes the **empty** result yield one
    /// anyway — see [`RowChunks::next_chunk`]'s implementation below.
    started: bool,
    #[allow(clippy::type_complexity)]
    watch: Option<Box<dyn FnMut(u64) -> bool + 'a>>,
}

impl<'a> SliceChunks<'a> {
    pub fn new(rs: &'a ResultSet, order: &'a [usize], size: usize) -> Self {
        SliceChunks {
            rs,
            order,
            at: 0,
            size: if size == 0 { usize::MAX } else { size },
            started: false,
            watch: None,
        }
    }

    /// Watch the export go by: `f` is called as each chunk is handed out, with
    /// the **running row total**, and returning `false` stops the export.
    ///
    /// **One hook doing both jobs, on purpose.** They are asked at the same
    /// instant and answered from the same place — the caller reports the count
    /// to its progress channel and asks its cancellation token in one closure —
    /// and two hooks would be two chances to check them at different moments,
    /// which is how a Stop comes to be noticed one chunk late.
    ///
    /// A `false` surfaces as an [`io::Error`], because that is the only way a
    /// [`RowChunks`] can end an export *without* it looking finished: the
    /// renderers treat `Ok(None)` as end-of-stream and would publish a truncated
    /// file as a complete one. Same reasoning, and the same failure, as
    /// `PullChunks`' cancelled-read arm.
    pub fn watching(mut self, f: impl FnMut(u64) -> bool + 'a) -> Self {
        self.watch = Some(Box::new(f));
        self
    }
}

impl RowChunks for SliceChunks<'_> {
    fn next_chunk(&mut self) -> io::Result<Option<RowChunk<'_>>> {
        // **An empty result still yields one (empty) chunk**, exactly as
        // [`OneChunk`] does — which is what `started` is for, and the opposite of
        // what stood here first.
        //
        // Five renderers write their header *on the first chunk they see*. Ending
        // the stream before any chunk therefore writes no header at all: a
        // `SELECT` that matched nothing exported a **0-byte file** where the
        // whole-render path wrote `id,name\n`. The rule that reads as safe here
        // is the one that loses the file, and
        // `an_empty_result_writes_the_same_bytes_through_either_path` is what
        // holds the two paths together.
        if self.at >= self.order.len() && self.started {
            return Ok(None);
        }
        self.started = true;
        let end = self.at.saturating_add(self.size).min(self.order.len());
        let range = self.at..end;
        self.at = end;
        // Before the chunk is handed over, so a stop is acted on rather than
        // reported after the rows it was meant to prevent were written.
        if let Some(w) = self.watch.as_mut()
            && !w(end as u64)
        {
            return Err(io::Error::other("export cancelled"));
        }
        Ok(Some(RowChunk {
            rs: self.rs,
            order: &self.order[range],
        }))
    }
}

/// A [`RowChunks`] over whole result sets pulled from `next` — every row of each,
/// in the order the server sent them.
///
/// The adapter between a loader that produces results and an export that consumes
/// rows. `next` blocks: on the export's own thread that is the point, since the
/// alternative is a buffer that grows to whatever the disk is behind by.
///
/// The natural order is materialised as a `Vec<usize>` rather than threaded
/// through the renderers as an `Option`, and the buffer is **reused** across
/// chunks — a chunk is thousands of rows, so this is one allocation for the whole
/// export, against five renderers that would each need a second code path.
pub struct PullChunks<F> {
    next: F,
    cur: Option<ResultSet>,
    order: Vec<usize>,
}

impl<F> PullChunks<F>
where
    F: FnMut() -> io::Result<Option<ResultSet>>,
{
    pub fn new(next: F) -> Self {
        PullChunks {
            next,
            cur: None,
            order: Vec::new(),
        }
    }
}

impl<F> RowChunks for PullChunks<F>
where
    F: FnMut() -> io::Result<Option<ResultSet>>,
{
    fn next_chunk(&mut self) -> io::Result<Option<RowChunk<'_>>> {
        let Some(rs) = (self.next)()? else {
            return Ok(None);
        };
        let n = rs.row_count();
        self.order.clear();
        self.order.extend(0..n);
        self.cur = Some(rs);
        Ok(Some(RowChunk {
            // `cur` was just assigned, so the borrow is of a live value.
            rs: self.cur.as_ref().expect("just assigned"),
            order: &self.order,
        }))
    }
}

/// A default file name for saving a result: the source table's display name when
/// the tab has one, else `result` — plus the format's extension.
///
/// `base` is **sanitized**, not trusted: a table name is server-controlled and may
/// hold characters no filesystem accepts (`/`, `:`, `*`, …), so those become `_`.
/// Windows also rejects a trailing dot or space and reserves a handful of device
/// names, so the stem is trimmed and a reserved stem is prefixed. A base that
/// sanitizes away to nothing falls back to `result`.
pub fn suggested_filename(base: Option<&str>, format: ExportFormat) -> String {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let mut stem: String = base
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Windows won't accept a name ending in a dot or space.
    stem = stem.trim_end_matches(['.', ' ']).trim_start().to_string();
    // Keep the whole name comfortably inside the usual 255-byte component limit.
    if stem.chars().count() > 100 {
        stem = stem.chars().take(100).collect();
    }
    if stem.is_empty() {
        stem = "result".to_string();
    }
    if RESERVED.contains(&stem.to_ascii_uppercase().as_str()) {
        stem = format!("_{stem}");
    }
    format!("{stem}.{}", format.extension())
}

/// One file name per table, for an export that writes a **folder** rather than a
/// file — the schema tree's `Export ▸ CSV` and its siblings.
///
/// Each name is [`suggested_filename`]'s, so the sanitizing rule is stated once;
/// what this adds is the part a single file never needed: **uniqueness**. Two
/// distinct tables can sanitize to one name (`a:b` and `a*b` both become `a_b`),
/// and on Windows and macOS `Orders` and `orders` are one file — so a folder
/// export that trusted the names would write the second table over the first and
/// still report both. A later name gets `_2`, `_3`, … before the extension.
///
/// A generated suffix never takes a name some **other** table would have had:
/// a real `a_b_2` standing beside two tables that both want `a_b` would
/// otherwise be handed `a_b_2.csv` by the deduplicator, and the real one pushed
/// to `a_b_2_2.csv`.
///
/// **Every base name is reserved before any is handed out**, which is what makes
/// that independent of the order the tables arrive in. Checking the suffix
/// against the names issued *so far* only worked while the real `a_b_2` came
/// first — and the picker sorts, where `*` (42) and `:` (58) both precede `_`
/// (95), so it never did: `a*b`, `a:b`, `a_b_2` put `a:b`'s rows in
/// `a_b_2.csv`, which is the file a user would open expecting `a_b_2`'s.
///
/// Returned in `tables`' order, one entry each, so a caller can zip the two.
pub fn export_file_names(tables: &[String], format: ExportFormat) -> Vec<String> {
    let ext = format.extension();
    let base: Vec<String> = tables
        .iter()
        .map(|t| suggested_filename(Some(t), format))
        .collect();
    // Case-insensitively, because `Orders` and `orders` are one file on Windows
    // and macOS — the same reason `taken` is keyed that way.
    let reserved: std::collections::HashSet<String> =
        base.iter().map(|n| n.to_lowercase()).collect();
    let mut taken: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(tables.len());
    for name in base {
        // The stem, so the counter goes before the extension rather than after
        // it — `a_b_2.csv`, not `a_b.csv_2`.
        let stem = name
            .strip_suffix(&format!(".{ext}"))
            .unwrap_or(&name)
            .to_string();
        let mut candidate = name;
        let mut n = 1u32;
        loop {
            let key = candidate.to_lowercase();
            // `n > 1` is what lets a table keep its *own* name: the reservation
            // set holds it, and only a **generated** suffix has to step around
            // the set.
            if !(n > 1 && reserved.contains(&key)) && taken.insert(key) {
                break;
            }
            n += 1;
            candidate = format!("{stem}_{n}.{ext}");
        }
        out.push(candidate);
    }
    out
}

/// A cell as a JSON value (non-finite floats → null).
pub fn value_to_json(v: &Value) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        Value::Null => J::Null,
        Value::Int(i) => J::from(*i),
        Value::UInt(u) => J::from(*u),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(J::Number)
            .unwrap_or(J::Null),
        Value::Str(s) => J::String(s.clone()),
    }
}

/// Quote a CSV field if it contains a delimiter/quote/newline, and neutralize
/// spreadsheet formula/DDE injection (§7.5): a value a spreadsheet would evaluate
/// as a formula — leading `=`, `+`, `@`, `-`, or a `\t`/`\r` control char — is
/// prefixed with a single quote so Excel/Sheets import it as text (a cell
/// `=HYPERLINK(...)` otherwise executes on open).
///
/// **Leading `-` is guarded only when the value isn't a number.** It was once
/// skipped entirely, on the grounds that prefixing it would corrupt every
/// negative value — but that dichotomy isn't forced. `-1+1+cmd|' /C calc'!A0` is
/// a DDE payload and `-5.25` is a number, and [`is_negative_number`] tells them
/// apart, so both cases can be served.
/// Is `s` a plain negative number — the one leading-`-` shape a spreadsheet
/// should be allowed to evaluate?
///
/// Deliberately strict: a decimal or scientific-notation literal and nothing
/// else. Anything a formula could hide in — an operator, a cell reference, a
/// `|` DDE separator — fails, and a false negative only costs a leading
/// apostrophe on a value that wasn't a number anyway.
fn is_negative_number(s: &str) -> bool {
    // A lone `-` never reaches here (the caller skips it — there is nothing after
    // the sign for a formula to hide in), so `rest` is non-empty in practice.
    let rest = &s[1..];
    if rest.is_empty() {
        return false;
    }
    // At most one exponent, split on it; each part must look numeric.
    let (mantissa, exponent) = match rest.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (rest, None),
    };
    let mantissa_ok = !mantissa.is_empty()
        && mantissa.bytes().filter(|b| *b == b'.').count() <= 1
        && mantissa.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && mantissa.bytes().any(|b| b.is_ascii_digit());
    let exponent_ok = match exponent {
        None => true,
        Some(e) => {
            let digits = e.strip_prefix(['+', '-']).unwrap_or(e);
            !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
        }
    };
    mantissa_ok && exponent_ok
}

pub fn csv_field(s: &str) -> String {
    let guarded;
    let s = if matches!(
        s.as_bytes().first(),
        Some(b'=' | b'+' | b'@' | b'\t' | b'\r')
    ) || (s.len() > 1 && s.starts_with('-') && !is_negative_number(s))
    {
        guarded = format!("'{s}");
        guarded.as_str()
    } else {
        s
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// A cell as a SQL literal (non-finite float → NULL; strings escaped).
///
/// **Backslashes are dialect-critical.** MySQL treats `\` as an escape character
/// inside a string literal, so it must be doubled. PostgreSQL, under its default
/// `standard_conforming_strings = on` (since 9.1), takes a backslash literally —
/// doubling it there would silently *corrupt* the value (`C:\tmp` → `C:\\tmp`).
/// Doubling the single quote is the injection guard on both.
pub fn sql_literal(v: &Value, dialect: SqlDialect) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) if !f.is_finite() => "NULL".to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => {
            let escaped = match dialect {
                SqlDialect::MySql => s.replace('\\', "\\\\").replace('\'', "''"),
                // SQLite is standard-conforming like Postgres and has no
                // backslash escape at all, so doubling one would corrupt the
                // value exactly as it would there.
                SqlDialect::Postgres | SqlDialect::Sqlite => s.replace('\'', "''"),
            };
            format!("'{escaped}'")
        }
    }
}

/// Quote a SQL identifier in the connection's dialect, doubling the embedded
/// quote character: MySQL `` `name` ``, PostgreSQL and SQLite `"name"`. The
/// *other* dialect's quote char is an ordinary character and passes through
/// untouched.
///
/// SQLite accepts `"x"`, `` `x` `` and `[x]` when *reading* (the lexer knows all
/// three), but **emits only the standard form**: `"` is the one with a defined
/// escape, since a `]` cannot be written inside brackets at all, so a name
/// containing one would be unquotable in the form we chose to generate.
pub fn ident_sql(name: &str, dialect: SqlDialect) -> String {
    match dialect {
        SqlDialect::MySql => format!("`{}`", name.replace('`', "``")),
        SqlDialect::Postgres | SqlDialect::Sqlite => {
            format!("\"{}\"", name.replace('"', "\"\""))
        }
    }
}

/// The same, but only when leaving the name bare would name something else.
///
/// A bare identifier is safe exactly when it is a plain lower-case ASCII word and
/// not reserved: PostgreSQL folds an unquoted name to lower case, so anything with
/// an upper-case letter, a space, punctuation or a non-ASCII byte has to be quoted
/// there, and a reserved word has to be quoted on either engine.
///
/// This is the rule for SQL Schemaic *generates for the user to read* — the
/// completion layer's auto-join and star expansion, and `filter::table_query`'s
/// `ORDER BY`. Quoting unconditionally would be simpler, but it would put
/// backticks around every ordinary MySQL name in text the user is about to edit.
/// For SQL that is only executed, [`ident_sql`] and its unconditional quoting is
/// the right choice and stays what the write paths use.
pub fn ident_if_needed(name: &str, dialect: SqlDialect) -> String {
    let plain = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !name.as_bytes()[0].is_ascii_digit()
        // The identifier question, not the alias one: `CAST`/`IF`/`RAISE` are
        // valid SQLite aliases but cannot be a bare column or table name.
        && !crate::intel::must_quote_ident(name, dialect);
    if plain {
        name.to_string()
    } else {
        ident_sql(name, dialect)
    }
}

/// The whole result as a pretty JSON array of row objects (keyed by column name).
/// Duplicate column names (e.g. `a.id, b.id` from a join) are suffixed `_2`,
/// `_3`, … so a JSON object doesn't silently drop all but the last (§7.4).
pub fn export_json(rs: &ResultSet, order: &[usize]) -> String {
    to_string(|w| export_json_to(w, rs, order))
}

/// One row as a JSON object, with the keys in **column order**.
///
/// Not a `serde_json::Map`: that's a `BTreeMap` (the `preserve_order` feature
/// isn't on), so building one sorts the keys alphabetically and a `SELECT id,
/// name` exported as `{"name": …, "id": …}` — the column order the user chose,
/// silently discarded. Emitting the entries directly keeps it.
struct RowObject<'a> {
    rs: &'a ResultSet,
    keys: &'a [String],
    /// Which columns' text stands in for bytes this result never carried —
    /// [`binary_mask`]. Computed once for the whole export, not per row.
    mask: &'a [bool],
    di: usize,
}

impl serde::Serialize for RowObject<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(Some(self.keys.len()))?;
        for (ci, key) in self.keys.iter().enumerate() {
            let v = self
                .rs
                .cell(self.di, ci)
                // JSON is a format Schemaic reads back, so a blob's placeholder
                // becomes `null` rather than the string — see
                // [`dropped_binary_columns`].
                .filter(|c| !withheld_binary(self.mask, ci, c))
                .map(|c| value_to_json(&c.to_value()))
                .unwrap_or(serde_json::Value::Null);
            m.serialize_entry(key, &v)?;
        }
        m.end()
    }
}

/// [`export_json`], streamed.
///
/// This is the one format that genuinely had to buffer: it built the entire array
/// as a `serde_json::Value` before `to_string_pretty` could see it, so a large
/// export held the rows a third time (result set, `Value` tree, output string).
/// Serializing the array element-by-element through a `serde_json::Serializer`
/// emits the same pretty output while only ever holding one row.
pub fn export_json_to<W: Write>(w: &mut W, rs: &ResultSet, order: &[usize]) -> io::Result<()> {
    export_json_chunks(w, &mut OneChunk::new(rs, order)).map(|_| ())
}

/// [`export_json_to`] over a [`RowChunks`] source — the form the whole-table
/// export uses, and the one the single-result form above is a special case of.
/// Returns the number of rows written.
///
/// The `serialize_seq` handle stays open across every chunk, so the array is one
/// array however many chunks it took: the keys are derived from the first chunk's
/// columns and reused, since a source that changed them mid-stream would already
/// have broken every other format's header.
pub fn export_json_chunks<W: Write>(w: &mut W, src: &mut dyn RowChunks) -> io::Result<ExportTally> {
    use serde::ser::{SerializeSeq, Serializer as _};

    let mut ser = serde_json::Serializer::pretty(w);
    let mut seq = ser.serialize_seq(None).map_err(io::Error::other)?;
    let mut keys: Vec<String> = Vec::new();
    let mut first = true;
    let mut tally = ExportTally::default();
    loop {
        // **A source error closes the array before it propagates.** Every other
        // format leaves a usable prefix when a stream is cancelled or the
        // connection drops — CSV, Markdown and SQL are line-based and a browser
        // closes an HTML table itself — while JSON's `]` is written once, after
        // the loop, so a `?` inside it left `…},` on disk and every parser
        // rejects the 90% of rows that did arrive. "Incomplete" reads very
        // differently for a file with most of the data in it than for one with
        // none.
        //
        // The source's error, not the writer's: a writer that has failed cannot
        // be asked to write a bracket, and `seq.end()`'s own failure is dropped
        // for the same reason.
        let chunk = match src.next_chunk() {
            Ok(chunk) => chunk,
            Err(e) => {
                let _ = seq.end();
                return Err(e);
            }
        };
        let Some(c) = chunk else { break };
        if first {
            keys = unique_column_keys(c.rs);
            first = false;
        }
        // Per chunk, not once: whether a binary column's text is a placeholder
        // this codebase generated is a fact about the *rows in hand*, and a
        // streamed export never has them all at once. A column carrying real
        // bytes in one chunk and a placeholder in the next is withheld only
        // where it is a placeholder, which is the same rule the one-shot path
        // applies to the only chunk it has.
        let dropped = dropped_binary_columns(c.rs, c.order);
        let mask = binary_mask(c.rs, &dropped);
        tally.note(c.rs, &dropped);
        for &di in c.order.iter().filter(|&&di| di < c.rs.row_count()) {
            seq.serialize_element(&RowObject {
                rs: c.rs,
                keys: &keys,
                mask: &mask,
                di,
            })
            .map_err(io::Error::other)?;
            tally.rows += 1;
        }
    }
    seq.end().map_err(io::Error::other)?;
    Ok(tally)
}

/// Column names made unique for use as JSON object keys: a repeated name gets a
/// `_2`/`_3`/… suffix (first occurrence keeps the bare name).
fn unique_column_keys(rs: &ResultSet) -> Vec<String> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    rs.columns
        .iter()
        .map(|c| {
            let n = seen.entry(c.name.clone()).or_insert(0);
            *n += 1;
            if *n == 1 {
                c.name.clone()
            } else {
                format!("{}_{}", c.name, n)
            }
        })
        .collect()
}

/// One column's values as a JSON array (for building arrays out of a column).
pub fn export_column_json(rs: &ResultSet, order: &[usize], ci: usize) -> String {
    to_string(|w| export_column_json_to(w, rs, order, ci))
}

/// [`export_column_json`], streamed.
pub fn export_column_json_to<W: Write>(
    w: &mut W,
    rs: &ResultSet,
    order: &[usize],
    ci: usize,
) -> io::Result<()> {
    use serde::ser::{SerializeSeq, Serializer as _};

    let mask = binary_mask(rs, &dropped_binary_columns(rs, order));
    let mut ser = serde_json::Serializer::pretty(w);
    let mut seq = ser.serialize_seq(None).map_err(io::Error::other)?;
    for &di in order {
        let v = rs
            .cell(di, ci)
            .filter(|c| !withheld_binary(&mask, ci, c))
            .map(|c| value_to_json(&c.to_value()))
            .unwrap_or(serde_json::Value::Null);
        seq.serialize_element(&v).map_err(io::Error::other)?;
    }
    seq.end().map_err(io::Error::other)
}

/// One column's values as a newline-separated list (a single-column CSV).
pub fn export_column_csv(rs: &ResultSet, order: &[usize], ci: usize) -> String {
    to_string(|w| export_column_csv_to(w, rs, order, ci))
}

/// [`export_column_csv`], streamed.
pub fn export_column_csv_to<W: Write>(
    w: &mut W,
    rs: &ResultSet,
    order: &[usize],
    ci: usize,
) -> io::Result<()> {
    // The same withholding the whole-result CSV does, and it matters as much:
    // this is the "copy this column" path, and the column being copied is
    // exactly the one a caller might paste into an `IN (…)` or a spreadsheet.
    let mask = binary_mask(rs, &dropped_binary_columns(rs, order));
    for &di in order {
        match rs.cell(di, ci) {
            None => {}
            Some(c) if c.is_null() => {}
            Some(c) if withheld_binary(&mask, ci, &c) => {}
            Some(c) => w.write_all(csv_field(c.display()).as_bytes())?,
        }
        w.write_all(b"\n")?;
    }
    Ok(())
}

/// The whole result as CSV (header row + data rows; NULL → empty field).
pub fn export_csv(rs: &ResultSet, order: &[usize]) -> String {
    to_string(|w| export_csv_to(w, rs, order))
}

/// [`export_csv`], streamed.
pub fn export_csv_to<W: Write>(w: &mut W, rs: &ResultSet, order: &[usize]) -> io::Result<()> {
    export_csv_chunks(w, &mut OneChunk::new(rs, order)).map(|_| ())
}

/// [`export_csv_to`] over a [`RowChunks`] source. Returns the rows written.
///
/// The header comes from the **first** chunk and is written once — which is why
/// [`OneChunk`] yields even for an empty result, so a header-only CSV stays a
/// header-only CSV rather than an empty file.
pub fn export_csv_chunks<W: Write>(w: &mut W, src: &mut dyn RowChunks) -> io::Result<ExportTally> {
    let mut first = true;
    let mut tally = ExportTally::default();
    while let Some(c) = src.next_chunk()? {
        if first {
            for (ci, col) in c.rs.columns.iter().enumerate() {
                if ci > 0 {
                    w.write_all(b",")?;
                }
                w.write_all(csv_field(&col.name).as_bytes())?;
            }
            w.write_all(b"\n")?;
            first = false;
        }
        // CSV is a format Schemaic reads back, so a blob's placeholder must not
        // go out in it — see [`dropped_binary_columns`]. An empty field, which
        // is already how this format renders NULL. Per chunk, for the reason
        // given in [`export_json_chunks`].
        let dropped = dropped_binary_columns(c.rs, c.order);
        let mask = binary_mask(c.rs, &dropped);
        tally.note(c.rs, &dropped);
        for &di in c.order {
            if di >= c.rs.row_count() {
                continue;
            }
            for ci in 0..c.rs.columns.len() {
                if ci > 0 {
                    w.write_all(b",")?;
                }
                match c.rs.cell(di, ci) {
                    None => {}
                    Some(cell) if cell.is_null() => {}
                    Some(cell) if withheld_binary(&mask, ci, &cell) => {}
                    Some(cell) => w.write_all(csv_field(cell.display()).as_bytes())?,
                }
            }
            w.write_all(b"\n")?;
            tally.rows += 1;
        }
    }
    Ok(tally)
}

/// Rows a worksheet can hold, header included — Excel's own ceiling, not ours.
pub const XLSX_MAX_ROWS: u32 = 1_048_576;
/// Columns a worksheet can hold.
pub const XLSX_MAX_COLS: u16 = 16_384;
/// Characters one worksheet cell can hold. A `TEXT`, `JSON` or `BLOB`-as-text
/// column routinely holds more, which is what [`ExportTally::cut`] is for.
pub const XLSX_MAX_CELL_CHARS: usize = 32_767;

/// Write the rows as an Excel workbook: one worksheet, a frozen bold header row,
/// and every cell carrying its **type** rather than its text.
///
/// **Typed cells are the whole reason this format is worth having.** A CSV opened
/// in Excel is a wall of text that the application then guesses at — the guess
/// that turns `SET-1` into a date and drops the leading zeros off a postcode.
/// Writing an `Int`/`Float` cell as a number and everything else as a string
/// leaves nothing to guess: [`crate::model::CellTag`] already carries what the
/// server said each value was, so the file states it.
///
/// The exception is a number Excel could not hold *exactly*. A worksheet number
/// is an `f64`, so an `i64`/`u64` past 2^53 — a Snowflake id, a `BIGINT` key —
/// would come back a different number. Those go out as text, which is the same
/// trade [`crate::model`] already makes by keeping every value's canonical text.
/// A non-finite float has no cell representation at all and goes the same way.
///
/// **This is the one function in `schemaic-core` whose tests touch the
/// filesystem**, and it is deliberate. `constant_memory` mode spills each row's
/// XML to a library-managed temp file the moment the next row begins, which is
/// what holds an export of an arbitrarily long table to a fixed cost — the bound
/// [`RowChunks`] exists to give, and one an in-memory workbook would throw away
/// at exactly the size where it matters (a 200k x 50 result is ~10M cell structs
/// before a single byte is compressed). The file is created and removed inside
/// this call, so a test of it is still deterministic and still needs no server;
/// it is the same pragmatic exception in-memory SQLite already is in `db`.
pub fn export_xlsx_chunks<W: Write + Send>(
    w: &mut W,
    src: &mut dyn RowChunks,
    source: Option<(&str, Option<&str>, &str)>,
) -> io::Result<ExportTally> {
    use rust_xlsxwriter::{Format, Workbook};

    let mut wb = Workbook::new();
    let header = Format::new().set_bold();
    // The format that makes a blank cell *exist*. Excel — and this writer,
    // matching it — drops an unformatted blank, so `Format::default()` here
    // writes nothing and a row of nothing but NULLs would still vanish. Cell
    // protection is the one property that changes nothing a reader can see on a
    // sheet that is never protected, which this one is not.
    let blank = Format::new().set_unlocked();
    let mut tally = ExportTally::default();
    {
        let sheet = wb.add_worksheet_with_constant_memory();
        sheet
            .set_name(sheet_name(source))
            .map_err(io::Error::other)?;
        let mut first = true;
        // Row 0 is the header, so data starts at 1 — but only once a chunk has
        // arrived to say what the columns are. A source that ends before its
        // first chunk writes an empty sheet, matching what CSV does with no
        // header to take.
        let mut row: u32 = 0;
        while let Some(c) = src.next_chunk()? {
            // Asked of **every** chunk, not just the first. `RowChunk` documents
            // same-columns-per-chunk as a precondition, but a first-chunk-only
            // check means a source that breaks it reaches the writer and fails
            // with a message naming neither the count nor the way out — and the
            // `ci as u16` casts below have no other bound.
            if c.rs.columns.len() > XLSX_MAX_COLS as usize {
                return Err(too_wide(c.rs.columns.len()));
            }
            if first {
                for (ci, col) in c.rs.columns.iter().enumerate() {
                    sheet
                        .write_string_with_format(0, ci as u16, &col.name, &header)
                        .map_err(io::Error::other)?;
                }
                // Scrolling a 200k-row export past the header and losing track of
                // which column is which is the first thing anyone does with it.
                sheet.set_freeze_panes(1, 0).map_err(io::Error::other)?;
                row = 1;
                first = false;
            }
            // Same withholding rule as CSV and JSON: a blob arrives as a
            // `<n bytes>` placeholder, and this is a format Schemaic reads back,
            // so the placeholder must not go out as data. See
            // [`dropped_binary_columns`].
            let dropped = dropped_binary_columns(c.rs, c.order);
            let mask = binary_mask(c.rs, &dropped);
            tally.note(c.rs, &dropped);
            for &di in c.order {
                if di >= c.rs.row_count() {
                    continue;
                }
                if row >= XLSX_MAX_ROWS {
                    return Err(too_many_rows());
                }
                let mut wrote_a_cell = false;
                for ci in 0..c.rs.columns.len() {
                    let Some(cell) = c.rs.cell(di, ci) else {
                        continue;
                    };
                    // A NULL and a withheld blob are both *no cell at all*,
                    // which is how a worksheet spells absent.
                    //
                    // This shares CSV's one ambiguity and does not fix it: an
                    // empty *string* also writes no cell, because the writer
                    // emits nothing for one, so it reads back as a NULL. The
                    // format could distinguish them — a real empty-string cell
                    // exists, and the import reader handles one — but this
                    // writer cannot produce it, so the round trip does not.
                    if cell.is_null() || withheld_binary(&mask, ci, &cell) {
                        continue;
                    }
                    wrote_a_cell |=
                        write_xlsx_cell(sheet, row, ci as u16, &cell, c.rs, &mut tally)?;
                }
                // **A row that wrote no cell would be no row at all.** The
                // writer emits a `<row>` only for a row that received one, and
                // the sheet's dimension comes from the cells actually written —
                // so an all-NULL (or all-empty-string, or all-withheld) row at
                // the *end* of a result disappeared from the file while
                // `tally.rows` below still counted it, and the two disagreed
                // with nothing on screen saying so. An interior one survived
                // only by accident, stretched into existence by a later row.
                // A blank cell is the worksheet's way of saying "this row is
                // here and empty", which is what CSV's unconditional `\n` says.
                if !wrote_a_cell {
                    sheet
                        .write_blank(row, 0, &blank)
                        .map_err(io::Error::other)?;
                }
                row += 1;
                tally.rows += 1;
            }
        }
    }
    wb.save_to_writer(&mut *w).map_err(io::Error::other)?;
    Ok(tally)
}

/// One cell, typed. Split out of [`export_xlsx_chunks`]'s loop so the tag →
/// worksheet-type decision is one testable thing rather than six arms nested
/// three loops deep.
///
/// Answers **whether a cell actually reached the sheet**, which is not the same
/// as "was called": an empty string writes nothing, because this writer drops an
/// unformatted blank exactly as Excel does. The caller needs that answer to know
/// whether the row exists at all — see its `wrote_a_cell`.
fn write_xlsx_cell(
    sheet: &mut rust_xlsxwriter::Worksheet,
    row: u32,
    col: u16,
    cell: &crate::model::CellRef<'_>,
    rs: &ResultSet,
    tally: &mut ExportTally,
) -> io::Result<bool> {
    use crate::model::CellTag;
    let text = cell.text();
    let number = match cell.tag {
        // Past 2^53 an `f64` no longer holds every integer, and a key that comes
        // back off by one is worse than a key that comes back as text.
        CellTag::Int => text
            .parse::<i64>()
            .ok()
            .filter(|n| n.unsigned_abs() <= 1 << 53)
            .map(|n| n as f64),
        CellTag::UInt => text
            .parse::<u64>()
            .ok()
            .filter(|n| *n <= 1 << 53)
            .map(|n| n as f64),
        // NaN and the infinities have no worksheet representation; their text
        // does.
        CellTag::Float => text.parse::<f64>().ok().filter(|f| f.is_finite()),
        // `Str` covers dates, `SET-1`, JSON and every exotic type, all of which
        // arrive over the text protocol precisely so they round-trip losslessly.
        // Handing those to an `f64` here is the one way to undo that.
        //
        // **The exception is the fixed-point family**, which is `Str`-tagged for
        // the same exactness reason and is the commonest numeric column there
        // is. Sending a `DECIMAL(10,2)` money column out as text is the CSV
        // behaviour this format exists to replace: `=SUM(B:B)` reads 0 and the
        // column sorts as words. So a decimal column's value becomes a number
        // when — and only when — an `f64` holds it exactly ([`exact_decimal`]).
        // The *column's type* is what licenses it, never the shape of the text:
        // a `VARCHAR` zip code that looks like a number is not one.
        //
        // `Null` is here only to keep the match exhaustive — the caller writes
        // no cell at all for one, so a null never reaches this function. Don't
        // read this arm as null handling; that lives in `export_xlsx_chunks`.
        CellTag::Str if is_fixed_point(&rs.columns[col as usize].type_name) => exact_decimal(text),
        CellTag::Str | CellTag::Null => None,
    };
    let wrote = match number {
        Some(n) => {
            sheet.write_number(row, col, n).map_err(io::Error::other)?;
            true
        }
        None => {
            let fitted = fit_cell(text);
            if fitted.len() < text.len() {
                tally.note_cut(rs, col as usize);
            }
            sheet
                .write_string(row, col, fitted)
                .map_err(io::Error::other)?;
            !fitted.is_empty()
        }
    };
    Ok(wrote)
}

/// Is this column an engine's **exact fixed-point** type — the family that
/// arrives as text so it stays exact, and that a spreadsheet user expects to be
/// able to sum?
///
/// Matched on the leading token, so `DECIMAL(10,2)` and `numeric` both answer.
/// Deliberately narrow: `FLOAT`/`DOUBLE` already arrive tagged `Float`, and
/// everything outside this list keeps the conservative text rendering.
fn is_fixed_point(type_name: &str) -> bool {
    let head = type_name
        .trim()
        .split(|c: char| c == '(' || c.is_whitespace())
        .next()
        .unwrap_or("");
    matches!(
        head.to_ascii_uppercase().as_str(),
        "DECIMAL" | "DEC" | "NUMERIC" | "FIXED" | "MONEY" | "SMALLMONEY"
    )
}

/// `text` as an `f64` **only if the `f64` is that decimal exactly**.
///
/// The comparison is against the shortest representation that round-trips
/// (`f64`'s own `Display`), with the text first normalised the way that
/// representation is — a leading `+`, redundant leading zeros and trailing
/// fractional zeros are notation, not value, and `1234.50` is the everyday shape
/// of a `DECIMAL(10,2)`. Anything the `f64` cannot carry digit for digit —
/// `0.1234567890123456789`, `12345678901234567890.5` — fails the comparison and
/// stays text.
fn exact_decimal(text: &str) -> Option<f64> {
    let n: f64 = text.parse().ok()?;
    if !n.is_finite() {
        return None;
    }
    (normalise_decimal(text) == format!("{n}")).then_some(n)
}

/// `text` written the way `f64`'s `Display` would write the same value: no
/// leading `+`, no redundant leading zeros, no trailing fractional zeros.
fn normalise_decimal(text: &str) -> String {
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text.strip_prefix('+').unwrap_or(text)),
    };
    let (int, frac) = match digits.split_once('.') {
        Some((i, f)) => (i, f.trim_end_matches('0')),
        None => (digits, ""),
    };
    let int = int.trim_start_matches('0');
    let int = if int.is_empty() { "0" } else { int };
    if frac.is_empty() {
        format!("{sign}{int}")
    } else {
        format!("{sign}{int}.{frac}")
    }
}

/// `s` cut to what one worksheet cell holds, on a **character** boundary.
///
/// Excel counts characters, not bytes, and slicing a `&str` mid-codepoint
/// panics — so the cut is by `char_indices`, and an all-ASCII value (the common
/// case) is returned untouched without walking it twice.
fn fit_cell(s: &str) -> &str {
    if s.len() <= XLSX_MAX_CELL_CHARS {
        // A char is at least one byte and at most two UTF-16 units, and a UTF-16
        // unit costs at least one byte in UTF-8 — so a string this short in
        // *bytes* cannot be too long in units either.
        return s;
    }
    let mut units = 0usize;
    for (i, c) in s.char_indices() {
        let next = units + c.len_utf16();
        if next > XLSX_MAX_CELL_CHARS {
            return &s[..i];
        }
        units = next;
    }
    s
}

/// The worksheet name for an export of `source`: the table's own name, cut and
/// scrubbed to what Excel accepts, or `Result` when the result isn't a table.
///
/// Excel's rules are its own and it rejects a workbook that breaks them rather
/// than repairing it: at most 31 characters, none of `[ ] : * ? / \`, no leading
/// or trailing apostrophe, and not the reserved name `History` (in any case),
/// which belongs to a shared workbook's change log. A table name is
/// server-controlled, so every one of those is reachable — `db."my/table"` is
/// legal in PostgreSQL, and `history` is an ordinary table name everywhere.
/// `rust_xlsxwriter` enforces the first three for us and documents the fourth
/// without checking it, so that one is ours.
fn sheet_name(source: Option<(&str, Option<&str>, &str)>) -> String {
    let base = source.map(|(_, _, t)| t).unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '[' | ']' | ':' | '*' | '?' | '/' | '\\' => '_',
            c => c,
        })
        .take(31)
        .collect();
    let trimmed = cleaned.trim_matches('\'').trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("history") {
        "Result".to_string()
    } else {
        trimmed.to_string()
    }
}

/// The refusal when a result has more rows than a worksheet can hold.
///
/// **A refusal and not a truncation.** Every other loss this module reports is
/// one the file can still be useful despite; silently stopping at row 1,048,576
/// of a 3M-row table would produce a file that looks like the whole export and
/// is not. The `.part` dance means an errored export leaves the destination
/// alone, so the user still has whatever was there before.
fn too_many_rows() -> io::Error {
    io::Error::other(format!(
        "this result has more than {} rows, which is all an Excel worksheet can hold — export it as CSV or JSON instead",
        crate::text::human_count(XLSX_MAX_ROWS as usize - 1)
    ))
}

/// The same refusal for width. Unreachable in practice — no engine returns
/// 16,384 columns — but a silent truncation of the columns past it would be the
/// worse failure, so it is stated rather than assumed.
fn too_wide(n: usize) -> io::Error {
    io::Error::other(format!(
        "this result has {n} columns; an Excel worksheet holds {} — export it as CSV or JSON instead",
        crate::text::human_count(XLSX_MAX_COLS as usize)
    ))
}

/// Escape a Markdown table cell. A `|` starts a new column, so it must be
/// backslash-escaped; backslash is Markdown's escape char, so a literal `\`
/// doubles (else it would swallow a following `|`). Newlines would break the
/// row — GitHub renders `<br>` inside table cells, so map them there (a lone CR
/// is dropped so CRLF doesn't emit a double break).
pub fn md_cell(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\r', "")
        .replace('\n', "<br>")
}

/// Escape text for HTML element content. `&` is replaced first so the `&` in
/// the `&lt;`/`&gt;` entities isn't re-escaped.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The whole result as a GitHub-Flavored-Markdown table (header row + `---`
/// separator + data rows). Cells are escaped via [`md_cell`]; NULL renders as an
/// empty cell (matching [`export_csv`]).
pub fn export_markdown(rs: &ResultSet, order: &[usize]) -> String {
    to_string(|w| export_markdown_to(w, rs, order))
}

/// [`export_markdown`], streamed.
pub fn export_markdown_to<W: Write>(w: &mut W, rs: &ResultSet, order: &[usize]) -> io::Result<()> {
    export_markdown_chunks(w, &mut OneChunk::new(rs, order)).map(|_| ())
}

/// One row's cells, already escaped, as `| a | b |`. The separator row is a fixed
/// `---` per column, so no pass over the data is needed to size them — which is
/// what lets Markdown stream at all. Mirrors the original
/// `format!("| {} |\n", cells.join(" | "))` exactly, including the degenerate
/// zero-column case (`|  |`).
fn md_row_line<W: Write>(w: &mut W, cells: &mut dyn Iterator<Item = String>) -> io::Result<()> {
    w.write_all(b"| ")?;
    for (i, cell) in cells.enumerate() {
        if i > 0 {
            w.write_all(b" | ")?;
        }
        w.write_all(cell.as_bytes())?;
    }
    w.write_all(b" |\n")
}

/// [`export_markdown_to`] over a [`RowChunks`] source. Returns the rows written.
///
/// No withheld columns in its [`ExportTally`], deliberately — Markdown keeps the
/// `<n bytes>` placeholder because nothing reads it back and there the
/// placeholder is the useful rendering. A blanked column is still reported: an
/// empty cell reads as empty whatever the format.
pub fn export_markdown_chunks<W: Write>(
    w: &mut W,
    src: &mut dyn RowChunks,
) -> io::Result<ExportTally> {
    let mut first = true;
    let mut tally = ExportTally::default();
    while let Some(c) = src.next_chunk()? {
        tally.note(c.rs, &[]);
        let n = c.rs.columns.len();
        if first {
            md_row_line(w, &mut c.rs.columns.iter().map(|col| md_cell(&col.name)))?;
            md_row_line(w, &mut (0..n).map(|_| "---".to_string()))?;
            first = false;
        }
        for &di in c.order {
            if di >= c.rs.row_count() {
                continue;
            }
            md_row_line(
                w,
                &mut (0..n).map(|ci| match c.rs.cell(di, ci) {
                    None => String::new(),
                    Some(cell) if cell.is_null() => String::new(),
                    Some(cell) => md_cell(cell.display()),
                }),
            )?;
            tally.rows += 1;
        }
    }
    Ok(tally)
}

/// The whole result as an HTML `<table>` (thead + tbody). Cells/headers are
/// escaped via [`html_escape`]; NULL renders as an empty `<td>` (matching
/// [`export_csv`]).
pub fn export_html(rs: &ResultSet, order: &[usize]) -> String {
    to_string(|w| export_html_to(w, rs, order))
}

/// [`export_html`], streamed. The preamble and closing tags are fixed strings, so
/// nothing here needs to see the whole result first.
pub fn export_html_to<W: Write>(w: &mut W, rs: &ResultSet, order: &[usize]) -> io::Result<()> {
    export_html_chunks(w, &mut OneChunk::new(rs, order)).map(|_| ())
}

/// [`export_html_to`] over a [`RowChunks`] source. Returns the rows written.
///
/// The closing tags are written **only if the opening ones were** — a source that
/// yields no chunk at all produces an empty file rather than a `</tbody></table>`
/// closing a table that was never opened.
pub fn export_html_chunks<W: Write>(w: &mut W, src: &mut dyn RowChunks) -> io::Result<ExportTally> {
    let mut opened = false;
    let mut tally = ExportTally::default();
    while let Some(c) = src.next_chunk()? {
        tally.note(c.rs, &[]);
        if !opened {
            // The charset declaration is not optional. The bytes written here are
            // UTF-8, but for a `file://` URL with no declaration and no BOM the
            // HTML spec leaves the default to the user agent — windows-1252 in
            // Western locales — so `José` opened as `JosÃ©`. Chrome dropped its
            // manual encoding override in 2014, so there was no in-browser
            // workaround; the user had to edit the file.
            w.write_all(b"<meta charset=\"utf-8\">\n")?;
            w.write_all(b"<table>\n<thead>\n<tr>")?;
            for col in &c.rs.columns {
                w.write_all(b"<th>")?;
                w.write_all(html_escape(&col.name).as_bytes())?;
                w.write_all(b"</th>")?;
            }
            w.write_all(b"</tr>\n</thead>\n<tbody>\n")?;
            opened = true;
        }
        for &di in c.order {
            if di >= c.rs.row_count() {
                continue;
            }
            w.write_all(b"<tr>")?;
            for ci in 0..c.rs.columns.len() {
                w.write_all(b"<td>")?;
                match c.rs.cell(di, ci) {
                    None => {}
                    Some(cell) if cell.is_null() => {}
                    Some(cell) => w.write_all(html_escape(cell.display()).as_bytes())?,
                }
                w.write_all(b"</td>")?;
            }
            w.write_all(b"</tr>\n")?;
            tally.rows += 1;
        }
    }
    if opened {
        w.write_all(b"</tbody>\n</table>\n")?;
    }
    Ok(tally)
}

/// The result as `INSERT` statements, in the connection's dialect. `source` is
/// the real `(database, namespace, table)` when known; otherwise a `table`
/// placeholder is emitted for the user to fill in.
///
/// Identifiers and literals are quoted per `dialect` (see [`ident_sql`] and
/// [`sql_literal`]) so the output pastes straight into a client for that engine —
/// backticks and backslash-escaping for MySQL, double quotes and literal
/// backslashes for PostgreSQL.
///
/// A PostgreSQL namespace qualifies the table *instead of* the database — a PG
/// connection is bound to one database, so `schema.table` is the addressable
/// name, exactly as everywhere else in the app.
pub fn export_inserts(
    rs: &ResultSet,
    order: &[usize],
    source: Option<(&str, Option<&str>, &str)>,
    dialect: SqlDialect,
) -> String {
    to_string(|w| export_inserts_to(w, rs, order, source, dialect))
}

/// How generated SQL addresses a table, per engine — **the one rule**, shared by
/// the SQL export and by [`crate::import::build_insert`], which had it twice.
///
/// MySQL qualifies with the **database**, since its connection is server-level.
/// PostgreSQL qualifies with the **namespace** (the connection is already bound
/// to one database), falling back to the database when no namespace is given.
///
/// SQLite names the table **bare**, and that is the case worth stating: a
/// connection *is* one file, so there is nothing to disambiguate — and `main` is
/// not a name the user chose but SQLite's word for "the file you opened", so
/// emitting `"main"."t"` is noise on every row of an export and actively wrong
/// the moment that SQL is pasted into a session where a *different* file is
/// attached under that name. Same reasoning as [`crate::filter::table_query`]'s.
pub fn qualified_table(
    database: &str,
    schema: Option<&str>,
    table: &str,
    dialect: SqlDialect,
) -> String {
    let q = |s: &str| ident_sql(s, dialect);
    match dialect {
        SqlDialect::Sqlite => q(table),
        _ => match schema {
            Some(ns) => format!("{}.{}", q(ns), q(table)),
            // **An empty `database` means "don't qualify"**, and no engine has a
            // database whose name is the empty string, so nothing legal collides
            // with it. The dump needs this: its `CREATE`/`DROP` name a MySQL table
            // bare so the file's own `USE` line is the one thing to edit to
            // retarget it, and an `INSERT` that qualified with the *source*
            // database refilled the database the rows came from — a success
            // report, an empty target, and no duplicate-key error to stop it.
            None if database.is_empty() => q(table),
            None => format!("{}.{}", q(database), q(table)),
        },
    }
}

/// Columns whose exported cells stand in for bytes this result never carried.
///
/// A raw-bytes cell renders as [`crate::model::binary_display`]'s `<n bytes>`,
/// because a `Value` has no bytes variant to hold the real thing — so writing
/// that text into an `INSERT` produces a script that *silently stores the
/// placeholder* as the column's data on re-import. This is the pre-pass that
/// finds it, and it deliberately requires **both** signals to agree: the
/// column carries bytes, and the cell's text is one this codebase
/// generated. Either alone is wrong in a way that loses data — a SQLite `BLOB`
/// column is only an affinity and may hold ordinary text, and a user's prose
/// can spell `<12 bytes>` without being a blob.
///
/// **"The column carries bytes" has two sources, and the second is why SQLite
/// works here at all.** [`crate::model::Column::is_binary`] reads the type name
/// and the wire origin, and on SQLite neither can answer: `origin` is
/// unconditionally `None` and `decl_type()` is an affinity or nothing, so an
/// untyped column — the ordinary shape for a blob store — said "not binary" and
/// the placeholder went into CSV, JSON *and* SQL as though it were the data.
/// [`ResultSet::binary_columns`] is the backend's own per-value assertion for
/// that case. It **widens** the type signal rather than replacing it: the cell
/// test still has to agree, so real text in a column that held a blob elsewhere
/// is untouched.
///
/// Returns column *indices*, not names: two result columns can share a name
/// (`SELECT a.data, b.data`) and only one of them may be the blob.
///
/// **Which exports honour this, and which deliberately don't.** The formats
/// Schemaic itself reads back — `import::ImportFormat` is CSV and JSON — must
/// not carry the placeholder, because a round trip through one stores it *as*
/// the column's data; the SQL export must not for the same reason. Markdown and
/// HTML keep it: nothing reads those back, and there the placeholder is the
/// useful rendering, since blanking it would make a 4 MB blob indistinguishable
/// from an empty cell.
fn dropped_binary_columns(rs: &ResultSet, order: &[usize]) -> Vec<usize> {
    (0..rs.columns.len())
        .filter(|&ci| {
            crate::edit::holds_bytes(rs, ci)
                && order.iter().any(|&di| {
                    di < rs.row_count()
                        && rs
                            .cell(di, ci)
                            .is_some_and(|c| crate::model::is_binary_display(c.text()))
                })
        })
        .collect()
}

/// [`dropped_binary_columns`] in the shape the **cell loop** needs: one `bool`
/// per column, indexed by `ci`.
///
/// Two shapes for one answer, because the two readers ask it differently. The
/// `-- NOTE:` line names the columns in order and wants the `Vec<usize>`; the
/// loop asks once per *cell*, and `Vec::contains` there is a linear scan over
/// the answer — 12M of them on a 200k × 60 result, to re-derive a per-column
/// fact that was already computed. Same hoist `Db::convert_row` and `pg_cell`
/// make for `Column::is_binary`.
fn binary_mask(rs: &ResultSet, dropped: &[usize]) -> Vec<bool> {
    let mut mask = vec![false; rs.columns.len()];
    for &ci in dropped {
        if let Some(slot) = mask.get_mut(ci) {
            *slot = true;
        }
    }
    mask
}

/// Is this cell one whose real bytes the result never carried?
///
/// **The one per-cell test**, shared by every export that withholds the
/// placeholder, so the two-signals rule (the column says bytes *and* the text is
/// one this codebase generated) cannot be spelled differently in one of them.
/// `mask` comes from [`binary_mask`].
fn withheld_binary(mask: &[bool], ci: usize, c: &crate::model::CellRef<'_>) -> bool {
    mask.get(ci).copied().unwrap_or(false) && crate::model::is_binary_display(c.text())
}

/// The sentence a finished export puts on the grid's bar, or `None` when there
/// is nothing to say.
///
/// **Silence is the default and a caveat overrides it.** A save of what is
/// already on screen has nothing to report that the screen doesn't show, and a
/// note on every save is a note nobody reads — so an ordinary `Fetched` export
/// stays quiet, and only a streamed one announces its row count.
///
/// **Two of the three losses override that silence; the third does not.** A
/// column *blanked* or *cut* is a surprise — the value was there, the file has
/// less of it than the screen does, and nothing on screen says so. A **withheld**
/// binary column is not: the grid renders that cell as `<7 bytes>`, so the user
/// picking CSV or JSON for it can already see what they are asking a text format
/// to carry, and "a text export cannot hold raw bytes" restated it at a length
/// that painted past the export modal's own width. So `withheld` is tallied and
/// not said. It is still a caveat on the tally — [`ExportTally::has_caveat`] and
/// the SQL writer's `-- binary column` comment both read it — this function just
/// doesn't spend a sentence on it.
///
/// The caveats that *are* said read the way the grid's own arena note does — the
/// column names, then what happened to them — because a user comparing the file
/// to the screen needs to know *which* part of it to distrust, and "some data was
/// lost" tells them nothing.
pub fn export_note(t: &ExportTally, name: &str, streaming: bool) -> Option<String> {
    // **`withheld` is tallied but not said**, which is why this asks for the
    // caveats that speak rather than `has_caveat`. A binary column the grid
    // already renders as `<7 bytes>` gains nothing from a clause explaining that
    // a text file cannot hold bytes — and that clause was long enough to paint
    // past the export modal it lands in. The tally still records it: the SQL
    // writer's `-- binary column` comment and `has_caveat` both read `withheld`,
    // and this only decides what the *sentence* carries.
    if !streaming && t.blanked.is_empty() && t.cut.is_empty() {
        return None;
    }
    let n = t.rows as usize;
    let mut s = format!(
        "Exported {} {} to {name}",
        crate::text::human_count(n),
        crate::text::plural(n, "row", "rows")
    );
    if !t.blanked.is_empty() {
        s.push_str(" — ");
        s.push_str(&format!(
            "{} too large to hold in full: later rows are blank",
            t.blanked.join(", ")
        ));
    }
    if !t.cut.is_empty() {
        s.push_str(if t.blanked.is_empty() { " — " } else { "; " });
        // The exact figure, not `human_count`'s "32.77k": this is a hard limit
        // a user may want to check a column against, and a rounded one answers
        // no question they would ask it.
        s.push_str(&format!(
            "{} cut to Excel's {XLSX_MAX_CELL_CHARS}-character cell limit",
            t.cut.join(", "),
        ));
    }
    Some(s)
}

/// The scratch name an export writes to before it is finished: the destination
/// with `.part` after it.
///
/// **The destination is not opened until the export has succeeded.** It used to be
/// opened first — `File::create` truncates — so a stream that died ten minutes in
/// had already destroyed whatever the user was overwriting, and all the bar could
/// do was say so. Writing a sibling and renaming it over the target on success is
/// the dance `persist` already does for every config file, and it is atomic on
/// both platforms *because it is a sibling*: a rename within one directory never
/// crosses a filesystem.
///
/// A visible, self-describing suffix rather than a hidden temp file, because when
/// an export does fail the fragment is the one thing the user may still want —
/// it is left behind and named in the message rather than swept away.
pub fn part_path(name: &str) -> String {
    format!("{name}.part")
}

/// What a failed export says. `partial` is the destination's file name when the
/// write had begun, and `None` when the export never got that far.
///
/// **A failure is the case where the user is least likely to look.** What it needs
/// to say has changed with [`part_path`]: the destination is no longer touched
/// until the export succeeds, so the sentence is no longer "your file is a
/// fragment" but "your file is untouched, and the rows that did arrive are in the
/// sibling". Both halves matter — the first is the reassurance, the second is
/// where the partial went.
///
/// `None` is not a formality: an export refused before the write starts (no
/// connection, one already running) must not mention a file at all.
pub fn export_failure_note(message: &str, partial: Option<&str>) -> String {
    match partial {
        Some(name) => format!(
            "{message} — {name} was not changed; the rows that were written are in {}",
            part_path(name)
        ),
        None => message.to_string(),
    }
}

/// What a **cancelled** export says.
///
/// The same two facts as [`export_failure_note`], in the voice the cancel arm has
/// always used: stopping was the user's own doing, so this is a note rather than an
/// error. It said `— {name} is incomplete`, which was true when the destination was
/// truncated at `t = 0` and is now the opposite of true.
///
/// `partial` is [`ExportFormat::writes_incrementally`]'s answer, for the same
/// reason [`export_failure_note`] takes one: a buffered format's sibling holds
/// nothing, so pointing at it is directing the user to an empty file.
pub fn export_cancel_note(name: &str, partial: bool) -> String {
    if partial {
        format!(
            "Export cancelled — {name} was not changed; the rows that were written are in {}",
            part_path(name)
        )
    } else {
        format!("Export cancelled — {name} was not changed.")
    }
}

/// What a finished **folder** export says: how many files landed, where, what
/// they could not carry, and what was ticked but never found.
///
/// The file-per-table sibling of [`export_note`], and it delegates the caveat
/// half to that function rather than restating it — the wording of "this column
/// could not be carried" belongs in one place, and a folder export loses exactly
/// what a single file does. What it adds is the count: a folder is a thing whose
/// completeness the user cannot see at a glance, so the number of files is the
/// first fact, and the tables that went missing are the last.
///
/// `missing` is [`crate::dump::FilePlan::missing`] — ticked tables this run's own
/// introspection could not find. Named here as well as counted, because a folder
/// one file short of what was asked for looks exactly like a complete one.
pub fn files_note(
    files: usize,
    tally: &ExportTally,
    folder: &str,
    missing: &[String],
    replaced: &[String],
) -> String {
    // **The count first, the destination in the second clause** — the shape the
    // dump's own report has (`Wrote 5 tables. Exported 115k rows to shop.sql`),
    // and the reason to follow it rather than say "12 files to out" here is that
    // `export_note` names the destination too: both would spell the folder, one
    // sentence apart, in the message a user reads once.
    let mut s = format!(
        "Wrote {files} {}.",
        crate::text::plural(files, "file", "files")
    );
    // `streaming: true`, so the row count is always stated: a folder export has
    // no other way to say how much arrived, and `export_note` otherwise returns
    // `None` for a clean write.
    if let Some(note) = export_note(tally, folder, true) {
        s.push(' ');
        s.push_str(&note);
        // **`export_note` does not end its own sentence**, because its single-file
        // caller is the last clause of one. Here it is not: `missing_clause`
        // follows, and the two ran together into "…Exported 115k rows to
        // sakila-csv 1 ticked table not found and was no file: ghost."
        s.push('.');
    }
    s.push_str(&replaced_clause(replaced));
    s.push_str(&missing_clause(missing));
    s
}

/// The files a folder export **overwrote**, as a sentence — or nothing at all
/// when the folder held none of them.
///
/// **Shared by all three of the folder export's reports, for the reason
/// [`missing_clause`] is.** A directory picker has no overwrite dialog, so the
/// same product gesture that a single-file export guards with the save dialog's
/// "replace?" had, in the folder form, no guard and no mention: files the user
/// had in the folder they picked were replaced in silence, and a stopped or
/// failed export is *more* likely to be inspected than a finished one.
///
/// It names them rather than counting them. The count answers "did I lose
/// anything"; only the names answer "what".
fn replaced_clause(replaced: &[String]) -> String {
    if replaced.is_empty() {
        return String::new();
    }
    format!(
        " {} existing {} replaced: {}.",
        replaced.len(),
        crate::text::plural(replaced.len(), "file was", "files were"),
        replaced.join(", "),
    )
}

/// The tables a folder export was asked for and could not find, as a sentence —
/// or nothing at all when it found them.
///
/// **Shared by all three of the folder export's reports**, because all three
/// need it and the arm least likely to be read is the one that used to go
/// without: a folder two files short of what was ticked looks exactly like a
/// complete one whether the export finished, stopped, or failed, and a stopped
/// export is *more* likely to be inspected than a finished one.
fn missing_clause(missing: &[String]) -> String {
    if missing.is_empty() {
        return String::new();
    }
    format!(
        " {} ticked {} not found and {} no file: {}.",
        missing.len(),
        crate::text::plural(missing.len(), "table", "tables"),
        crate::text::plural(missing.len(), "was", "were"),
        missing.join(", "),
    )
}

/// What a **stopped** folder export says.
///
/// It leads with what the user *kept*, which is where this parts company with
/// [`export_cancel_note`]: a stopped single-file export has nothing but a
/// fragment to talk about, while this leaves whole, published files behind — each
/// renamed into place only once its table was complete. The fragment of the table
/// that was in flight is mentioned second, and not at all when there is no file
/// to be proud of yet.
pub fn files_cancel_note(
    files: usize,
    folder: &str,
    missing: &[String],
    replaced: &[String],
) -> String {
    let mut s = if files == 0 {
        format!("Export cancelled — no file was finished, so nothing was written to {folder}.")
    } else {
        format!(
            "Export cancelled — {files} {} finished in {folder}; the table in progress was left \
             unwritten.",
            crate::text::plural(files, "file was", "files were")
        )
    };
    s.push_str(&replaced_clause(replaced));
    s.push_str(&missing_clause(missing));
    s
}

/// What a **failed** folder export says.
///
/// [`export_failure_note`]'s question with the other answer: there, the
/// reassurance is that the destination was not touched, because there was one
/// destination and the user may already have had a file at it. Here the folder
/// is not empty, and the useful fact is how much of the export is really in it —
/// a message that stopped at the error would send the user to a folder they
/// could not interpret.
///
/// `files == 0` is the refused-before-anything-landed case, and it must not
/// mention a folder at all — the same rule [`export_failure_note`]'s `None` arm
/// follows.
pub fn files_failure_note(
    message: &str,
    files: usize,
    folder: &str,
    missing: &[String],
    replaced: &[String],
) -> String {
    let mut s = if files == 0 {
        message.to_string()
    } else {
        format!(
            "{message} — {files} {} already written to {folder} {} kept.",
            crate::text::plural(files, "file", "files"),
            crate::text::plural(files, "is", "are"),
        )
    };
    s.push_str(&replaced_clause(replaced));
    s.push_str(&missing_clause(missing));
    s
}

/// The `All rows` menu entry's label: the scope's size, and every way the file
/// it writes will differ from the grid it was launched from.
///
/// **A disclosure at the point of choice, because after the file is written is
/// too late.** Two differences, and neither is visible in the result:
///
/// - `sorted` — a column-header sort is a permutation of the rows in hand
///   (`compute_order`), not something the server was asked for, so the re-run
///   returns rows in the server's order.
/// - `manual_tx` — the re-run is a *second read on a fresh connection*
///   (`Db::stream_query`, deliberately outside the tab's pinned session), so a
///   manual-transaction tab's uncommitted rows are on screen and absent from the
///   file, and rows it deleted are in the file and gone from the screen.
///
/// `size` is pre-rendered by the caller (`~16k`, or empty when the total is not
/// known) because the estimate and its `~` belong to the stats line's vocabulary,
/// not to this decision.
pub fn all_rows_label(size: &str, sorted: bool, manual_tx: bool) -> String {
    let mut notes: Vec<&str> = Vec::new();
    if !size.is_empty() {
        notes.push(size);
    }
    if sorted {
        notes.push("server order");
    }
    if manual_tx {
        notes.push("committed rows only");
    }
    if notes.is_empty() {
        "All rows".to_string()
    } else {
        format!("All rows ({})", notes.join(", "))
    }
}

/// How much SQL one `INSERT` may carry before the emitter closes it and starts
/// another.
///
/// **The reason this exists at all is the round trip.** Schemaic's own Import
/// replays a script one statement at a time, one network round trip each, so a
/// file of one `INSERT` per row costs one round trip per row: measured on
/// MariaDB over loopback, 20 000 rows took **17 401 ms** against **162 ms** for
/// the same rows batched — 107x, extrapolating to hours for a million rows at a
/// real 20 ms RTT. Batching in the *runner* was the wrong end: it would break
/// the per-statement line number the panel reports a failure by. The file is
/// where the shape belongs, and it is the shape `mysqldump` writes.
///
/// 512 KiB is the smallest bound that makes the cost disappear and stays well
/// inside every engine's limit — MySQL's `max_allowed_packet` was 4 MB on 5.7
/// and is 64 MB now, PostgreSQL has no statement limit, and SQLite's
/// `SQLITE_MAX_SQL_LENGTH` is a megabyte on the most conservative builds. A
/// batch is closed *before* the row that would cross it, so one enormous row
/// still gets a statement of its own rather than being split.
pub const INSERT_BATCH_BYTES: usize = 512 * 1024;

/// And a row ceiling, for the same reason from the other direction: a table of
/// narrow rows would otherwise put tens of thousands of tuples in one `VALUES`.
/// Older SQLite builds cap a compound `VALUES` at `SQLITE_MAX_COMPOUND_SELECT`
/// (500), and a parser is happier with a statement it can hold.
pub const INSERT_BATCH_ROWS: usize = 250;

/// [`export_inserts`], streamed. Rows are batched into multi-row `INSERT`s (see
/// [`INSERT_BATCH_BYTES`]); the table and column lists are computed once and
/// repeated verbatim per statement.
pub fn export_inserts_to<W: Write>(
    w: &mut W,
    rs: &ResultSet,
    order: &[usize],
    source: Option<(&str, Option<&str>, &str)>,
    dialect: SqlDialect,
) -> io::Result<()> {
    export_inserts_chunks(w, &mut OneChunk::new(rs, order), source, dialect).map(|_| ())
}

/// [`export_inserts_to`] over a [`RowChunks`] source. Returns the rows written.
///
/// **The `-- NOTE:` line is the one thing here that a stream cannot know up
/// front.** Which binary columns were withheld is a fact about the rows, and a
/// streamed export never holds them all: a column can carry its real bytes for a
/// million rows and hit a blob placeholder in the next chunk. So the note is
/// emitted the first time a column is actually withheld, naming the ones newly
/// discovered, and again later only if a further column joins them. Over a single
/// chunk that collapses to exactly the old behaviour — one note, before the first
/// `INSERT`, naming every withheld column — which is what
/// `a_chunked_export_matches_the_same_rows_in_one_go` pins.
///
/// A comment and not a refusal, either way: the script still runs, and the one
/// thing it must not do is pretend the placeholder was the data.
///
/// **A batch spans chunks**, which is what keeps that byte-equality gate
/// meaningful: flushing at every chunk boundary would make a streamed export and
/// a one-go export of the same rows differ in nothing but how the source
/// happened to page them. A `-- NOTE:` closes the open statement first, since a
/// comment cannot appear inside a `VALUES` list.
pub fn export_inserts_chunks<W: Write>(
    w: &mut W,
    src: &mut dyn RowChunks,
    source: Option<(&str, Option<&str>, &str)>,
    dialect: SqlDialect,
) -> io::Result<ExportTally> {
    let q = |s: &str| ident_sql(s, dialect);
    let table_sql = match source {
        Some((db, ns, table)) => qualified_table(db, ns, table, dialect),
        None => q("table"),
    };
    let mut cols = String::new();
    let mut noted: Vec<bool> = Vec::new();
    let mut first = true;
    let mut tally = ExportTally::default();
    // The statement being built, carried **across chunks** — see the doc above.
    let mut open_rows = 0usize;
    let mut open_bytes = 0usize;
    while let Some(c) = src.next_chunk()? {
        if first {
            cols =
                c.rs.columns
                    .iter()
                    .map(|col| q(&col.name))
                    .collect::<Vec<_>>()
                    .join(", ");
            noted = vec![false; c.rs.columns.len()];
            first = false;
        }
        let dropped = dropped_binary_columns(c.rs, c.order);
        let mask = binary_mask(c.rs, &dropped);
        tally.note(c.rs, &dropped);
        let fresh: Vec<usize> = dropped
            .iter()
            .copied()
            .filter(|&ci| !noted.get(ci).copied().unwrap_or(true))
            .collect();
        if !fresh.is_empty() {
            for &ci in &fresh {
                noted[ci] = true;
            }
            // A comment cannot sit inside a `VALUES` list.
            close_batch(w, &mut open_rows)?;
            writeln!(
                w,
                "-- NOTE: binary column{} {} exported as NULL — a text export cannot carry raw bytes.",
                if fresh.len() == 1 { "" } else { "s" },
                fresh
                    .iter()
                    .map(|&ci| q(&c.rs.columns[ci].name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
        }
        for &di in c.order {
            if di >= c.rs.row_count() {
                continue;
            }
            // The tuple is built before anything is written, because whether it
            // fits inside the open statement is what decides where it goes.
            let mut tuple = String::from("(");
            for ci in 0..c.rs.columns.len() {
                if ci > 0 {
                    tuple.push_str(", ");
                }
                let lit =
                    c.rs.cell(di, ci)
                        .map(|cell| {
                            if withheld_binary(&mask, ci, &cell) {
                                "NULL".to_string()
                            } else {
                                sql_literal(&cell.to_value(), dialect)
                            }
                        })
                        .unwrap_or_else(|| "NULL".to_string());
                tuple.push_str(&lit);
            }
            tuple.push(')');

            // Closed *before* the row that would cross the bound, so one very
            // wide row gets a statement of its own rather than being split.
            if open_rows > 0
                && (open_bytes + tuple.len() + 2 > INSERT_BATCH_BYTES
                    || open_rows >= INSERT_BATCH_ROWS)
            {
                close_batch(w, &mut open_rows)?;
            }
            if open_rows == 0 {
                let head = format!("INSERT INTO {table_sql} ({cols}) VALUES\n");
                open_bytes = head.len();
                w.write_all(head.as_bytes())?;
            } else {
                w.write_all(b",\n")?;
                open_bytes += 2;
            }
            w.write_all(tuple.as_bytes())?;
            open_bytes += tuple.len();
            open_rows += 1;
            tally.rows += 1;
        }
    }
    close_batch(w, &mut open_rows)?;
    Ok(tally)
}

/// End the `INSERT` currently being built, if there is one.
///
/// Every path that must not write inside a `VALUES` list goes through this: the
/// `-- NOTE:` comment, the batch bounds, and the end of the export.
fn close_batch<W: Write>(w: &mut W, open_rows: &mut usize) -> io::Result<()> {
    if *open_rows > 0 {
        w.write_all(b";\n")?;
        *open_rows = 0;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Column;

    fn col(name: &str) -> Column {
        Column {
            name: name.to_string(),
            type_name: "VARCHAR".to_string(),
            origin: None,
        }
    }

    fn rs() -> ResultSet {
        ResultSet::from_rows(
            vec![col("id"), col("a`b")],
            vec![
                vec![Value::Int(1), Value::Str("x".to_string())],
                vec![Value::Null, Value::Str("y".to_string())],
            ],
        )
    }

    use crate::intel::SqlDialect::{MySql, Postgres, Sqlite};

    /// Rows with something for every renderer to trip over: a NULL, an embedded
    /// quote, a pipe and a newline (Markdown), an angle bracket (HTML), a
    /// backslash (the dialect-sensitive SQL literal), and a non-ASCII glyph.
    /// **NULLs in the same row, not just in the same fixture.** This had a NULL
    /// in each column but never both at once, and no `Float`, no `UInt` and no
    /// over-long `Str` — so the chunk-seam gate crossed a boundary with three of
    /// the four typed arms untouched, and the all-NULL row that produced no
    /// worksheet row at all could not appear on either side of the comparison.
    /// The row *before* the last one is all-NULL deliberately: an interior one
    /// survives by accident, because a later row stretches the used range.
    fn awkward_rows() -> (Vec<Column>, Vec<Vec<Value>>) {
        let cols = vec![col("id"), col("a`b")];
        let rows = vec![
            vec![Value::Int(1), Value::Str("x".to_string())],
            vec![Value::Null, Value::Str("y | z".to_string())],
            vec![Value::Int(3), Value::Str("line\nbreak".to_string())],
            vec![Value::Int(4), Value::Str("<b>&\"quoted\"".to_string())],
            vec![Value::Int(5), Value::Str("back\\slash".to_string())],
            vec![Value::Int(6), Value::Str("José".to_string())],
            vec![Value::Float(-0.5), Value::Str("x".repeat(300))],
            vec![Value::UInt(u64::MAX), Value::Str(String::new())],
            vec![Value::Null, Value::Null],
            vec![Value::Int(7), Value::Null],
        ];
        (cols, rows)
    }

    /// The formats whose output is a `String`, for the tests that assert on
    /// bytes. Derived from [`ExportFormat::is_text`] rather than listed, so a
    /// binary format added without answering that predicate lands in a loop that
    /// renders it to `String` and fails loudly instead of passing on `""`.
    fn text_formats() -> impl Iterator<Item = ExportFormat> {
        ExportFormat::ALL.into_iter().filter(|f| f.is_text())
    }

    /// Read a rendered workbook back as `(header, rows of cell text)`, with an
    /// empty cell distinguishable from an empty string by the `Option`.
    ///
    /// **The export and the import halves of this feature check each other.**
    /// `rust_xlsxwriter` wrote these bytes and `calamine` reads them, so a test
    /// that round-trips through both is not a test of one crate's idea of
    /// itself — and it is the same path an `.xlsx` import takes, so a change
    /// that breaks one half fails here.
    fn read_back(bytes: &[u8]) -> (Vec<String>, Vec<Vec<Option<String>>>) {
        use calamine::{Reader, Xlsx};
        let mut wb: Xlsx<_> =
            Xlsx::new(std::io::Cursor::new(bytes.to_vec())).expect("a workbook we just wrote");
        let name = wb.sheet_names()[0].clone();
        let range = wb.worksheet_range(&name).expect("the first sheet");
        let mut rows = range.rows();
        let header = rows
            .next()
            .map(|r| r.iter().map(|c| c.to_string()).collect())
            .unwrap_or_default();
        let body = rows
            .map(|r| {
                r.iter()
                    .map(|c| match c {
                        calamine::Data::Empty => None,
                        other => Some(other.to_string()),
                    })
                    .collect()
            })
            .collect();
        (header, body)
    }

    /// The sheet name of a rendered workbook.
    fn sheet_of(bytes: &[u8]) -> String {
        use calamine::{Reader, Xlsx};
        let wb: Xlsx<_> =
            Xlsx::new(std::io::Cursor::new(bytes.to_vec())).expect("a workbook we just wrote");
        wb.sheet_names()[0].clone()
    }

    fn to_xlsx(
        rs: &ResultSet,
        order: &[usize],
        src: Option<(&str, Option<&str>, &str)>,
    ) -> Vec<u8> {
        let mut buf = Vec::new();
        ExportFormat::Xlsx
            .render_to(&mut buf, rs, order, src, MySql)
            .expect("writing to a Vec cannot fail");
        buf
    }

    /// Split `rows` into `ResultSet`s of at most `size` rows, as a source.
    fn chunked(cols: &[Column], rows: &[Vec<Value>], size: usize) -> impl RowChunks {
        let mut queue: std::collections::VecDeque<ResultSet> = rows
            .chunks(size)
            .map(|part| ResultSet::from_rows(cols.to_vec(), part.to_vec()))
            .collect();
        PullChunks::new(move || Ok(queue.pop_front()))
    }

    /// **The seam this whole design rests on.** Every `export_*_to` is now the
    /// streaming renderer over a source of one chunk, so the only way the two can
    /// disagree is at a chunk boundary — a header written twice, a JSON array
    /// closed and reopened, a `-- NOTE:` line that moves. Splitting the same rows
    /// every possible way and demanding the same bytes is what catches that; a
    /// test of either half alone would not.
    #[test]
    fn a_chunked_export_matches_the_same_rows_in_one_go() {
        let (cols, rows) = awkward_rows();
        let whole = ResultSet::from_rows(cols.clone(), rows.clone());
        let order: Vec<usize> = (0..rows.len()).collect();
        let src_name = Some(("shop", None, "orders"));
        // Text formats only: this compares *bytes*, and a workbook's are a ZIP.
        // The same seam is checked for Excel by reading both back and comparing
        // cells — `a_chunked_xlsx_export_matches_the_same_rows_in_one_go`.
        for format in text_formats() {
            let one_go = to_string(|w| {
                format
                    .render_to(w, &whole, &order, src_name, MySql)
                    .map(|_| ())
            })
            .into_bytes();
            // Every split from one row per chunk up to one chunk for the lot,
            // plus a size that leaves a short final chunk.
            for size in 1..=rows.len() + 1 {
                let mut src = chunked(&cols, &rows, size);
                let mut buf = Vec::new();
                let n = format
                    .stream_to(&mut buf, &mut src, src_name, MySql)
                    .expect("writing to a Vec cannot fail");
                assert_eq!(
                    String::from_utf8_lossy(&buf),
                    String::from_utf8_lossy(&one_go),
                    "{} differs when chunked {size} rows at a time",
                    format.label()
                );
                assert_eq!(
                    n.rows,
                    rows.len() as u64,
                    "{} miscounted its rows at chunk size {size}",
                    format.label()
                );
            }
        }
    }

    /// **One `INSERT` per row is one network round trip per row**, because
    /// Schemaic's own Import replays a script statement by statement: measured
    /// on MariaDB over loopback, 20 000 rows took 17 401 ms against 162 ms for
    /// the same rows batched. The file is where the shape belongs — batching in
    /// the runner would break the per-statement line number a failure is
    /// reported by.
    ///
    /// The bound is bytes, and the batch is closed *before* the row that would
    /// cross it, so one enormous row still gets a statement of its own rather
    /// than being split across two.
    #[test]
    fn rows_are_batched_into_multi_row_inserts() {
        let rs = ResultSet::from_rows(
            vec![col("id")],
            (0..1000).map(|i| vec![Value::Int(i)]).collect(),
        );
        let order: Vec<usize> = (0..1000).collect();
        let sql = export_inserts(&rs, &order, Some(("shop", None, "t")), MySql);

        // 1000 rows at the 250-row ceiling is four statements, not a thousand.
        assert_eq!(sql.matches("INSERT INTO").count(), 4, "{sql}");
        assert_eq!(sql.matches(";\n").count(), 4);
        // Every row is still there, once.
        for i in 0..1000 {
            assert!(sql.contains(&format!("({i})")), "row {i} is missing");
        }
        // And it is a script: nothing left open at the end.
        assert!(sql.trim_end().ends_with(';'), "{}", &sql[sql.len() - 40..]);

        // The byte bound: rows wide enough that two do not fit take one
        // statement each, rather than one being cut in half.
        let wide = "x".repeat(INSERT_BATCH_BYTES / 2);
        let rs = ResultSet::from_rows(
            vec![col("id")],
            (0..3).map(|_| vec![Value::Str(wide.clone())]).collect(),
        );
        let sql = export_inserts(&rs, &[0, 1, 2], Some(("shop", None, "t")), MySql);
        assert_eq!(
            sql.matches("INSERT INTO").count(),
            3,
            "a row that does not fit beside another takes a statement of its own"
        );
    }

    /// A single row is still a single statement, and reads as one — the shape
    /// every other test in this module and every fixture on disk was written
    /// against.
    #[test]
    fn one_row_is_one_ordinary_insert() {
        let rs = ResultSet::from_rows(vec![col("id")], vec![vec![Value::Int(7)]]);
        assert_eq!(
            export_inserts(&rs, &[0], Some(("shop", None, "t")), MySql),
            "INSERT INTO `shop`.`t` (`id`) VALUES\n(7);\n"
        );
        // No rows: no statement at all, not an `INSERT` with an empty `VALUES`.
        let empty = ResultSet::from_rows(vec![col("id")], vec![]);
        assert_eq!(
            export_inserts(&empty, &[], Some(("shop", None, "t")), MySql),
            ""
        );
    }

    /// **One empty chunk and no chunk at all are different things**, and the
    /// distinction is the whole reason [`OneChunk`] yields when it has nothing to
    /// yield. A result with no rows still knows its columns, so it gets its
    /// header; a source that ended before producing anything — a query that
    /// failed before its first block — knows nothing, and a header invented for
    /// it would describe columns nobody ever saw.
    #[test]
    fn an_empty_chunk_carries_a_header_and_no_chunk_carries_nothing() {
        let (cols, _) = awkward_rows();
        let empty = ResultSet::from_rows(cols.clone(), vec![]);
        // Text formats only — the assertions here are about the bytes. Excel's
        // empty-source behaviour is the same question asked of a workbook, in
        // `an_empty_xlsx_export_still_carries_its_header`.
        for format in text_formats() {
            // One empty chunk — what the grid exports for a result with no rows.
            let one_go = to_string(|w| format.render_to(w, &empty, &[], None, MySql).map(|_| ()));
            match format {
                ExportFormat::Csv => assert_eq!(one_go, "id,a`b\n"),
                ExportFormat::Markdown => assert_eq!(one_go, "| id | a`b |\n| --- | --- |\n"),
                ExportFormat::Json => assert_eq!(one_go, "[]"),
                ExportFormat::Html => {
                    assert!(one_go.contains("<th>id</th>"), "{one_go}");
                    assert!(one_go.ends_with("</tbody>\n</table>\n"), "{one_go}");
                }
                // SQL has no header to write — the column list lives on each
                // `INSERT`, so no rows means no output at all.
                ExportFormat::Sql => assert_eq!(one_go, ""),
                ExportFormat::Xlsx => unreachable!("filtered out by `text_formats`"),
            }

            // No chunk at all: `[].chunks(n)` yields nothing, so this is a source
            // that ended before its first block.
            let mut src = chunked(&cols, &[], usize::MAX);
            let mut buf = Vec::new();
            let n = format
                .stream_to(&mut buf, &mut src, None, MySql)
                .expect("writing to a Vec cannot fail");
            let streamed = String::from_utf8(buf).expect("utf-8");
            assert_eq!(n.rows, 0, "{} counted rows it never saw", format.label());
            match format {
                // The array framing is the format's own, not the data's, so an
                // empty array is still valid JSON — where a bare header row is a
                // claim about columns.
                ExportFormat::Json => assert_eq!(streamed, "[]"),
                _ => assert_eq!(
                    streamed,
                    "",
                    "{} wrote a header for a source that produced nothing",
                    format.label()
                ),
            }
        }
    }

    /// The `-- NOTE:` line is the one piece of SQL output that used to need the
    /// whole result. A column whose placeholder only shows up in a later chunk
    /// still gets a note — once, before the rows that dropped it — and a column
    /// already noted is not announced again.
    #[test]
    fn the_binary_note_finds_a_column_that_only_drops_later() {
        let mut blob = col("thumb");
        blob.type_name = "BLOB".to_string();
        let cols = vec![col("id"), blob];
        let rows = vec![
            vec![Value::Int(1), Value::Str("real text".to_string())],
            vec![Value::Int(2), Value::Str("still text".to_string())],
            vec![
                Value::Int(3),
                Value::Str(crate::model::binary_display(4096)),
            ],
            vec![
                Value::Int(4),
                Value::Str(crate::model::binary_display(8192)),
            ],
            // A second chunk that also drops the column — without these the
            // fixture never asks whether the note repeats, and a renderer with
            // no de-duplication at all passes.
            vec![Value::Int(5), Value::Str(crate::model::binary_display(16))],
            vec![Value::Int(6), Value::Str(crate::model::binary_display(32))],
        ];
        let mut src = chunked(&cols, &rows, 2);
        let mut buf = Vec::new();
        ExportFormat::Sql
            .stream_to(&mut buf, &mut src, Some(("shop", None, "img")), MySql)
            .expect("writing to a Vec cannot fail");
        let out = String::from_utf8(buf).expect("utf-8");
        assert_eq!(
            out.matches("-- NOTE:").count(),
            1,
            "the note should be announced once, not per chunk: {out}"
        );
        // The rows carrying real text keep it; only the placeholder rows are
        // nulled. A note emitted for the whole export up front would have been
        // right about the columns and wrong about these two rows.
        assert!(out.contains("(1, 'real text')"), "{out}");
        assert!(out.contains("(2, 'still text')"), "{out}");
        assert!(out.contains("(3, NULL)"), "{out}");
        assert!(!out.contains("4096 bytes"), "placeholder exported: {out}");
        // And the note precedes the first row it applies to, not the file.
        let note_at = out.find("-- NOTE:").expect("a note");
        let row3_at = out.find("(3, NULL)").expect("row 3");
        assert!(
            note_at < row3_at,
            "the note came after the dropped row: {out}"
        );
    }

    /// **A display order can name a row that is gone.** All five renderers guard
    /// it — four with `if di >= c.rs.row_count() { continue }`, JSON with a
    /// `filter` — and no test passed an out-of-range `order`, so dropping any one
    /// guard left a green suite and a format that panics on `rs.cell`'s slice
    /// arithmetic or writes a short file.
    ///
    /// It is unreachable on the *streaming* path (`PullChunks` rebuilds `order` as
    /// `0..row_count` per chunk) and live on the one-shot path, where the order is
    /// the caller's: the grid reads `gs.rs` and `gs.order` as two separate
    /// untracked reads, so an order can outlive the result it indexes.
    #[test]
    fn an_order_naming_a_row_that_is_gone_is_skipped_by_every_format() {
        let rs = ResultSet::from_rows(
            vec![col("id"), col("note")],
            vec![
                vec![Value::Int(1), Value::Str("one".into())],
                vec![Value::Int(2), Value::Str("two".into())],
            ],
        );
        // Two real rows with a stale index between them — the shape a result
        // spliced smaller under a held order produces.
        let order = [0usize, 5, 1];
        // Excel asks the same question in `a_stale_order_index_is_skipped_by_the
        // _xlsx_export`, where the answer is read back as cells rather than
        // searched for as text.
        for format in text_formats() {
            let mut buf = Vec::new();
            let tally = format
                .render_to(&mut buf, &rs, &order, Some(("shop", None, "t")), MySql)
                .expect("writing to a Vec cannot fail");
            assert_eq!(
                tally.rows,
                2,
                "{}: only the rows that exist are written",
                format.label()
            );
            let out = String::from_utf8(buf).expect("utf-8");
            assert!(out.contains("one"), "{}: {out}", format.label());
            assert!(out.contains("two"), "{}: {out}", format.label());
            // Nothing invented for the missing index: two data *rows*, not
            // three. Counted as tuples rather than as statements — rows are
            // batched into one multi-row `INSERT` now, so a statement count
            // measures the batching and not the rows.
            if format == ExportFormat::Sql {
                assert_eq!(out.matches("INSERT INTO").count(), 1, "{out}");
                assert_eq!(
                    out.matches(
                        "),
("
                    )
                    .count()
                        + 1,
                    2,
                    "{out}"
                );
            }
            if format == ExportFormat::Json {
                let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid json");
                assert_eq!(parsed.as_array().map(Vec::len), Some(2), "{out}");
            }
            if format == ExportFormat::Csv {
                assert_eq!(out.lines().count(), 3, "header + two rows: {out}");
            }
        }
    }

    /// A source that fails mid-stream must not be reported as a finished export.
    /// This is the dropped connection and the cancelled table export, which the
    /// one-shot path could never produce because its source was already in memory.
    #[test]
    fn a_source_failing_mid_stream_fails_the_export() {
        let (cols, rows) = awkward_rows();
        for format in ExportFormat::ALL {
            let mut sent = 0;
            let cols = cols.clone();
            let rows = rows.clone();
            let mut src = PullChunks::new(move || {
                sent += 1;
                match sent {
                    1 => Ok(Some(ResultSet::from_rows(cols.clone(), rows[..2].to_vec()))),
                    _ => Err(io::Error::other("connection reset")),
                }
            });
            let mut buf = Vec::new();
            let err = format
                .stream_to(&mut buf, &mut src, None, MySql)
                .expect_err("a failing source must fail the export");
            assert!(
                err.to_string().contains("connection reset"),
                "{}: the source's reason should survive: {err}",
                format.label()
            );
            // **And what is on disk is still openable.** JSON's `]` is written
            // once, after the loop, so a source error used to leave `…},` — a
            // file every parser rejects, i.e. the one format whose partial export
            // is worth nothing. The rows that did arrive are the point of a
            // cancel.
            if format == ExportFormat::Json {
                let text = String::from_utf8(buf).expect("utf-8");
                let parsed: serde_json::Value =
                    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
                assert_eq!(
                    parsed.as_array().map(Vec::len),
                    Some(2),
                    "the rows that arrived before the failure: {text}"
                );
            }
        }
    }

    /// A writer that fails after `ok_bytes` bytes — stands in for a full disk or a
    /// revoked permission part-way through a large export.
    struct FailingWriter {
        written: usize,
        ok_bytes: usize,
    }

    impl std::io::Write for FailingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.written >= self.ok_bytes {
                return Err(std::io::Error::other("disk full"));
            }
            let n = buf.len().min(self.ok_bytes - self.written);
            self.written += n;
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// The streaming and buffered paths must agree byte-for-byte, in every format.
    /// This is what lets the file export stream while the clipboard keeps a
    /// `String` without the two drifting — the same guarantee `ExportFormat`
    /// itself exists to give the Copy and Download menus.
    #[test]
    fn streaming_render_matches_the_string_render_in_every_format() {
        let rs = rs();
        let order = [1usize, 0];
        let source = Some(("db", None, "t"));
        // `render` returns a `String`, so this pairing only exists for the
        // formats `is_text` admits — which is the predicate's whole point.
        for f in text_formats() {
            let mut buf: Vec<u8> = Vec::new();
            f.render_to(&mut buf, &rs, &order, source, MySql).unwrap();
            assert_eq!(
                String::from_utf8(buf).unwrap(),
                f.render(&rs, &order, source, MySql),
                "{:?} streamed output differs from the buffered one",
                f
            );
        }
    }

    /// A result whose every cell needs escaping *somewhere*: a CSV delimiter and
    /// quote, an HTML entity, a Markdown pipe and backslash, a SQL quote and
    /// backslash, a formula trigger, a newline, and non-ASCII text.
    fn nasty_rs() -> ResultSet {
        ResultSet::from_rows(
            vec![col("a,b"), col("c|d"), col("e<f>")],
            vec![
                vec![
                    Value::Str("he\"llo, world".to_string()),
                    Value::Str(r"pipe | and \ backslash".to_string()),
                    Value::Str("<script>&amp;".to_string()),
                ],
                vec![
                    Value::Str("=HYPERLINK(\"x\")".to_string()),
                    Value::Str("-1+1+cmd|' /C calc'!A0".to_string()),
                    Value::Str("line\nbreak\ttab".to_string()),
                ],
                vec![
                    Value::Str("it's a 'quote'".to_string()),
                    Value::Str(r"C:\temp".to_string()),
                    Value::Str("José 東京 €".to_string()),
                ],
            ],
        )
    }

    /// The anti-drift gate above ran on data that exercised **none** of the
    /// escaping paths — plain `x`/`y` strings — so the two renderers could have
    /// disagreed on every escape in the codebase and it would still have passed.
    /// Escaping is exactly where a streamed and a buffered writer diverge, since
    /// that is where each one decides what bytes to emit.
    #[test]
    fn streaming_and_buffered_agree_on_data_that_needs_escaping() {
        let rs = nasty_rs();
        let order = [2usize, 0, 1];
        let source = Some(("db", None, "t"));
        for dialect in [MySql, Postgres] {
            for f in text_formats() {
                let mut buf: Vec<u8> = Vec::new();
                f.render_to(&mut buf, &rs, &order, source, dialect).unwrap();
                assert_eq!(
                    String::from_utf8(buf).unwrap(),
                    f.render(&rs, &order, source, dialect),
                    "{f:?}/{dialect:?} streamed output differs from the buffered one"
                );
            }
        }
    }

    /// …and that the escaping actually fired, so the fixture can't quietly stop
    /// being nasty.
    #[test]
    fn the_escaping_fixture_really_exercises_each_escape() {
        let rs = nasty_rs();
        let order = [0usize, 1, 2];
        let csv = ExportFormat::Csv.render(&rs, &order, None, MySql);
        assert!(csv.contains("\"he\"\"llo, world\""), "CSV quote doubling");
        assert!(csv.contains("'=HYPERLINK"), "CSV formula guard");
        assert!(csv.contains("'-1+1+cmd"), "CSV leading-dash guard");

        let html = ExportFormat::Html.render(&rs, &order, None, MySql);
        assert!(html.contains("&lt;script&gt;&amp;amp;"), "HTML entities");
        assert!(html.contains("José 東京 €"), "HTML non-ASCII");

        let md = ExportFormat::Markdown.render(&rs, &order, None, MySql);
        assert!(md.contains(r"\|"), "Markdown pipe escape");

        let my = ExportFormat::Sql.render(&rs, &order, Some(("db", None, "t")), MySql);
        assert!(my.contains(r"'C:\\temp'"), "MySQL backslash doubling");
        assert!(my.contains("'it''s a ''quote'''"), "SQL quote doubling");

        let pg = ExportFormat::Sql.render(&rs, &order, Some(("db", None, "t")), Postgres);
        assert!(pg.contains(r"'C:\temp'"), "PostgreSQL leaves backslashes");
    }

    #[test]
    fn streaming_column_exports_match_the_string_versions() {
        let rs = rs();
        let order = [0usize, 1];
        for ci in 0..2 {
            let mut csv: Vec<u8> = Vec::new();
            export_column_csv_to(&mut csv, &rs, &order, ci).unwrap();
            assert_eq!(
                String::from_utf8(csv).unwrap(),
                export_column_csv(&rs, &order, ci)
            );
            let mut json: Vec<u8> = Vec::new();
            export_column_json_to(&mut json, &rs, &order, ci).unwrap();
            assert_eq!(
                String::from_utf8(json).unwrap(),
                export_column_json(&rs, &order, ci)
            );
        }
    }

    /// The JSON array is emitted incrementally rather than built as one
    /// `serde_json::Value`, so pin the exact pretty-printed shape — a formatting
    /// drift here would silently change every exported file.
    ///
    /// Keys follow **column order** (`id` before `a\`b`), not alphabetical order —
    /// see [`RowObject`]. Going through `serde_json::Map` would sort them.
    #[test]
    fn streaming_json_keeps_the_pretty_array_layout() {
        let rs = rs();
        let mut buf: Vec<u8> = Vec::new();
        export_json_to(&mut buf, &rs, &[0, 1]).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[\n  {\n    \"id\": 1,\n    \"a`b\": \"x\"\n  },\n  \
             {\n    \"id\": null,\n    \"a`b\": \"y\"\n  }\n]"
        );
    }

    /// The export must preserve the order the user selected. A `SELECT` names its
    /// columns for a reason, and an alphabetically-sorted export silently throws
    /// that away — worst on a wide result, where `id` ends up buried mid-object.
    #[test]
    fn json_keys_follow_column_order_not_alphabetical() {
        let rs = ResultSet::from_rows(
            vec![col("zebra"), col("apple"), col("middle")],
            vec![vec![Value::Int(1), Value::Int(2), Value::Int(3)]],
        );
        let out = export_json(&rs, &[0]);
        let z = out.find("zebra").unwrap();
        let a = out.find("apple").unwrap();
        let m = out.find("middle").unwrap();
        assert!(z < a && a < m, "keys were reordered:\n{out}");
    }

    #[test]
    fn streaming_json_of_no_rows_is_an_empty_array() {
        let rs = rs();
        let mut buf: Vec<u8> = Vec::new();
        export_json_to(&mut buf, &rs, &[]).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "[]");
        assert_eq!(export_json(&rs, &[]), "[]");
    }

    /// A write failure must surface. Buffering into a `String` first made the
    /// whole export either succeed or never start; streaming can fail half-way
    /// through, and a caller that ignored that would leave a truncated file
    /// looking like a complete one.
    #[test]
    fn a_failing_writer_reports_the_error_in_every_format() {
        let rs = rs();
        for f in ExportFormat::ALL {
            let mut w = FailingWriter {
                written: 0,
                ok_bytes: 4,
            };
            let err = f.render_to(&mut w, &rs, &[0, 1], None, MySql).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::Other, "{:?}", f);
        }
    }

    #[test]
    fn ident_quotes_per_dialect() {
        // MySQL backticks (doubling an embedded backtick); Postgres double-quotes
        // (doubling an embedded double-quote).
        assert_eq!(ident_sql("a`b", MySql), "`a``b`");
        assert_eq!(ident_sql("plain", Postgres), "\"plain\"");
        assert_eq!(ident_sql("a\"b", Postgres), "\"a\"\"b\"");
        // The other dialect's quote char is NOT special — it's just a character.
        assert_eq!(ident_sql("a\"b", MySql), "`a\"b`");
        assert_eq!(ident_sql("a`b", Postgres), "\"a`b\"");
    }

    /// How a table is addressed in generated SQL, per engine — and the reason
    /// SQLite names it **bare**.
    #[test]
    fn a_table_is_qualified_the_way_its_engine_needs() {
        // MySQL: a connection is server-level, so the database qualifies.
        assert_eq!(
            qualified_table("shop", None, "orders", MySql),
            "`shop`.`orders`"
        );
        // Postgres: the connection is already bound to the database, so the
        // *namespace* qualifies — and `public` is dropped by the caller.
        assert_eq!(
            qualified_table("shop", Some("sales"), "orders", Postgres),
            "\"sales\".\"orders\""
        );
        assert_eq!(
            qualified_table("shop", None, "orders", Postgres),
            "\"shop\".\"orders\""
        );
        // SQLite: a connection *is* one file. `main` is not a name the user
        // chose — it is SQLite's word for "the file you opened" — so qualifying
        // with it is noise here and actively wrong in a session where another
        // file is attached under that name.
        assert_eq!(
            qualified_table("main", None, "orders", Sqlite),
            "\"orders\""
        );
        // …and it stays bare whatever it is asked to qualify with.
        assert_eq!(
            qualified_table("other", Some("x"), "orders", Sqlite),
            "\"orders\""
        );
    }

    /// SQLite *reads* `"x"`, `` `x` `` and `[x]`, but emits only the standard
    /// form: `"` is the one of the three with a defined escape, and a name
    /// carrying a `]` could not be written in brackets at all.
    #[test]
    fn sqlite_emits_the_standard_quoting_of_its_three() {
        assert_eq!(ident_sql("plain", Sqlite), "\"plain\"");
        assert_eq!(ident_sql("a\"b", Sqlite), "\"a\"\"b\"");
        // The two compatibility quotings are ordinary characters on the way out.
        assert_eq!(ident_sql("a`b", Sqlite), "\"a`b\"");
        assert_eq!(ident_sql("a]b", Sqlite), "\"a]b\"");
    }

    /// SQLite has no backslash escape, so doubling one would corrupt the value —
    /// the same reason Postgres doesn't, and the same failure (`C:\tmp` →
    /// `C:\\tmp` written into the row).
    #[test]
    fn sql_literal_does_not_touch_a_backslash_on_sqlite() {
        let v = Value::Str("C:\\tmp".to_string());
        assert_eq!(sql_literal(&v, Sqlite), "'C:\\tmp'");
        assert_eq!(sql_literal(&v, Postgres), "'C:\\tmp'");
        assert_eq!(sql_literal(&v, MySql), "'C:\\\\tmp'");
        // The injection guard is doubling the quote, and that is on everywhere.
        assert_eq!(
            sql_literal(&Value::Str("O'Hara".to_string()), Sqlite),
            "'O''Hara'"
        );
    }

    /// Every identifier-quoting entry point in the workspace answers to
    /// `ident_sql`. The *reading* side has had a single-lexer invariant since
    /// round 1; the writing side had four implementations that had each
    /// independently arrived at the same escaping — which is the drift hazard,
    /// not the reassurance, and one of them (`ddl_string`, the literal half) had
    /// already been found wrong.
    ///
    /// The `schemaic-db` pair is bound by a matching test in that crate, since
    /// its functions are private to it.
    #[test]
    fn every_identifier_quoter_agrees_with_ident_sql() {
        let nasty = [
            "plain",
            "MixedCase",
            "with space",
            "a`b",
            "a\"b",
            "both`and\"",
            "sélect",
            "",
        ];
        for name in nasty {
            for d in [MySql, Postgres, Sqlite] {
                assert_eq!(
                    crate::schema::ddl_ident_in(name, d),
                    ident_sql(name, d),
                    "ddl_ident_in({name:?}, {d:?})"
                );
            }
            // `filter`'s is `ident_sql` itself now; assert the re-export really
            // is the same function rather than a lookalike that could be swapped
            // back to a local copy.
            assert_eq!(
                crate::filter::quoted_ident_for_test(name, MySql),
                ident_sql(name, MySql)
            );
        }
    }

    #[test]
    fn sql_literal_handles_nonfinite_and_escapes() {
        assert_eq!(sql_literal(&Value::Float(f64::NAN), MySql), "NULL");
        assert_eq!(sql_literal(&Value::Float(f64::INFINITY), MySql), "NULL");
        assert_eq!(
            sql_literal(&Value::Str("O'Hara".to_string()), MySql),
            "'O''Hara'"
        );
    }

    #[test]
    fn sql_literal_only_escapes_backslashes_on_mysql() {
        // MySQL treats `\` as an escape inside a string, so it must be doubled.
        assert_eq!(
            sql_literal(&Value::Str(r"C:\tmp".to_string()), MySql),
            r"'C:\\tmp'"
        );
        // Postgres (standard_conforming_strings = on, the default since 9.1) takes
        // a backslash literally — doubling it would silently CORRUPT the value,
        // turning `C:\tmp` into `C:\\tmp`.
        assert_eq!(
            sql_literal(&Value::Str(r"C:\tmp".to_string()), Postgres),
            r"'C:\tmp'"
        );
        // Quote-doubling is the injection guard on both.
        assert_eq!(
            sql_literal(&Value::Str("x'; DROP TABLE t; --".to_string()), Postgres),
            "'x''; DROP TABLE t; --'"
        );
    }

    #[test]
    fn c5_inserts_use_real_table_and_escape_identifiers() {
        let out = export_inserts(&rs(), &[0, 1], Some(("shop", None, "cust")), MySql);
        // Real qualified table, not a `table` placeholder; column `a`b` escaped.
        assert!(out.contains("INSERT INTO `shop`.`cust` (`id`, `a``b`) VALUES"));
        assert!(out.contains("(1, 'x')"));
        assert!(out.contains("(NULL, 'y')"));
        // Placeholder only when the source is unknown.
        assert!(export_inserts(&rs(), &[0], None, MySql).contains("INSERT INTO `table` ("));
    }

    #[test]
    fn inserts_qualify_by_namespace_instead_of_database() {
        // A PostgreSQL connection is bound to one database, so the namespace is
        // what makes the name resolvable — `schema.table`, not `db.table`.
        let out = export_inserts(
            &rs(),
            &[0],
            Some(("warehouse", Some("sales"), "orders")),
            Postgres,
        );
        assert!(out.contains("INSERT INTO \"sales\".\"orders\" ("), "{out}");
        assert!(!out.contains("warehouse"), "{out}");
    }

    #[test]
    fn inserts_for_postgres_are_valid_postgres() {
        // The whole statement has to be pasteable into a PG client: every
        // identifier double-quoted, none backtick-quoted. Note the fixture's
        // column is literally named "a`b" — on Postgres that backtick is an
        // ordinary character inside the name, so the check is that no identifier
        // is *wrapped* in backticks, not that none appears at all.
        let out = export_inserts(
            &rs(),
            &[0, 1],
            Some(("db", Some("public"), "cust")),
            Postgres,
        );
        assert!(
            out.contains("INSERT INTO \"public\".\"cust\" (\"id\", \"a`b\") VALUES"),
            "{out}"
        );
        assert!(!out.contains("`id`"), "MySQL quoting leaked: {out}");
        assert!(!out.contains("`cust`"), "MySQL quoting leaked: {out}");
        // The unknown-source placeholder follows the dialect too.
        let ph = export_inserts(&rs(), &[0], None, Postgres);
        assert!(ph.contains("INSERT INTO \"table\" ("), "{ph}");
        assert!(!ph.contains("`table`"), "{ph}");
    }

    /// A BLOB has no text form, so the grid shows `<n bytes>` — and an INSERT
    /// export that copies that placeholder through as a string literal produces
    /// a script which *silently writes the wrong bytes* into the column on
    /// re-import. `NULL` is wrong too, but visibly so, and the header comment
    /// says which columns it happened to.
    #[test]
    fn inserts_never_write_a_binary_placeholder_as_the_data() {
        let mut blob = col("thumb");
        blob.type_name = "BLOB".to_string();
        let rs = ResultSet::from_rows(
            vec![col("id"), blob],
            vec![vec![
                Value::Int(1),
                Value::Str(crate::model::binary_display(4096)),
            ]],
        );
        let out = export_inserts(&rs, &[0], Some(("shop", None, "img")), MySql);
        assert!(
            !out.contains("4096 bytes"),
            "placeholder exported as data: {out}"
        );
        assert!(out.contains("(1, NULL)"), "{out}");
        assert!(
            out.contains("thumb"),
            "the note should name the column: {out}"
        );
    }

    /// The note is a *comment*, so a script with a nulled BLOB still runs. And
    /// it must not appear for a result with no binary column at all — a note on
    /// every export is a note nobody reads.
    #[test]
    fn the_binary_note_is_a_comment_and_only_when_there_is_one() {
        let mut blob = col("thumb");
        blob.type_name = "BLOB".to_string();
        let with_blob = ResultSet::from_rows(
            vec![blob],
            vec![vec![Value::Str(crate::model::binary_display(1))]],
        );
        let out = export_inserts(&with_blob, &[0], None, MySql);
        assert!(out.starts_with("--"), "{out}");
        assert!(!export_inserts(&rs(), &[0], None, MySql).contains("--"));
    }

    /// A text column whose value happens to read like the placeholder is not a
    /// blob, and nulling it would delete real data. Both signals must agree.
    #[test]
    fn a_text_column_that_merely_looks_like_a_placeholder_is_left_alone() {
        let rs = ResultSet::from_rows(
            vec![col("note")],
            vec![vec![Value::Str("<12 bytes>".to_string())]],
        );
        let out = export_inserts(&rs, &[0], None, MySql);
        assert!(out.contains("'<12 bytes>'"), "{out}");
    }

    /// The inverse: a genuinely binary column holding a value that is *not* the
    /// placeholder (PostgreSQL's `bytea_output = escape`, say) still carries its
    /// own text, and replacing that with NULL would be the data loss this test
    /// exists to prevent.
    #[test]
    fn a_binary_column_with_real_text_is_not_nulled() {
        let mut b = col("payload");
        b.type_name = "BYTEA".to_string();
        let rs = ResultSet::from_rows(vec![b], vec![vec![Value::Str("\\x4869".to_string())]]);
        let out = export_inserts(&rs, &[0], None, Postgres);
        assert!(out.contains("4869"), "{out}");
    }

    fn blob_rs() -> ResultSet {
        let mut blob = col("thumb");
        blob.type_name = "BLOB".to_string();
        ResultSet::from_rows(
            vec![col("id"), blob],
            vec![vec![
                Value::Int(1),
                Value::Str(crate::model::binary_display(4096)),
            ]],
        )
    }

    /// **The two formats Schemaic itself re-imports.** `import::ImportFormat` is
    /// CSV and JSON, so a blob exported to either and read back stores the
    /// placeholder *as the column's data* — the same round trip
    /// `inserts_never_write_a_binary_placeholder_as_the_data` closed for the SQL
    /// export, left open at the other two emitters.
    ///
    /// Blank rather than the text, which is how both formats already render
    /// NULL: it is the honest claim, since the bytes genuinely aren't here.
    #[test]
    fn the_re_importable_formats_never_carry_a_binary_placeholder() {
        let rs = blob_rs();
        let csv = export_csv(&rs, &[0]);
        assert!(!csv.contains("4096 bytes"), "csv: {csv}");
        assert_eq!(csv, "id,thumb\n1,\n");

        let json = export_json(&rs, &[0]);
        assert!(!json.contains("4096 bytes"), "json: {json}");
        assert!(json.contains("\"thumb\": null"), "{json}");

        // …and the single-column forms, which are the "copy this column" paths.
        let ccsv = export_column_csv(&rs, &[0], 1);
        assert!(!ccsv.contains("4096 bytes"), "column csv: {ccsv}");
        assert_eq!(ccsv, "\n");
        let cjson = export_column_json(&rs, &[0], 1);
        assert!(!cjson.contains("4096 bytes"), "column json: {cjson}");
        assert!(cjson.contains("null"), "{cjson}");
    }

    /// **Markdown and HTML keep it, deliberately.** Neither is a format anything
    /// reads back — they are for a person to look at — and there the placeholder
    /// is the *useful* rendering: blanking it would make a 4 MB blob
    /// indistinguishable from an empty cell and from NULL, which is strictly
    /// less than what the grid itself shows.
    #[test]
    fn the_presentation_formats_keep_the_placeholder() {
        let rs = blob_rs();
        assert!(export_markdown(&rs, &[0]).contains("<4096 bytes>"));
        // HTML-escaped, but still there.
        assert!(export_html(&rs, &[0]).contains("&lt;4096 bytes&gt;"));
    }

    /// Both signals must agree in *every* format, not only in the SQL one: a
    /// text column whose value reads like the placeholder is real data, and
    /// blanking it in a CSV would delete it.
    #[test]
    fn a_text_column_that_looks_like_a_placeholder_survives_every_format() {
        let rs = ResultSet::from_rows(
            vec![col("note")],
            vec![vec![Value::Str("<12 bytes>".to_string())]],
        );
        assert!(export_csv(&rs, &[0]).contains("<12 bytes>"));
        assert!(export_json(&rs, &[0]).contains("<12 bytes>"));
        assert!(export_column_csv(&rs, &[0], 0).contains("<12 bytes>"));
        assert!(export_column_json(&rs, &[0], 0).contains("<12 bytes>"));
    }

    /// **A bit-field's number must reach SQL as a number.** `8ed98fe` took `BIT`
    /// off the binary list on the stated grounds that a bit-field "has a lossless
    /// text form (its number, which is also what MySQL accepts back)" — true of
    /// the bare token `10`, false of `'10'`, which is the only thing an export can
    /// emit for a `Value::Str`. MySQL's `Field_bit::store(const char*, …)` takes
    /// the raw bits of a string's bytes, so `'10'` is `0x3132` = 12594 on a
    /// `BIT(16)` and "Data too long" on a `BIT(8)`: the round trip that change
    /// was made to enable wrote wrong data instead of withholding it.
    ///
    /// The composition, not the pieces: `bit_display` had a full table of tests
    /// and was right all along, and `a_bit_fields_bytes_read_as_a_big_endian_number`
    /// is green against this bug. What nothing asked was which `Value` the
    /// loader wraps the number in.
    #[test]
    fn a_bit_field_exports_as_a_bare_number() {
        use crate::model::bit_cell;
        // What the MySQL loader stores for `BIT(16)` holding 10 …
        let cell = bit_cell(&[0x00, 0x0A]);
        // … reaches SQL unquoted, on every dialect.
        for d in [MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert_eq!(sql_literal(&cell, d), "10", "{d:?}");
        }
        // …and JSON as a number, not a string.
        assert_eq!(value_to_json(&cell), serde_json::json!(10));
        // The widest field there is, and the empty run a server should never
        // send — both still numbers.
        assert_eq!(
            sql_literal(&bit_cell(&[0xFF; 8]), MySql),
            u64::MAX.to_string()
        );
        assert_eq!(sql_literal(&bit_cell(&[]), MySql), "0");
        // And it is still the same digits the grid shows.
        let rs = {
            let mut b = col("flags");
            b.type_name = "BIT(16)".to_string();
            ResultSet::from_rows(vec![b], vec![vec![bit_cell(&[0x00, 0x0A])]])
        };
        assert_eq!(export_csv(&rs, &[0]), "flags\n10\n");
        assert!(
            export_inserts(&rs, &[0], None, MySql).contains("(10)"),
            "{}",
            export_inserts(&rs, &[0], None, MySql)
        );
    }

    // ── What the export says it could not carry ───────────────────────────
    //
    // The loss itself is right (above); these are about *saying so*. CSV and
    // JSON have no comment syntax, so the withheld column left no trace in the
    // file at all — and an empty CSV field is what a NULL looks like.

    /// **The formats that drop the bytes report the columns they dropped.** The
    /// SQL emitter writes a `-- NOTE:` line for exactly this loss; the two
    /// formats Schemaic itself re-imports wrote nothing anywhere.
    #[test]
    fn a_withholding_export_reports_the_columns_it_blanked() {
        let rs = blob_rs();
        for format in [ExportFormat::Csv, ExportFormat::Json, ExportFormat::Sql] {
            let mut buf = Vec::new();
            let t = format
                .render_to(&mut buf, &rs, &[0], None, MySql)
                .expect("writing to a Vec cannot fail");
            assert_eq!(t.rows, 1, "{}", format.label());
            assert_eq!(t.withheld, vec!["thumb".to_string()], "{}", format.label());
            assert!(t.has_caveat(), "{}", format.label());
        }
        // Markdown and HTML keep the placeholder on purpose, so they have
        // nothing to disclose — reporting a loss they did not take would send
        // the user looking for data that is in the file.
        for format in [ExportFormat::Markdown, ExportFormat::Html] {
            let mut buf = Vec::new();
            let t = format
                .render_to(&mut buf, &rs, &[0], None, MySql)
                .expect("writing to a Vec cannot fail");
            assert!(t.withheld.is_empty(), "{}", format.label());
            assert!(!t.has_caveat(), "{}", format.label());
        }
    }

    /// A column named once, however many chunks carried it — a streamed export
    /// sees the same blob column in every block.
    #[test]
    fn a_withheld_column_is_named_once_across_a_stream() {
        let rs = blob_rs();
        // Three chunks of the same one-row result: the same column withheld in
        // each. `chunked` is the same helper the round-trip test uses.
        struct Thrice<'a>(&'a ResultSet, &'a [usize], usize);
        impl RowChunks for Thrice<'_> {
            fn next_chunk(&mut self) -> io::Result<Option<RowChunk<'_>>> {
                if self.2 == 0 {
                    return Ok(None);
                }
                self.2 -= 1;
                Ok(Some(RowChunk {
                    rs: self.0,
                    order: self.1,
                }))
            }
        }
        let mut src = Thrice(&rs, &[0], 3);
        let mut buf = Vec::new();
        let t = ExportFormat::Csv
            .stream_to(&mut buf, &mut src, None, MySql)
            .expect("writing to a Vec cannot fail");
        assert_eq!(t.rows, 3);
        assert_eq!(t.withheld, vec!["thumb".to_string()]);
    }

    /// **The arena ceiling, which nothing read.** A column whose 512 MiB text
    /// arena filled up renders blank from that row on; the grid says so in its
    /// own note, but a streamed chunk is never mounted in a grid, so a
    /// whole-table export could write a file with holes in it and report a full
    /// row count. Every format reports it — an empty cell reads as empty
    /// whatever the syntax around it.
    #[test]
    fn a_column_blanked_by_the_arena_ceiling_is_reported_by_every_format() {
        let mut rs = blob_rs();
        // The blanking is `ColumnData::finish_cell`'s and is tested there; what
        // reaches an exporter is the flag.
        rs.capped_columns = vec![1];
        for format in ExportFormat::ALL {
            let mut buf = Vec::new();
            let t = format
                .render_to(&mut buf, &rs, &[0], None, MySql)
                .expect("writing to a Vec cannot fail");
            assert_eq!(t.blanked, vec!["thumb".to_string()], "{}", format.label());
            assert!(t.has_caveat(), "{}", format.label());
        }
    }

    /// A dump writes many tables into one file and reports one sentence about
    /// it. The fold used to be written out inside the writer loop, where nothing
    /// could reach it: the caller needs a `Db`, a runtime handle and two
    /// channels.
    #[test]
    fn absorbing_another_tables_tally_sums_rows_and_names_a_column_once() {
        let mut total = ExportTally {
            rows: 10,
            withheld: vec!["body".to_string()],
            blanked: vec!["notes".to_string()],
            cut: vec!["memo".to_string()],
        };
        // A second table with the *same* wide column: named once, not twice.
        total.absorb(ExportTally {
            rows: 5,
            withheld: vec!["body".to_string()],
            cut: vec!["memo".to_string()],
            ..Default::default()
        });
        assert_eq!(total.rows, 15);
        assert_eq!(total.withheld, vec!["body".to_string()]);

        // A third with a different one: both kept, in first-seen order — the
        // caveat has to name every part of the file to distrust.
        total.absorb(ExportTally {
            rows: 1,
            withheld: vec!["thumb".to_string()],
            blanked: vec!["notes".to_string(), "memo".to_string()],
            cut: vec!["notes".to_string()],
        });
        assert_eq!(total.rows, 16);
        assert_eq!(
            total.withheld,
            vec!["body".to_string(), "thumb".to_string()]
        );
        assert_eq!(total.blanked, vec!["notes".to_string(), "memo".to_string()]);
        // The third category folds by the same rule: `memo` once, `notes` kept.
        assert_eq!(total.cut, vec!["memo".to_string(), "notes".to_string()]);
        assert!(total.has_caveat());
    }

    #[test]
    fn absorbing_a_clean_table_says_nothing_new() {
        let mut total = ExportTally::default();
        total.absorb(ExportTally {
            rows: 7,
            ..Default::default()
        });
        assert_eq!(total.rows, 7);
        assert!(!total.has_caveat());
    }

    /// The sentence the bar shows. **Silence is the default and a caveat
    /// overrides it**: a save of what is already on screen says nothing, a
    /// streamed one says its count, and a loss is said in either scope.
    #[test]
    fn the_export_note_says_what_could_not_be_carried() {
        let clean = ExportTally {
            rows: 2,
            ..Default::default()
        };
        assert_eq!(export_note(&clean, "docs.csv", false), None);
        assert_eq!(
            export_note(&clean, "docs.csv", true).as_deref(),
            Some("Exported 2 rows to docs.csv")
        );

        // **A withheld binary column is tallied but not said.** The grid already
        // shows the cell as `<7 bytes>`, so "a text export cannot hold raw bytes"
        // told the user what the screen in front of them had already told them —
        // and it did so in a clause long enough to overflow the export modal it
        // was rendered in. The tally still records it (`withheld` is what the SQL
        // writer's `-- binary column` comment and `has_caveat` read), it just no
        // longer earns a sentence.
        let one = ExportTally {
            rows: 2,
            withheld: vec!["file".to_string()],
            ..Default::default()
        };
        // Not merely trimmed — a withheld column no longer breaks the silence of
        // a non-streamed save at all, which is the half a caller-blind test of
        // the string would have missed.
        assert_eq!(export_note(&one, "docs.csv", false), None);
        assert!(one.has_caveat(), "the loss is still recorded on the tally");
        let two = ExportTally {
            rows: 1,
            withheld: vec!["file".to_string(), "thumb".to_string()],
            ..Default::default()
        };
        assert_eq!(
            export_note(&two, "docs.csv", true).as_deref(),
            Some("Exported 1 row to docs.csv")
        );
        let blanked = ExportTally {
            rows: 2_000_000,
            blanked: vec!["body".to_string()],
            ..Default::default()
        };
        assert_eq!(
            export_note(&blanked, "docs.csv", true).as_deref(),
            Some(
                "Exported 2m rows to docs.csv — body too large to hold in full: later rows are blank"
            )
        );
        // Both losses, one sentence — and the seam is now the *first* said
        // caveat's. A withheld column beside a blanked one must not leave the
        // stray `; ` that a dropped clause would: this is the composition the
        // string edit could break in silence.
        let both = ExportTally {
            rows: 3,
            withheld: vec!["file".to_string()],
            blanked: vec!["body".to_string()],
            cut: Vec::new(),
        };
        assert_eq!(
            export_note(&both, "docs.csv", true).as_deref(),
            Some(
                "Exported 3 rows to docs.csv — body too large to hold in full: later rows are blank"
            )
        );

        // The truncation caveat joins the same sentence, and reads on its own
        // when it is the only loss — the `— ` / `; ` seam is per-category, so a
        // caveat that only ever appeared alongside another would hide a bug here.
        let cut = ExportTally {
            rows: 4,
            cut: vec!["payload".to_string()],
            ..Default::default()
        };
        assert_eq!(
            export_note(&cut, "docs.xlsx", false).as_deref(),
            Some(
                "Exported 4 rows to docs.xlsx — payload cut to Excel's 32767-character cell limit"
            )
        );
        let all_three = ExportTally {
            rows: 5,
            withheld: vec!["file".to_string()],
            blanked: vec!["body".to_string()],
            cut: vec!["payload".to_string()],
        };
        let msg = export_note(&all_three, "docs.xlsx", true).expect("a caveat is always said");
        assert!(msg.contains("; payload cut to Excel's"), "{msg}");
        assert!(!msg.contains("raw bytes"), "{msg}");
        // A withheld column alongside a *cut* one — the seam `cut` used to reach
        // through `withheld` to compute. It must open the sentence, not join it.
        let withheld_and_cut = ExportTally {
            rows: 5,
            withheld: vec!["file".to_string()],
            cut: vec!["payload".to_string()],
            ..Default::default()
        };
        assert_eq!(
            export_note(&withheld_and_cut, "docs.xlsx", true).as_deref(),
            Some(
                "Exported 5 rows to docs.xlsx — payload cut to Excel's 32767-character cell limit"
            )
        );
    }

    /// **A failure leaves the destination alone**, which is the sentence's whole
    /// job now: the rows go to a `.part` sibling and it is renamed over the target
    /// only when the export finished, so the file the user was overwriting is
    /// intact — and the rows that did arrive are still somewhere they can be had.
    ///
    /// It used to say `— orders.csv is incomplete`, which was true when
    /// `File::create` truncated the destination at `t = 0` and is now the opposite
    /// of true. And an export refused *before* the write still must not mention a
    /// file at all.
    #[test]
    fn a_failed_write_says_the_destination_survived_and_where_the_rows_went() {
        let msg = export_failure_note("Export failed: No space left on device", Some("orders.csv"));
        assert_eq!(
            msg,
            "Export failed: No space left on device — orders.csv was not changed; \
             the rows that were written are in orders.csv.part"
        );
        // The two facts, named rather than pattern-matched, so a reworded sentence
        // that drops one of them fails here.
        assert!(msg.contains("was not changed"), "{msg}");
        assert!(msg.contains("orders.csv.part"), "{msg}");
        assert!(
            !msg.contains("is incomplete"),
            "the destination is not a fragment any more: {msg}"
        );

        assert_eq!(
            export_failure_note("An export is already running.", None),
            "An export is already running."
        );
    }

    /// The cancel arm says the same two things, in the voice it has always used —
    /// stopping was the user's own doing, so it is a note and not an error.
    #[test]
    fn a_cancelled_export_says_the_same_two_things() {
        let msg = export_cancel_note("orders.csv", true);
        assert_eq!(
            msg,
            "Export cancelled — orders.csv was not changed; \
             the rows that were written are in orders.csv.part"
        );
        assert!(msg.starts_with("Export cancelled"), "{msg}");
        assert!(!msg.contains("is incomplete"), "{msg}");
    }

    // ── The folder export's three sentences ──────────────────────────────────

    #[test]
    fn a_folder_export_reports_its_files_and_its_caveats() {
        let t = ExportTally {
            rows: 115_000,
            ..Default::default()
        };
        let msg = files_note(12, &t, "sakila-csv", &[], &[]);
        assert!(msg.starts_with("Wrote 12 files."), "{msg}");
        assert!(msg.contains("115k rows"), "{msg}");
        // The destination is stated once, by `export_note` — naming it in both
        // clauses spells the folder twice in a sentence read once.
        assert_eq!(msg.matches("sakila-csv").count(), 1, "{msg}");
    }

    #[test]
    fn a_folder_export_of_one_file_says_file() {
        let t = ExportTally {
            rows: 3,
            ..Default::default()
        };
        assert!(
            files_note(1, &t, "out", &[], &[]).starts_with("Wrote 1 file."),
            "{}",
            files_note(1, &t, "out", &[], &[])
        );
    }

    #[test]
    fn a_folder_export_names_what_the_files_could_not_carry() {
        // The whole reason the tally travels with the count: a green "Wrote 12
        // files." over a folder whose every column was truncated is the failure
        // this sentence exists to prevent. Same rule as `export_note`'s — and the
        // same exception, since a **withheld** binary column is no longer said
        // there, so it is not said here either.
        let t = ExportTally {
            rows: 10,
            blanked: vec!["photo".to_string()],
            ..Default::default()
        };
        let msg = files_note(12, &t, "out", &[], &[]);
        assert!(msg.contains("photo"), "{msg}");
        assert!(msg.contains("too large to hold in full"), "{msg}");

        // The folder export is `streaming: true`, so a withheld-only tally still
        // gets its count sentence — it just carries no caveat, and above all no
        // dangling em dash where the clause used to be.
        let withheld_only = ExportTally {
            rows: 10,
            withheld: vec!["photo".to_string()],
            ..Default::default()
        };
        let msg = files_note(12, &withheld_only, "out", &[], &[]);
        assert_eq!(msg, "Wrote 12 files. Exported 10 rows to out.");
    }

    /// **The two clauses have to be one readable sentence.** `export_note` does
    /// not end its own — its single-file caller is the last clause of one — and
    /// `missing_clause` opens with a space and a digit, so the two ran together
    /// into "…Exported 115k rows to sakila-csv 1 ticked table not found and was
    /// no file: ghost." Six tests asserted fragments of each half and none the
    /// join, which is the whole shape of how it shipped.
    #[test]
    fn the_row_count_and_the_missing_tables_do_not_run_into_one_sentence() {
        let t = ExportTally {
            rows: 115_000,
            ..Default::default()
        };
        let msg = files_note(11, &t, "sakila-csv", &["ghost".to_string()], &[]);
        assert!(
            msg.contains("to sakila-csv. 1 ticked table"),
            "the row count's sentence has to end before the next one starts: {msg}"
        );
    }

    /// **A folder export overwrites without asking, so it has to say so.**
    /// `select_directories()` has no "replace?" — the guard the single-file
    /// export gets from the save dialog — and nothing between the picker and the
    /// `rename` looked, so files in the folder the user chose were replaced in
    /// silence and named nowhere.
    #[test]
    fn a_folder_export_names_the_files_it_replaced() {
        let t = ExportTally::default();
        let replaced = ["orders.csv".to_string(), "staff.csv".to_string()];
        let msg = files_note(4, &t, "out", &[], &replaced);
        assert!(msg.contains("2 existing files were replaced"), "{msg}");
        assert!(msg.contains("orders.csv, staff.csv"), "{msg}");
        // Singular reads as one file, not "1 files".
        let one = files_note(1, &t, "out", &[], &replaced[..1]);
        assert!(one.contains("1 existing file was replaced"), "{one}");
    }

    /// And all three arms carry it, for `missing_clause`'s reason turned up one
    /// notch: a stopped or failed export is *more* likely to be inspected than a
    /// finished one, and it has already replaced whatever it got to.
    #[test]
    fn every_folder_outcome_says_what_it_replaced() {
        let t = ExportTally::default();
        let gone = ["orders.csv".to_string()];
        for msg in [
            files_note(3, &t, "out", &[], &gone),
            files_cancel_note(2, "out", &[], &gone),
            files_cancel_note(0, "out", &[], &gone),
            files_failure_note("Export failed: disk", 1, "out", &[], &gone),
        ] {
            assert!(msg.contains("orders.csv"), "{msg}");
        }
        // And none of them invents one when the folder was empty.
        assert!(!files_note(3, &t, "out", &[], &[]).contains("replaced"));
        assert!(!files_cancel_note(2, "out", &[], &[]).contains("replaced"));
    }

    #[test]
    fn a_folder_export_names_the_tables_it_could_not_find() {
        // A folder one file short of what was ticked looks exactly like a
        // complete one, so the shortfall goes in the same sentence as the count.
        let t = ExportTally::default();
        let msg = files_note(
            2,
            &t,
            "out",
            &["ghost".to_string(), "gone".to_string()],
            &[],
        );
        assert!(msg.contains("ghost, gone"), "{msg}");
        assert!(msg.contains("not found"), "{msg}");
    }

    #[test]
    fn a_stopped_folder_export_says_what_it_kept() {
        // The completed files are whole and are what the user asked for, so the
        // sentence leads with them rather than with the fragment.
        let msg = files_cancel_note(7, "out", &[], &[]);
        assert!(msg.starts_with("Export cancelled"), "{msg}");
        assert!(msg.contains("7 files"), "{msg}");
        assert!(msg.contains("out"), "{msg}");
        // Nothing was written yet: the sentence must not claim files that are
        // not there.
        let none = files_cancel_note(0, "out", &[], &[]);
        assert!(!none.contains("0 files"), "{none}");
        assert!(none.contains("no file"), "{none}");
    }

    /// **Every arm names the tables that were never found**, not just the happy
    /// one — a folder short of what was ticked looks complete whichever way the
    /// export ended, and a stopped export is the more likely of the two to be
    /// inspected.
    #[test]
    fn a_stopped_or_failed_folder_export_still_names_what_went_missing() {
        let gone = vec!["ghost".to_string(), "gone".to_string()];
        for msg in [
            files_cancel_note(7, "out", &gone, &[]),
            files_cancel_note(0, "out", &gone, &[]),
            files_failure_note("Export failed: disk", 3, "out", &gone, &[]),
            files_failure_note("Export failed: no connection", 0, "out", &gone, &[]),
        ] {
            assert!(msg.contains("ghost, gone"), "{msg}");
            assert!(msg.contains("not found"), "{msg}");
        }
        // And nothing is appended when nothing went missing — the common case
        // must not grow a clause about an empty set.
        assert!(!files_cancel_note(7, "out", &[], &[]).contains("not found"));
    }

    #[test]
    fn a_failed_folder_export_still_names_the_files_that_landed() {
        // The difference from `export_failure_note`: the folder is not empty, and
        // a message that did not say so sends the user looking for nothing.
        let msg = files_failure_note("Export failed: No space left on device", 3, "out", &[], &[]);
        assert!(msg.starts_with("Export failed: No space left"), "{msg}");
        assert!(msg.contains("3 files"), "{msg}");
        // Failed before anything landed — no count to report, and no folder to
        // send anyone to.
        let none = files_failure_note("Export failed: no connection", 0, "out", &[], &[]);
        assert_eq!(none, "Export failed: no connection");
    }

    /// The sibling is the destination plus a suffix and nothing cleverer — a
    /// rename inside one directory is what makes the publish atomic, so the name
    /// must not move the file anywhere.
    #[test]
    fn the_part_file_is_a_sibling_of_the_destination() {
        assert_eq!(part_path("orders.csv"), "orders.csv.part");
        // Extensions and dots in the stem are left exactly alone: the suffix is
        // appended, never substituted, so nothing can collide with a real file the
        // user has by having its extension replaced.
        assert_eq!(part_path("a.b.c.json"), "a.b.c.json.part");
        assert_eq!(part_path("no-extension"), "no-extension.part");
        // No separator is introduced, on either platform's spelling.
        for name in ["orders.csv", "a.b.c.json", "no-extension"] {
            let p = part_path(name);
            assert!(p.starts_with(name), "{p}");
            assert!(!p[name.len()..].contains('/'), "{p}");
            assert!(!p[name.len()..].contains('\\'), "{p}");
        }
    }

    /// **Every way the file will differ from the screen, at the point of
    /// choice.** The sort was disclosed; the transaction was not, and it is the
    /// larger of the two — an `All rows` export re-runs on a fresh connection,
    /// so a manual-transaction tab's uncommitted rows are on screen and absent
    /// from the file.
    #[test]
    fn the_all_rows_label_discloses_every_way_the_file_differs() {
        assert_eq!(all_rows_label("", false, false), "All rows");
        assert_eq!(all_rows_label("~16k", false, false), "All rows (~16k)");
        assert_eq!(
            all_rows_label("", true, false),
            "All rows (server order)",
            "the sort was already disclosed and must stay so"
        );
        assert_eq!(
            all_rows_label("~16k", true, false),
            "All rows (~16k, server order)"
        );
        assert_eq!(
            all_rows_label("", false, true),
            "All rows (committed rows only)"
        );
        assert_eq!(
            all_rows_label("~16k", false, true),
            "All rows (~16k, committed rows only)"
        );
        assert_eq!(
            all_rows_label("", true, true),
            "All rows (server order, committed rows only)"
        );
        assert_eq!(
            all_rows_label("~16k", true, true),
            "All rows (~16k, server order, committed rows only)"
        );
    }

    /// And the inverse, again in every format: a binary column carrying real
    /// text is not blanked.
    #[test]
    fn a_binary_column_with_real_text_survives_every_format() {
        let mut b = col("payload");
        b.type_name = "BYTEA".to_string();
        let rs = ResultSet::from_rows(vec![b], vec![vec![Value::Str("\\x4869".to_string())]]);
        assert!(export_csv(&rs, &[0]).contains("4869"));
        assert!(export_json(&rs, &[0]).contains("4869"));
        assert!(export_column_csv(&rs, &[0], 0).contains("4869"));
    }

    // ── export formats + save-file naming ─────────────────────────────────

    #[test]
    fn every_format_has_a_distinct_label_and_extension() {
        let labels: Vec<&str> = ExportFormat::ALL.iter().map(|f| f.label()).collect();
        let exts: Vec<&str> = ExportFormat::ALL.iter().map(|f| f.extension()).collect();
        for v in [&labels, &exts] {
            let mut s = v.clone();
            s.sort_unstable();
            s.dedup();
            assert_eq!(s.len(), v.len(), "duplicates in {v:?}");
        }
        // No leading dot — Floem's FileSpec adds it.
        assert!(exts.iter().all(|e| !e.starts_with('.')));
    }

    /// The predicate the Copy menu filters on. Stated as a whole-enum fact
    /// rather than `assert!(!Xlsx.is_text())`, so a seventh binary format that
    /// forgot to answer it is caught here rather than by a user whose clipboard
    /// silently emptied.
    #[test]
    fn excel_is_the_one_format_the_clipboard_cannot_take() {
        let binary: Vec<_> = ExportFormat::ALL
            .into_iter()
            .filter(|f| !f.is_text())
            .collect();
        assert_eq!(binary, [ExportFormat::Xlsx]);
        // And the reason: a `String` rendering of it is empty, which is exactly
        // what would reach the clipboard if the menu didn't filter.
        assert_eq!(ExportFormat::Xlsx.render(&rs(), &[0, 1], None, MySql), "");
    }

    /// **A failed xlsx export must not point at a file that holds nothing.**
    /// Nothing reaches the sink until `save_to_writer`, so every pre-save
    /// failure left a zero-byte `.part` sibling that the message named as
    /// holding "the rows that were written".
    ///
    /// Both halves are asserted, and both matter: the capability, and that the
    /// sink really is untouched when the writer returns `Err` — the second is
    /// what makes the first true rather than a claim about it.
    #[test]
    fn a_buffered_format_leaves_nothing_behind_when_it_fails() {
        let buffered: Vec<_> = ExportFormat::ALL
            .into_iter()
            .filter(|f| !f.writes_incrementally())
            .collect();
        assert_eq!(buffered, [ExportFormat::Xlsx]);

        // Driven at the row ceiling, which is the failure a user actually hits.
        const CHUNK: usize = 4096;
        let cols = vec![col("id")];
        let block: Vec<Vec<Value>> = (0..CHUNK).map(|i| vec![Value::Int(i as i64)]).collect();
        let full = ResultSet::from_rows(cols, block);
        let mut sent = 0usize;
        let target = XLSX_MAX_ROWS as usize;
        let mut src = PullChunks::new(move || {
            sent += CHUNK;
            Ok(if sent <= target + CHUNK {
                Some(full.clone())
            } else {
                None
            })
        });
        let mut buf = Vec::new();
        ExportFormat::Xlsx
            .stream_to(&mut buf, &mut src, None, MySql)
            .expect_err("past the ceiling");
        assert!(buf.is_empty(), "{} bytes reached the sink", buf.len());

        // …and a text format's sink is not empty at the same point, which is
        // why the sentence was written unconditionally in the first place.
        let rs = rs();
        let mut src = PullChunks::new({
            let rs = rs.clone();
            let mut once = false;
            move || {
                if once {
                    return Err(std::io::Error::other("the connection dropped"));
                }
                once = true;
                Ok(Some(rs.clone()))
            }
        });
        let mut buf = Vec::new();
        ExportFormat::Csv
            .stream_to(&mut buf, &mut src, None, MySql)
            .expect_err("the read failed");
        assert!(!buf.is_empty(), "an incremental format wrote nothing");
    }

    /// The two notes agree with the capability, and neither names a file that
    /// holds nothing.
    #[test]
    fn neither_note_points_at_an_empty_sibling() {
        for f in ExportFormat::ALL {
            let partial = f.writes_incrementally();
            let fail = export_failure_note("Export failed: x", partial.then_some("orders.xlsx"));
            let cancel = export_cancel_note("orders.xlsx", partial);
            assert_eq!(fail.contains(".part"), partial, "{f:?}: {fail}");
            assert_eq!(cancel.contains(".part"), partial, "{f:?}: {cancel}");
            // Both still say the destination survived, which is the reassurance
            // half and is true either way.
            assert!(cancel.contains("was not changed"), "{f:?}: {cancel}");
        }
    }

    /// **The list the menu is built from, not just the predicate under it.**
    /// The filter used to live inline in `grid_toolbar`, in a file with no test
    /// module — so deleting it left the suite green and shipped an "Excel" entry
    /// that sets the clipboard to `""`. Every format the list offers renders to
    /// something, and the one it withholds is the one that cannot.
    #[test]
    fn the_clipboard_list_is_every_format_that_renders_to_text() {
        let offered: Vec<_> = ExportFormat::clipboard_formats().collect();
        assert!(!offered.contains(&ExportFormat::Xlsx));
        // In menu order, and nothing else dropped.
        assert_eq!(
            offered,
            ExportFormat::ALL
                .into_iter()
                .filter(|f| *f != ExportFormat::Xlsx)
                .collect::<Vec<_>>()
        );
        // Driven, not asserted about: every entry the menu would build puts
        // something on the clipboard.
        for f in ExportFormat::clipboard_formats() {
            assert!(
                !f.render(&rs(), &[0, 1], None, MySql).is_empty(),
                "{f:?} would empty the clipboard"
            );
        }
    }

    /// **The point of the format.** A CSV opened in Excel is text the
    /// application then guesses at; a workbook states each cell's type, so
    /// there is nothing to guess.
    #[test]
    fn an_xlsx_export_writes_each_cell_with_the_type_the_server_gave_it() {
        let cols = vec![col("n"), col("f"), col("s"), col("nul")];
        let rs = ResultSet::from_rows(
            cols,
            vec![vec![
                Value::Int(-42),
                Value::Float(1.5),
                // A value Excel would read as a date if it arrived as text —
                // the concrete failure typed cells exist to prevent.
                Value::Str("SET-1".to_string()),
                Value::Null,
            ]],
        );
        let bytes = to_xlsx(&rs, &[0], None);
        let (header, rows) = read_back(&bytes);
        assert_eq!(header, ["n", "f", "s", "nul"]);
        assert_eq!(rows.len(), 1);
        // Numbers came back as numbers…
        assert_eq!(rows[0][0].as_deref(), Some("-42"));
        assert_eq!(rows[0][1].as_deref(), Some("1.5"));
        // …the text stayed text, unconverted…
        assert_eq!(rows[0][2].as_deref(), Some("SET-1"));
        // …and NULL is an *empty cell* rather than the four characters `NULL`,
        // which is the failure mode a text export has to work to avoid.
        assert_eq!(rows[0][3], None);
    }

    /// A worksheet number is an `f64`, so past 2^53 it is no longer the number
    /// it was given. A `BIGINT` key that comes back off by one is a silent
    /// corruption in the format people paste into other systems.
    #[test]
    fn an_integer_too_large_for_a_worksheet_number_goes_out_as_text() {
        let big = (1i64 << 53) + 1;
        let rs = ResultSet::from_rows(
            vec![col("fits"), col("does_not"), col("unsigned")],
            vec![vec![
                Value::Int(1 << 53),
                Value::Int(big),
                Value::UInt(u64::MAX),
            ]],
        );
        let (_, rows) = read_back(&to_xlsx(&rs, &[0], None));
        // Exactly on the boundary it is still exact, so it stays a number.
        assert_eq!(rows[0][0].as_deref(), Some("9007199254740992"));
        // One past it: text, carrying every digit.
        assert_eq!(rows[0][1].as_deref(), Some(&big.to_string()[..]));
        assert_eq!(rows[0][2].as_deref(), Some("18446744073709551615"));
    }

    /// NaN and the infinities have no worksheet representation at all; their
    /// text does.
    #[test]
    fn a_non_finite_float_goes_out_as_text_rather_than_a_broken_cell() {
        let rs = ResultSet::from_rows(
            vec![col("a"), col("b")],
            vec![vec![
                Value::Float(f64::NAN),
                Value::Float(f64::NEG_INFINITY),
            ]],
        );
        let (_, rows) = read_back(&to_xlsx(&rs, &[0], None));
        assert_eq!(rows[0][0].as_deref(), Some("NaN"));
        assert_eq!(rows[0][1].as_deref(), Some("-inf"));
    }

    /// Excel caps a cell at 32,767 characters and a `TEXT` or `JSON` column
    /// routinely holds more. The cut itself is unavoidable; **saying nothing
    /// about it is not** — a truncated cell, unlike a blanked one, looks
    /// complete.
    #[test]
    fn a_cell_too_long_for_excel_is_cut_and_its_column_named() {
        let long = "x".repeat(XLSX_MAX_CELL_CHARS + 500);
        let rs = ResultSet::from_rows(
            vec![col("short"), col("payload")],
            vec![vec![Value::Str("ok".into()), Value::Str(long)]],
        );
        let mut buf = Vec::new();
        let tally = ExportFormat::Xlsx
            .render_to(&mut buf, &rs, &[0], None, MySql)
            .expect("writing to a Vec cannot fail");
        let (_, rows) = read_back(&buf);
        assert_eq!(
            rows[0][1].as_deref().map(str::len),
            Some(XLSX_MAX_CELL_CHARS)
        );
        assert_eq!(tally.cut, vec!["payload".to_string()]);
        // The column that fitted is not named — a caveat that named every
        // column would say nothing about which one to distrust.
        assert!(!tally.cut.contains(&"short".to_string()));
        assert!(tally.has_caveat());
        assert!(
            export_note(&tally, "t.xlsx", false)
                .is_some_and(|m| m.contains("payload cut to Excel's")),
            "the caveat has to reach the bar"
        );
    }

    /// Cutting a `&str` by bytes panics mid-codepoint, and Excel counts
    /// characters anyway — so a wide-character column is where a byte-based cut
    /// would both crash and cut in the wrong place.
    #[test]
    fn a_long_multibyte_cell_is_cut_on_a_character_boundary() {
        // 3 bytes per char, so this is well past the byte ceiling but only just
        // past the character one.
        let long = "日".repeat(XLSX_MAX_CELL_CHARS + 10);
        let rs = ResultSet::from_rows(vec![col("payload")], vec![vec![Value::Str(long.clone())]]);
        let (_, rows) = read_back(&to_xlsx(&rs, &[0], None));
        let cell = rows[0][0].clone().expect("a value");
        assert_eq!(cell.chars().count(), XLSX_MAX_CELL_CHARS);
        assert!(long.starts_with(&cell));

        // And a string that is long in *bytes* but short in characters is not
        // touched at all — the fast path must not cut what fits.
        let fits = "é".repeat(XLSX_MAX_CELL_CHARS / 2 + 10);
        assert!(fits.len() > XLSX_MAX_CELL_CHARS);
        assert!(fits.chars().count() < XLSX_MAX_CELL_CHARS);
        assert_eq!(fit_cell(&fits), fits);
    }

    /// **Excel counts UTF-16 units, not Rust `char`s.** Its documented ceiling —
    /// 32,767 characters in a cell — is counted in the units it stores strings
    /// as, so an astral char costs two. Cutting by `char` sent an emoji-heavy
    /// cell out at 65,534 units, twice the limit, which Excel repairs rather
    /// than opens. The test above pins only the BMP case (`日` is 3 bytes and
    /// one unit), where the two counts coincide.
    #[test]
    fn a_cell_of_astral_characters_is_cut_by_what_excel_counts() {
        let emoji = "😀".repeat(40_000);
        let cut = fit_cell(&emoji);
        assert!(
            cut.encode_utf16().count() <= XLSX_MAX_CELL_CHARS,
            "{} units",
            cut.encode_utf16().count()
        );
        // Cut on a character boundary, and a prefix of what came in.
        assert!(emoji.starts_with(cut));
        // Not over-cut either: one more character would cross the ceiling.
        assert_eq!(cut.chars().count(), XLSX_MAX_CELL_CHARS / 2);
        // A BMP string is unaffected — one char, one unit — so the stricter
        // count must not shorten what already fitted.
        let bmp = "日".repeat(XLSX_MAX_CELL_CHARS);
        assert_eq!(fit_cell(&bmp), bmp);
    }

    /// The chunk seam, asked of Excel the only way it can be: by reading both
    /// workbooks back. `a_chunked_export_matches_the_same_rows_in_one_go` is
    /// this test for the text formats, and it is the same hazard — a header
    /// written once per chunk, or a row index that restarts at a boundary.
    #[test]
    fn a_chunked_xlsx_export_matches_the_same_rows_in_one_go() {
        let (cols, rows) = awkward_rows();
        let whole = ResultSet::from_rows(cols.clone(), rows.clone());
        let order: Vec<usize> = (0..rows.len()).collect();
        let src_name = Some(("shop", None, "orders"));
        let mut one_go_buf = Vec::new();
        let whole_tally = ExportFormat::Xlsx
            .render_to(&mut one_go_buf, &whole, &order, src_name, MySql)
            .expect("writing to a Vec cannot fail");
        let one_go = read_back(&one_go_buf);
        assert_eq!(one_go.1.len(), rows.len());
        for size in 1..=rows.len() {
            let mut src = chunked(&cols, &rows, size);
            let mut buf = Vec::new();
            let tally = ExportFormat::Xlsx
                .stream_to(&mut buf, &mut src, src_name, MySql)
                .expect("writing to a Vec cannot fail");
            // The whole tally, not just its row count: a seam that lost a
            // truncation note or a dropped column is the same class of bug and
            // `rows` alone cannot see it.
            assert_eq!(tally, whole_tally, "chunks of {size}");
            assert_eq!(read_back(&buf), one_go, "chunks of {size}");
        }
    }

    /// **A row that writes no cell writes no row.** `rust_xlsxwriter` emits a
    /// `<row>` only for a row that received a cell, and the sheet's dimension is
    /// computed from the cells actually written — so an all-NULL row at the
    /// *end* of a result vanished from the file while `tally.rows` still counted
    /// it, and `SELECT max(total) … WHERE 1=0` exported a header, no data row,
    /// and a bar reading "Exported 1 row". An interior one survived only because
    /// a later row stretched the range.
    ///
    /// Every way a row can write nothing is asked: all NULL, all empty string
    /// (which this writer cannot distinguish from NULL — see the loop's own
    /// note), and a mixture.
    ///
    /// **Asserted on the sheet's declared dimension, not on `read_back`.** That
    /// is what a spreadsheet application reads to decide how far the sheet goes,
    /// and it is the only reader that can see the difference: calamine's
    /// `worksheet_range` drops every `Empty` cell before computing its own
    /// range, so a trailing blank row is invisible to it whether it is in the
    /// file or not — which is precisely why the review's own fix sketch, an
    /// assertion on `read_back(...).1.len()`, would have passed a broken fix.
    /// (`Format::default()` in that sketch writes nothing either: this writer,
    /// like Excel, drops an *unformatted* blank cell.)
    #[test]
    fn a_row_that_writes_no_cell_still_occupies_a_worksheet_row() {
        let cols = vec![col("a"), col("b")];
        for rows in [
            vec![
                vec![Value::Str("x".into()), Value::Int(1)],
                vec![Value::Null, Value::Null],
            ],
            vec![
                vec![Value::Str("x".into()), Value::Int(1)],
                vec![Value::Null, Value::Null],
                vec![Value::Null, Value::Null],
            ],
            vec![
                vec![Value::Null, Value::Null],
                vec![Value::Null, Value::Null],
            ],
            vec![
                vec![Value::Str("x".into()), Value::Int(1)],
                vec![Value::Str(String::new()), Value::Str(String::new())],
            ],
            vec![
                vec![Value::Str("x".into()), Value::Int(1)],
                vec![Value::Null, Value::Str(String::new())],
            ],
        ] {
            let rs = ResultSet::from_rows(cols.clone(), rows.clone());
            let order: Vec<usize> = (0..rows.len()).collect();
            let mut buf = Vec::new();
            let tally = ExportFormat::Xlsx
                .render_to(&mut buf, &rs, &order, None, MySql)
                .expect("writing to a Vec cannot fail");
            assert_eq!(tally.rows as usize, rows.len(), "{rows:?}");
            // Row 0 is the header, so the last data row is at `rows.len()`.
            assert_eq!(
                sheet_dimension(&buf).end.0 as usize,
                rows.len(),
                "the sheet must reach every row the tally counts: {rows:?}"
            );
            // And the forced cell holds nothing: every value that did get
            // written still reads back exactly as before.
            let (_, back) = read_back(&buf);
            for (i, row) in back.iter().enumerate() {
                for (ci, cell) in row.iter().enumerate() {
                    if matches!(rows[i][ci], Value::Null)
                        || rows[i][ci] == Value::Str(String::new())
                    {
                        assert_eq!(cell.as_deref(), None, "{rows:?}");
                    }
                }
            }
        }
    }

    /// The row/column extent the sheet **declares** — `<dimension ref="A1:B3"/>`
    /// — which is what a spreadsheet application reads and what `read_back`
    /// cannot show, since calamine recomputes its own range from non-empty
    /// cells only.
    fn sheet_dimension(bytes: &[u8]) -> calamine::Dimensions {
        use calamine::{Reader, Xlsx};
        let mut wb: Xlsx<_> =
            Xlsx::new(std::io::Cursor::new(bytes.to_vec())).expect("a workbook we just wrote");
        let name = wb.sheet_names()[0].clone();
        wb.worksheet_cells_reader(&name)
            .expect("the first sheet")
            .dimensions()
    }

    /// **A refusal, not a truncation.** Stopping at row 1,048,576 of a bigger
    /// table would write a file that looks like the whole export and is not —
    /// the one loss this module will not report, because the file itself would
    /// carry no trace of it. The `.part` dance means the error leaves the
    /// destination untouched.
    ///
    /// Driven through the real writer at the real ceiling rather than by
    /// checking the predicate: the off-by-one that matters is whether the
    /// *header* row counts against it, and only the writer knows that.
    #[test]
    fn a_result_taller_than_a_worksheet_is_refused_rather_than_cut() {
        const CHUNK: usize = 4096;
        let cols = vec![col("id")];
        // One chunk's worth of rows, re-yielded — the source never has to hold
        // a million of anything.
        let block: Vec<Vec<Value>> = (0..CHUNK).map(|i| vec![Value::Int(i as i64)]).collect();
        let full = ResultSet::from_rows(cols.clone(), block);
        let mut sent = 0usize;
        // Exactly enough chunks to reach the ceiling, plus one row past it.
        let target = XLSX_MAX_ROWS as usize;
        let mut src = PullChunks::new(move || {
            sent += CHUNK;
            Ok(if sent <= target + CHUNK {
                Some(full.clone())
            } else {
                None
            })
        });
        let mut buf = Vec::new();
        let err = ExportFormat::Xlsx
            .stream_to(&mut buf, &mut src, None, MySql)
            .expect_err("a result this tall does not fit a worksheet");
        assert!(
            err.to_string().contains("Excel worksheet can hold"),
            "{err}"
        );
        // The advice is the actionable half: CSV and JSON have no such ceiling.
        assert!(err.to_string().contains("CSV"), "{err}");
    }

    /// The width ceiling, driven the same way its taller sibling is — and for
    /// the same reason. It had **no test at all**: deleting the whole refusal
    /// left the suite green, and the `ci as u16` casts whose only bound is that
    /// check then wrap silently at 65,536 columns.
    ///
    /// The last case is the second half of the same guard: the check used to run
    /// on the *first* chunk only, so a later, wider one reached the writer and
    /// failed with a message naming neither the column count nor the way out.
    #[test]
    fn a_result_wider_than_a_worksheet_is_refused_rather_than_cut() {
        let wide = |n: usize| {
            let cols: Vec<Column> = (0..n).map(|i| col(&format!("c{i}"))).collect();
            let row = vec![Value::Int(1); n];
            ResultSet::from_rows(cols, vec![row])
        };
        // Exactly on the ceiling it exports, header and all.
        let ok = wide(XLSX_MAX_COLS as usize);
        let (header, _) = read_back(&to_xlsx(&ok, &[0], None));
        assert_eq!(header.len(), XLSX_MAX_COLS as usize);

        // One past it is a refusal that names the count and the way out.
        let too_wide = wide(XLSX_MAX_COLS as usize + 1);
        let mut buf = Vec::new();
        let err = ExportFormat::Xlsx
            .render_to(&mut buf, &too_wide, &[0], None, MySql)
            .expect_err("a result this wide does not fit a worksheet");
        assert!(err.to_string().contains("columns"), "{err}");
        assert!(err.to_string().contains("CSV"), "{err}");

        // …including when the width arrives on a later chunk.
        let narrow = ResultSet::from_rows(vec![col("id")], vec![vec![Value::Int(1)]]);
        let mut sent = 0u8;
        let mut src = PullChunks::new(move || {
            sent += 1;
            Ok(match sent {
                1 => Some(narrow.clone()),
                2 => Some(too_wide.clone()),
                _ => None,
            })
        });
        let mut buf = Vec::new();
        let err = ExportFormat::Xlsx
            .stream_to(&mut buf, &mut src, None, MySql)
            .expect_err("the width check must not be a first-chunk-only step");
        assert!(err.to_string().contains("columns"), "{err}");
    }

    /// **The format's headline promise, on the commonest numeric column.**
    /// `DECIMAL`/`NUMERIC` arrives over the text protocol precisely so it is
    /// exact, and `CellTag::Str` sent all of it to a text cell — so `=SUM(B:B)`
    /// was `0`, the column sorted lexicographically (`"100.00" < "9.00"`), and
    /// Excel flagged every cell as a number stored as text. Exactly the CSV
    /// behaviour the format exists to replace.
    ///
    /// The two values a worksheet cannot hold are pinned in the same test, so
    /// the guard cannot be widened by accident.
    #[test]
    fn an_exact_decimal_goes_out_as_a_number_and_an_inexact_one_stays_text() {
        let dec = |name: &str| Column {
            name: name.to_string(),
            type_name: "DECIMAL(30,10)".to_string(),
            origin: None,
        };
        for (text, want_number) in [
            ("1234.50", true),
            ("100.00", true),
            ("-0.10", true),
            ("9.00", true),
            ("0", true),
            // Past what an `f64` holds: 19 significant digits, and a magnitude
            // beyond 2^53. Both must stay text, digit for digit.
            ("0.1234567890123456789", false),
            ("12345678901234567890.5", false),
            // Not a number at all — a DECIMAL column can still carry this on the
            // way through, and it must not be guessed at.
            ("NaN", false),
        ] {
            let rs =
                ResultSet::from_rows(vec![dec("total")], vec![vec![Value::Str(text.to_string())]]);
            let (_, rows) = read_back(&to_xlsx(&rs, &[0], None));
            let got = rows[0][0].as_deref();
            if want_number {
                let n: f64 = got.unwrap_or("").parse().unwrap_or(f64::NAN);
                assert_eq!(n, text.parse::<f64>().unwrap(), "{text} -> {got:?}");
            } else {
                assert_eq!(got, Some(text), "{text} must stay text");
            }
        }
        // An empty cell in a decimal column is still an empty cell — the
        // conversion must not turn absence into a zero.
        let rs = ResultSet::from_rows(
            vec![dec("total"), dec("t2")],
            vec![vec![Value::Str(String::new()), Value::Str("1.5".into())]],
        );
        let (_, rows) = read_back(&to_xlsx(&rs, &[0], None));
        assert_eq!(rows[0][0].as_deref(), None);
        assert_eq!(rows[0][1].as_deref(), Some("1.5"));

        // **The type is what licenses the conversion, not the shape of the
        // text.** A `VARCHAR` holding a phone number or a zip code is not a
        // number and must not become one — that guess is the CSV behaviour.
        let rs = ResultSet::from_rows(
            vec![col("code")],
            vec![vec![Value::Str("1234567890".to_string())]],
        );
        let (_, rows) = read_back(&to_xlsx(&rs, &[0], None));
        assert_eq!(rows[0][0].as_deref(), Some("1234567890"));
    }

    /// A display order can name a row a later splice removed. Every other
    /// format skips it; a worksheet must too, and must not leave a *gap* where
    /// it was — the row index would then run past the data.
    #[test]
    fn a_stale_order_index_is_skipped_by_the_xlsx_export() {
        let rs = ResultSet::from_rows(
            vec![col("id"), col("t")],
            vec![
                vec![Value::Int(1), Value::Str("one".into())],
                vec![Value::Int(2), Value::Str("two".into())],
            ],
        );
        let mut buf = Vec::new();
        let tally = ExportFormat::Xlsx
            .render_to(&mut buf, &rs, &[0, 5, 1], None, MySql)
            .expect("writing to a Vec cannot fail");
        assert_eq!(tally.rows, 2);
        let (_, rows) = read_back(&buf);
        // Two rows, adjacent — not three, and no blank one between them.
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][1].as_deref(), Some("one"));
        assert_eq!(rows[1][1].as_deref(), Some("two"));
    }

    /// The empty-result case CSV, Markdown and HTML each answer with a bare
    /// header. A workbook's answer is the same, and a source that ended before
    /// its first chunk still has to produce a *valid* file.
    #[test]
    fn an_empty_xlsx_export_still_carries_its_header() {
        let (cols, _) = awkward_rows();
        let empty = ResultSet::from_rows(cols.clone(), vec![]);
        let (header, rows) = read_back(&to_xlsx(&empty, &[], None));
        assert_eq!(header, ["id", "a`b"]);
        assert!(rows.is_empty());

        // No chunk at all — nothing is known, so nothing is claimed, but the
        // workbook still opens.
        let mut src = chunked(&cols, &[], usize::MAX);
        let mut buf = Vec::new();
        let tally = ExportFormat::Xlsx
            .stream_to(&mut buf, &mut src, None, MySql)
            .expect("writing to a Vec cannot fail");
        assert_eq!(tally.rows, 0);
        let (header, rows) = read_back(&buf);
        assert!(header.is_empty(), "no chunk means no columns to name");
        assert!(rows.is_empty());
    }

    /// Excel is a format Schemaic reads back, so a blob's `<n bytes>`
    /// placeholder must not go out in it as data — the same rule CSV and JSON
    /// follow, and the same reason.
    #[test]
    fn a_withheld_blob_leaves_an_empty_cell_and_names_its_column() {
        let rs = blob_rs();
        let mut buf = Vec::new();
        let tally = ExportFormat::Xlsx
            .render_to(&mut buf, &rs, &[0], None, MySql)
            .expect("writing to a Vec cannot fail");
        assert_eq!(tally.withheld, vec!["thumb".to_string()]);
        let (header, rows) = read_back(&buf);
        let ci = header
            .iter()
            .position(|h| h == "thumb")
            .expect("the column");
        // The column keeps its place — it is the *bytes* that could not come,
        // not the column — and the cell is empty rather than the placeholder.
        assert_eq!(rows[0][ci], None);
    }

    /// The worksheet is named after the table, because a workbook of three
    /// exports whose tabs all say `Sheet1` is a workbook nobody can read. Excel
    /// rejects a name that breaks its rules rather than repairing it, and a
    /// table name is server-controlled, so every one of them is reachable.
    #[test]
    fn the_worksheet_is_named_after_the_source_table_and_scrubbed() {
        assert_eq!(sheet_name(Some(("shop", None, "orders"))), "orders");
        // No source — a query result rather than a table.
        assert_eq!(sheet_name(None), "Result");
        // …and a name that scrubs away to nothing falls back too, rather than
        // producing the blank name Excel refuses.
        assert_eq!(sheet_name(Some(("db", None, "///"))), "___");
        assert_eq!(sheet_name(Some(("db", None, "  "))), "Result");
        assert_eq!(sheet_name(Some(("db", None, "'quoted'"))), "quoted");
        // Excel's fifth rule, and the only one `rust_xlsxwriter` does not
        // enforce for us: `History` is reserved for a shared workbook's change
        // log, case-insensitively, and a workbook using it as an ordinary sheet
        // name is repaired rather than opened. A table can be called that.
        for reserved in ["history", "History", "HISTORY", " history "] {
            assert_eq!(
                sheet_name(Some(("db", None, reserved))),
                "Result",
                "{reserved}"
            );
        }
        // Every character Excel forbids, replaced rather than dropped, so two
        // different tables cannot collapse to one name.
        assert_eq!(
            sheet_name(Some(("db", None, "a[b]c:d*e?f/g\\h"))),
            "a_b_c_d_e_f_g_h"
        );
        // 31 characters is the ceiling.
        let long = "n".repeat(40);
        assert_eq!(sheet_name(Some(("db", None, &long))).chars().count(), 31);

        // And the name actually reaches the file — the scrubbing is worthless
        // if the writer never asks for it.
        assert_eq!(
            sheet_of(&to_xlsx(&rs(), &[0], Some(("shop", None, "cust")))),
            "cust"
        );
        assert_eq!(sheet_of(&to_xlsx(&rs(), &[0], None)), "Result");
    }

    #[test]
    fn format_render_matches_the_direct_call() {
        // The enum is the single dispatch point for both menus, so it must agree
        // with the functions it fronts.
        let (rs, order) = (rs(), [0, 1][..].to_vec());
        let src = Some(("shop", None, "cust"));
        for f in text_formats() {
            let via_enum = f.render(&rs, &order, src, MySql);
            let direct = match f {
                ExportFormat::Json => export_json(&rs, &order),
                ExportFormat::Csv => export_csv(&rs, &order),
                ExportFormat::Sql => export_inserts(&rs, &order, src, MySql),
                ExportFormat::Markdown => export_markdown(&rs, &order),
                ExportFormat::Html => export_html(&rs, &order),
                ExportFormat::Xlsx => unreachable!("filtered out by `text_formats`"),
            };
            assert_eq!(via_enum, direct, "{}", f.label());
        }
        // Only SQL is dialect- and source-sensitive.
        assert_ne!(
            ExportFormat::Sql.render(&rs, &order, src, MySql),
            ExportFormat::Sql.render(&rs, &order, src, Postgres)
        );
        assert_eq!(
            ExportFormat::Csv.render(&rs, &order, src, MySql),
            ExportFormat::Csv.render(&rs, &order, None, Postgres)
        );
    }

    #[test]
    fn suggested_filename_uses_the_source_table() {
        assert_eq!(
            suggested_filename(Some("orders"), ExportFormat::Csv),
            "orders.csv"
        );
        // A schema-qualified name keeps its dot — it's a legal file-name char.
        assert_eq!(
            suggested_filename(Some("sales.orders"), ExportFormat::Json),
            "sales.orders.json"
        );
        // No source (an arbitrary SELECT) → a neutral default.
        assert_eq!(suggested_filename(None, ExportFormat::Sql), "result.sql");
        assert_eq!(
            suggested_filename(Some(""), ExportFormat::Markdown),
            "result.md"
        );
    }

    #[test]
    fn suggested_filename_sanitizes_a_hostile_table_name() {
        // A table name comes from the server, so it can hold anything. None of it
        // may become a path separator or an illegal component.
        let out = suggested_filename(Some("a/b\\c:d*e?f\"g<h>i|j"), ExportFormat::Csv);
        assert_eq!(out, "a_b_c_d_e_f_g_h_i_j.csv");
        assert!(!out.contains(['/', '\\']), "{out}");
        // Control characters too.
        assert_eq!(
            suggested_filename(Some("a\nb\tc"), ExportFormat::Csv),
            "a_b_c.csv"
        );
        // A name that sanitizes to nothing falls back rather than yielding ".csv".
        assert_eq!(
            suggested_filename(Some("..."), ExportFormat::Csv),
            "result.csv"
        );
        assert_eq!(
            suggested_filename(Some("   "), ExportFormat::Csv),
            "result.csv"
        );
        // Windows rejects a trailing dot/space.
        assert_eq!(
            suggested_filename(Some("orders. "), ExportFormat::Csv),
            "orders.csv"
        );
        // Reserved device names are escaped, case-insensitively.
        assert_eq!(
            suggested_filename(Some("CON"), ExportFormat::Csv),
            "_CON.csv"
        );
        assert_eq!(
            suggested_filename(Some("nul"), ExportFormat::Csv),
            "_nul.csv"
        );
        // A very long name is capped (component limits are ~255 bytes).
        let long = "x".repeat(400);
        let out = suggested_filename(Some(&long), ExportFormat::Csv);
        assert!(out.len() < 120, "{} chars", out.len());
    }

    /// A fetched export renders in blocks now, so there is something to report
    /// while it runs — and the block boundary must not change one byte of the
    /// output. That is the whole risk of the change: five renderers write a
    /// header on the first chunk and rows on every one, so a source that
    /// suddenly yields three chunks where it yielded one is exactly how a CSV
    /// grows two extra header lines.
    #[test]
    fn chunking_a_fetched_result_cannot_change_the_bytes() {
        let (cols, rows) = awkward_rows();
        let rs = ResultSet::from_rows(cols, rows);
        let order: Vec<usize> = (0..rs.row_count()).collect();
        for f in ExportFormat::ALL {
            if !f.is_text() {
                continue; // compared through its own reader below
            }
            let whole = to_string(|w| f.render_to(w, &rs, &order, None, MySql).map(|_| ()));
            // Every size that puts a boundary somewhere interesting: before the
            // first row, between rows, and past the end.
            for size in 1..=order.len() + 1 {
                let mut src = SliceChunks::new(&rs, &order, size);
                let chunked = to_string(|w| f.stream_to(w, &mut src, None, MySql).map(|_| ()));
                assert_eq!(
                    whole,
                    chunked,
                    "{} differs when chunked {size} at a time",
                    f.label()
                );
            }
        }
    }

    #[test]
    fn slice_chunks_yields_every_row_once_in_order() {
        let rs = rs();
        let order = vec![1usize, 0];
        let mut src = SliceChunks::new(&rs, &order, 1);
        let mut seen = Vec::new();
        while let Some(c) = src.next_chunk().unwrap() {
            seen.extend_from_slice(c.order);
        }
        assert_eq!(seen, order, "the display order is what is written");
    }

    #[test]
    fn slice_chunks_of_an_empty_result_still_yields_one_chunk() {
        // **One empty chunk, then done** — matching [`OneChunk`], which hands out
        // a chunk whether or not the order is empty.
        //
        // The opposite was written here first, with the reasoning exactly
        // backwards: five renderers write their header *on the first chunk they
        // see*, so a source that yields none for an empty result writes no header
        // at all. That shipped a 0-byte CSV where the whole-render path wrote
        // `id,name\n`, and this test asserted it was correct.
        let rs = rs();
        let order: Vec<usize> = Vec::new();
        let mut src = SliceChunks::new(&rs, &order, 4);
        let first = src
            .next_chunk()
            .unwrap()
            .expect("a chunk, so the header lands");
        assert!(first.order.is_empty(), "the chunk carries no rows");
        assert!(
            src.next_chunk().unwrap().is_none(),
            "and exactly one, so no renderer writes a second header"
        );
    }

    /// The empty-result half of `chunking_a_fetched_result_cannot_change_the_bytes`.
    ///
    /// Its own test rather than another `size` in that loop, because the fixture
    /// has to differ: that one's rows are what make a chunk boundary interesting,
    /// and **an empty order has no boundary to place** — which is precisely why
    /// the parity loop could not catch this. A result with no rows is an ordinary
    /// thing to export (a `SELECT` that matched nothing, a filter narrowed to
    /// zero), and the file it writes must still be a file.
    #[test]
    fn an_empty_result_writes_the_same_bytes_through_either_path() {
        let rs = rs();
        let order: Vec<usize> = Vec::new();
        for f in ExportFormat::ALL {
            if !f.is_text() {
                continue; // binary; its own round-trip tests cover it
            }
            let whole = to_string(|w| f.render_to(w, &rs, &order, None, MySql).map(|_| ()));
            let mut src = SliceChunks::new(&rs, &order, 4);
            let chunked = to_string(|w| f.stream_to(w, &mut src, None, MySql).map(|_| ()));
            assert_eq!(whole, chunked, "{} differs on an empty result", f.label());
        }
        // And it is not vacuously equal because both are empty: CSV still has to
        // carry its header row.
        let mut src = SliceChunks::new(&rs, &order, 4);
        let csv = to_string(|w| {
            ExportFormat::Csv
                .stream_to(w, &mut src, None, MySql)
                .map(|_| ())
        });
        assert!(!csv.is_empty(), "an empty result is not a 0-byte file");
        assert!(csv.contains("id"), "the header names the columns: {csv:?}");
    }

    /// **Xlsx too, and it is the one production actually renders this way.** The
    /// parity test above skips every non-text format and defers to their
    /// round-trip tests — which never use `SliceChunks` and never an empty
    /// order, while a `Fetched` export renders *every* format through it. A
    /// workbook is a ZIP, so this compares bytes rather than text, and an empty
    /// one is emphatically not a 0-byte file: it is a whole workbook with a
    /// header row and no data rows.
    #[test]
    fn an_empty_result_writes_the_same_workbook_through_either_path() {
        let bytes = |f: &mut dyn FnMut(&mut Vec<u8>) -> io::Result<()>| -> Vec<u8> {
            let mut buf = Vec::new();
            f(&mut buf).expect("the writer is a Vec");
            buf
        };
        let rs = rs();
        let order: Vec<usize> = Vec::new();
        let whole = bytes(&mut |w| {
            ExportFormat::Xlsx
                .render_to(w, &rs, &order, None, MySql)
                .map(|_| ())
        });
        let chunked = bytes(&mut |w| {
            let mut src = SliceChunks::new(&rs, &order, 4);
            ExportFormat::Xlsx
                .stream_to(w, &mut src, None, MySql)
                .map(|_| ())
        });
        assert_eq!(whole, chunked, "the two paths must write one workbook");
        assert!(
            whole.len() > 1000,
            "an empty result is still a whole workbook, not a stub: {} bytes",
            whole.len()
        );
        // A ZIP, so the reader that opens it can at least find the signature.
        assert_eq!(&whole[..2], b"PK", "not a workbook at all");
    }

    #[test]
    fn a_watched_slice_reports_the_running_total() {
        let (cols, rows) = awkward_rows();
        let rs = ResultSet::from_rows(cols, rows);
        let order: Vec<usize> = (0..rs.row_count()).collect();
        let mut seen: Vec<u64> = Vec::new();
        {
            let mut src = SliceChunks::new(&rs, &order, 2).watching(|n| {
                seen.push(n);
                true
            });
            while src.next_chunk().unwrap().is_some() {}
        }
        // Cumulative, not per-chunk — the modal shows "40k of ~180k", so what it
        // is handed has to be the total so far.
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
        assert_eq!(
            seen.last().copied(),
            Some(order.len() as u64),
            "the last report is every row: {seen:?}"
        );
    }

    #[test]
    fn a_watcher_that_says_stop_fails_the_export_rather_than_ending_it() {
        // **An error, not `Ok(None)`.** End-of-stream is what a finished export
        // looks like, so a stop reported that way would have the writer rename a
        // truncated file over the destination and call it done — the exact
        // failure `PullChunks`' cancelled-read arm exists to prevent.
        let (cols, rows) = awkward_rows();
        let rs = ResultSet::from_rows(cols, rows);
        let order: Vec<usize> = (0..rs.row_count()).collect();
        let mut src = SliceChunks::new(&rs, &order, 1).watching(|_| false);
        let err = match src.next_chunk() {
            Err(e) => e,
            Ok(_) => panic!("a stop must be an error, not an end of stream"),
        };
        assert!(err.to_string().contains("cancelled"), "{err}");
    }

    #[test]
    fn a_stopped_export_writes_nothing_through_the_renderer() {
        // The seam, not the source in isolation: `stream_to` has to propagate the
        // refusal rather than treat it as a short read. Asked of every text
        // format, because each writes its header on the first chunk and a
        // swallowed error would leave a header-only file looking valid.
        let (cols, rows) = awkward_rows();
        let rs = ResultSet::from_rows(cols, rows);
        let order: Vec<usize> = (0..rs.row_count()).collect();
        for f in ExportFormat::ALL {
            let mut out: Vec<u8> = Vec::new();
            let mut src = SliceChunks::new(&rs, &order, 1).watching(|_| false);
            assert!(
                f.stream_to(&mut out, &mut src, None, MySql).is_err(),
                "{} swallowed the stop",
                f.label()
            );
        }
    }

    #[test]
    fn slice_chunks_never_takes_a_zero_step() {
        // A `0` here would loop forever handing out empty chunks. Clamped rather
        // than asserted: the size comes from a tuning constant, and an export
        // that hangs is a worse answer than one that writes a single block.
        let rs = rs();
        let order: Vec<usize> = (0..rs.row_count()).collect();
        let mut src = SliceChunks::new(&rs, &order, 0);
        let first = src.next_chunk().unwrap().expect("a chunk").order.len();
        assert_eq!(first, order.len(), "a zero step writes the lot in one go");
        assert!(src.next_chunk().unwrap().is_none());
    }

    #[test]
    fn export_file_names_are_one_per_table_in_order() {
        let tables = vec!["actor".to_string(), "film".to_string()];
        assert_eq!(
            export_file_names(&tables, ExportFormat::Csv),
            vec!["actor.csv".to_string(), "film.csv".to_string()]
        );
    }

    #[test]
    fn export_file_names_sanitize_each_table() {
        // Same rule as `suggested_filename`, because it is the same function
        // underneath — a server-controlled name must not become a path.
        let tables = vec!["a/b".to_string(), "CON".to_string()];
        assert_eq!(
            export_file_names(&tables, ExportFormat::Json),
            vec!["a_b.json".to_string(), "_CON.json".to_string()]
        );
    }

    #[test]
    fn export_file_names_break_a_sanitizing_collision() {
        // Two distinct tables whose names sanitize to one file. Writing both
        // would leave the folder holding the second under the first's name — a
        // silently incomplete export, which is exactly what a backup must not be.
        let tables = vec!["a:b".to_string(), "a*b".to_string(), "a?b".to_string()];
        assert_eq!(
            export_file_names(&tables, ExportFormat::Csv),
            vec![
                "a_b.csv".to_string(),
                "a_b_2.csv".to_string(),
                "a_b_3.csv".to_string()
            ]
        );
    }

    #[test]
    fn export_file_names_collide_case_insensitively() {
        // Windows and macOS filesystems fold case, so `Orders.csv` and
        // `orders.csv` are one file there and two on Linux. Deduplicating
        // case-sensitively would make the export lossy on the two platforms this
        // app actually ships to.
        let tables = vec!["Orders".to_string(), "orders".to_string()];
        assert_eq!(
            export_file_names(&tables, ExportFormat::Csv),
            vec!["Orders.csv".to_string(), "orders_2.csv".to_string()]
        );
    }

    #[test]
    fn export_file_names_keep_a_qualified_name_distinct() {
        // PostgreSQL's display names carry the namespace, so two same-named
        // tables in different schemas need no suffix at all.
        let tables = vec!["public.orders".to_string(), "sales.orders".to_string()];
        assert_eq!(
            export_file_names(&tables, ExportFormat::Csv),
            vec![
                "public.orders.csv".to_string(),
                "sales.orders.csv".to_string()
            ]
        );
    }

    /// **The suffix must step around every real name, whatever order they
    /// arrive in.** A real `a_b_2` beside two tables that both sanitize to `a_b`
    /// would otherwise be handed `a_b_2.csv` by the deduplicator, and the real
    /// one pushed off to `a_b_2_2.csv`.
    ///
    /// Both orders, and the second is the one that matters: the picker sorts,
    /// and `*` (42) and `:` (58) both precede `_` (95), so `a*b`, `a:b`, `a_b_2`
    /// is the order production actually produces. The old guard checked the
    /// names issued *so far*, which is only enough while the real one comes
    /// first — the one order the old test pinned.
    #[test]
    fn export_file_names_suffix_does_not_collide_with_a_real_table() {
        let names = |tables: Vec<&str>| {
            let owned: Vec<String> = tables.iter().map(|t| t.to_string()).collect();
            let out = export_file_names(&owned, ExportFormat::Csv);
            owned.into_iter().zip(out).collect::<Vec<_>>()
        };

        // The order the old guard happened to work in.
        let real_first = names(vec!["a:b", "a_b_2", "a*b"]);
        assert!(real_first.contains(&("a_b_2".to_string(), "a_b_2.csv".to_string())));

        // The order the picker actually produces, sorted.
        let sorted = names(vec!["a*b", "a:b", "a_b_2"]);
        assert!(
            sorted.contains(&("a_b_2".to_string(), "a_b_2.csv".to_string())),
            "the real `a_b_2` keeps its own file: {sorted:?}"
        );
        // …and nobody else got it.
        let holders: Vec<&String> = sorted
            .iter()
            .filter(|(_, f)| f == "a_b_2.csv")
            .map(|(t, _)| t)
            .collect();
        assert_eq!(holders, vec!["a_b_2"], "{sorted:?}");

        // Whatever the order, three tables get three distinct files.
        for set in [real_first, sorted] {
            let files: std::collections::HashSet<&String> = set.iter().map(|(_, f)| f).collect();
            assert_eq!(files.len(), 3, "{set:?}");
        }
    }

    #[test]
    fn export_file_names_handles_an_empty_selection() {
        assert!(export_file_names(&[], ExportFormat::Csv).is_empty());
    }

    #[test]
    fn csv_quotes_only_when_needed() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("he\"llo"), "\"he\"\"llo\"");
    }

    #[test]
    fn csv_neutralizes_formula_injection() {
        // Leading formula/DDE triggers get a `'` prefix (then quoted if needed).
        assert_eq!(csv_field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_field("+1+2"), "'+1+2");
        assert_eq!(csv_field("@SUM(A1)"), "'@SUM(A1)");
        // Tab isn't a CSV delimiter, so the guarded value isn't additionally quoted.
        assert_eq!(csv_field("\tcmd"), "'\tcmd");
        // A `=` mid-value is harmless — only leading chars trigger a formula.
        assert_eq!(csv_field("a=b"), "a=b");
    }

    /// A leading `-` was let through on the grounds that guarding it would
    /// corrupt every negative number. That dichotomy isn't forced: a number and
    /// a formula are distinguishable, so guard the one and leave the other.
    #[test]
    fn csv_guards_a_leading_dash_that_is_not_a_number() {
        // The DDE payload the finding was written from.
        assert_eq!(
            csv_field("-1+1+cmd|' /C calc'!A0"),
            "'-1+1+cmd|' /C calc'!A0"
        );
        assert_eq!(csv_field("-A1"), "'-A1");
        assert_eq!(csv_field("-=1"), "'-=1");
    }

    /// …and every shape of negative number still exports unguarded, which is the
    /// whole reason the character was skipped in the first place.
    #[test]
    fn csv_leaves_negative_numbers_alone() {
        for n in [
            "-5",
            "-0",
            "-5.25",
            "-.5",
            "-1e10",
            "-1E-10",
            "-1234567890123456789",
        ] {
            assert_eq!(csv_field(n), n, "{n} is a number, not a formula");
        }
        // A bare dash isn't a formula either — it's a common "no value" marker.
        assert_eq!(csv_field("-"), "-");
    }

    #[test]
    fn json_suffixes_duplicate_columns() {
        let rs = ResultSet::from_rows(
            vec![col("id"), col("id"), col("id")],
            vec![vec![Value::Int(1), Value::Int(2), Value::Int(3)]],
        );
        let v: serde_json::Value = serde_json::from_str(&export_json(&rs, &[0])).unwrap();
        assert_eq!(v[0]["id"], 1);
        assert_eq!(v[0]["id_2"], 2);
        assert_eq!(v[0]["id_3"], 3);
    }

    #[test]
    fn json_respects_display_order() {
        // order [1, 0] → the NULL-id row first.
        let out = export_json(&rs(), &[1, 0]);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v[0]["id"].is_null());
        assert_eq!(v[1]["id"], 1);
    }

    #[test]
    fn value_to_json_maps_each_variant_and_nulls_nonfinite() {
        use serde_json::Value as J;
        assert_eq!(value_to_json(&Value::Null), J::Null);
        assert_eq!(value_to_json(&Value::Int(-3)), J::from(-3i64));
        assert_eq!(value_to_json(&Value::UInt(3)), J::from(3u64));
        assert_eq!(value_to_json(&Value::Float(1.5)), J::from(1.5));
        assert_eq!(
            value_to_json(&Value::Str("s".into())),
            J::String("s".into())
        );
        // Non-finite floats have no JSON representation → null.
        assert_eq!(value_to_json(&Value::Float(f64::NAN)), J::Null);
        assert_eq!(value_to_json(&Value::Float(f64::INFINITY)), J::Null);
    }

    #[test]
    fn export_csv_has_header_and_nulls_are_empty() {
        let out = export_csv(&rs(), &[0, 1]);
        let lines: Vec<&str> = out.lines().collect();
        // Header quotes the backtick column only because... it has no comma; stays bare.
        assert_eq!(lines[0], "id,a`b");
        assert_eq!(lines[1], "1,x");
        // NULL id renders as an empty leading field.
        assert_eq!(lines[2], ",y");
    }

    #[test]
    fn export_column_csv_is_newline_separated_with_blank_nulls() {
        // Column 0 (id): 1, then NULL → blank line.
        let out = export_column_csv(&rs(), &[0, 1], 0);
        assert_eq!(out, "1\n\n");
    }

    #[test]
    fn md_cell_escapes_pipe_backslash_and_newline() {
        // A pipe would start a new column — escape it. Backslash is Markdown's
        // escape char, so a literal `\` must double (else it'd escape the `|`).
        assert_eq!(md_cell("a|b"), "a\\|b");
        assert_eq!(md_cell("C:\\x"), "C:\\\\x");
        assert_eq!(md_cell("a\\|b"), "a\\\\\\|b");
        // Newlines would break the row → GFM `<br>`; a lone CR is dropped.
        assert_eq!(md_cell("a\nb"), "a<br>b");
        assert_eq!(md_cell("a\r\nb"), "a<br>b");
        assert_eq!(md_cell("plain"), "plain");
    }

    #[test]
    fn export_markdown_has_header_separator_and_orders_rows() {
        // order [1, 0] → NULL-id row first; NULL renders as an empty cell.
        let out = export_markdown(&rs(), &[1, 0]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "| id | a`b |");
        assert_eq!(lines[1], "| --- | --- |");
        assert_eq!(lines[2], "|  | y |");
        assert_eq!(lines[3], "| 1 | x |");
    }

    #[test]
    fn html_escape_orders_ampersand_first() {
        // `&` must be escaped before `<`/`>` or `&lt;` would become `&amp;lt;`.
        assert_eq!(html_escape("a<b>&c"), "a&lt;b&gt;&amp;c");
        assert_eq!(html_escape("plain"), "plain");
    }

    #[test]
    fn export_html_escapes_entities_and_nulls_are_empty() {
        let rs = ResultSet::from_rows(
            vec![col("a<b>")],
            vec![vec![Value::Str("x&y".to_string())], vec![Value::Null]],
        );
        let out = export_html(&rs, &[0, 1]);
        assert!(out.contains("<th>a&lt;b&gt;</th>"));
        assert!(out.contains("<td>x&amp;y</td>"));
        // NULL → empty cell, not the literal "NULL".
        assert!(out.contains("<td></td>"));
        // Well-formed table scaffolding, behind the charset declaration.
        assert!(out.trim_start().starts_with("<meta charset=\"utf-8\">"));
        assert!(out.contains("<table>"));
        assert!(out.contains("<thead>") && out.contains("<tbody>"));
        assert!(out.trim_end().ends_with("</table>"));
    }

    /// Without a declared encoding, a browser opening the saved `file://` HTML
    /// falls back to windows-1252 in Western locales and renders `José` as
    /// `JosÃ©`. The bytes were always correct UTF-8; nothing said so.
    #[test]
    fn export_html_declares_utf8_so_non_ascii_survives() {
        let rs = ResultSet::from_rows(
            vec![col("name")],
            vec![vec![Value::Str("José 東京 €".to_string())]],
        );
        let out = export_html(&rs, &[0]);
        assert!(
            out.trim_start().starts_with("<meta charset=\"utf-8\">"),
            "the declaration must precede any content:\n{out}"
        );
        assert!(out.contains("José 東京 €"), "and the text passes through");
    }

    #[test]
    fn export_column_json_is_array_in_display_order() {
        // Column 1 (a`b) in reversed order.
        let out = export_column_json(&rs(), &[1, 0], 1);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v[0], "y");
        assert_eq!(v[1], "x");
        // Column 0 with a NULL becomes JSON null.
        let out = export_column_json(&rs(), &[0, 1], 0);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v[0], 1);
        assert!(v[1].is_null());
    }
}
