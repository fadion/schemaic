# `ui/overlays.rs`

`overlays.rs` — absolutely-positioned popups: connection/active-db/schema menus, schema context
menu, generic grid popup, the date picker's calendar (`date_pick_overlay`, whose panel is
`cell_editors::calendar_panel` — up here because the field it drops from sits inside a scrolling
strip that would clip it), Find-Anywhere, error modal. **What separates the two kinds here is
the backdrop, not the name.** The menus are shrink-wrapped to their panel; Find-Anywhere and
the error/confirm/transaction prompts paint `modal_backdrop()` over the window, which puts
them in `modals::modal_layer` with the rest of the modals. Find-Anywhere is the one that
looks like it belongs on the other side of that line — it is a palette, and a click away
closes it — and it is the reason the line is drawn where it is: mounted with the menus, its
backdrop covered the title bar, and a press aimed at the caption buttons found the click-away
instead, so the only thing the window did was dismiss the palette. Its `find_top()` is measured
from the top of the *window*, so the layer's `header_h()` comes off it where the margin is set —
and it is **scaled, because what it is subtracted from is**: as a literal `80` against a
`header_h()` that grows, the gap under the title bar ran 48 / 40 / 28 / 16px across the four
scales, shrinking as everything around it grew, which is the inversion no reader of the number
would predict. `menu_icon_tuck()` and `menu_edge_pad()` are functions for the same reason — an
offset frozen between two growing boxes is not an offset but a drift, and at 160% a 154px panel
tucked by a literal 30 landed its right edge ~18px inside the icon instead of flush past it.
**The three dropdowns placed by hand cap at the room below their anchor and scroll** (GitHub
#21) — `active_db_menu_overlay`, `conn_menu_overlay` and `db_visibility_overlay`, which build
their own panels rather than going through `menu_panel` and so never had its cap: a connection
with many databases ran the QUERY toolbar's selector off the bottom of the window, the
databases below the fold out of reach. Each takes `max_height(widgets::room_below(…))` from its
own anchor's bottom edge — `active_db_anchor`'s `y`, the switcher's `header_h() - scaled(4)`
(the same term its `margin_top` is), the gear's `anchor.y + MENU_ICON_DROP` — and each list
carries `min_width_full` for `menu_stack`'s reason: floem's `Scroll` lays its child out at
content width, so without it the rows' hover and hit box stop short of the panel's right edge.
The scroll is `widgets::shift_hscroll`, not a bare `scroll`: a name wider than the panel pans
on a horizontal wheel *and* on Shift+wheel, which Windows delivers as a vertical delta — the
rule every scroll in the app follows. The selector's panel is `db_menu_w()`, 220 since the
same change (170 let a longish database name run past its edge).
**In the connection switcher only the connections scroll.** The list is
`flex_shrink(1).min_height(0)` and the rule and *Manage Connections* under it `flex_shrink(0)`,
because the way out of a long list must not be the thing scrolled out of sight — wrapping the
whole panel in the scroll, as the database selector's is, would be the tidy-up that loses it.
None of the three flips above its anchor or takes arrow-key navigation, before or after; the
cap, the scroll and the selector's width are all that changed.
**No Drop here asks before its preview.** The table/view/sequence Drop, a standalone object's,
the database's and the schema's each open the DDL preview directly — `preview_change_warning`,
`preview_of`/`open_preview` over `ddl::drop_item`, and `preview_container` twice — where every
one of them raised a `Confirm` first and the preview's Apply then asked the same question under
the same warning. The user asked for the first to go. The container and account drops' confirm
had its body from `risk_prompt(change, dialect)` — born `container_drop_prompt`, renamed when
`users_view`'s account drop became its third caller — which rendered `Change::risks` so the
confirm and the preview could not say different things about one act; with the confirm gone
the preview's risk block *is* the change's risks (`Change::risk_lines`, worded by
`ddl_preview::risk_text`), there is no second rendering to keep in step,
and the function went with its last caller. The table drop's confirm had said two things the
preview did not — the row figure, and a MariaDB sequence named as a sequence rather than as the
table `DROP TABLE` treats it as — and both now ride into the preview's warning through
`stats::drop_warning`. **Truncate still asks first** (`stats::truncate_prompt`): it is
not a drop, and it keeps its confirm-then-preview.
**Its words are the catalogue's** (`tree-*`, `dialog-*`, `tx-prompt-*`). The
context menu and the SCHEMA gear share `tree-*`, so an action both offer cannot be worded two
ways; the confirm dialog's buttons are `dialog-cancel`/`dialog-yes`/`dialog-no`, named for no
one dialog so other files can take them; and the transaction prompt's body is one message per
variant (`tx-prompt-body`, `tx-prompt-body-aborted`) with the statement count's plural inside
it, where it was a `1 statement`/`{n} statements` match spliced into a `format!`
(`the_transaction_prompt_is_one_sentence_with_its_count` pins the English). The `{kind}` inside
*Create {kind}* and *Edit {kind}* — and `CreateKind::label`'s fallback for an object kind — is
`i18n::object_kind`, the catalogue's `kind-*` rather than `ObjectKind::label()`. Some words
reaching these popups were another module's English, and one still is: `ExportFormat::label`,
whose names are English on purpose. `activity::interval_label` is gone — Server Activity's poll
menu, the `dyn_container` over `activity::POLL_INTERVALS`, draws its entries through
`i18n::poll_interval` (`activity-interval-off`, `activity-interval` with `$secs`);
`stats::truncate_prompt`'s question is `i18n::truncate_prompt`'s
now (`tree-truncate-rows`, `-every-row`, over core's `confirm_figure`; the first selects on
`$n`, the figure's own number, so an exact count of one asks "Delete the 1 row", as core's
English does). The colour presets' names
(`lib.rs`'s `CONN_COLOR_PRESETS`) were the fourth until `lib.rs` drew them from `colour-*`.
**A PostgreSQL foreign table is offered what its server takes** (`object_entries` over
`TableShape::ForeignTable`): its Drop, which `object_drop_change` makes `DROP FOREIGN TABLE`;
not Edit table, since the designer's keys and indexes are refused on one (*cannot create index
on relation*, measured on 16) and it has no field for the server or options, so the form would
be half refusals; not Truncate, which is the wrapper's to allow and `file_fdw` refuses; and not
Import or triggers. `field_entries` and `key_entries` answer no for it as well, being the
second door into the same designer (`a_foreign_table_offers_its_drop_and_not_the_designer`).
**`CreateKind::Extension` is the Create ▸ submenu's *Extension***, gated on
`ddl::supports_extensions` and placed after Procedure and Event and before Database and Schema
— after everything that lives in a namespace, the extension being the database's own, and
before the containers (`only_postgres_is_offered_an_extension`;
`postgres_offers_its_standalone_objects_too` pins PostgreSQL's whole list). It opens
`database_editor::open_for_extension` on the node's database rather than the object editor.
**`whole_ui_gate` 13 → 4, and six of the nine that came down were the small menus** — they read
their bundles into locals at the top and never touch the root again, so `popup_menu_overlay`
and `tx_prompt_overlay` take `OverlayUi`, `db_visibility_overlay` `SchemaUi`,
`conn_menu_overlay` `ConnUi`, `active_db_menu_overlay` `(TabsUi, SchemaUi)` and
`activity_menu_overlay` `(ActivityUi, LayoutUi)`, each beside the `Rc` closures it calls.
`export_submenu` takes the export modal's own `&DumpCtx`, and `schema_settings_overlay` and
`create_submenu` take `SchemaActions` whole — the last two only once `database_editor.rs` and
`table_designer.rs` had taken their `open_for_new` doors off the root bundle, which is the
tranche's own lesson and is under `lib.rs`'s `whole_ui_gate` with the counts. **The four left
are wide because what they do is wide, not because a callee holds them there**:
`context_menu_overlay` reaches seventeen `Ui` fields (the tree's whole right-click surface),
`palette_commands` nine, `find_overlay` eight and `error_modal_overlay` five. **Two call
spellings in this file are load-bearing and were kept through all of it**: `menu_order_gate`'s
`the_scan_sees_the_export_submenu_in_all_three_arms` reads this file's own source for lines
beginning `entries.push(export_submenu(` and `entries.extend(create_submenu(`, so changing what
those two calls *take* costs nothing and re-wrapping how they are *written* blinds the gate.
**The palette's *Go to Statement* is the script's outline**: a `CmdArg::Options` command whose
list is `core::sql::outline` over the active tab's text in its connection's dialect — each
statement `statement_ranges` finds, labelled `line: code` with the code folded onto one line by
`history::preview` and both the line and the offset taken **past any comment above the
statement**, so a leading `-- note` is neither the label nor where the jump lands
(`each_statement_is_listed_at_its_code`). The pick sets `Tab::jump_offset`, the request
*Go to Line* uses. A long script's `CREATE`s, temp tables and `WITH`s are found by typing part
of what they say. The options filter matches the label alone here: the value is a byte offset,
and matching it once kept the statement *at offset* 40 ahead of line 40's for
`>go to statement 40`, so Enter jumped to the wrong one. A `CmdArg::Options` says whether its
values are words worth typing (`keyed`), and `option_kept` is the one filter
(`overlays::option_filter_tests`).
**A label that long is why the palette truncates.** A row's text either side of the match
shrinks to a `…` while the match itself does not (`highlighted_primary`; shrinking all three
alike collapsed a hit deep in a statement to `c…`), and the field's grey completion ghost is
bounded at the field's inner edge with `inset_right` and ends in a `…` too — both used to run
past the palette. Text Tab writes into the field is the field's own and scrolls there.
**The palette's *Reopen Closed Tab* is the reopen ring as a list** — a `CmdArg::Options` command
(hint `<tab>`) right after *Close Tab*, listing `TabsActions::closed_tabs_list`: the active
connection's ring entries, most recent first, keyed by their **ring index** and worded by
`tabsel::closed_tab_choice`. The pick runs `TabsActions::reopen_closed_at` with that index —
the same rebuild Ctrl+Shift+T's `reopen_closed_tab` delegates to once it has found the first
entry on the active connection — which removes the entry and saves the ring. It is scoped to
the active connection for the shortcut's reason: the strip shows only that connection's tabs,
so reopening another's would restore it out of sight.
**A palette command answers to two names, its label's and its English one.** `Command` is
`{ id, label, hint, arg }`: `label` and `hint` are translated `String`s from the `palette-*`
messages, and `name()` — the label lowercased — is what the parser matches, because Tab
completes the label into the box. `id` is the English name lowercased, and it matches in every
language too, as VS Code's translated palette accepts its English names: the list filter takes
either (`name()` or `id`), `arg_names` hands the parser both (`Command::names`, once in English
where they are the same string), and `is_named` finds the command by whichever came back. **`id`
is also what everything else keys on** — `COMMAND_KEYS`, the `retain` filters that drop *Server
Variables* and *Ask AI* — so nothing holds a translated string as an identity.
`assert_names_match_labels` holds `id` to the English label only while the interface is
English, the one language where that is checkable. Its word-prefix check is **wider than the
parser's invariant**: no argument command's name, in either spelling, may be a word-prefix of
*any* command's name, instant ones included — in German *Verlauf* (History) takes an argument,
so typing all of *Verlauf leeren* would resolve to it with `leeren` as the argument, which is
why German's *Clear History* is *Abfrageverlauf leeren*. Toggle Panel's and Indent Style's
option labels are translated while their values stay English, and the options filter matches
a `keyed` list's values too, so `>toggle panel ai` still works in German. The themes are
keyed the same way. *Go to statement*, *Reopen closed tab* and *Switch connection* are not:
their values are an offset, a ring index and a connection id, which a typed number would
match by accident.
**The error modal wears `modal_title("Error", …)` and its ✕**, where it used to have neither a
title row nor a close button and only click-away and Escape closed it. All three exits run the
one `close`, which is also what clears `error_modal_text` so the editor bar's next *View* falls
back to the tab's error. The ✕ joins the modal's ring, and on one of Schemaic's own notices it
is the **only** stop, since those offer no action (`core::model::ErrorModalContent`); the
actions row drops its top margin when it is empty. The body's padding sits on a body container
rather than on the panel, because the title row carries its own padding and a bottom border
that a padded panel would inset, and the scroll's `min_height` came down 160 → 120 to pay for
the row the title adds.
**The confirm modal and the transaction prompt wear the standard modal too** (GitHub #25) —
`modal_title_owned`/`modal_title` with its ✕, a body padded `modal_pad_h()` × `scaled(18)`, and
`modal_footer` holding filled `action_button`s — where both were bare panels (`panel_style` at
20px, a bold 15px line for a title) answered by coloured *text* buttons nothing else in the app
wore. The confirm is No `Neutral` and Yes `Danger`, since it is the question raised before a
destructive act (its Yes was red text before it was a fill); the prompt is Cancel `Neutral`,
Rollback `Danger` — it discards writes — and Commit `Primary`, still built only when the
transaction can commit, never built and hidden. Each ✕ gives the answer Escape gives — No,
Cancel — and sits at `TITLE_CLOSE_TAB`, **last** in the ring, so the first Tab still lands on
No or Cancel, the side that loses nothing; the buttons keep `ACTION_TAB`, `+10`, `+20`. The
prompt still has no click-away (*A modal's click-to-dismiss*).
