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
    MenuEntry, autohide, debounced, highlight_mono, highlight_text, section_title, toolbar_icon,
};
use crate::{FieldCfg, OverlayUi, Ui, edit_field, icons, theme};

/// Which of a row's two texts an inline field is editing.
#[derive(Clone, Copy, PartialEq)]
enum RowEdit {
    Name,
    /// The expansion trigger. Committing an empty one *removes* the abbrev,
    /// which is the only way to take one back off a snippet.
    Abbrev,
}

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
    // Which row is being edited inline, and which of its two texts. Inline
    // rather than a modal because naming a saved query is the same act as
    // renaming a tab, which is already inline — and there is no text-prompt
    // modal in this codebase to reach for.
    let renaming: RwSignal<Option<(u64, RowEdit)>> = RwSignal::new(None);
    let rename_buf = RwSignal::new(String::new());

    let groups = create_memo(move |_| {
        let conn = active_conn.get();
        let q = search.get();
        items.with(|all| snippet::grouped(all, dialect.get(), conn, &q))
    });

    let list = dyn_container(move || groups.get(), {
        let actions = actions.clone();
        move |groups: Vec<snippet::Group>| {
            if groups.is_empty() {
                return empty_state(items.with_untracked(|v| v.is_empty())).into_any();
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

    let scrolled =
        autohide(scroll(list)).style(|s| s.flex_grow(1.0_f32).width_full().min_height(0.0));

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
            if let Some(id) = (save_current)() {
                // Straight into the rename field: a snippet arrives named after
                // its tab, which is a placeholder rather than an answer.
                rename_buf.set(String::new());
                renaming.set(Some((id, RowEdit::Name)));
            }
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

/// Nothing to show: either the library is empty, or the filter matched nothing.
/// Two different sentences, because "no snippets" under a filter reads as data
/// loss.
fn empty_state(library_empty: bool) -> impl IntoView {
    let msg = if library_empty {
        "No snippets yet — save one with + above."
    } else {
        "No snippet matches."
    };
    container(text(msg).style(|s| {
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
    renaming: RwSignal<Option<(u64, RowEdit)>>,
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
    // The abbrev: a chip when it has one, a field while it is being set, and
    // nothing at all otherwise — an empty chip on every row would be noise on
    // the many snippets that never want a trigger.
    let abbrev_view = dyn_container(move || renaming.get() == Some((id, RowEdit::Abbrev)), {
        let existing = snip.abbrev.clone();
        let set_abbrev = actions.set_abbrev.clone();
        move |editing: bool| {
            if !editing {
                let Some(a) = existing.clone().filter(|a| !a.is_empty()) else {
                    return empty().into_any();
                };
                return h_stack((
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
                .into_any();
            }
            let commit = {
                let set_abbrev = set_abbrev.clone();
                Rc::new(move || {
                    let typed = rename_buf.get_untracked().trim().to_string();
                    // Empty *removes* the abbrev — the only way to take a
                    // trigger back off a snippet, and the reason this commits
                    // an empty value where the name field refuses one.
                    (set_abbrev)(id, (!typed.is_empty()).then_some(typed));
                    renaming.set(None);
                })
            };
            let on_escape = Rc::new(move || renaming.set(None));
            edit_field(
                rename_buf,
                FieldCfg {
                    placeholder: "abbrev",
                    mono: true,
                    autofocus: true,
                    on_submit: Some(commit.clone()),
                    on_blur: Some(commit),
                    on_escape: Some(on_escape),
                    ..Default::default()
                },
            )
            .style(|s| s.width(theme::scaled(90.0)).flex_shrink(0.0_f32))
            .into_any()
        }
    });

    // Renaming this row: the name is replaced by a field, committed on Enter or
    // blur. Same act as an inline tab rename, so it looks the same.
    let name_view = dyn_container(move || renaming.get() == Some((id, RowEdit::Name)), {
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
    let body_view = highlight_mono(
        snippet::collapsed(&snip.body),
        term.clone(),
        font_body,
        theme::text_dim,
        1.4,
    )
    .style(move |s| {
        s.width_full()
            .max_height((font_body() as f64) * 1.4 * 3.0)
            .margin_top(theme::scaled(3.0))
    })
    .clip();

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
            overlay.popup_menu.set(Some(row_menu(
                &menu_snip,
                &menu_actions,
                renaming,
                rename_buf,
            )));
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
fn row_menu(
    snip: &Snippet,
    actions: &Rc<crate::SnippetActions>,
    renaming: RwSignal<Option<(u64, RowEdit)>>,
    rename_buf: RwSignal<String>,
) -> Vec<MenuEntry> {
    let id = snip.id;
    let name = snip.name.clone();
    let abbrev = snip.abbrev.clone().unwrap_or_default();
    let has_abbrev = !abbrev.is_empty();
    let builtin = snip.source == Source::Builtin;

    let open = actions.open_in_tab.clone();
    let open_snip = snip.clone();
    let duplicate = actions.duplicate.clone();
    let remove = actions.remove.clone();

    let mut entries = vec![MenuEntry::action("Open in new tab", move || {
        (open)(open_snip.clone())
    })];
    if !builtin {
        entries.push(MenuEntry::action("Rename…", move || {
            rename_buf.set(name.clone());
            renaming.set(Some((id, RowEdit::Name)));
        }));
        // Named for what it does rather than for the field: an abbrev is a thing
        // you *type* to get the snippet, and "Set abbrev" says less than that.
        let label = if has_abbrev {
            "Change expansion shortcut…"
        } else {
            "Add expansion shortcut…"
        };
        entries.push(MenuEntry::action(label, move || {
            rename_buf.set(abbrev.clone());
            renaming.set(Some((id, RowEdit::Abbrev)));
        }));
    }
    entries.push(MenuEntry::action("Duplicate", move || (duplicate)(id)));
    if !builtin {
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::action("Delete…", move || (remove)(id)));
    }
    entries
}

/// Current wall-clock time, unix millis — for the "3d ago" label, exactly as the
/// history panel computes it.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
