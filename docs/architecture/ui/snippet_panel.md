# `ui/snippet_panel.rs`

`snippet_panel.rs` — the **Snippet Library** right-column panel (`RightPanel::Snippets`, the
toolbar's bookmark toggle): the saved queries that apply to the active connection, under the
scope bands `core::snippet::grouped` returns, over the History panel's chrome. It decides
nothing — which snippets apply, how they group and sort, and what the filter matches are all
`core::snippet`'s answers, under test. Its **dialect comes from the active connection**, not
the active tab: a tab keeps the connection it was opened on, while the library sits under the
connection selected above it. That memo is `ConnUi::dialect` — one memo on the bundle, which
this panel and `history_panel` both read — because it had been written out three times
identically and only the app's copy carried the ten-line reason it must be a **tracked** memo:
the built-in pack is `snippet::builtins(dialect)`, so an untracked read recomputes only when
`snippets` changes and every writer of that signal is a user action. Switching to a connection
of another engine therefore left the whole shipped pack of the *previous* engine in place, which
`snippet::applies` then filtered out entirely — the panel, abbrev expansion and Find-Anywhere
all lost their built-ins until the next time a user snippet happened to be written. The two
copies without that note were the ones most likely to be "simplified" to a `get_untracked`. **A row click inserts the body at the caret** (`Tab::insert_req`),
where a history row opens a new tab — a record of a past run wants its own tab, a snippet is
something you compose *with*; "Open in new tab" is on the row's menu for when it isn't. The
editor pane's `insert_req` effect splices it over the selection when there is one and hands it
to `completion::begin_snippet`, which selects the first `:name` stop for Tab to walk from
(`tab_stops.rs`), or with none puts the caret after the body, as the effect used to itself — and
then asks focus back for the editor, as the jump-to-offset effect does: the click leaves it on
the panel, where the typing meant for the first stop went nowhere and Tab walked the window's
focus ring instead of the stops. The
parameter chips under a body are read live from `params::names`, never stored, so they cannot
disagree with the body above them. **The body preview is syntax-coloured** (`highlight_sql_mono`,
over `sql_highlight::ui_spans` — the editor's own lexer in the *interface* theme's SQL palette,
`UiTheme::sql_*` — with the search match added last so it still wins the overlap) and has **no
surface of its own**: base `theme::text_dim`, straight on the row, its vertical spacing kept and
its background, side padding and radius gone. It has been on two surfaces before, both of them
the wrong axis. First `bg_editor` — the UI theme's inset surface under the *editor* theme's
tokens — which is the pairing `contrast.rs`'s entry records at 1.70:1. Then the editor's own
surface and base (`theme::preview_bg`/`preview_fg`), on the reasoning that those token colours
reproduce published palettes tuned against the editor background; that was legible, but the
editor theme is chosen independently of the light/dark UI theme, so a Light UI under a dark
editor theme drew a dark block in a light panel. The palette moved to the UI axis instead, and
`contrast.rs` measures it on `bg_panel`. The Query History panel's preview runs through the same
`highlight_sql_mono` with the same base and no surface either (a long list of past runs read as
the same wall of grey this one did); and the AI panel's fenced blocks take the same treatment, a
**third instance rather than the exception** they were once recorded as here — a SQL block goes
through the same `highlight_sql_mono` once the turn has settled, base `theme::text`, and a block
that stays plain (non-SQL, or still streaming) takes the same base; `markdown::code_block`'s
wrapper lost its fill and keeps its border and header, a second look for a plain block being two
kinds of block in one conversation. The two previews are the same treatment
down to the base the uncoloured identifiers take: the panels share a column and are read the
same way, and a brighter base in one of them spent on identifiers the contrast the keywords are
there to carry.
**A saved snippet is scoped to the connection it was saved on and stamped as just-used**, so it
lands in the topmost band's topmost row and the panel scrolls there — the first spelling (engine
scope, no stamp) dropped a new row into the middle of a long alphabetical list, where nobody
could find the thing they had just saved. Widening it afterwards is one click in *Show in*. The
scroll goes through a `scroll_to` signal **cleared on the next tick**: a `Some` left standing
re-scrolls on every later layout pass, which is the sticky-`scroll_to` trap that also shaped the
grid's tail-follow. That same scroll is wrapped in `autohide_tracking` rather than `autohide`,
because the scope bands are laid out by `widgets::banded_list` and their headers stick, as the
history panel's do — the bands are titled by `group_title`, the one part of them that is this
panel's own, and the `viewport` they read is panel-level for the reason given there.
Naming that new snippet is inline (an `edit_field` in place of the name), matching the tab
rename, because there is no text-prompt modal here and naming a saved query is the same kind of
act. That is the **only** inline edit: the name and the abbrev both used to have menu entries
and inline fields of their own, which made three routes into the three fields `snippet_edit`
already owns — the menu now offers *Edit* and the dialog does the rest. The menu's **Show in**
submenu is the scope picker: its three choices are `snippet::scope_options` (narrowest first), so the order you pick
from and the order of the bands a row can move to cannot drift apart — a test pins each choice
to the band it lands under. The current one is tinted rather than ticked, the convention
`cell_editors::pick_entries` set. A built-in offers none of the three: Duplicate is how you get
an editable copy of one.
**Its *This connection* choice means the connection you are on, and `snippet_row` therefore takes
`active_conn` as a `RwSignal<u64>` rather than a `u64`.** The row list is a `dyn_container` keyed
on `(groups, search, dialect)` — `active_conn` is in none of the three — and the id was read
*inside* the child builder, which does not track, so *Show in → This connection* wrote
`Scope::Conn(previous)`: the snippet vanished from the panel in front of the user with no
message and came back only after switching away and back. That was survivable only while `groups`
changed on a connection switch, and the ordinary case is that it does not — with no `Scope::Conn`
snippet on either connection the empty bucket is dropped, the `Bucket::Conn` band is absent from
both results and the two `Vec<Group>` compare equal, so the memo never notified. The file's own
comment says that mechanism is why `search` and `dialect` joined the tuple, and stops one term
short. The fix is **not** a fourth key term: `row_menu` is built inside
`on_secondary_click_stop`, so the row takes the signal and the menu reads it with
`get_untracked` when it is *raised*, which is when the answer is wanted — the same shape Server
Activity's row menu takes. In `core::snippet`,
`two_connections_group_identically_when_neither_has_a_scoped_snippet` asserts the **cause**
rather than the fix, because it is the precondition that made the missing key term
load-bearing and would otherwise have to be rediscovered; its second half pins that
`grouped` does read `conn_id`, so the equality is a statement about the library and not about an
ignored argument.
