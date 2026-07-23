//! Terminal backend: a shell running on a PTY, driven into an
//! `alacritty_terminal` grid, exposed to the UI as a plain, framework-agnostic
//! [`Screen`] snapshot (rows of colored text runs + cursor).
//!
//! Threading: `portable-pty` spawns the shell; a reader thread pumps bytes
//! through the VTE parser into the shared `Term` (behind a `FairMutex`), then
//! calls the `notify` callback so the UI repaints. Input and resize come from
//! the UI thread via [`Terminal::send_input`] / [`Terminal::resize`].

pub mod shell;

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionRange, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode, point_to_viewport, viewport_to_point};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Processor, Rgb};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub use shell::{ShellConfig, ShellProfile, TerminalSettings};

/// Default terminal foreground/background (the UI paints the panel with `BG`).
pub const DEFAULT_FG: (u8, u8, u8) = (0xCC, 0xCC, 0xCC);
pub const DEFAULT_BG: (u8, u8, u8) = (0x0E, 0x0F, 0x13);
/// Block-cursor color (drawn as an inverted cell).
pub const CURSOR: (u8, u8, u8) = (0xC6, 0xC8, 0xD6);
/// Selection highlight background.
pub const SELECTION: (u8, u8, u8) = (0x33, 0x3B, 0x5C);

// ── Snapshot model (no alacritty/Floem types leak past here) ────────────────

/// A contiguous run of same-styled cells on one row.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub fg: (u8, u8, u8),
    pub bg: Option<(u8, u8, u8)>,
    pub bold: bool,
    /// If this run is (part of) a URL, the full URL to open on click.
    pub link: Option<String>,
}

/// One rendered row: its runs left-to-right (trailing blank cells trimmed).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub runs: Vec<Run>,
}

/// A full render snapshot of the visible viewport.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    pub rows: Vec<Row>,
    pub cols: usize,
    /// Viewport (row, col) of the block cursor, if shown.
    pub cursor: Option<(usize, usize)>,
    /// Lines scrolled up into history (0 = at the bottom / live).
    pub display_offset: usize,
    /// Total scrollable lines (scrollback history + the visible viewport). Lets
    /// the UI size a scrollback scrollbar: viewport = `rows.len()`, thumb sits at
    /// `total_lines - rows - display_offset` from the top.
    pub total_lines: usize,
}

// ── Event proxy: reply to terminal queries by writing back to the PTY ───────

#[derive(Clone)]
struct EventProxy {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event
            && let Ok(mut w) = self.writer.lock()
        {
            let _ = w.write_all(text.as_bytes());
            let _ = w.flush();
        }
    }
}

// ── Dimensions ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Dims {
    cols: usize,
    lines: usize,
}

impl Dimensions for Dims {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

// ── Terminal ────────────────────────────────────────────────────────────────

type SharedTerm = Arc<FairMutex<Term<EventProxy>>>;

pub struct Terminal {
    term: SharedTerm,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    notify: Arc<dyn Fn() + Send + Sync>,
}

impl Terminal {
    /// Spawn `shell` on a fresh PTY sized `cols`×`rows`. `notify` is invoked
    /// (from the reader thread) whenever the grid changes.
    pub fn spawn(
        shell: &ShellConfig,
        cols: u16,
        rows: u16,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> std::io::Result<Terminal> {
        let cols = cols.max(1);
        let rows = rows.max(1);

        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(to_io)?;

        let mut cmd = CommandBuilder::new(&shell.program);
        cmd.args(&shell.args);
        cmd.env("TERM", "xterm-256color");
        for (k, v) in &shell.env {
            cmd.env(k, v);
        }
        let cwd = shell
            .cwd
            .clone()
            .or_else(home_dir)
            .filter(|d| std::path::Path::new(d).is_dir());
        if let Some(dir) = cwd {
            cmd.cwd(dir);
        }

        let child = pair.slave.spawn_command(cmd).map_err(to_io)?;
        // Close the slave in the parent so read hits EOF when the child exits.
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().map_err(to_io)?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer().map_err(to_io)?));

        let proxy = EventProxy {
            writer: writer.clone(),
        };
        let term = Term::new(
            Config::default(),
            &Dims {
                cols: cols as usize,
                lines: rows as usize,
            },
            proxy,
        );
        let term: SharedTerm = Arc::new(FairMutex::new(term));

        // Reader thread: pump PTY bytes → VTE parser → grid, then notify.
        {
            let term = term.clone();
            let notify = notify.clone();
            std::thread::Builder::new()
                .name("schemaic-term-reader".into())
                .spawn(move || read_loop(reader, term, notify))
                .ok();
        }

        Ok(Terminal {
            term,
            writer,
            master: Mutex::new(pair.master),
            child: Mutex::new(child),
            notify,
        })
    }

    /// Feed raw bytes (already VT-encoded) to the shell's stdin.
    pub fn send_input(&self, bytes: &[u8]) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// Resize both the PTY and the grid to `cols`×`rows`.
    pub fn resize(&self, cols: u16, rows: u16) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if let Ok(master) = self.master.lock() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        self.term.lock().resize(Dims {
            cols: cols as usize,
            lines: rows as usize,
        });
        (self.notify)();
    }

    /// Scroll the viewport by `delta` lines (positive = into history).
    pub fn scroll(&self, delta: i32) {
        self.term.lock().scroll_display(Scroll::Delta(delta));
        (self.notify)();
    }

    /// Reset the viewport to the live bottom (e.g. on new input).
    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    /// Begin a text selection at viewport cell (row, col).
    pub fn selection_start(&self, vrow: usize, vcol: usize) {
        let mut term = self.term.lock();
        let off = term.grid().display_offset();
        let point = viewport_to_point(off, Point::new(vrow, Column(vcol)));
        term.selection = Some(Selection::new(SelectionType::Simple, point, Side::Left));
        drop(term);
        (self.notify)();
    }

    /// Extend the current selection to viewport cell (row, col).
    pub fn selection_update(&self, vrow: usize, vcol: usize) {
        let mut term = self.term.lock();
        let off = term.grid().display_offset();
        let point = viewport_to_point(off, Point::new(vrow, Column(vcol)));
        if let Some(sel) = term.selection.as_mut() {
            sel.update(point, Side::Right);
        }
        drop(term);
        (self.notify)();
    }

    /// Clear any selection.
    pub fn selection_clear(&self) {
        self.term.lock().selection = None;
        (self.notify)();
    }

    /// The selected text, if any.
    pub fn selection_text(&self) -> Option<String> {
        self.term
            .lock()
            .selection_to_string()
            .filter(|s| !s.is_empty())
    }

    /// Paste `text` into the shell (bracketed if the app enabled that mode).
    pub fn paste(&self, text: &str) {
        let bracketed = self.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
        if bracketed {
            self.send_input(b"\x1b[200~");
            self.send_input(text.as_bytes());
            self.send_input(b"\x1b[201~");
        } else {
            let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
            self.send_input(normalized.as_bytes());
        }
        self.scroll_to_bottom();
    }

    /// Build a render snapshot. `cursor_on` shows the cursor (pass the panel's
    /// focus state — folded with any blink phase — so it hides when unfocused).
    /// `bake_block` inverts the cursor cell for a block cursor; bar/underline
    /// shapes pass `false` and are drawn by the UI as an overlay using `cursor`.
    pub fn snapshot(&self, cursor_on: bool, bake_block: bool) -> Screen {
        let term = self.term.lock();
        let cols = term.columns();
        let lines = term.screen_lines();

        let content = term.renderable_content();
        let display_offset = content.display_offset;
        let cursor_pt = content.cursor.point;
        let cursor_shown = cursor_on && content.cursor.shape != CursorShape::Hidden;
        let palette = content.colors;
        let selection = content.selection;

        // Fill a dense grid of resolved cells, then coalesce into runs.
        let blank = CellData::blank();
        let mut grid: Vec<Vec<CellData>> = vec![vec![blank.clone(); cols]; lines];
        for ind in content.display_iter {
            let Some(vp) = point_to_viewport(display_offset, ind.point) else {
                continue;
            };
            let (vl, vc) = (vp.line, vp.column.0);
            if vl < lines && vc < cols {
                let mut cd = CellData::resolve(ind.cell, palette);
                if selection.is_some_and(|r| in_selection(&r, ind.point)) {
                    cd.bg = Some(SELECTION);
                }
                grid[vl][vc] = cd;
            }
        }

        for row in grid.iter_mut() {
            tag_links(row);
        }

        let cursor = if cursor_shown {
            point_to_viewport(display_offset, cursor_pt).map(|vp| (vp.line, vp.column.0))
        } else {
            None
        };
        // Bake a block cursor into the grid so it renders inline with the runs.
        // Bar/underline shapes skip this and are overlaid by the UI.
        if bake_block
            && let Some((cr, cc)) = cursor
            && cr < lines
            && cc < cols
        {
            let cell = &mut grid[cr][cc];
            let glyph_fg = DEFAULT_BG;
            cell.fg = glyph_fg;
            cell.bg = Some(CURSOR);
        }

        let total_lines = term.grid().history_size() + lines;
        let rows = grid.into_iter().map(coalesce_row).collect();
        Screen {
            rows,
            cols,
            cursor,
            display_offset,
            total_lines,
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            // Reap it — `kill()` alone leaves a zombie until the parent waits
            // (a fresh one per terminal restart / DB-CLI open) (§7.3).
            let _ = child.wait();
        }
    }
}

fn read_loop(
    mut reader: Box<dyn Read + Send>,
    term: SharedTerm,
    notify: Arc<dyn Fn() + Send + Sync>,
) {
    let mut parser: Processor = Processor::new();
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                {
                    let mut term = term.lock();
                    for &b in &buf[..n] {
                        parser.advance(&mut *term, b);
                    }
                }
                notify();
            }
        }
    }
    notify();
}

// ── Cell resolution + coalescing ────────────────────────────────────────────

#[derive(Clone, PartialEq)]
struct CellData {
    c: char,
    fg: (u8, u8, u8),
    bg: Option<(u8, u8, u8)>,
    bold: bool,
    link: Option<String>,
}

impl CellData {
    fn blank() -> Self {
        CellData {
            c: ' ',
            fg: DEFAULT_FG,
            bg: None,
            bold: false,
            link: None,
        }
    }

    fn resolve(
        cell: &alacritty_terminal::term::cell::Cell,
        palette: &alacritty_terminal::term::color::Colors,
    ) -> Self {
        let flags = cell.flags;
        // Wide-char spacers hold no glyph of their own.
        let c = if flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER | Flags::HIDDEN)
        {
            // Spacer cells hold no glyph; hidden cells render blank.
            ' '
        } else {
            cell.c
        };
        let bold = flags.contains(Flags::BOLD) || flags.contains(Flags::DIM_BOLD);
        let mut fg = resolve_color(cell.fg, palette);
        let mut bg = match cell.bg {
            AnsiColor::Named(NamedColor::Background) => None,
            other => Some(resolve_color(other, palette)),
        };
        if flags.contains(Flags::INVERSE) {
            let new_bg = fg;
            fg = bg.unwrap_or(DEFAULT_BG);
            bg = Some(new_bg);
        }
        CellData {
            c,
            fg,
            bg,
            bold,
            link: None,
        }
    }
}

fn is_url_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c)
}

fn starts_with_at(chars: &[char], i: usize, pat: &str) -> bool {
    pat.chars()
        .enumerate()
        .all(|(k, pc)| chars.get(i + k) == Some(&pc))
}

// Tag cells that form an http(s) URL with the full URL (for click-to-open).
fn tag_links(cells: &mut [CellData]) {
    let chars: Vec<char> = cells.iter().map(|c| c.c).collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if starts_with_at(&chars, i, "https://") || starts_with_at(&chars, i, "http://") {
            let mut j = i;
            while j < n && is_url_char(chars[j]) {
                j += 1;
            }
            while j > i && matches!(chars[j - 1], '.' | ',' | ')' | ']' | '}' | '>' | '"' | '\'') {
                j -= 1;
            }
            if j - i > 8 {
                let url: String = chars[i..j].iter().collect();
                for cell in cells[i..j].iter_mut() {
                    cell.link = Some(url.clone());
                }
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
}

// Is the grid cell at `p` inside the selection range?
fn in_selection(r: &SelectionRange, p: Point) -> bool {
    if r.is_block {
        p.line >= r.start.line
            && p.line <= r.end.line
            && p.column >= r.start.column
            && p.column <= r.end.column
    } else {
        let after_start =
            p.line > r.start.line || (p.line == r.start.line && p.column >= r.start.column);
        let before_end = p.line < r.end.line || (p.line == r.end.line && p.column <= r.end.column);
        after_start && before_end
    }
}

fn coalesce_row(cells: Vec<CellData>) -> Row {
    // Trim trailing blank (space, default fg, no bg) cells to keep view count low.
    let mut end = cells.len();
    while end > 0 {
        let cell = &cells[end - 1];
        if cell.c == ' ' && cell.bg.is_none() {
            end -= 1;
        } else {
            break;
        }
    }
    let mut runs: Vec<Run> = Vec::new();
    for cell in &cells[..end] {
        match runs.last_mut() {
            Some(run)
                if run.fg == cell.fg
                    && run.bg == cell.bg
                    && run.bold == cell.bold
                    && run.link == cell.link =>
            {
                run.text.push(cell.c);
            }
            _ => runs.push(Run {
                text: cell.c.to_string(),
                fg: cell.fg,
                bg: cell.bg,
                bold: cell.bold,
                link: cell.link.clone(),
            }),
        }
    }
    Row { runs }
}

fn resolve_color(c: AnsiColor, palette: &alacritty_terminal::term::color::Colors) -> (u8, u8, u8) {
    match c {
        AnsiColor::Spec(rgb) => (rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(i) => palette[i as usize]
            .map(rgb_tuple)
            .unwrap_or_else(|| indexed_rgb(i)),
        AnsiColor::Named(n) => palette[n].map(rgb_tuple).unwrap_or_else(|| named_rgb(n)),
    }
}

fn rgb_tuple(rgb: Rgb) -> (u8, u8, u8) {
    (rgb.r, rgb.g, rgb.b)
}

/// The 16 base ANSI colors (Windows Terminal "Campbell").
const ANSI16: [(u8, u8, u8); 16] = [
    (0x0C, 0x0C, 0x0C),
    (0xC5, 0x0F, 0x1F),
    (0x13, 0xA1, 0x0E),
    (0xC1, 0x9C, 0x00),
    (0x00, 0x37, 0xDA),
    (0x88, 0x17, 0x98),
    (0x3A, 0x96, 0xDD),
    (0xCC, 0xCC, 0xCC),
    (0x76, 0x76, 0x76),
    (0xE7, 0x48, 0x56),
    (0x16, 0xC6, 0x0C),
    (0xF9, 0xF1, 0xA5),
    (0x3B, 0x78, 0xFF),
    (0xB4, 0x00, 0x9E),
    (0x61, 0xD6, 0xD6),
    (0xF2, 0xF2, 0xF2),
];

fn named_rgb(n: NamedColor) -> (u8, u8, u8) {
    use NamedColor::*;
    match n {
        Black | DimBlack => ANSI16[0],
        Red | DimRed => ANSI16[1],
        Green | DimGreen => ANSI16[2],
        Yellow | DimYellow => ANSI16[3],
        Blue | DimBlue => ANSI16[4],
        Magenta | DimMagenta => ANSI16[5],
        Cyan | DimCyan => ANSI16[6],
        White | DimWhite => ANSI16[7],
        BrightBlack => ANSI16[8],
        BrightRed => ANSI16[9],
        BrightGreen => ANSI16[10],
        BrightYellow => ANSI16[11],
        BrightBlue => ANSI16[12],
        BrightMagenta => ANSI16[13],
        BrightCyan => ANSI16[14],
        BrightWhite => ANSI16[15],
        // The "foreground" color role resolves to the default foreground whether
        // it lands in a fg or bg slot (e.g. under reverse video).
        Foreground | BrightForeground | DimForeground => DEFAULT_FG,
        Background => DEFAULT_BG,
        Cursor => CURSOR,
    }
}

/// Resolve an xterm 256-color index to RGB (16 base + 6×6×6 cube + grayscale).
fn indexed_rgb(i: u8) -> (u8, u8, u8) {
    match i {
        0..=15 => ANSI16[i as usize],
        16..=231 => {
            let x = i - 16;
            let step = |v: u8| if v == 0 { 0 } else { v * 40 + 55 };
            (step(x / 36), step((x / 6) % 6), step(x % 6))
        }
        232..=255 => {
            let v = (i - 232) * 10 + 8;
            (v, v, v)
        }
    }
}

fn home_dir() -> Option<String> {
    std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())
}

fn to_io(e: anyhow::Error) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::index::Line;

    fn cells(s: &str) -> Vec<CellData> {
        s.chars()
            .map(|c| CellData {
                c,
                fg: DEFAULT_FG,
                bg: None,
                bold: false,
                link: None,
            })
            .collect()
    }

    #[test]
    fn indexed_rgb_base_cube_and_grayscale() {
        // 0..=15 map straight to the base palette.
        assert_eq!(indexed_rgb(0), ANSI16[0]);
        assert_eq!(indexed_rgb(15), ANSI16[15]);
        // 6×6×6 cube: 16 is black, 231 is white.
        assert_eq!(indexed_rgb(16), (0, 0, 0));
        assert_eq!(indexed_rgb(231), (255, 255, 255));
        // A mid-cube step uses v*40+55.
        assert_eq!(indexed_rgb(196), (255, 0, 0)); // x=180 → (5,0,0)
        // Grayscale ramp: 232 = 8, 255 = 238.
        assert_eq!(indexed_rgb(232), (8, 8, 8));
        assert_eq!(indexed_rgb(255), (238, 238, 238));
    }

    #[test]
    fn named_rgb_maps_ansi_and_dim_aliases() {
        assert_eq!(named_rgb(NamedColor::Red), ANSI16[1]);
        assert_eq!(named_rgb(NamedColor::DimRed), ANSI16[1]); // dim aliases base
        assert_eq!(named_rgb(NamedColor::BrightWhite), ANSI16[15]);
        assert_eq!(named_rgb(NamedColor::Foreground), DEFAULT_FG);
        assert_eq!(named_rgb(NamedColor::Background), DEFAULT_BG);
    }

    #[test]
    fn is_url_char_accepts_url_bytes_rejects_others() {
        assert!(is_url_char('a'));
        assert!(is_url_char('Z'));
        assert!(is_url_char('9'));
        assert!(is_url_char(':'));
        assert!(is_url_char('/'));
        assert!(is_url_char('%'));
        assert!(!is_url_char(' '));
        assert!(!is_url_char('{'));
        assert!(!is_url_char('«'));
    }

    #[test]
    fn tag_links_marks_url_cells_and_trims_trailing_punctuation() {
        let mut c = cells("see https://example.com/x.");
        tag_links(&mut c);
        // The trailing period is NOT part of the link.
        let linked: String = c
            .iter()
            .filter(|cell| cell.link.is_some())
            .map(|cell| cell.c)
            .collect();
        assert_eq!(linked, "https://example.com/x");
        // Every linked cell carries the whole URL.
        assert!(
            c.iter()
                .filter(|cell| cell.link.is_some())
                .all(|cell| cell.link.as_deref() == Some("https://example.com/x"))
        );
        // Leading prose ("see ") is not linked.
        assert!(c[0].link.is_none());
    }

    #[test]
    fn tag_links_ignores_non_url_and_too_short() {
        let mut c = cells("no links here, and http:// alone");
        tag_links(&mut c);
        assert!(c.iter().all(|cell| cell.link.is_none()));
    }

    #[test]
    fn coalesce_row_trims_trailing_blanks_and_merges_same_style() {
        // "ab" same style + trailing spaces (bg none) → one run "ab".
        let row = coalesce_row(cells("ab   "));
        assert_eq!(row.runs.len(), 1);
        assert_eq!(row.runs[0].text, "ab");

        // A style break splits runs.
        let mut c = cells("ab");
        c[1].bold = true;
        let row = coalesce_row(c);
        assert_eq!(row.runs.len(), 2);
        assert_eq!(row.runs[0].text, "a");
        assert_eq!(row.runs[1].text, "b");
        assert!(row.runs[1].bold);
    }

    #[test]
    fn coalesce_row_keeps_trailing_space_with_background() {
        // A trailing space that carries a background is meaningful → kept.
        let mut c = cells("a ");
        c[1].bg = Some((10, 20, 30));
        let row = coalesce_row(c);
        // Two runs: 'a' (no bg) and ' ' (bg) — not trimmed.
        assert_eq!(row.runs.iter().map(|r| r.text.len()).sum::<usize>(), 2);
    }

    fn pt(line: i32, col: usize) -> Point {
        Point {
            line: Line(line),
            column: Column(col),
        }
    }

    #[test]
    fn in_selection_linear_spans_wrapped_lines() {
        // Linear selection from (0,2) to (2,3): whole middle line is included.
        let r = SelectionRange {
            start: pt(0, 2),
            end: pt(2, 3),
            is_block: false,
        };
        assert!(!in_selection(&r, pt(0, 1))); // before start column on start line
        assert!(in_selection(&r, pt(0, 2))); // at start
        assert!(in_selection(&r, pt(1, 0))); // full middle line
        assert!(in_selection(&r, pt(2, 3))); // at end
        assert!(!in_selection(&r, pt(2, 4))); // past end column on end line
        assert!(!in_selection(&r, pt(3, 0))); // past end line
    }

    #[test]
    fn in_selection_block_is_a_rectangle() {
        // Block selection columns 2..=4 across lines 0..=2.
        let r = SelectionRange {
            start: pt(0, 2),
            end: pt(2, 4),
            is_block: true,
        };
        assert!(in_selection(&r, pt(1, 3))); // inside the rectangle
        assert!(in_selection(&r, pt(0, 2))); // corner
        assert!(!in_selection(&r, pt(1, 1))); // left of the column band
        assert!(!in_selection(&r, pt(1, 5))); // right of the column band
        assert!(!in_selection(&r, pt(3, 3))); // below the row band
    }
}
