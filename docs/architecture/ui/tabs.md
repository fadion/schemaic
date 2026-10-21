# `ui/tabs.rs`

`tabs.rs` — query-tab strip, and where a **`.sql`-backed tab** shows itself. The state behind
that is five signals on `Tab`: `path`, `disk_sql` (the file's text as of the last open / save /
reload — `None` means *unknown*, which reads as modified, the safe direction), `file_format`
(the `SqlFormat` a save has to put back, and whether the read was lossy), `reload_gen`, and
`disk_changed` (the file's text as the app's window-focus check last found it, when that differs
from `disk_sql` — what the bar above the editor offers back; see the app's file wiring). `Tab::title` falls back name → file name → "Query N", the user-assigned name
winning on purpose, since renaming a tab is an explicit act a Save As shouldn't silently undo;
`Tab::modified` is `path.is_some() && query != disk_sql`, and so always false for a tab with no
file — an ordinary tab is session-persisted and has nothing to be unsaved *against*.
**The chip says two different things and says them two different ways**, which is the whole
design: *being* a file is a standing fact, so it gets a glyph — a dim 14px lucide `file` leading
the title, tinted `tab_close` like the trailing ×/pin so it reads as chrome; *having drifted* from
that file is transient, so it gets **italic title text** and no glyph at all. Both markers as
dots was the first attempt and the wrong one on this chip, because a tab can already carry the
DB-identity dot: the strip ended up with two dots of unrelated meanings a few pixels apart.
Italic also costs no width, so unlike `tab_dot_w()` and `tab_file_w()` — neither of which is inside
`tab_title_avail()`'s 40, so a title has to shed whichever of them is showing or a full-width one
pushes the × past the chip cap — the slant needs nothing shed for it. (Truncation is measured
upright: an italic face is a hair wider, and being a hair late to ellipsize isn't worth a second
text measurement.) A file tab's tooltip is its **full path**, which subsumes the truncated-title
case and answers what the chip cannot — *which* `orders.sql` — with "— unsaved changes" appended
when it is modified, since nothing else on screen explains the slant.
The chip's content is a `dyn_container` keyed on
`(editing, title, pinned, modified, path-as-string)`. Two of those are less obvious than they
look: the `modified` read tracks `query`, so the closure re-runs on every keystroke while the key
only *changes* when the flag flips, which is the one thing that has to rebuild the row; and the
path is keyed as its display string rather than an `is_some()`, because a Save As from one file
to another on a tab that also carries a user-assigned name moves neither the title nor the icon,
and the tooltip would have gone on naming the old file.
The context menu is three groups, separated: the clicked tab (Pin / Rename / Duplicate / Close),
then its file (Open file / Save / Save as / Reload from disk), then the strip (Reopen last tab
/ Close other tabs / Close all tabs). Sentence case and no ellipses, like every other entry —
this menu doesn't mark which entries open something. Open leads the file group because Ctrl+O is
otherwise keyboard-only: there is no menu bar to put it on. Save is offered on a tab with no file
too, since it falls through to Save as, which is the answer to "save this" there, while Reload is
*dimmed* rather than hidden, the way "Reopen last tab" is on an empty ring, so the menu keeps
one shape.
