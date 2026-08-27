//! The Snippet Library panel: the saved queries that apply to the active
//! connection, grouped by scope, on the right column (same chrome as the
//! History / AI / Terminal panels).
//!
//! Each row shows the snippet's name, its abbrev (when it has one), the body
//! collapsed to three clipped lines, and a chip per `:name` the body holds —
//! those chips are read live from `schemaic_core::params::names`, never stored,
//! so they cannot disagree with the body above them.
//!
//! **Clicking a row inserts the body at the caret**, where clicking a history
//! row opens a new tab. The difference is deliberate: a history entry is a
//! record of something that already ran, so a fresh tab is right; a snippet is a
//! thing you are composing *with*.

use std::rc::Rc;
use std::time::Duration;

use floem::prelude::*;
use floem::reactive::create_memo;

use schemaic_core::params;
use schemaic_core::snippet::{self, Bucket, Snippet, Source};

use schemaic_core::intel::SqlDialect;

use crate::consts::{MONO_FAMILY, SEARCH_DEBOUNCE_MS};
use crate::theme::{font_body, font_label};
use crate::widgets::{
    MenuEntry, autohide, debounced, highlight_sql_mono, highlight_text, section_title, toolbar_icon,
};
use crate::{FieldCfg, OverlayUi, Ui, edit_field, icons, theme};

pub(crate) fn snippet_panel(ui: Ui) -> impl IntoView {
    let items = ui.snippets.items;
    let actions = ui.snippet_actions.clone();
    let active_conn = ui.conn.active_conn;
    let connections = ui.conn.connections;
    let overlay = ui.overlay;
    let menus = crate::widgets::MenuFlags::of(&ui);
    // The panel is scoped to the **active connection**, so its dialect comes
    // from that connection rather than from the active tab: a tab keeps the
    // connection it was opened on, and the library in front of you is the one
    // for the connection selected above it.
    let dialect = create_memo(move |_| {
        let cid = active_conn.get();
        connections
            .with(|cs| {
                cs.iter()
                    .find(|c| c.id == cid)
                    .map(|c| SqlDialect::from_db_type(&c.db_type))
            })
            .unwrap_or_default()
    });

    // Panel-local, like the history panel's: the filter resets when the panel is
    // re-opened, and the rename buffer belongs to this build of the list.
    let search_input = RwSignal::new(String::new());
    let search = debounced(search_input, Duration::from_millis(SEARCH_DEBOUNCE_MS));
    // The row whose name is being typed, right after a save — the one inline
    // edit left, and the only one that has to be inline: a snippet arrives named
    // after its tab, which is a placeholder rather than an answer. Everything
    // else about a snippet is changed in the editor dialog.
    let renaming: RwSignal<Option<u64>> = RwSignal::new(None);
    let rename_buf = RwSignal::new(String::new());
    // Set to the top when a save lands, then cleared. `scroll_to` is **sticky**:
    // a `Some` left standing re-scrolls on every later layout pass, which is how
    // a list that follows its tail ends up refusing to be scrolled by hand.
    let list_scroll: RwSignal<Option<floem::kurbo::Point>> = RwSignal::new(None);

    let groups = create_memo(move |_| {
        let conn = active_conn.get();
        let q = search.get();
        items.with(|all| snippet::grouped(all, dialect.get(), conn, &q))
    });

    let list = dyn_container(move || groups.get(), {
        let actions = actions.clone();
        move |groups: Vec<snippet::Group>| {
            let conn = active_conn.get_untracked();
            if groups.is_empty() {
                return empty_state().into_any();
            }
            let term = {
                let t = search.get_untracked();
                let t = t.trim().to_string();
                (!t.is_empty()).then_some(t)
            };
            let actions = actions.clone();
            let rows = groups
                .into_iter()
                .enumerate()
                .flat_map(|(gi, group)| {
                    let header = group_header(group.bucket, group.items.len(), gi == 0);
                    let (actions, term) = (actions.clone(), term.clone());
                    std::iter::once(header).chain(
                        group
                            .items
                            .into_iter()
                            .map(move |s| {
                                snippet_row(
                                    s,
                                    actions.clone(),
                                    term.clone(),
                                    dialect.get_untracked(),
                                    conn,
                                    renaming,
                                    rename_buf,
                                    overlay,
                                    menus,
                                )
                                .into_any()
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>();
            v_stack_from_iter(rows)
                .style(|s| s.flex_col().width_full())
                .into_any()
        }
    })
    .style(|s| s.flex_col().width_full());

    let scrolled = autohide(scroll(list).scroll_to(move || list_scroll.get()))
        .style(|s| s.flex_grow(1.0_f32).width_full().min_height(0.0));

    // Title row: "SNIPPET LIBRARY" left, a + right — save what the editor holds.
    // Unlike the history panel's trash this destroys nothing, so it doesn't ask.
    let save_current = actions.save_current.clone();
    let can_save = ui.snippets.can_save;
    let plus = toolbar_icon(
        icons::PLUS,
        5.0,
        7.0,
        move || can_save.get(),
        move || {
            let Some(id) = (save_current)() else {
                return;
            };
            // A new snippet is scoped to **this connection** and sorts first, so
            // it lands in the topmost band's topmost row — which is only useful
            // if that row is on screen. The list may be scrolled anywhere, so
            // put it back at the top.
            list_scroll.set(Some(floem::kurbo::Point::ZERO));
            floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                list_scroll.set(None);
            });
            // Straight into the name field: the snippet arrived named after its
            // tab, which is a placeholder rather than an answer.
            rename_buf.set(String::new());
            renaming.set(Some(id));
        },
    )
    .tooltip(|| text("Save the editor's SQL as a snippet").style(crate::widgets::tooltip_style));

    let title_row = h_stack((section_title("SNIPPET LIBRARY"), plus))
        .style(|s| s.width_full().flex_row().items_start().justify_between());

    v_stack((
        title_row,
        // The same 5px above / 10px below the search box the history and schema
        // panels use; spacers rather than margins so the flex-grow scroll's
        // height stays exact.
        empty().style(|s| s.height(theme::scaled(5.0)).flex_shrink(0.0_f32)),
        snippet_search(search_input),
        empty().style(|s| s.height(theme::scaled(10.0)).flex_shrink(0.0_f32)),
        scrolled,
    ))
    .style(move |s| {
        s.width(crate::widgets::right_panel_w().get())
            .flex_shrink(0.0_f32)
            .height_full()
            .flex_col()
            .background(theme::bg_panel())
            .border_left(1.0)
            .border_color(theme::border())
    })
}

/// The panel's filter box — the same look and placeholder as the history
/// panel's. Non-empty narrows by name / abbrev / body.
fn snippet_search(filter: RwSignal<String>) -> impl IntoView {
    edit_field(
        filter,
        FieldCfg {
            placeholder: "Search…",
            background: theme::bg_chrome,
            clearable: true,
            ..Default::default()
        },
    )
    .style(|s| {
        s.margin_left(theme::scaled(12.0))
            .margin_right(theme::scaled(12.0))
            .flex_shrink(0.0_f32)
    })
}

/// Nothing to show, which — since every engine ships a built-in pack — can only
/// mean the filter matched nothing. There is no "library is empty" state to
/// write: `snippet::library` always has the pack in it.
fn empty_state() -> impl IntoView {
    container(text("No snippet matches.").style(|s| {
        s.font_size(font_label())
            .color(theme::text_faint())
            .padding_horiz(theme::scaled(12.0))
            .padding_vert(theme::scaled(10.0))
    }))
    .style(|s| s.width_full())
}

/// A scope band — `THIS CONNECTION` / `MYSQL` / `ALL CONNECTIONS` — and how many
/// are under it. Same weights and colours as the history panel's recency bands,
/// including the top rule on the first one only (floem doesn't collapse adjacent
/// borders, so every other header follows a row that already drew one).
fn group_header(bucket: Bucket, count: usize, first: bool) -> floem::AnyView {
    let title = match bucket {
        // Named for what it *means* rather than for the connection: the panel is
        // already scoped to the active connection, and repeating its name here
        // says nothing the header above the panel doesn't.
        Bucket::Conn(_) => "THIS CONNECTION".to_string(),
        Bucket::Dialect(d) => d.engine_label().to_uppercase(),
        Bucket::Global => "ALL CONNECTIONS".to_string(),
    };
    let label = text(title).style(|s| s.font_size(font_label()).font_bold().color(theme::accent()));
    let n = text(count.to_string()).style(|s| {
        s.font_size(font_label())
            .color(theme::text_dim())
            .flex_shrink(0.0_f32)
    });
    h_stack((label, empty().style(|s| s.flex_grow(1.0_f32)), n))
        .style(move |s| {
            let s = s
                .width_full()
                .items_center()
                .padding_horiz(theme::scaled(12.0))
                .padding_vert(theme::scaled(8.0))
                .background(theme::group_header_bg())
                .border_bottom(1.0)
                .border_color(theme::border());
            if first { s.border_top(1.0) } else { s }
        })
        .into_any()
}

/// One snippet row.
#[allow(clippy::too_many_arguments)]
fn snippet_row(
    snip: Snippet,
    actions: Rc<crate::SnippetActions>,
    term: Option<String>,
    dialect: SqlDialect,
    conn_id: u64,
    renaming: RwSignal<Option<u64>>,
    rename_buf: RwSignal<String>,
    overlay: OverlayUi,
    menus: crate::widgets::MenuFlags,
) -> impl IntoView {
    let id = snip.id;
    let insert = actions.insert.clone();
    let click_snip = snip.clone();

    // The heading: name, abbrev chip, and the right-hand label — when it was
    // last used, or `Built-in` for a shipped snippet, which has no "last used"
    // worth showing on a row nobody has touched.
    let right = match (snip.source, snip.last_used) {
        (Source::Builtin, _) => "Built-in".to_string(),
        (_, Some(ts)) => schemaic_core::history::relative_time(ts, now_millis()),
        (_, None) => String::new(),
    };
    // The abbrev, when it has one — a chip and nothing else. Setting it lives in
    // the editor dialog beside the body and the name; a row with an empty chip
    // on it would be noise on the many snippets that never want a trigger.
    let abbrev_view: floem::AnyView = match snip.abbrev.clone().filter(|a| !a.is_empty()) {
        None => empty().into_any(),
        Some(a) => h_stack((
            crate::icons::icon_wh(icons::KEYBOARD, 12.0, 0.0),
            text(a).style(|s| {
                s.font_family(MONO_FAMILY.to_string())
                    .font_size(font_label())
            }),
        ))
        .style(|s| {
            s.items_center()
                .gap(theme::scaled(4.0))
                .color(theme::text_dim())
                .flex_shrink(0.0_f32)
        })
        .into_any(),
    };

    // Naming a **brand-new** snippet: the row it was just saved into shows a
    // field instead of its placeholder name. That is the only way in — the menu
    // has no Rename, because *Edit* changes the name along with everything else.
    let name_view = dyn_container(move || renaming.get() == Some(id), {
        let name = snip.name.clone();
        let term = term.clone();
        let rename = actions.rename.clone();
        move |editing: bool| {
            if !editing {
                return highlight_text(
                    name.clone(),
                    term.clone(),
                    font_body,
                    theme::text,
                    false,
                    1.0,
                )
                .style(|s| s.min_width(0.0))
                .into_any();
            }
            let commit = {
                let rename = rename.clone();
                Rc::new(move || {
                    let typed = rename_buf.get_untracked().trim().to_string();
                    // An empty name is refused rather than committed: a row with
                    // no name is unfindable, and the snippet already has one.
                    if !typed.is_empty() {
                        (rename)(id, typed);
                    }
                    renaming.set(None);
                })
            };
            let on_escape = Rc::new(move || renaming.set(None));
            edit_field(
                rename_buf,
                FieldCfg {
                    placeholder: "Snippet name",
                    autofocus: true,
                    on_submit: Some(commit.clone()),
                    on_blur: Some(commit),
                    on_escape: Some(on_escape),
                    ..Default::default()
                },
            )
            .style(|s| s.width_full())
            .into_any()
        }
    })
    .style(|s| s.min_width(0.0).flex_grow(1.0_f32));

    let heading = {
        let mut cells: Vec<floem::AnyView> = vec![name_view.into_any(), abbrev_view.into_any()];
        cells.push(
            text(right)
                .style(|s| {
                    s.font_size(font_label())
                        .color(theme::text_faint())
                        .flex_shrink(0.0_f32)
                })
                .into_any(),
        );
        floem::views::stack_from_iter(cells).style(|s| {
            s.flex_row()
                .items_center()
                .width_full()
                .gap(theme::scaled(8.0))
        })
    };

    // The body, collapsed and clipped to three lines — the same treatment the
    // history panel gives a statement, and the same reason: it is SQL, in the
    // face the editor it came from uses.
    // Syntax-coloured, as the history panel's preview is: this is the panel's
    // one large block of text, and colouring it is what stops a list of saved
    // queries reading as a wall of grey. The colours are the editor's own
    // (`sql_highlight`), so a snippet looks in the library the way it will once
    // it is inserted — the identifiers, which are most of the text, stay in the
    // quiet base and only keywords, strings and numbers carry colour.
    let body_view = highlight_sql_mono(
        snippet::collapsed(&snip.body),
        term.clone(),
        font_body,
        theme::text_dim,
        1.4,
        dialect,
    )
    .style(move |s| {
        s.width_full()
            .min_width(0.0)
            .max_height((font_body() as f64) * 1.4 * 3.0)
    })
    // **The `min_width(0)` on the `Clip` is what makes the preview wrap**, and
    // it has to be repeated here because `.clip()` is not a style: it wraps the
    // view in an unstyled `Clip` *node*, so the styles above land on the text
    // and this node sits between them and the parent. `collapsed` folds a body
    // to one long line, so three visible lines are three *soft-wrapped* ones,
    // and a `RichText` soft-wraps only when the width it is handed is narrower
    // than that line. Floem's `container` below is a **row** stack, which makes
    // the `Clip` a main-axis flex item, and a main-axis item's automatic
    // minimum size is its content's — the whole statement. It took that width,
    // the text's `width_full` resolved against it, and the preview became one
    // line cut off at the panel's edge. Before the background container was
    // added the `Clip` was a *cross*-axis item in the row's `flex_col`, where
    // no content-based minimum applies and an auto width simply stretches to
    // the row, which is why the same text wrapped there without being asked.
    .clip()
    .style(|s| s.width_full().min_width(0.0));
    // **On the editor's surface, not the panel's.** The token colours are
    // reproductions of published palettes (One Dark Pro, Tokyo Night, Catppuccin
    // Latte) tuned against the *editor* background, which `contrast.rs` gates
    // them on and deliberately does not gate anywhere else — and the editor
    // theme is chosen independently of the light/dark UI theme, so Latte's
    // dark-on-light tokens can be live while the panel is dark. Painting the
    // preview on `bg_editor` is what keeps that pairing the one it was designed
    // for; the AI panel's code blocks take the same surface for the same reason.
    let body_view = container(body_view).style(|s| {
        s.width_full()
            .min_width(0.0)
            .margin_top(theme::scaled(5.0))
            .padding_horiz(theme::scaled(7.0))
            .padding_vert(theme::scaled(5.0))
            .background(theme::bg_editor())
            .border_radius(5.0)
    });

    // A chip per `:name` the body holds — read live, never stored, so they can't
    // drift from the body they sit under.
    let names = params::names(&snip.body, dialect);
    let params_row: Option<floem::AnyView> = (!names.is_empty()).then(|| {
        let chips = names.into_iter().map(|n| {
            text(format!(":{n}"))
                .style(|s| {
                    s.font_family(MONO_FAMILY.to_string())
                        .font_size(font_label())
                        .color(theme::text_faint())
                        .padding_horiz(theme::scaled(6.0))
                        .background(theme::bg_editor())
                        .border(1.0)
                        .border_color(theme::border())
                        .border_radius(4.0)
                })
                .into_any()
        });
        floem::views::stack_from_iter(chips)
            .style(|s| {
                s.flex_row()
                    .width_full()
                    .gap(theme::scaled(5.0))
                    .margin_top(theme::scaled(5.0))
            })
            .into_any()
    });

    let mut rows: Vec<floem::AnyView> = vec![heading.into_any(), body_view.into_any()];
    rows.extend(params_row);
    let inner = floem::views::stack_from_iter(rows)
        .style(|s| s.flex_col().width_full().gap(theme::scaled(4.0)));

    // The row's own menu. A built-in can't be renamed or deleted — Duplicate is
    // how you get an editable copy of one.
    let menu_actions = actions.clone();
    let menu_snip = snip.clone();
    container(inner)
        .on_click_stop(move |_| (insert)(click_snip.clone()))
        .on_secondary_click_stop(move |_| {
            // At the pointer, through the app-wide popup menu — the same route
            // the Activity panel's rows take, including closing whatever other
            // menu was open.
            menus.close_except(Some(crate::widgets::MenuId::Popup));
            overlay.popup_anchor.set(None);
            overlay.popup_width.set(180.0);
            overlay
                .popup_menu
                .set(Some(row_menu(&menu_snip, &menu_actions, dialect, conn_id)));
        })
        .style(|s| {
            s.width_full()
                .padding_horiz(theme::scaled(12.0))
                .padding_vert(theme::scaled(9.0))
                .border_bottom(1.0)
                .border_color(theme::border())
                .hover(|s| s.background(theme::row_hover_soft()))
        })
}

/// The row's right-click menu.
///
/// **The two ways to use a snippet come first**, in the order the row itself
/// offers them: a click already inserts, and the entry says so for anyone who
/// looked in the menu for it. Name and abbrev have no entries of their own —
/// *Edit* changes both, along with the body, and a menu with three ways into one
/// dialog is three things to read rather than one.
fn row_menu(
    snip: &Snippet,
    actions: &Rc<crate::SnippetActions>,
    dialect: SqlDialect,
    conn_id: u64,
) -> Vec<MenuEntry> {
    let id = snip.id;
    let builtin = snip.source == Source::Builtin;

    let insert = actions.insert.clone();
    let insert_snip = snip.clone();
    let open = actions.open_in_tab.clone();
    let open_snip = snip.clone();
    let edit = actions.edit.clone();
    let duplicate = actions.duplicate.clone();
    let remove = actions.remove.clone();

    let mut entries = vec![
        MenuEntry::action("Insert into editor", move || (insert)(insert_snip.clone())),
        MenuEntry::action("Open in new tab", move || (open)(open_snip.clone())),
    ];
    if !builtin {
        entries.push(MenuEntry::action("Edit", move || (edit)(id)));
        entries.push(scope_menu(snip, actions, dialect, conn_id));
    }
    entries.push(MenuEntry::action("Duplicate", move || (duplicate)(id)));
    if !builtin {
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::action("Delete", move || (remove)(id)));
    }
    entries
}

/// The **Show in** submenu: which connections this snippet is offered on.
///
/// The three choices come from `snippet::scope_options`, narrowest first — the
/// same order the panel's bands are in, so the choice you pick second is the
/// heading the row moves to second. The current one is tinted rather than
/// ticked, the convention the cell editors' value pickers already use.
///
/// Named "Show in" rather than "Scope": the row is answering *where does this
/// appear*, which is the question someone right-clicking has, and "scope" is a
/// word from the storage model rather than from the panel.
fn scope_menu(
    snip: &Snippet,
    actions: &Rc<crate::SnippetActions>,
    dialect: SqlDialect,
    conn_id: u64,
) -> MenuEntry {
    let id = snip.id;
    let current = snip.scope.clone();
    let children = snippet::scope_options(dialect, conn_id)
        .into_iter()
        .map(|scope| {
            let label = match &scope {
                // Not the connection's *name*: the panel is already scoped to
                // the active connection, and the band this choice moves the row
                // to says "THIS CONNECTION" too.
                snippet::Scope::Conn(_) => "This connection".to_string(),
                snippet::Scope::Dialect(d) => format!("Every {} connection", d.engine_label()),
                _ => "All connections".to_string(),
            };
            let held = scope == current;
            let set_scope = actions.set_scope.clone();
            let act = move || (set_scope)(id, scope.clone());
            if held {
                MenuEntry::action_colored(label, theme::accent, act)
            } else {
                MenuEntry::action(label, act)
            }
        })
        .collect();
    MenuEntry::sub("Show in", children)
}

/// Current wall-clock time, unix millis — for the "3d ago" label, exactly as the
/// history panel computes it.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
