# `ui/history_panel.rs`

`history_panel.rs` — Query History right-column panel; its row preview is syntax-coloured in
the interface theme's SQL palette with no surface of its own, like the snippet library's, for
the reason `snippet_panel.rs` gives below. Its **dialect comes from the active connection**, same memo and same reasoning as the
library's: the list is filtered to that connection, so every row on screen ran against it. That
memo is tracked in the row `dyn_container`'s trigger tuple alongside `visible` and `search` —
two connections that both have no history yield the same empty `visible`, and without it the
rows would stay built with the previous engine's lexer.
**A *Current | All* switch under the title lifts that filter** — `widgets::segmented_tabs(
vec![(tr!("history-scope-current"), false), (tr!("history-scope-all"), true)], …)` on
`HistoryUi::all_conns`, persisted as
`UiState::history_all_connections`, with the activity panel's Sessions/Locks spacing exactly
(`scaled(12)` side padding, `scaled(4)` from the title, `scaled(10)` spacers either side of the
search box). It replaced a `widgets::toggle_icon(icons::DATABASE, …)` beside the trash: an
unlabelled icon was poor at saying what it switched, and the monitor already had the tab bar
for exactly this. `visible` is the rows of `history::panel_view` with scope
`(!all_conns).then(active_conn)` and cap `MAX_PER_CONN`, and `all_conns` joins the trigger
tuple; where the cut left rows out, a muted line between the search box and the list says
*Showing the newest N of M — search to narrow.* — outside the scroll, so `banded_list`'s sticky
headers keep measuring from the list's own top. With it on, the
reasoning above no longer holds — the rows on screen ran against several engines — so each row
is lexed in its own connection's dialect (`SqlDialect::from_db_type` of that connection in
`conn.connections`, falling back to the active memo's for one the list no longer has) and its
heading reads `<connection> · <database>`, so two servers' `shop` read as two places.
`connections` is read untracked there, so a renamed connection relabels its rows at the next
rebuild, not at once. **The trash keeps to the active connection either way** — its count is
still `count_conn` of the active connection, and its tooltip and confirm say *this connection*
— so *All* left on never widens what a click deletes. Opening another connection's row
switches to it first (`open_history`, in *app/main.rs*'s `app_view` — `history_store.rs` says
why it is not there).
Its TODAY / THIS WEEK / EARLIER bands (`history::group_by_recency`) are titled by
`bucket_title(bucket)` — `history::Bucket::label`'s word in the interface's language
(`history-band-*`), drawn here rather than translated from core's, which stays English;
`english_titles_every_band_as_core_does` holds English equal to core's. That test lists the
three variants by hand, so a new `Bucket` is caught by `bucket_title`'s exhaustive `match`, not
by it. The bands are laid out by `widgets::banded_list`, so their headers stick while their
rows scroll. The `viewport` signal that list reads is created at panel level, outside that
`dyn_container`: it belongs to the scroll, which outlives every rebuild of the rows, and the scroll publishes into
it through `autohide_tracking`.
**A row's right-click menu takes the snippet library's route exactly** —
`menus.close_except(Some(MenuId::Popup))`, `popup_anchor` cleared so the panel opens at the
pointer, `popup_width` from `menu_panel_width(&entries)`, then the fill — and the width is
*measured* rather than a constant for the reason recorded there: `popup_width` is the panel's
`min_width`, so a number picked by eye becomes a floor the rows cannot pull back in. `row_menu`
offers **Open in new tab** (what the single click already does) and **Delete**, with a
`MenuEntry::Separator` before the destructive one, the same convention the library's row menu
follows. **Delete is deliberately not behind the shared confirm the trash button uses**: that
one clears a whole connection's log at once and names the count in the modal, while this
destroys the one row under the pointer and re-running the statement records it again — a modal
would cost more than the mistake it prevents. The opener is built **once in `history_panel`**
as an `Rc<dyn Fn(HistoryEntry)>` that `history_row` takes as a single parameter, rather than
assembled per row: `overlay` and `menus` are panel-wide `Copy` bundles, threading them through
every row would say otherwise, and the row goes on taking only what it draws with.
