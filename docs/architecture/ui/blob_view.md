# `ui/blob_view.rs`

`blob_view.rs` — the **binary-cell panel** (`blob_overlay`), over `core::blob`: what a
`<n bytes>` cell looks like when you open it. Raised from the grid's cell menu and mounted in
the modal layer's **workspace** group beside the properties, monitor and ERD modals — it is a
question asked about the *result on screen* rather than about the schema tree — and counted in
`workspace_modals_up`, without which its backdrop would resolve `inset(0)` against nothing.
Its content does not come from the result: `BlobUi`'s `state` is filled by the app's
`ViewBlobFn` — `(conn_id, Option<BlobRef>, BlobTarget, Option<BlobStage>)`, the `BlobRef`
absent for a cell with nothing committed to read and the stage fn absent for one nothing can
write — and the three things that can arrive each get a sentence: the bytes, `Empty`
(the cell is NULL *or* the row is gone *or* the row is not committed yet, one state because the
panel's honest sentence is the same and the difference is not one this can establish), or a
failure. There is deliberately no
fourth "not fetched yet" state: the fetch starts as the modal opens, so `Loading` is what a
panel with something to read is born in. **`BlobUi::open` takes the state it opens in**
(`open(target, stage, state)`) rather than always `Loading`, because a cell with a `None`
`BlobRef` — a NULL cell, a pending row — has nothing to fetch and the app answers `Empty` for it
without a round trip. Saying so a line *later* was a panic: the two calls are one turn apart, so
the rebuild queued by `open`'s own `target.set` carried a `Loading` phase over a state that was
already `Empty`, and the builder asserted the two agreed. Born in the final state there is no
transition to disagree about, and it is the truer statement anyway — the panel knows. Pinned by
`a_panel_with_nothing_to_fetch_opens_empty_in_one_step`, asserted through `open` rather than on
the ordering inside it: the question is whether a caller can still put the panel in a state its
own rebuild key disagrees with. **`BlobUi::open` resets every field, not the two that obviously
change** — a
second cell opened after a first left `note` holding the first one's "Saved to …" under a
title naming the second — and **which pane opens is decided in `loaded`, not in the view**,
since a view that picked it would re-pick it on every rebuild and drag the user back off the
pane they switched to. The hex dump is virtualized on the results grid's own
`VirtualVector<usize>` shape (a capped fetch is still four million lines); the panel's width
is the wider of a comfortable modal and what a `hex_line` actually measures, because a dump that
wraps stops being a dump. The header names the kind and the whole value's size through
`format::human_bytes`, and adds *showing the first N* when the fetch was capped — the
same fact that disables **Save to file**, since a save of a truncated buffer writes a file that
is the front of a blob and looks like the blob. **Both sentences report the buffer's own length
rather than `FETCH_CAP`**, which is the cut point only on PostgreSQL and SQLite: MySQL's
`SELECT` asks for `SUBSTRING(col, 1, LEAST(?, PACKET_ROOM))`, the room being
`max_allowed_packet − 1 MiB` — measured 15,728,640 on stock MariaDB 10.11 and 66,060,288 on
MySQL 8.4, the second *below* `FETCH_CAP` on that engine's default, so it is not a
misconfiguration. Naming the constant made a 20 MiB `LONGBLOB` read *"Binary data · 20.0 MB ·
showing the first 64.0 MB"* — a 20 MB value cannot have a 64 MB front — and made `save_offer`
refuse it as "over 64.0 MB", false about the value and pointing at a limit the user cannot
change instead of the one they can. Nothing here can learn *why* it was cut, since `PACKET_ROOM`
is evaluated inside the statement and never crosses back, but it does not need to: how much
arrived is the fact the reader wants, and it is `bytes.len()`. `save_offer` still reads the
**server's** length for the yes/no, which is what keeps it right for a value that happens to be
exactly `FETCH_CAP` long. The Preview/Hex switch is a **dropdown**
(`pane_picker` → `settings::focusable_dropdown`), not a pair of buttons: two mutually exclusive
views of one value is what the app's `<select>` is for, and going through `in_ring_picker`
inherits the edge-flipping, the arrow keys, the Enter/Escape peel and the tint-not-fill
vocabulary rather than approximating them. It exists only for bytes that are an image — a
dropdown offering one option is a control that cannot do anything. **Both panes are built once
and switched by style**, not rebuilt per switch: `img()` decodes at construction, so a rebuild
each way would decode the whole image again for a view whose bytes have not moved, and taffy
filters `Display::None` out before layout so the hidden one costs nothing to keep — which is
also why `pane` is not a term in the panel's rebuild key.
**The preview's container needs `min_size_full`, and the centring on it does nothing without
that.** A `scroll` measures its child against unbounded space, so the container inside it hugged
the image and `items_center().justify_center()` had nothing to distribute: a 16 × 16 favicon sat
in the top-left corner of a 420 px pane. The *minimum* and not `size_full`, because the pane
shows an image at its natural size — a minimum fills the viewport while the image is smaller
than it and grows past it when the image is larger, where a fitted `size_full` would cap the
child at the viewport and leave nothing to scroll. `hex_view` one pane over has carried
`min_width_full` for the same reason all along.
**Nothing reaches `img()` unmeasured.** The renderer decodes to RGBA, so a preview costs
width x height x 4 — driven by what the *header claims*, not by what the blob weighs, and
`FETCH_CAP` bounds neither term. These bytes came out of a database, so that claim is untrusted
input on the way to an allocation: a 70-byte PNG can declare 65535 x 65535 and ask for 17 GB.
`blob_view::image_dims` reads the header and stops (`ImageReader::into_dimensions`, never a
decode) and `core::blob::preview_verdict` rules on it against `PREVIEW_PIXEL_CAP`, 32 megapixels
— ~128 MB of RGBA, above any image a cell realistically holds.
**There are two caps, they bound different quantities, and the renderer's real constraint is the
second one.** `PREVIEW_EDGE_CAP` is 4096 along *either* edge, asked **before** the area and
folded into the same `TooLarge { width, height }` arm, since the panel says the same sentence
either way and the dimensions are what a reader can act on. The area cap bounds what the decode
allocates; the edge bounds what the GPU can be asked for — floem's images live in the same atlas
as its glyphs, that atlas grows to `2 × max(width, height)` with no clamp of its own, wgpu's
default `max_texture_dimension_2d` is 8192, and no floem crate installs an `on_uncaptured_error`
handler, so a `create_texture` that fails validation ends the **process** rather than the
preview. An ordinary 6000 × 4000 photograph is 24 megapixels — comfortably *inside* the pixel cap
and 12000 past the atlas limit — so an image can pass either cap and fail the other, and both are
checked. 4096 and not 8192 because the atlas doubles the larger edge, and adapters reporting less
than the default limit exist. At 4096 the area cap is unreachable behind the edge cap, and that
is **asserted rather than commented** —
`the_area_cap_sits_behind_the_edge_cap_at_these_numbers` — so an edge cap raised to 5,657 or
beyond, where its square first exceeds 32 megapixels, fails a test rather than quietly bringing
the second check back to life unread. Every one of its refusals *says so*
rather than leaving an empty box, and `Unmeasurable` is one of them: `sniff` matches magic bytes
and stops, so bytes that are not the image it took them for still reach the preview, and floem
would decode nothing and draw nothing under a caption naming the format.
**`Truncated` is the third arm, and it is asked *before* the dimensions because the dimensions
are fine.** A PNG's IHDR is bytes 16..24 and a JPEG's SOF is near the top, so a 16 MiB scan cut
at 15 MiB by MySQL's packet room still measures 3000 × 2400 and still passes every cap:
`Unmeasurable` catches a corrupt *header* and cannot catch a missing *body*, which is the case
the app produces itself (`BlobValue::truncated`). floem's `img` is `load_from_memory(..).ok()`
with the width defaulting to `0`, so the decode error becomes a 0 × 0 image and an empty `Blob` —
a blank pane with no explanation, on the pane the panel chose for the user, which is exactly the
outcome `Unmeasurable` exists to prevent reached by the one route it did not cover. A decoder
that renders what it has, as some JPEG decoders do, is no better: a partial image with nothing
saying so is worse than a sentence. So `preview_verdict` takes `truncated` as a second parameter
and answers it first, and every other check in it is about an image these bytes do not contain.
The measurement is the UI's because it needs a
decoder; the decision is core's, pure and tested, and the composition is pinned by a fixture
that rewrites a real PNG's IHDR rather than encoding one at size — encoding it would be the
very allocation the gate exists to prevent.
The modal owns the save dialog and nothing more — `BlobSaveFn` does the write on a blocking
worker (a 64 MiB `fs::write` on the UI thread is a frozen window, the ERD export's reason), and
the `BlobRef` that addresses the row stays in the app with the connection it has to be asked
over, so two places are never responsible for aiming one fetch.
**The panel is a write surface now, not only a viewer.** `BlobUi::stage` holds where a loaded
file goes: a `BlobStage` the **grid** supplies, because the grid is what holds the staged
edit and this panel knows the cell only well enough to *name* it — giving it the coordinates
would put one cell's identity in two places, which `BlobTarget` already refuses for the read
half. `None` there is a cell nothing can write, and it is what leaves *Load from file*
disabled. The two file actions sit at the footer's **left**, *Load from file* (upload icon)
then *Save to file* (download icon), with the status line after them as the part that
shrinks, and *Close* alone on the right — they act on the value, Close on the panel; the
three take `ACTION_TAB + 1/2/3`, left to right.
**It is a struct with two closures — `put` and `live` — and not a bare `Rc<dyn Fn(Arc<[u8]>) ->
bool>`**, because `put` asks `is_live` inside itself and so no caller can skip the check. The
signal holding it is window-scoped while the sink closes over a `GridState`, so the two do not
die together, and floem's `get_untracked` is `try_get_untracked().unwrap()`: a stale sink is a
panic that takes the window and every tab's uncommitted edits, not a no-op. The *offer* is
guarded too — `loadable` asks `is_live` rather than merely `is_some`, and `stage` is a term in
the panel's rebuild key, which it was not: the one thing that clears the signal after the panel
is up (the grid behind it being disposed) left the button enabled over a sink that could no
longer take anything, and the user waited out a file dialog and a read of up to 64 MiB to be told
the cell could not be written.
The panel owns the open dialog and the app does the read: `BlobLoadFn`
(`TabActions::load_blob`, a `std::fs::read` on a blocking worker) is `BlobSaveFn` in reverse and
off-thread for the same reason — the file can be as large as the value it replaces.
Unlike Save it does not depend on what was *read*: a value too large to save whole can still be
replaced, and a NULL cell has nothing to read and is exactly where a first file goes. `stage` is
reset by `open`/`close` like every other field, and for a sharper reason than the status line —
carrying the previous cell's sink over would aim the button at a row the panel is no longer
about, a wrong write rather than a wrong sentence. `BlobUi::loaded_file(epoch, …)` takes a
`Result<Vec<u8>, String>` — the file's bytes or why they could not be read, and **not** its
name. The status line sits under a title that already names the cell and has to fit beside three
buttons, so it spends its words on *Loaded file. Commit in the grid to write it.* and the name
is dropped rather than threaded to a parameter nothing reads. It stages the bytes, shows them,
and says **staged**: they are in the grid's
dirty map like any typed edit and reach the table on Commit, so a line reading "Loaded" would
read as a save. It is epoch-guarded like every other report here, and it is the one in the
family whose late arrival would write to a *database* rather than to a label; a load arriving
with no sink is refused in words rather than dropped, because doing nothing quietly looks
exactly like a load that worked.
**`put` takes an `Arc<[u8]>`, not a `Vec<u8>`, because the seam forced a copy.** The panel read
the file into a `Vec`, cloned it to hand over, and `CellEdit::bytes` built its `Arc<[u8]>` by
copying *that* — three buffers live at once for one cell, which at the picker's 64 MiB cap is
three 64 MiB buffers on the path a `LONGBLOB` column exists for. `BlobState::Ready` had already
chosen the `Arc` for this reason ("the buffer is up to `FETCH_CAP` and every read of the signal
would otherwise copy it"); the clone one screen down reintroduced the copy before the `Arc` was
built. The panel now builds the `Arc` once and shares it, so the middle buffer goes and the peak
is two rather than three. The remaining pair is `BlobValue::bytes`, still a `Vec`, and the grid's
staged `Arc` — collapsing those wants `BlobValue` to hold an `Arc<[u8]>` too, which is a wider
change and has not been made.
**And the sink itself can refuse**: `BlobStage::put` returns a `bool`, and the panel says which
way it went. The file dialog stands open for as long as the user takes to choose and the grid is free
to move underneath it — the pending row discarded, the row marked for deletion, the result re-run
read-only — so the gate `blob_launch` passed is not necessarily the one that holds when the bytes
land, and `GridState::stage_bytes` asks it again (see the grid). Reporting a refusal as *Loaded
file.* is the worst of the outcomes available: it is indistinguishable from a write that will
happen, and the difference is a column that does or does not get written. A refusal replaces
nothing —
the panel goes on showing the value that is still there — and the note says so, both halves
pinned by `a_sink_that_refuses_is_reported_rather_than_dressed_as_a_load`.
**An oversized file is refused here, before anything is staged**, against `BlobTarget::cap` —
the column's declared capacity, which `grid::blob_launch` reads off `EditModel::byte_cap`. The
message names both sizes ("That file is 136.7 KB — this column holds at most 64.0 KB.") because
the column's own limit is a fact the user has no other way to learn, and it renders the pair
through **`format::contrasting_bytes`** rather than `human_bytes` twice: a 65,536-byte file
refused by a `BLOB` (65,535) read *"That file is 64.0 KB — this column holds at most 64.0 KB."*,
one decimal being right for a size shown alone and wrong for a sentence whose whole content is
that the two differ. The alternative is the
server's, and it is much worse placed: MySQL's `ERROR 1406: Data too long` arrives at the
commit, names the column rather than the file, lands after this panel has closed, and takes
every other staged edit in the batch down with the rollback. The comparison is `>`, so a file of
exactly the cap fits, and a `None` cap refuses nothing — "no answer" is not "no limit", and the
server remains the authority behind it
(`a_file_over_the_columns_cap_is_refused_before_anything_is_staged`,
`with_no_cap_the_panel_refuses_nothing`). That is the *column's* limit and it is the only one
this panel knows: the app's `load_blob` applies `blob::LOAD_CAP` before it reads the file at all,
and its refusal arrives here as an ordinary load failure. Both sentences, and the sink's, land in
the same status line as any other error.
`saved` became `note` because there is one line under the body
and both reports use it — they are never in flight at once, a load being precisely what makes a
stale "Saved to …" a sentence about a file that is no longer what the panel is showing — and
`saved_at` now formats its own "Saved to {path}". All three spellings of that line — the note,
the failure, the save hint — carry `text_ellipsis`, because it shares the footer with three
buttons: `widgets::modal_footer_split` is what lets the slot shrink instead of pushing them out,
and the ellipsis is the caller's half of that bargain. A load failure carries the OS's message,
which is the one with no length bound at all.
**The `dyn_container` key is a memo of the panel's *shape*, not of its signals** — the rule
`widgets::overlay_open_key` states, and this panel is where breaking it costs most. A key that
read `state` and `note` directly rebuilt everything on every write to either, which finished a
save by clearing focus off the button just pressed (floem drops `app_state.focus` when a view is
removed) and re-ran `img()` over the whole buffer. The memo is `(epoch, open, phase)` —
`epoch` is the session term, since a second cell is a different panel even when both are
`Ready` — and the status line has a container of its own so a save's outcome moves nothing else.
**The phase term is there to *trigger* the rebuild, not to describe it**, and the builder
accordingly ignores the `phase` it is handed and reads the live state. A queued key can be one
revision behind the signals: two terms written in one turn queue two values — the memo notifies
on each — and floem builds both, in order, against whatever the signals hold at processing time.
Reading the state makes the first of those a redundant rebuild with the right content. Trusting
the tuple made it a `debug_assert_eq!(phase_of(&state), phase)` that *Edit binary* on a NULL cell
tripped, and that assert is gone. `open` taking its state closed the one path that produced a
stale key here; the builder reading the state is what stops the next caller to add one from
bringing the panic back.
**That key is why `loaded_file` writes in the order it does.** A file loaded over the bytes
already on screen is `Ready` → `Ready`, which changes no `phase` and no `open`, so the epoch
bump is the only thing that gets a rebuild at all: without it the panel goes on showing the
*old* image over the new bytes' status line. And the bump goes **last**, because the builder
reads the payload untracked — bumping first rebuilds the body against the value being replaced,
and the `state.set` behind it then changes no term and so rebuilds nothing.
**Every exit cancels before it closes** (`TabsActions::cancel_blob`): the epoch guard already
stops a late answer landing in the next panel, but that is about correctness rather than work,
and an uncancelled read streams up to `FETCH_CAP` to nobody while holding a connection — or the
tab's pinned session — busy. Opening a second cell supersedes the first read the same way, and
the app's `view_blob` does that **before** it branches on the `BlobRef`. The cancel block sat
below the `None` early return, so the one opening with nothing of its own to fetch — a NULL cell,
a pending row — skipped it entirely and left an earlier fetch streaming to `FETCH_CAP` over a
connection whose only reader had gone, which is precisely the cost `cancel_blob`'s doc says the
cancel exists to avoid. The new token also has to *replace* the stored one rather than only
cancel it: keeping the stale token there pointed the new panel's Escape at a read that was
already over.
**Its words are the catalogue's** (`blob-*`) — the pane switch's labels
(`pane_label` returns `String`), the Reading…/NULL-cell notes, the preview's three refusals
(`blob-preview-too-large` with `$width`/`$height`, `blob-preview-truncated`,
`blob-preview-unreadable` — the last two selecting the kind's noun on `$key`, `blob_kind_key`'s
word, since the capitalised `blob-kind-*` label pasted into the sentence read "like a Icon"),
`save_offer`'s refusal (`blob-save-truncated`), Load from file and
Save to file and their dialogs' window titles, and `BlobUi`'s status lines — `saved_at`'s
`blob-saved-to` (`$path`), and `loaded_file`'s `blob-loaded-file`, `blob-not-writable`,
`blob-file-too-large` (`$got`, `$most`) and `blob-not-accepting`; Close is the shared
`dialog-close`. `summary_line` is two whole messages, `blob-summary` and
`blob-summary-truncated` (`$kind`, `$size`, `$shown`). The kind is core's word drawn UI-side:
`blob_kind_label(BlobKind)` replaces `BlobKind::label` in the summary and the preview's notes
(`blob-kind-*`), and `english_words_every_blob_kind_as_core_does` holds its English equal to
core's — but the save dialog's `FileSpec` still takes `kind.label()`, since it needs a
`&'static str`. `the_composed_lines_read_as_they_did_in_english` pins the assembled lines; the
saved-to, `save_offer` and loaded-line tests pass unchanged. The sizes are core's
`human_bytes` through `i18n::figure` and `i18n::contrasting_bytes`, so in the interface's
marks. English stays: the save dialog's file type name, the suggested file name, and a load
failure's OS message.
