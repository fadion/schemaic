//! **The binary-cell panel** — what a `<n bytes>` cell looks like when you open
//! it.
//!
//! The grid shows raw bytes as a byte count and nothing else, because it holds
//! no bytes to show: they are dropped at the wire on every engine, for the
//! reasons [`schemaic_core::blob`] gives. So this modal's content does not come
//! from the result — it comes from a second, targeted query the app runs when
//! the panel opens, and everything here is a rendering of what came back.
//!
//! Three things can arrive, and the panel says which: the bytes
//! ([`BlobState::Ready`]), nothing at all because the cell is `NULL` or the row
//! is gone ([`BlobState::Empty`]), or a failure. There is deliberately no fourth
//! state for "not fetched yet but might be fine" — the fetch starts as the modal
//! opens, so [`BlobState::Loading`] is what a panel with something to fetch is
//! born in. One with nothing to fetch is born [`BlobState::Empty`]: the panel
//! opens *in* its state rather than being told a moment later, which is
//! [`BlobUi::open`]'s reason for taking one.
//!
//! **It is a write surface too.** A file loaded here does not go to the server
//! from this module — it goes to the grid's staged edits through the
//! [`BlobUi::stage`] sink the grid supplies, and reaches the table on the grid's
//! Commit like any typed edit.
//!
//! **The hex dump is virtualized and the preview is not.** A capped fetch is
//! still 64 MiB, which is four million hex lines, so they are built one visible
//! row at a time out of the shared buffer ([`schemaic_core::blob::hex_line`]) —
//! the same `VirtualVector<usize>` shape the results grid uses. The image
//! preview has no such trick available and needs none: floem's `img` decodes
//! once at construction, and an image that decodes at all is already in memory.

use std::rc::Rc;
use std::sync::Arc;

use floem::action::{open_file, save_as};
use floem::file::{FileDialogOptions, FileSpec};
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::views::{VirtualDirection, VirtualItemSize, VirtualVector, virtual_stack};

use schemaic_core::blob::{
    BlobKind, BlobValue, FETCH_CAP, HEX_COLS, PreviewVerdict, hex_line, hex_row_count,
    preview_verdict,
};
use schemaic_core::format::{contrasting_bytes, human_bytes};

use crate::theme;
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, focus_root_with_ring, modal_footer_split,
    modal_h, modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use crate::{BlobLoadRequest, BlobSaveRequest, Ui};

/// The cell the panel is open on — what it needs to *say*, not what it needs to
/// fetch.
///
/// The [`schemaic_core::blob::BlobRef`] that addresses the row stays in the app,
/// with the connection it must be asked over: this modal never issues a query,
/// and giving it the means to would make two places responsible for aiming one.
#[derive(Clone, Debug, PartialEq)]
pub struct BlobTarget {
    /// `staff.picture` — the modal's title.
    pub title: String,
    /// The file name a save offers, without an extension: `staff_picture_1`.
    /// The extension comes from what the bytes turn out to be, which is not
    /// known yet when this is built.
    pub stem: String,
    /// The most bytes this column can hold, if its declared type says — see
    /// [`schemaic_core::blob::column_byte_cap`]. `None` is "no answer", never
    /// "no limit".
    ///
    /// Carried on the target rather than asked for at load time because it is a
    /// fact about the *cell the panel is open on*, which is exactly what this
    /// struct is, and because by the time a file comes back the grid the cap was
    /// read from may have been re-run underneath.
    pub cap: Option<u64>,
}

/// Which of the two views of the same bytes is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlobPane {
    Preview,
    Hex,
}

/// What the fetch has produced.
#[derive(Clone, Debug)]
pub enum BlobState {
    /// The query is out. The panel opens in this state.
    Loading,
    /// Bytes, and what they turned out to be.
    ///
    /// `Arc` because the buffer is up to [`FETCH_CAP`] and every read of the
    /// signal would otherwise copy it — the hex view reads it once per visible
    /// line.
    Ready {
        value: Arc<BlobValue>,
        kind: BlobKind,
    },
    /// The cell is SQL `NULL`, or the row is no longer there.
    ///
    /// One state for both, because the panel's honest sentence is the same and
    /// the difference is not one this can establish: a row deleted between the
    /// result loading and the panel opening is indistinguishable from a cell
    /// that was always `NULL`, from here.
    Empty,
    Failed(String),
}

/// The panel's signals. Owned by [`Ui`], like every other modal's.
#[derive(Clone, Copy)]
pub struct BlobUi {
    /// The cell being looked at; `Some` ⇒ the modal is up.
    pub target: RwSignal<Option<BlobTarget>>,
    pub state: RwSignal<BlobState>,
    /// Which pane. Set when the bytes land — an image opens on its preview and
    /// anything else on the hex — and then only by the user.
    pub pane: RwSignal<BlobPane>,
    /// The status line under the body: what the last save or load did, or why it
    /// failed. Cleared when the panel closes.
    ///
    /// One line for both because there is one place to put it, and the two are
    /// never in flight at once — a load replaces what a save reported on, so a
    /// stale "Saved to …" beside freshly loaded bytes would be a sentence about
    /// a file that is no longer what the panel is showing.
    pub note: RwSignal<Option<Result<String, String>>>,
    /// Where a file loaded here goes — `None` when the cell cannot be written,
    /// which is what leaves *Load from file* **disabled**. Disabled rather than
    /// absent, unlike the grid's menu entry: the entry is the only sign a
    /// `<n bytes>` cell has anything behind it, so one that opens a panel to
    /// refuse is worse than none — a button in a panel already open is the
    /// opposite case, and a footer that changed shape between cells would read
    /// as the panel being a different thing each time.
    /// See [`crate::BlobStageFn`].
    pub stage: RwSignal<Option<crate::BlobStageFn>>,
    /// Which opening of the panel is the current one.
    ///
    /// **Only the opening that started a fetch may report into it.** The panel
    /// takes the keyboard but not the clock: Escape closes it while its `SELECT`
    /// is still out, and the next cell opened is a *different* panel that the
    /// first fetch would otherwise land in — one cell's bytes under another
    /// cell's title, which is the same failure the export modal's `id` exists to
    /// prevent and is indistinguishable from the app simply being wrong about
    /// which row it read.
    pub epoch: RwSignal<u64>,
}

impl BlobUi {
    pub fn new() -> BlobUi {
        BlobUi {
            target: RwSignal::new(None),
            state: RwSignal::new(BlobState::Loading),
            pane: RwSignal::new(BlobPane::Hex),
            note: RwSignal::new(None),
            stage: RwSignal::new(None),
            epoch: RwSignal::new(0),
        }
    }

    /// Put the panel up on `target` and start it loading; the returned epoch is
    /// what the fetch must hand back to [`BlobUi::loaded`].
    ///
    /// **Every field is reset, not just the two that obviously change.** A
    /// second cell opened after a first left `note` holding the first one's
    /// "Saved to …", under a title naming the second — a sentence about a file
    /// that has nothing to do with what is on screen.
    ///
    /// `stage` is where the next cell's bytes go, so it is reset like the rest:
    /// carrying the previous cell's over would aim *Load from file* at a row the
    /// panel is no longer about, and that is a wrong write rather than a wrong
    /// sentence.
    ///
    /// **`state` is a parameter, not always `Loading`.** A cell with bytes to
    /// fetch opens `Loading`, but one with nothing committed behind it — a NULL
    /// cell, a pending new row — is `Empty` the moment it opens and there is
    /// nothing to wait for. Reporting that *after* opening is what made the panel
    /// panic: `open` and the report ran in the same turn, so the rebuild queued
    /// by `target` carried a `Loading` phase over a state that was already
    /// `Empty`. Opening in the final state leaves no transition to disagree
    /// about, and it is the truer statement anyway — the panel is born knowing.
    pub fn open(
        self,
        target: BlobTarget,
        stage: Option<crate::BlobStageFn>,
        state: BlobState,
    ) -> u64 {
        let epoch = self.epoch.get_untracked().wrapping_add(1);
        self.epoch.set(epoch);
        self.state.set(state);
        self.pane.set(BlobPane::Hex);
        self.note.set(None);
        self.stage.set(stage);
        self.target.set(Some(target));
        epoch
    }

    /// Take the panel down.
    ///
    /// The epoch moves here too, so a fetch still in flight when the user
    /// pressed Escape cannot report into whatever opens next.
    pub fn close(self) {
        self.epoch.set(self.epoch.get_untracked().wrapping_add(1));
        self.target.set(None);
        self.state.set(BlobState::Loading);
        self.note.set(None);
        self.stage.set(None);
    }

    /// Report a finished fetch, and open the pane the content deserves.
    ///
    /// A report from a superseded opening is dropped: see [`BlobUi::epoch`].
    ///
    /// **The pane is chosen here rather than in the view** because it is a
    /// decision made once, when the bytes arrive — a view that picked it would
    /// re-pick it on every rebuild and drag the user back off the pane they
    /// switched to.
    pub fn loaded(self, epoch: u64, state: BlobState) {
        if self.epoch.get_untracked() != epoch {
            return;
        }
        if let BlobState::Ready { kind, .. } = &state
            && kind.is_image()
        {
            self.pane.set(BlobPane::Preview);
        }
        self.state.set(state);
    }

    /// Report a finished save — the path written, or why it failed.
    ///
    /// Epoch-guarded like [`BlobUi::loaded`], and for a failure that is easier
    /// to hit: a save is slower than a fetch, so "Save, Close, open another
    /// cell" leaves the first file's `Saved to …` sitting under the second
    /// cell's title, claiming a file was written *from* it.
    pub fn saved_at(self, epoch: u64, outcome: Result<String, String>) {
        if self.epoch.get_untracked() != epoch {
            return;
        }
        self.note
            .set(Some(outcome.map(|path| format!("Saved to {path}"))));
    }

    /// Report a finished **load**: show the file's bytes and stage them into the
    /// cell the panel was opened on.
    ///
    /// `outcome` is the file's bytes, or why they could not be read — **not** its
    /// name. The status line does not use one: it sits under a title already
    /// naming the cell, and the sentence that has to fit beside three buttons is
    /// better spent saying the bytes are staged rather than which file they came
    /// from. Threading a name nothing reads is how a parameter rots.
    ///
    /// Epoch-guarded like every other report here — a read that outlived its
    /// panel must not stage into whatever the user opened next, which is the one
    /// failure in this family that writes to a database rather than to a label.
    ///
    /// **The epoch moves, and it moves last.** The panel's rebuild key is
    /// `(epoch, open, phase)`, and replacing one `Ready` value with another
    /// changes no phase — so without the bump the body is never torn down and
    /// the panel goes on showing the *old* image over the new bytes' status
    /// line. Moving it is also truthful about the read still in flight on a cell
    /// the user has just replaced: its answer is about a value nobody is looking
    /// at.
    ///
    /// *Last* because the rebuild the bump triggers reads the payload
    /// **untracked** — that is the whole design of the key, and it means the
    /// state has to be there before the key admits it. Bumping first rebuilds
    /// the body against the value being replaced, and the `state.set` behind it
    /// then changes no key term and so rebuilds nothing: the panel would settle
    /// showing the old bytes under the new file's name. The same ordering
    /// `BlobUi::open` keeps, for the same reason.
    ///
    /// The sentence says **staged**, not written. The bytes are in the grid's
    /// dirty map like any typed edit and reach the table on Commit; a panel that
    /// said "Loaded" and left it there would read as a save.
    pub fn loaded_file(self, epoch: u64, outcome: Result<Vec<u8>, String>) {
        if self.epoch.get_untracked() != epoch {
            return;
        }
        let bytes = match outcome {
            Ok(v) => v,
            Err(e) => {
                self.note.set(Some(Err(e)));
                return;
            }
        };
        // No sink means the panel is a viewer; there was no button to press, so
        // this is unreachable rather than a case. Reported rather than ignored:
        // silently dropping the file would look exactly like a load that worked.
        let Some(stage) = self.stage.get_untracked() else {
            self.note
                .set(Some(Err("This cell cannot be written.".to_string())));
            return;
        };
        // **Refused here, where the user chose the file.** The server would
        // refuse it too, but at the commit: `ERROR 1406: Data too long` names
        // the column rather than the file, arrives after this panel has closed,
        // and takes every other staged edit in the batch down with it. Nothing
        // is staged, so the panel keeps showing the value that is still there.
        if let Some(cap) = self
            .target
            .with_untracked(|t| t.as_ref().and_then(|t| t.cap))
            && bytes.len() as u64 > cap
        {
            // `contrasting_bytes`, not `human_bytes` twice: the two numbers
            // being *different* is the whole message, and one decimal place
            // rounds 65,536 and a `BLOB`'s 65,535 to the same `64.0 KB`.
            let (got, most) = contrasting_bytes(bytes.len() as u64, cap);
            self.note.set(Some(Err(format!(
                "That file is {got} — this column holds at most {most}."
            ))));
            return;
        }
        // **The sink gets the last word.** The file dialog stood open while the
        // grid was free to move: the pending row may be gone, the row may have
        // been marked for deletion, the result may have been re-run read-only.
        // A refusal reported as "Loaded file." is the one outcome worse than
        // either — it looks exactly like a write that will happen and is not.
        if !(stage)(bytes.clone()) {
            self.note.set(Some(Err(
                "That cell is no longer accepting a value — nothing was staged.".to_string(),
            )));
            return;
        }
        let kind = schemaic_core::blob::sniff(&bytes);
        let value = Arc::new(BlobValue {
            len: bytes.len() as u64,
            bytes,
        });
        self.pane.set(match kind.is_image() {
            true => BlobPane::Preview,
            false => BlobPane::Hex,
        });
        self.state.set(BlobState::Ready { value, kind });
        self.epoch.set(self.epoch.get_untracked().wrapping_add(1));
        self.note.set(Some(Ok(
            "Loaded file. Commit in the grid to write it.".to_string()
        )));
    }
}

impl Default for BlobUi {
    fn default() -> Self {
        BlobUi::new()
    }
}

/// Row source for the hex dump's virtual stack: line indices and nothing else,
/// exactly like the results grid's. The view function indexes into the shared
/// byte buffer, so four million lines cost four million `usize`s of nothing.
struct HexRows {
    len: usize,
}

impl VirtualVector<usize> for HexRows {
    fn total_len(&self) -> usize {
        self.len
    }

    fn slice(&mut self, range: std::ops::Range<usize>) -> impl Iterator<Item = usize> {
        range
    }
}

/// The height of one hex line, and the width the dump needs — both derived from
/// the mono metrics rather than guessed, so the ASCII panel does not wrap at an
/// interface scale nobody tested.
fn hex_font() -> f32 {
    theme::scaled(12.0) as f32
}

fn hex_row_h() -> f64 {
    (hex_font() as f64 * 1.45).round()
}

/// Characters in one [`hex_line`]: the eight-digit offset and its two spaces,
/// [`HEX_COLS`] three-character byte groups plus the gap between the two halves,
/// then the ASCII panel between its two pipes.
const HEX_LINE_CHARS: usize = 8 + 2 + HEX_COLS * 3 + 1 + HEX_COLS + 2;

/// The panel's width: the wider of a comfortable modal and **whatever a hex line
/// actually needs**.
///
/// A dump that wraps is not a dump — the offsets stop lining up and the ASCII
/// panel lands under the bytes of the line above. The line's width is fixed by
/// construction, so it can be measured rather than guessed: `0.6023` is IBM Plex
/// Mono's advance ratio, the same figure `term_cell_wh` uses to size the
/// terminal. Taking the maximum is what keeps the modal honest at an interface
/// scale nobody tested, where the nominal width alone would be too narrow.
fn panel_w() -> f64 {
    const MONO_ADVANCE: f64 = 0.6023;
    let hex = HEX_LINE_CHARS as f64 * hex_font() as f64 * MONO_ADVANCE
        + theme::scaled(20.0)
        + modal_pad_h() * 2.0;
    modal_w(680.0).max(hex)
}

/// The body's height, so the two panes are the same size and switching between
/// them does not resize the window's worth of modal under the cursor.
fn body_h() -> f64 {
    modal_h(420.0)
}

/// The one sentence under the title: what these bytes are, how many there are,
/// and — when it applies — that this is not all of them.
///
/// The truncation clause is not decoration. It is the difference between a
/// panel showing a value and a panel showing the front of one, and the same
/// fact disables Save below.
fn summary_line(value: &BlobValue, kind: BlobKind) -> String {
    let mut s = format!("{} · {}", kind.label(), human_bytes(value.len as i64));
    if value.truncated() {
        s.push_str(&format!(
            " · showing the first {}",
            human_bytes(FETCH_CAP as i64)
        ));
    }
    s
}

/// What each pane is called, in the switch and nowhere else.
fn pane_label(pane: BlobPane) -> &'static str {
    match pane {
        BlobPane::Preview => "Preview",
        BlobPane::Hex => "Hex",
    }
}

/// The pane switch: the app's own `<select>`, not a pair of buttons.
///
/// Two mutually exclusive views of one value is what a dropdown is for, and it
/// is what the rest of the app already uses for the shape — so this inherits the
/// whole of [`crate::settings::in_ring_picker`]'s behaviour rather than
/// approximating it: the menu flips at a window edge from a predicted height,
/// the arrows walk it, Enter picks, Escape peels one layer before the modal, and
/// the value in effect is *tinted* rather than filled, which is this menu
/// system's vocabulary for "you are holding this one".
///
/// It is built only when both panes exist (see the header), so it is never a box
/// offering one choice.
fn pane_picker(current: RwSignal<BlobPane>, ring: FocusRing, tabindex: u32) -> impl IntoView {
    container(crate::settings::focusable_dropdown(
        current,
        [BlobPane::Preview, BlobPane::Hex],
        pane_label,
        ring,
        tabindex,
    ))
    .style(|s| s.width(theme::scaled(120.0)).flex_shrink(0.0_f32))
}

/// The hex dump: one virtualized monospace line per [`HEX_COLS`] bytes.
fn hex_view(value: Arc<BlobValue>) -> impl IntoView {
    let rows = hex_row_count(value.bytes.len());
    scroll(
        virtual_stack(
            VirtualDirection::Vertical,
            VirtualItemSize::Fixed(Box::new(hex_row_h)),
            move || HexRows { len: rows },
            |i| *i,
            move |i| {
                let text = hex_line(&value.bytes, i);
                label(move || text.clone()).style(|s| {
                    s.font_family(crate::consts::MONO_FAMILY.to_string())
                        .font_size(hex_font())
                        .height(hex_row_h())
                        .color(theme::text())
                        .padding_horiz(theme::scaled(10.0))
                })
            },
        )
        .style(|s| s.flex_col().min_width_full()),
    )
    .style(move |s| s.size_full().background(theme::bg_editor()))
}

/// What the image's header says it would decode to, or `None` if it does not
/// parse as one.
///
/// **Reads the header and stops.** `into_dimensions` does not decode the image,
/// which is the point: this runs *to decide whether decoding is safe*, so it
/// must not be the thing that allocates. `with_guessed_format` sniffs the same
/// magic bytes [`schemaic_core::blob::sniff`] does, and disagreeing with it is
/// not a problem — a format one recognises and the other cannot measure comes
/// back `None`, which the verdict treats as a refusal.
fn image_dims(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// The image preview: the bytes at their natural size, in a scroll — or a
/// sentence saying why there is no preview.
///
/// **Nothing is handed to `img()` unmeasured.** floem decodes to RGBA at
/// construction, so a preview costs width × height × 4 — driven by the header's
/// claim, not by the blob's size, and `FETCH_CAP` bounds neither term. These
/// bytes came out of a database, so that claim is untrusted input on the way to
/// an allocation: a 40 KB PNG declaring 30000 × 30000 would ask for 3.6 GB.
/// [`schemaic_core::blob::preview_verdict`] is the gate, and both of its
/// refusals say so on screen rather than leaving an empty box.
///
/// **Natural size, not fitted**, because floem's `img` paints into whatever box
/// layout gives it and ignores its own `ObjectFit` — a fitted box would have to
/// scale by hand from these very dimensions, and scrolling a large image is a
/// smaller cost than showing every image stretched.
fn preview_view(value: Arc<BlobValue>, kind: BlobKind) -> impl IntoView {
    let body: floem::AnyView = match preview_verdict(image_dims(&value.bytes)) {
        PreviewVerdict::Show => scroll(container(img(move || value.bytes.clone())).style(|s| {
            s.padding(theme::scaled(12.0))
                .items_center()
                .justify_center()
        }))
        .style(|s| s.size_full())
        .into_any(),
        PreviewVerdict::TooLarge { width, height } => note_view(
            format!(
                "This image is {width} × {height} — too large to preview here. \
                 The bytes are intact: read them as Hex, or save them to a file."
            ),
            false,
        )
        .into_any(),
        PreviewVerdict::Unmeasurable => note_view(
            format!(
                "These bytes begin like a {} but cannot be read as one. \
                 Read them as Hex, or save them to a file.",
                kind.label()
            ),
            false,
        )
        .into_any(),
    };
    container(body).style(move |s| s.size_full().background(theme::bg_editor()))
}

/// A centred line, for the states with nothing to show.
fn note_view(text: String, danger: bool) -> impl IntoView {
    container(label(move || text.clone()).style(move |s| {
        if danger {
            s.color(theme::diag_error())
        } else {
            s.color(theme::text_muted())
        }
    }))
    .style(|s| {
        s.size_full()
            .items_center()
            .justify_center()
            .padding(theme::scaled(16.0))
    })
}

/// The panel's *shape* — what has to be torn down and rebuilt when it changes,
/// as opposed to what a label can simply re-read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Loading,
    Empty,
    Failed,
    Ready,
}

fn phase_of(state: &BlobState) -> Phase {
    match state {
        BlobState::Loading => Phase::Loading,
        BlobState::Empty => Phase::Empty,
        BlobState::Failed(_) => Phase::Failed,
        BlobState::Ready { .. } => Phase::Ready,
    }
}

/// The binary-cell panel, mounted in the modal layer.
pub(crate) fn blob_overlay(ui: Ui) -> impl IntoView {
    let b = ui.blob;
    let save = ui.tab_actions.save_blob.clone();
    let load = ui.tab_actions.load_blob.clone();
    let cancel = ui.tab_actions.cancel_blob.clone();

    // One decision for every exit — the ✕, the footer button and Escape — so
    // the three cannot disagree.
    //
    // **It cancels before it closes.** The epoch guard already stops a late
    // answer landing in the next panel, but that is about correctness, not
    // work: without the cancel the `SELECT` runs to completion and streams up
    // to `FETCH_CAP` to nobody, holding a connection (or the tab's pinned
    // session) busy the whole time.
    let exit: Rc<dyn Fn()> = Rc::new(move || {
        (cancel)();
        b.close();
    });

    // **A memo of the shape, not the signals themselves** — the rule
    // `widgets::overlay_open_key` states and the reason it exists.
    // `dyn_container` has no equality check of its own: floem calls `on_change`
    // on every re-run and `swap_val` then disposes the child scope and rebuilds
    // it unconditionally, so a key that read `state` and `saved` directly tore
    // the whole panel down on *every* write to either. Two costs, both real
    // here: floem clears `app_state.focus` when a view is removed, so finishing
    // a save moved focus off the button that was just pressed; and rebuilding
    // the body re-runs `img()`, which decodes the whole buffer again.
    //
    // `epoch` is this panel's session term — a second cell is a different panel
    // even when both are `Ready` — and `phase` is the only other thing that
    // changes which *views* exist. The pane deliberately is not a term: both
    // panes are built once and shown by style, so switching between them is a
    // restyle rather than a teardown, and the image is decoded once. The
    // payload behind a phase (bytes, kind, message) arrives with the change
    // that admits it, so the builder reads it untracked.
    //
    // **The phase term triggers the rebuild; it does not describe it.** The
    // builder below deliberately ignores the `phase` it is handed and reads the
    // live state, because a queued key can be one revision behind the signal by
    // the time floem processes it. Two terms changing in one turn queue *two*
    // values — the memo notifies on each — and both are built, in order, against
    // whatever the signals hold at processing time. That is harmless when the
    // builder reads the state (the first build is redundant, the second is the
    // same), and it is a panic when the builder trusts the tuple: opening the
    // panel on a NULL cell sets `target` and then reports `Empty` in the same
    // turn, so the first queued key said `Loading` over a state that was already
    // `Empty`. `BlobUi::open` takes the state it is opening in for that reason —
    // there is no transition left on that path — and this reads the state anyway,
    // so the next caller to add one cannot bring the panic back.
    let key = floem::reactive::create_memo(move |_| {
        (
            b.epoch.get(),
            b.target.with(|t| t.is_some()),
            b.state.with(phase_of),
        )
    });

    dyn_container(
        move || key.get(),
        move |(_epoch, open, _phase)| {
            if !open {
                return empty().into_any();
            }
            let Some(target) = b.target.get_untracked() else {
                return empty().into_any();
            };
            let state = b.state.get_untracked();
            let ring = FocusRing::new();
            let (exit_title, exit_btn, exit_esc) = (exit.clone(), exit.clone(), exit.clone());

            // What the bytes are, if they are here — the one value the header,
            // the body and the footer all read, so they cannot disagree about
            // whether there is anything to show.
            let ready = match &state {
                BlobState::Ready { value, kind } => Some((value.clone(), *kind)),
                _ => None,
            };

            let header: floem::AnyView = match &ready {
                Some((value, kind)) => {
                    let line = summary_line(value, *kind);
                    let show_switch = kind.is_image();
                    h_stack((
                        label(move || line.clone())
                            .style(|s| s.color(theme::text_muted()).flex_grow(1.0_f32)),
                        // The switch only exists when there are two things to
                        // switch between: an opaque blob has no preview, and a
                        // dropdown offering one option is a control that cannot
                        // do anything.
                        if show_switch {
                            pane_picker(b.pane, ring.clone(), ACTION_TAB).into_any()
                        } else {
                            empty().into_any()
                        },
                    ))
                    .style(|s| s.width_full().items_center().gap(theme::scaled(10.0)))
                    .into_any()
                }
                None => empty().into_any(),
            };

            // **Both panes are built once and toggled by style**, rather than
            // one being built per switch. `img()` decodes at construction, so
            // rebuilding the body on every Preview↔Hex would decode the whole
            // image again each way — for a view whose bytes have not moved.
            // Taffy filters `Display::None` out before layout, so the hidden
            // one costs nothing to have around, and the pane is no longer a
            // term in the panel's rebuild key.
            let shown = move |mine: BlobPane| {
                move |s: floem::style::Style| match b.pane.get() == mine {
                    true => s.size_full(),
                    false => s.display(floem::taffy::style::Display::None),
                }
            };
            let body: floem::AnyView = match &state {
                BlobState::Loading => note_view("Reading…".to_string(), false).into_any(),
                BlobState::Empty => note_view(
                    "This cell is NULL, or its row is no longer there.".to_string(),
                    false,
                )
                .into_any(),
                BlobState::Failed(msg) => note_view(msg.clone(), true).into_any(),
                // An opaque blob has no preview to build, so it does not get
                // the pair — the switch that would reach it is absent too.
                BlobState::Ready { value, kind } if kind.is_image() => stack((
                    preview_view(value.clone(), *kind).style(shown(BlobPane::Preview)),
                    hex_view(value.clone()).style(shown(BlobPane::Hex)),
                ))
                .style(|s| s.size_full().flex_col())
                .into_any(),
                BlobState::Ready { value, .. } => hex_view(value.clone()).into_any(),
            };

            // Save writes exactly what the panel is showing, so it is offered
            // only when that is the whole value. A truncated buffer would write
            // a file that is the front of a blob and looks like the blob.
            let saveable = ready.as_ref().map(|(v, _)| !v.truncated()).unwrap_or(false);
            let save_hint = match &ready {
                Some((v, _)) if v.truncated() => Some(format!(
                    "Too large to save from here — over {}.",
                    human_bytes(FETCH_CAP as i64)
                )),
                _ => None,
            };

            // **Its own container, so a finished save does not rebuild the
            // panel.** This line is the one part that changes without the shape
            // changing, and folding it into the outer key is what made
            // completing a save re-decode the image and drop keyboard focus.
            let status: floem::AnyView = dyn_container(
                move || b.note.get(),
                // `text_ellipsis` on each: the footer lets this line shrink
                // rather than push the buttons out, and a shrunk line should end
                // in a `…` rather than in a clipped word. A load failure carries
                // the OS's message, which is the one here with no length bound
                // at all.
                move |note| match (note, save_hint.clone()) {
                    (Some(Ok(msg)), _) => label(move || msg.clone())
                        .style(|s| s.color(theme::text_muted()).text_ellipsis())
                        .into_any(),
                    (Some(Err(msg)), _) => label(move || msg.clone())
                        .style(|s| s.color(theme::diag_error()).text_ellipsis())
                        .into_any(),
                    (None, Some(hint)) => label(move || hint.clone())
                        .style(|s| s.color(theme::text_muted()).text_ellipsis())
                        .into_any(),
                    (None, None) => empty().into_any(),
                },
            )
            .into_any();

            // *Load from file* is offered exactly when the grid handed over a
            // sink for the bytes — a writable cell. Unlike Save, it does not
            // depend on what was read: a cell too large to *save* whole can
            // still be replaced, and a NULL one has nothing to read and is
            // precisely where a first file goes.
            let loadable = b.stage.with_untracked(Option::is_some);
            let load_click = {
                let load = load.clone();
                let epoch = b.epoch.get_untracked();
                move || {
                    let load = load.clone();
                    open_file(
                        FileDialogOptions::new().title("Load binary value from file"),
                        move |file| {
                            let Some(path) = file.and_then(|f| f.path.first().cloned()) else {
                                return; // cancelled
                            };
                            (load)(BlobLoadRequest { path, epoch });
                        },
                    );
                }
            };

            let save_click = {
                let save = save.clone();
                let stem = target.stem.clone();
                let ready = ready.clone();
                // The opening this button belongs to, captured at build — so a
                // save that outlives its panel reports nowhere rather than into
                // whatever replaced it.
                let epoch = b.epoch.get_untracked();
                move || {
                    let Some((value, kind)) = ready.clone() else {
                        return;
                    };
                    if value.truncated() {
                        return;
                    }
                    let opts = FileDialogOptions::new()
                        .title("Save binary value")
                        .default_name(format!("{stem}.{}", kind.extension()))
                        .allowed_types(vec![FileSpec {
                            name: kind.label(),
                            extensions: kind.extensions(),
                        }]);
                    let save = save.clone();
                    save_as(opts, move |file| {
                        let Some(path) = file.and_then(|f| f.path.first().cloned()) else {
                            return; // cancelled
                        };
                        (save)(BlobSaveRequest {
                            path,
                            bytes: value.clone(),
                            epoch,
                        });
                    });
                }
            };

            let panel = v_stack((
                modal_title_owned(target.title.clone(), exit_title, ring.clone()),
                v_stack((
                    header,
                    container(body).style(move |s| {
                        s.width_full()
                            .height(body_h())
                            .border(1.0)
                            .border_color(theme::border())
                            .border_radius(6.0)
                    }),
                ))
                .style(|s| {
                    s.flex_col()
                        .width_full()
                        .gap(theme::scaled(10.0))
                        .padding_horiz(modal_pad_h())
                        .padding_vert(theme::scaled(14.0))
                }),
                modal_footer_split(
                    status,
                    h_stack((
                        action_button(
                            "Load from file",
                            ActionKind::Neutral,
                            loadable,
                            ring.clone(),
                            ACTION_TAB + 1,
                            load_click,
                        ),
                        action_button(
                            "Save to file",
                            ActionKind::Neutral,
                            saveable,
                            ring.clone(),
                            ACTION_TAB + 2,
                            save_click,
                        ),
                        action_button(
                            "Close",
                            ActionKind::Primary,
                            true,
                            ring.clone(),
                            ACTION_TAB + 3,
                            move || (exit_btn)(),
                        ),
                    ))
                    .style(|s| s.gap(theme::scaled(8.0)))
                    .into_any(),
                )
                .into_any(),
            ))
            .on_click_stop(|_| {})
            .style(move |s| panel_style(s).width(panel_w()));

            focus_root_with_ring(container(panel), ring)
                .on_key_down(
                    Key::Named(NamedKey::Escape),
                    |_| true,
                    move |_| (exit_esc)(),
                )
                .style(|s| {
                    s.size_full()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .background(theme::modal_backdrop())
                })
                .into_any()
        },
    )
    .style(move |s| {
        if b.target.get().is_some() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A target with no size cap — the shape every test here wants, and a
    /// constructor rather than a literal so a new field on `BlobTarget` does not
    /// have to be spelt out in a dozen fixtures that do not care about it.
    fn tgt(title: String, stem: String) -> BlobTarget {
        BlobTarget {
            title,
            stem,
            cap: None,
        }
    }

    fn value(len: u64, bytes: usize) -> BlobValue {
        BlobValue {
            bytes: vec![0; bytes],
            len,
        }
    }

    /// **A fetch that outlived its panel reports nowhere.**
    ///
    /// The panel takes the keyboard but not the clock: Escape closes it with the
    /// `SELECT` still out, and the cell opened next is a different panel. Asserted
    /// through `open`/`close`/`loaded` rather than on the epoch comparison alone —
    /// the comparison was never the part that could be wrong, the question is
    /// whether `close` moves the epoch at all.
    #[test]
    fn a_report_from_a_closed_panel_cannot_land_in_the_next_one() {
        let ui = BlobUi::new();
        let first = ui.open(
            tgt("staff.picture".into(), "staff_picture_1".into()),
            None,
            BlobState::Loading,
        );
        ui.close();
        let second = ui.open(
            tgt("staff.password".into(), "staff_password_1".into()),
            None,
            BlobState::Loading,
        );
        assert_ne!(first, second, "each opening needs its own epoch");

        // The first cell's bytes arrive, late.
        ui.loaded(
            first,
            BlobState::Ready {
                value: Arc::new(value(4, 4)),
                kind: BlobKind::Png,
            },
        );
        assert!(
            matches!(ui.state.get_untracked(), BlobState::Loading),
            "the first fetch reported into the second panel"
        );
        // And it must not have dragged the pane over to a preview either.
        assert_eq!(ui.pane.get_untracked(), BlobPane::Hex);

        // The second cell's own report still lands.
        ui.loaded(
            second,
            BlobState::Ready {
                value: Arc::new(value(4, 4)),
                kind: BlobKind::Opaque,
            },
        );
        assert!(matches!(ui.state.get_untracked(), BlobState::Ready { .. }));
    }

    /// The same guard on the save's report, which is slower and so likelier to
    /// outlive its panel.
    #[test]
    fn a_save_from_a_closed_panel_cannot_claim_the_next_one_wrote_a_file() {
        let ui = BlobUi::new();
        let first = ui.open(tgt("a.b".into(), "a_b".into()), None, BlobState::Loading);
        ui.close();
        let second = ui.open(tgt("c.d".into(), "c_d".into()), None, BlobState::Loading);

        ui.saved_at(first, Ok("C:/tmp/a_b.png".into()));
        assert_eq!(
            ui.note.get_untracked(),
            None,
            "a file saved from the previous panel was reported under this one"
        );
        ui.saved_at(second, Ok("C:/tmp/c_d.bin".into()));
        assert_eq!(
            ui.note.get_untracked(),
            Some(Ok("Saved to C:/tmp/c_d.bin".into()))
        );
    }

    /// **The epoch guard's one case that writes to a database.**
    ///
    /// Every other superseded report here lands in a label — a stale "Saved to
    /// …", a stale image. A stale *load* calls the staging closure, and the
    /// closure the panel is holding aims at whatever cell it was last opened on.
    /// So: pick a file, press Escape, open a different cell, and the read
    /// completes — the bytes must not be staged into the second cell, and must
    /// not be staged into the first either, since the panel that asked is gone.
    #[test]
    fn a_file_read_that_outlived_its_panel_stages_into_nothing() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let ui = BlobUi::new();
        let into_first: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let into_second: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = |log: &Rc<RefCell<Vec<Vec<u8>>>>| -> crate::BlobStageFn {
            let log = log.clone();
            Rc::new(move |b: Vec<u8>| {
                log.borrow_mut().push(b);
                true
            })
        };

        let first = ui.open(
            tgt("a.b".into(), "a_b".into()),
            Some(sink(&into_first)),
            BlobState::Loading,
        );
        ui.close();
        ui.open(
            tgt("c.d".into(), "c_d".into()),
            Some(sink(&into_second)),
            BlobState::Loading,
        );

        ui.loaded_file(first, Ok(vec![1, 2, 3]));
        assert!(
            into_first.borrow().is_empty() && into_second.borrow().is_empty(),
            "a file chosen in a panel that is gone was staged anyway"
        );
        assert_eq!(ui.note.get_untracked(), None, "and said nothing about it");
    }

    /// **A panel with nothing to fetch is `Empty` from the first frame.**
    ///
    /// It used to be told a line later, and the two lines are one turn apart —
    /// close enough that the rebuild queued by `open`'s own `target.set` carried
    /// a `Loading` phase over a state that was already `Empty`, which the panel's
    /// builder asserted against and panicked on. Clicking *Edit binary* on a
    /// NULL cell was the whole reproduction.
    ///
    /// Asserted through `open` rather than on the ordering inside it, because
    /// the ordering was never the part that could be wrong: the question is
    /// whether a caller can put the panel in a state its own rebuild key
    /// disagrees with, and the answer is that it no longer has the two steps
    /// needed to try.
    #[test]
    fn a_panel_with_nothing_to_fetch_opens_empty_in_one_step() {
        let ui = BlobUi::new();
        ui.open(
            tgt("staff.picture".into(), "staff_picture_1".into()),
            None,
            BlobState::Empty,
        );
        assert!(
            matches!(ui.state.get_untracked(), BlobState::Empty),
            "the panel must be born in the state it is in"
        );
        // The rebuild key's phase term and the state the builder reads are the
        // same fact here, which is the whole property: there is no moment
        // between them for a queued key to be stale about.
        assert_eq!(ui.state.with_untracked(phase_of), Phase::Empty);
        assert!(ui.target.with_untracked(Option::is_some), "and it is up");
    }

    /// The load's whole effect, in one place: the bytes are staged into the cell
    /// the panel is open on, the panel shows *them* rather than what it read,
    /// and the sentence says **staged** — not saved, not written.
    ///
    /// The epoch moving is part of the effect and is asserted here rather than on
    /// its own: the panel's rebuild key is `(epoch, open, phase)`, and replacing
    /// one `Ready` value with another changes no phase — so without the bump the
    /// body is never rebuilt and the old image stays on screen above the new
    /// bytes' status line.
    #[test]
    fn loading_a_file_stages_it_shows_it_and_says_it_is_not_written_yet() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let ui = BlobUi::new();
        let staged: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = {
            let staged = staged.clone();
            Rc::new(move |b: Vec<u8>| {
                staged.borrow_mut().push(b);
                true
            }) as crate::BlobStageFn
        };
        let epoch = ui.open(
            tgt("staff.picture".into(), "staff_picture_1".into()),
            Some(sink),
            BlobState::Loading,
        );
        // A one-pixel PNG, so `sniff` has something real to answer about and the
        // pane choice is the image one rather than a default.
        let png = bomb_png(1, 1);
        ui.loaded_file(epoch, Ok(png.clone()));

        assert_eq!(
            &*staged.borrow(),
            std::slice::from_ref(&png),
            "staged byte for byte"
        );
        match ui.state.get_untracked() {
            BlobState::Ready { value, kind } => {
                assert_eq!(value.bytes, png);
                assert_eq!(value.len, png.len() as u64);
                assert!(kind.is_image());
            }
            other => panic!("the panel should show what was loaded, got {other:?}"),
        }
        assert_eq!(ui.pane.get_untracked(), BlobPane::Preview);
        assert_ne!(
            ui.epoch.get_untracked(),
            epoch,
            "the body must be rebuilt, or the previous value stays on screen"
        );
        let note = ui.note.get_untracked().expect("a status line").expect("ok");
        assert!(
            note.contains("Commit"),
            "the line must not read as a write: {note}"
        );
    }

    /// **The oversized file is refused here, and nothing is staged.**
    ///
    /// A 137 KB JPEG into `sakila.staff.picture` — a `BLOB`, so 64 KiB — is the
    /// report this came from. The server does refuse it, but at the commit:
    /// `ERROR 1406: Data too long` names the column rather than the file,
    /// arrives after this panel has closed, and rolls back every other staged
    /// edit in the same batch. Refusing at the point the file was chosen costs
    /// one comparison and keeps the batch intact.
    ///
    /// Both halves matter and both are asserted: the message has to name the two
    /// sizes (a bare "too large" leaves the user guessing which file would fit),
    /// and the sink must not have been called — a panel that says no while
    /// staging anyway is the worst of the three outcomes.
    #[test]
    fn a_file_over_the_columns_cap_is_refused_before_anything_is_staged() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let ui = BlobUi::new();
        let staged: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = {
            let staged = staged.clone();
            Rc::new(move |b: Vec<u8>| {
                staged.borrow_mut().push(b);
                true
            }) as crate::BlobStageFn
        };
        let epoch = ui.open(
            BlobTarget {
                title: "staff.picture".into(),
                stem: "staff_picture_1".into(),
                cap: Some(65_535), // a MySQL `BLOB`
            },
            Some(sink),
            BlobState::Empty,
        );

        ui.loaded_file(epoch, Ok(vec![0u8; 140_000]));
        assert!(
            staged.borrow().is_empty(),
            "an oversized file was staged despite the refusal"
        );
        let msg = ui
            .note
            .get_untracked()
            .expect("a status line")
            .expect_err("a refusal");
        assert!(
            msg.contains("136.7 KB") && msg.contains("64.0 KB"),
            "the message must name both sizes, got {msg:?}"
        );
        // And the panel still shows what is really in the column.
        assert!(
            matches!(ui.state.get_untracked(), BlobState::Empty),
            "a refused load must not replace what the panel is showing"
        );

        // Exactly at the cap is a fit, not an overrun — the comparison is `>`,
        // and an off-by-one here refuses a file the column takes.
        ui.loaded_file(epoch, Ok(vec![0u8; 65_535]));
        assert_eq!(staged.borrow().len(), 1, "a file of exactly the cap fits");
    }

    /// No cap is "no answer", not "no limit" — an unloaded schema and a `bytea`
    /// give the same `None`, and the server stays the authority. A file that
    /// large is not something this panel refuses on its own guess.
    #[test]
    fn with_no_cap_the_panel_refuses_nothing() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let ui = BlobUi::new();
        let staged: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = {
            let staged = staged.clone();
            Rc::new(move |b: Vec<u8>| {
                staged.borrow_mut().push(b);
                true
            }) as crate::BlobStageFn
        };
        let epoch = ui.open(
            tgt("t.payload".into(), "t_payload".into()),
            Some(sink),
            BlobState::Empty,
        );
        ui.loaded_file(epoch, Ok(vec![0u8; 5_000_000]));
        assert_eq!(staged.borrow().len(), 1);
    }

    /// **A sink that refuses is said out loud.** The file dialog stands open for
    /// as long as the user takes to choose, and the grid is free to move
    /// underneath it — the pending row discarded, the row marked for deletion,
    /// the result re-run read-only. The panel used to call the sink and report
    /// success regardless, which for those cases is a sentence promising a write
    /// that will never happen.
    #[test]
    fn a_sink_that_refuses_is_reported_rather_than_dressed_as_a_load() {
        let ui = BlobUi::new();
        let refusing: crate::BlobStageFn = Rc::new(|_: Vec<u8>| false);
        let epoch = ui.open(
            tgt("t.payload".into(), "t_payload".into()),
            Some(refusing),
            BlobState::Empty,
        );
        ui.loaded_file(epoch, Ok(vec![1, 2, 3]));
        assert!(
            matches!(ui.note.get_untracked(), Some(Err(_))),
            "a refused stage must not read as a load"
        );
        assert!(
            matches!(ui.state.get_untracked(), BlobState::Empty),
            "and must not replace what the panel is showing"
        );
    }

    /// A read-only cell hands over no sink, and the panel refuses rather than
    /// dropping the file quietly — a load that silently did nothing looks exactly
    /// like one that worked.
    #[test]
    fn a_load_into_a_panel_with_nowhere_to_put_it_says_so() {
        let ui = BlobUi::new();
        let epoch = ui.open(
            tgt("v.blob".into(), "v_blob".into()),
            None,
            BlobState::Loading,
        );
        ui.loaded_file(epoch, Ok(vec![9]));
        assert!(
            matches!(ui.note.get_untracked(), Some(Err(_))),
            "a refusal, not silence"
        );
    }

    /// `open` resets the sink along with everything else. Carrying the previous
    /// cell's over would aim *Load from file* at a row the panel is no longer
    /// about — a wrong write rather than a wrong sentence.
    #[test]
    fn opening_another_cell_does_not_inherit_the_previous_sink() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let ui = BlobUi::new();
        let staged: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = {
            let staged = staged.clone();
            Rc::new(move |b: Vec<u8>| {
                staged.borrow_mut().push(b);
                true
            }) as crate::BlobStageFn
        };
        ui.open(
            tgt("a.b".into(), "a_b".into()),
            Some(sink),
            BlobState::Loading,
        );
        // The second cell is read-only, so it brings no sink of its own.
        let second = ui.open(tgt("c.d".into(), "c_d".into()), None, BlobState::Loading);
        assert!(ui.stage.with_untracked(Option::is_none));
        ui.loaded_file(second, Ok(vec![7]));
        assert!(
            staged.borrow().is_empty(),
            "the first cell took the second cell's file"
        );
    }

    /// Opening a second cell clears the first one's save sentence.
    #[test]
    fn opening_another_cell_does_not_inherit_the_previous_saved_line() {
        let ui = BlobUi::new();
        let first = ui.open(tgt("a.b".into(), "a_b".into()), None, BlobState::Loading);
        ui.saved_at(first, Ok("C:/tmp/a_b.png".into()));
        assert!(ui.note.get_untracked().is_some());
        ui.open(tgt("c.d".into(), "c_d".into()), None, BlobState::Loading);
        assert_eq!(ui.note.get_untracked(), None);
    }

    // ---- the preview gate, measured -----------------------------------------

    /// PNG's CRC-32 (ISO-HDLC), so [`bomb_png`] can rewrite a chunk and stay a
    /// valid PNG. Table-less: it runs over seventeen bytes, once.
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// A **valid** PNG whose IHDR declares `w` x `h`, built by encoding a real
    /// 1x1 image and rewriting that chunk.
    ///
    /// Rewritten rather than encoded at size, for the obvious reason: encoding
    /// 65535 x 65535 is the very allocation the gate exists to prevent. The CRC
    /// is recomputed because a decoder that rejected the chunk would make this
    /// test pass for the wrong reason — `Unmeasurable` is also a refusal, and
    /// the case under test is the one where the header parses perfectly.
    fn bomb_png(w: u32, h: u32) -> Vec<u8> {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("encode a 1x1 png");
        let mut png = png.into_inner();
        // 8-byte signature, then the IHDR chunk: length, type, 13 bytes of
        // data, CRC. Width and height are the first eight bytes of the data.
        png[16..20].copy_from_slice(&w.to_be_bytes());
        png[20..24].copy_from_slice(&h.to_be_bytes());
        let crc = crc32(&png[12..29]);
        png[29..33].copy_from_slice(&crc.to_be_bytes());
        png
    }

    /// **The bomb, end to end.** `preview_verdict` has a table of pure tests,
    /// but the property that protects the app is the *composition*: bytes out
    /// of a database, measured, and refused before anything decodes them.
    ///
    /// The fixture is a genuinely valid PNG — 70 bytes on the wire, declaring
    /// 65535 x 65535. That is 4.29 gigapixels against a 32-megapixel cap, and
    /// handing it to `img()` instead asks for 17 GB of RGBA. Nothing about the
    /// blob's *size* could have caught it.
    #[test]
    fn a_tiny_valid_header_claiming_enormous_dimensions_is_refused_before_decoding() {
        let bomb = bomb_png(65_535, 65_535);
        assert!(
            bomb.len() < 200,
            "the fixture is small: {} bytes",
            bomb.len()
        );
        assert_eq!(
            image_dims(&bomb),
            Some((65_535, 65_535)),
            "the header must measure cleanly — a refusal for being unreadable              would pass this test without exercising the cap"
        );
        assert_eq!(
            preview_verdict(image_dims(&bomb)),
            PreviewVerdict::TooLarge {
                width: 65_535,
                height: 65_535
            }
        );
    }

    /// The same fixture just under the cap is shown, so the refusal above is
    /// the *cap* talking and not the rewriting.
    #[test]
    fn the_same_fixture_within_budget_is_shown() {
        let ok = bomb_png(4_000, 4_000);
        assert_eq!(image_dims(&ok), Some((4_000, 4_000)));
        assert_eq!(preview_verdict(image_dims(&ok)), PreviewVerdict::Show);
    }

    /// An ordinary image measures and passes. Encoded here rather than pasted
    /// as a byte array so the test states its own dimensions.
    #[test]
    fn a_real_image_measures_and_is_shown() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(3, 2)
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("encode a 3x2 png");
        let bytes = png.into_inner();
        assert_eq!(image_dims(&bytes), Some((3, 2)));
        assert_eq!(preview_verdict(image_dims(&bytes)), PreviewVerdict::Show);
    }

    /// **Unmeasurable is a refusal, not a shrug.** `sniff` matches magic bytes
    /// and stops, so a truncated or corrupt PNG still reads as one; floem would
    /// then decode nothing and draw nothing, leaving a caption over an empty
    /// box. The panel says so instead.
    #[test]
    fn bytes_that_only_look_like_an_image_are_not_previewed() {
        // A PNG signature with no IHDR behind it — exactly what `sniff` calls a
        // PNG.
        let truncated: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        assert_eq!(
            schemaic_core::blob::sniff(truncated),
            BlobKind::Png,
            "the premise: this is what the header claims"
        );
        assert_eq!(image_dims(truncated), None);
        assert_eq!(
            preview_verdict(image_dims(truncated)),
            PreviewVerdict::Unmeasurable
        );
    }

    #[test]
    fn the_summary_names_the_kind_and_the_whole_size() {
        let line = summary_line(&value(36_365, 36_365), BlobKind::Png);
        assert_eq!(line, "PNG image · 35.5 KB");
    }

    /// **A truncated blob says so in the same sentence that gives its size**,
    /// because the size shown is the *value's* and the bytes on screen are not
    /// all of it — a summary that reported only the buffer would be a smaller,
    /// entirely plausible number with nothing to contradict it.
    #[test]
    fn the_summary_says_when_it_is_showing_only_the_front() {
        let line = summary_line(&value(200 * 1024 * 1024, FETCH_CAP), BlobKind::Opaque);
        assert!(line.starts_with("Binary data · 200.0 MB"), "{line}");
        assert!(line.contains("showing the first 64.0 MB"), "{line}");
    }

    #[test]
    fn an_untruncated_blob_says_nothing_about_showing_a_front() {
        let line = summary_line(&value(64, 64), BlobKind::Jpeg);
        assert!(!line.contains("showing"), "{line}");
    }

    /// The pane a fetch opens on follows the content: an image lands on its
    /// preview, anything else on the hex it can actually read.
    #[test]
    fn hex_rows_cover_the_buffer_the_view_will_index() {
        // The virtual list's length and the line builder must agree, or the
        // last line is either missing or blank.
        for n in [0usize, 1, 15, 16, 17, 4096] {
            let bytes = vec![0u8; n];
            let rows = hex_row_count(bytes.len());
            assert_eq!(rows, n.div_ceil(HEX_COLS));
            for r in 0..rows {
                assert!(!hex_line(&bytes, r).is_empty(), "{n} bytes, row {r}");
            }
        }
    }
}
