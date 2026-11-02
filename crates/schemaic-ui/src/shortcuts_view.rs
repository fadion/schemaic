//! The Shortcuts modal — every key the app answers, and the one place a
//! command's key is changed.
//!
//! Opened from the header's help (?) glyph. The rows come from
//! [`crate::shortcuts`]: the keymap's commands, whose keys the user can move,
//! and the fixed keys of `SHORTCUTS`, which they cannot. This view owns only
//! the layout and the editing gesture.
//!
//! **Changing a key.** A command row's menu offers *Assign*, *Clear* and
//! *Reset to default*; double-click or Enter also assigns. Assign takes the next
//! chord the row hears: **the row is the capture**, a focused view that takes
//! every key while it records. Its listener's `Stop` keeps the window's global
//! keys off the press; the focus ring's Tab step and Escape blur, which floem
//! runs on the same view regardless, stand down because the ring asks whether
//! the row took the press (`widgets::in_focus_ring_unless`). A chord another command
//! holds asks *Replace* or *Cancel*; a fixed or reserved one is refused with
//! its reason and the row goes on listening. Nothing is resolved silently.

use std::rc::Rc;

use floem::AnyView;
use floem::event::{Event, EventListener, EventPropagation};
use floem::keyboard::{Key, Modifiers, NamedKey};
use floem::prelude::*;
use schemaic_core::keymap::{Chord, Command, Verdict};

use crate::i18n::tr;
use crate::shortcuts::{
    self, Describe, FixedRow, Group, SHORTCUTS, chord_label, command_label, keymap, keys_label,
};
use crate::widgets::{
    ActionKind, FocusRing, MenuEntry, ROW_TAB_STRIDE, VALUE_TAB, action_button, autohide,
    focus_root_with_ring, in_focus_ring_unless, modal_title_owned, panel_style,
};
use crate::{FieldCfg, LayoutUi, edit_field, theme};

/// Where the modal's editing gesture stands.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Edit {
    Idle,
    /// The command's row is waiting for a chord; `refused` says why the last
    /// one could not be taken.
    Capturing {
        command: Command,
        refused: Option<String>,
    },
    /// `chord` is `holder`'s: replace it, or cancel.
    Confirm {
        command: Command,
        chord: Chord,
        holder: Command,
    },
}

impl Edit {
    fn is_for(&self, c: Command) -> bool {
        match self {
            Edit::Idle => false,
            Edit::Capturing { command, .. } | Edit::Confirm { command, .. } => *command == c,
        }
    }
}

/// What every command row shares.
#[derive(Clone)]
struct RowCtx {
    edit: RwSignal<Edit>,
    ring: FocusRing,
    fixed: Rc<Vec<FixedRow>>,
}

impl RowCtx {
    /// Act on a chord the capturing row heard.
    fn captured(&self, command: Command, chord: Chord) {
        let keys: Vec<_> = self.fixed.iter().map(|r| r.key).collect();
        match keymap().with_untracked(|m| m.check(command, chord, &keys)) {
            Verdict::Free => {
                self.assign(command, chord);
            }
            Verdict::Unchanged => self.edit.set(Edit::Idle),
            Verdict::Taken(holder) => self.edit.set(Edit::Confirm {
                command,
                chord,
                holder,
            }),
            Verdict::Fixed(i) => self.edit.set(Edit::Capturing {
                command,
                refused: Some(tr!(
                    "keymap-fixed",
                    keys = chord_label(chord),
                    holder = self
                        .fixed
                        .get(i)
                        .map(|r| (r.describe)())
                        .unwrap_or_default()
                )),
            }),
            Verdict::Refused(r) => self.edit.set(Edit::Capturing {
                command,
                refused: Some(tr!(
                    "keymap-refused",
                    keys = chord_label(chord),
                    reason = shortcuts::refusal_text(r)
                )),
            }),
        }
    }

    /// Give `command` the chord — taking it from whichever command held it —
    /// and save.
    fn assign(&self, command: Command, chord: Chord) {
        let keys: Vec<_> = self.fixed.iter().map(|r| r.key).collect();
        let mut map = keymap().get_untracked();
        if map.assign(command, chord, &keys) {
            shortcuts::save_keymap(map);
        }
        self.edit.set(Edit::Idle);
    }

    fn clear(&self, command: Command) {
        let mut map = keymap().get_untracked();
        map.clear(command);
        shortcuts::save_keymap(map);
        self.edit.set(Edit::Idle);
    }

    /// Back to the default — through the same check as an assignment, so a
    /// default since given to another command asks before it is taken back.
    fn reset(&self, command: Command) {
        self.captured(command, command.default_chord());
    }
}

/// Does a row match the search box? Its description or its keys, as shown,
/// case-insensitively — and the keys with the `+`s dropped too, so `ctrlt`
/// finds Ctrl+T.
fn row_matches(query: &str, what: &str, keys: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let keys = keys.to_lowercase();
    let bare = |s: &str| s.replace(['+', ' '], "");
    what.to_lowercase().contains(&q) || keys.contains(&q) || bare(&keys).contains(&bare(&q))
}

/// The keys a command row shows: its chord, or "Not set".
fn command_keys_text(command: Command) -> String {
    shortcuts::command_chord_label(command).unwrap_or_else(|| tr!("keymap-unset"))
}

/// One fixed line: description on the left, the key combo on the right in a
/// monospace pill.
///
/// **The description wraps and the pill never does.** `flex_grow(1) +
/// min_width(0)` lets the description shrink below its natural width, so a
/// German row that runs longer than the modal breaks onto a second line instead
/// of pushing the pill past the panel's edge; `flex_shrink(0)` keeps a long
/// chord (`Strg+Umschalt+C / Strg+Umschalt+V`) whole on one line.
fn shortcut_row(keys: &'static str, desc: String) -> impl IntoView {
    h_stack((
        text(desc).style(|s| {
            s.color(theme::text())
                .font_size(theme::font_body())
                .flex_grow(1.0_f32)
                .min_width(0.0)
        }),
        // Respelled for the platform and language here rather than in the
        // table: one choke point for every row the modal shows. See
        // `shortcuts::keys_label`.
        key_pill(keys_label(keys).into_owned()),
    ))
    .style(row_style)
}

fn row_style(s: floem::style::Style) -> floem::style::Style {
    s.width_full()
        .flex_row()
        .items_center()
        .gap(theme::scaled(12.0))
        .padding_vert(theme::scaled(3.0))
}

/// The keycap the modal draws, a size up from a menu's.
fn key_pill(keys: String) -> impl IntoView {
    text(keys).style(|s| {
        s.color(theme::text_muted())
            .font_size(theme::font_label())
            .font_family("IBM Plex Mono".to_string())
            .background(theme::bg_deepest())
            .padding_horiz(theme::scaled(6.0))
            .padding_vert(theme::scaled(2.0))
            .border_radius(4.0)
            .flex_shrink(0.0_f32)
    })
}

/// A command's line: what it does and its key, and — while it is being changed
/// — the prompt, the refusal, or the *Replace* / *Cancel* question under it.
fn command_row(command: Command, ctx: RowCtx, tabindex: u32) -> impl IntoView {
    let edit = ctx.edit;
    let right = dyn_container(
        move || (keymap().with(|m| m.chord(command)), edit.get()),
        move |(_, state)| match state {
            Edit::Capturing { command: c, .. } if c == command => text(tr!("keymap-press"))
                .style(|s| {
                    s.color(theme::accent())
                        .font_size(theme::font_label())
                        .flex_shrink(0.0_f32)
                })
                .into_any(),
            _ => match shortcuts::command_chord_label(command) {
                Some(keys) => key_pill(keys).into_any(),
                None => text(tr!("keymap-unset"))
                    .style(|s| {
                        s.color(theme::text_faint())
                            .font_size(theme::font_label())
                            .flex_shrink(0.0_f32)
                    })
                    .into_any(),
            },
        },
    );
    let line = h_stack((
        text(command_label(command)).style(|s| {
            s.color(theme::text())
                .font_size(theme::font_body())
                .flex_grow(1.0_f32)
                .min_width(0.0)
        }),
        right,
    ))
    .style(row_style);
    let row = v_stack((line, below(command, ctx.clone())));
    let id = row.id();

    let start = move || {
        edit.set(Edit::Capturing {
            command,
            refused: None,
        });
        id.request_focus();
    };
    let menu = {
        let ctx = ctx.clone();
        move || {
            let Some(ch) = crate::widgets::menu_channel() else {
                return;
            };
            let (c1, c2) = (ctx.clone(), ctx.clone());
            let map = keymap().get_untracked();
            let entries = vec![
                MenuEntry::action(tr!("keymap-assign"), start),
                MenuEntry::action(tr!("keymap-clear"), move || c1.clear(command))
                    .disabled(map.chord(command).is_none()),
                MenuEntry::action(tr!("keymap-reset"), move || c2.reset(command))
                    .disabled(map.is_default(command)),
            ];
            ch.menus.close_except(Some(crate::widgets::MenuId::Popup));
            ch.anchor.set(None);
            ch.width.set(crate::widgets::menu_panel_width(&entries));
            // The keyboard comes back to this row when the menu goes, as it
            // does to a tree row (`schema_tree::arm_menu_return`).
            crate::widgets::set_menu_return(crate::widgets::menu_return(move || {
                id.request_focus()
            }));
            ch.menus.popup.set(Some(entries));
        }
    };
    let key_menu = menu.clone();
    let key_start = start;
    let key_ctx = ctx.clone();
    // **Did this row's own listener take the press?** Floem runs every
    // listener a view has and ORs their answers, so the `Stop` below does not
    // keep the focus ring's Tab step or Escape blur from running after it — the
    // ring asks this instead (`in_focus_ring_unless`). A flag for *this press*
    // rather than "is it capturing": the Escape that ends a capture has already
    // set the state back to idle by the time the ring's listener asks.
    let took = Rc::new(std::cell::Cell::new(false));
    let took_here = took.clone();
    let row = row
        .on_event(EventListener::KeyDown, move |e| {
            let Event::KeyDown(ke) = e else {
                return EventPropagation::Continue;
            };
            let key = &ke.key.logical_key;
            let plain = ke.modifiers.is_empty();
            let answer = match key_ctx.edit.get_untracked() {
                Edit::Capturing { command: c, .. } if c == command => {
                    if *key == Key::Named(NamedKey::Escape) && plain {
                        key_ctx.edit.set(Edit::Idle);
                    } else if let Some(chord) =
                        shortcuts::chord_of(key, &ke.key.physical_key, ke.modifiers)
                    {
                        key_ctx.captured(command, chord);
                    }
                    // A lone modifier: still listening.
                    EventPropagation::Stop
                }
                Edit::Confirm {
                    command: c, chord, ..
                } if c == command => {
                    match key {
                        Key::Named(NamedKey::Enter) if plain => key_ctx.assign(command, chord),
                        Key::Named(NamedKey::Escape) if plain => key_ctx.edit.set(Edit::Idle),
                        _ => return EventPropagation::Continue,
                    }
                    EventPropagation::Stop
                }
                _ => match key {
                    Key::Named(NamedKey::Enter) if plain => {
                        key_start();
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::ContextMenu) => {
                        key_menu();
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::F10) if ke.modifiers == Modifiers::SHIFT => {
                        key_menu();
                        EventPropagation::Stop
                    }
                    _ => EventPropagation::Continue,
                },
            };
            took_here.set(answer.is_processed());
            answer
        })
        // A capture the keyboard has left is over — a click on the search
        // box must not leave the row promising to take the next key. Not a
        // pending *Replace*: pressing its buttons is what takes focus away.
        .on_event_cont(EventListener::FocusLost, move |_| {
            if matches!(edit.get_untracked(), Edit::Capturing { command: c, .. } if c == command) {
                edit.set(Edit::Idle);
            }
        })
        .on_secondary_click_stop(move |_| menu())
        .on_double_click_stop(move |_| start())
        .style(move |s| {
            let active = edit.with(|e| e.is_for(command));
            s.width_full()
                .flex_col()
                .padding_horiz(theme::scaled(6.0))
                .margin_horiz(theme::scaled(-6.0))
                .border_radius(4.0)
                .apply_if(active, |s| s.background(theme::bg_deepest()))
                .hover(|s| s.background(theme::bg_deepest()))
                .focus(|s| s.background(theme::bg_deepest()))
                .focus_visible(|s| s.outline(0.0))
        });
    in_focus_ring_unless(row, ctx.ring, tabindex, move || took.replace(false))
}

/// What stands under a command row while it is being changed.
fn below(command: Command, ctx: RowCtx) -> impl IntoView {
    let edit = ctx.edit;
    dyn_container(
        move || edit.get(),
        move |state| match state {
            Edit::Capturing {
                command: c,
                refused: Some(why),
            } if c == command => text(why)
                .style(|s| {
                    s.color(theme::error())
                        .font_size(theme::font_label())
                        .padding_bottom(theme::scaled(3.0))
                })
                .into_any(),
            Edit::Confirm {
                command: c,
                chord,
                holder,
            } if c == command => {
                let (yes, no) = (ctx.clone(), ctx.clone());
                h_stack((
                    text(tr!(
                        "keymap-taken",
                        keys = chord_label(chord),
                        holder = command_label(holder)
                    ))
                    .style(|s| {
                        s.color(theme::text_muted())
                            .font_size(theme::font_label())
                            .flex_grow(1.0_f32)
                            .min_width(0.0)
                    }),
                    action_button(
                        tr!("keymap-cancel"),
                        ActionKind::Neutral,
                        true,
                        ctx.ring.clone(),
                        30,
                        move || no.edit.set(Edit::Idle),
                    ),
                    action_button(
                        tr!("keymap-replace"),
                        ActionKind::Primary,
                        true,
                        ctx.ring.clone(),
                        31,
                        move || yes.assign(command, chord),
                    ),
                ))
                .style(|s| {
                    s.width_full()
                        .items_center()
                        .gap(theme::scaled(8.0))
                        .padding_bottom(theme::scaled(4.0))
                })
                .into_any()
            }
            _ => empty().into_any(),
        },
    )
}

/// A titled group: its commands, then its fixed keys, each kept only if it
/// matches the search.
fn shortcut_group(
    group: Group,
    rows: &'static [(&'static str, Describe)],
    query: &str,
    ctx: &RowCtx,
    next_tab: &mut u32,
) -> Option<AnyView> {
    let mut views: Vec<AnyView> = Vec::new();
    for &command in Command::ALL {
        if Group::of(command) != group
            || !row_matches(query, &command_label(command), &command_keys_text(command))
        {
            continue;
        }
        views.push(command_row(command, ctx.clone(), *next_tab).into_any());
        *next_tab += ROW_TAB_STRIDE;
    }
    for (keys, describe) in rows {
        let what = describe();
        if row_matches(query, &what, &keys_label(keys)) {
            views.push(shortcut_row(keys, what).into_any());
        }
    }
    if views.is_empty() {
        return None;
    }
    Some(
        v_stack((
            text(group.label()).style(|s| {
                s.font_size(theme::font_label())
                    .color(theme::text_dim())
                    .margin_bottom(theme::scaled(2.0))
            }),
            v_stack_from_iter(views).style(|s| s.flex_col()),
        ))
        .style(|s| s.flex_col().gap(theme::scaled(2.0)))
        .into_any(),
    )
}

/// Every group, filtered by `query`.
fn groups(query: &str, ctx: &RowCtx) -> AnyView {
    let mut next_tab = VALUE_TAB;
    let views: Vec<AnyView> = SHORTCUTS
        .iter()
        .filter_map(|(group, rows)| shortcut_group(*group, rows, query, ctx, &mut next_tab))
        .collect();
    if views.is_empty() {
        return text(tr!("keymap-no-match"))
            .style(|s| s.color(theme::text_dim()).font_size(theme::font_body()))
            .into_any();
    }
    v_stack_from_iter(views)
        .style(|s| s.flex_col().gap(theme::scaled(25.0)).width_full())
        .into_any()
}

// This is the app's *only* keyboard documentation, and for Ctrl+H and Ctrl+G it
// is the only affordance of any kind — so a binding missing here is a feature
// nobody can find. A test in `shortcuts.rs` fails the build when a
// Ctrl/Alt+letter binding exists with no row.
pub(crate) fn help_overlay(layout: LayoutUi) -> impl IntoView {
    let open = layout.help_open;

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = Rc::new(move || open.set(false));
            let ring = FocusRing::new();
            let ctx = RowCtx {
                edit: RwSignal::new(Edit::Idle),
                ring: ring.clone(),
                fixed: Rc::new(shortcuts::fixed_rows()),
            };
            let query = RwSignal::new(String::new());

            let search = edit_field(
                query,
                FieldCfg {
                    placeholder_of: Some(Rc::new(|| tr!("keymap-search"))),
                    clearable: true,
                    focus: Some((ring.clone(), 10)),
                    ..Default::default()
                },
            )
            .style(|s| s.width_full());
            let head = v_stack((
                text(tr!("keymap-hint")).style(|s| {
                    s.color(theme::text_dim())
                        .font_size(theme::font_label())
                        .width_full()
                }),
                search,
            ))
            .style(|s| {
                s.flex_col()
                    .gap(theme::scaled(8.0))
                    .padding_horiz(theme::scaled(14.0))
                    .padding_top(theme::scaled(12.0))
                    .width_full()
            });

            // Rebuilt on every keystroke of the search, never on a key change:
            // each row follows the keymap itself, so assigning a key keeps the
            // list — and the keyboard on its row — where it is.
            let body_ctx = ctx.clone();
            let body = dyn_container(
                move || query.get(),
                move |q| {
                    // A row being changed that the search hides is a change
                    // with nowhere to finish.
                    if body_ctx.edit.get_untracked() != Edit::Idle {
                        body_ctx.edit.set(Edit::Idle);
                    }
                    groups(&q, &body_ctx)
                },
            )
            .style(|s| s.padding(theme::scaled(14.0)).width_full());
            // Scroll the body so the modal never overflows the window.
            let body = autohide(scroll(body)).style(|s| {
                s.width_full()
                    .max_height(crate::widgets::modal_body_h(520.0))
            });

            let panel = v_stack((
                modal_title_owned(tr!("shortcuts-title"), close.clone(), ring.clone()),
                head,
                body,
            ))
            .on_click_stop(|_| {})
            .style(|s| {
                panel_style(s)
                    .background(theme::bg_panel())
                    // Room for German, whose rows and chords both run longer;
                    // a row that still does not fit wraps (`shortcut_row`).
                    .width(crate::widgets::modal_w(552.0))
            });

            let esc = close.clone();
            focus_root_with_ring(
                stack((crate::widgets::dismiss_layer(move || close()), panel)),
                ring,
            )
            .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
            .style(|s| {
                s.size_full()
                    .items_center()
                    .justify_center()
                    .background(theme::modal_backdrop())
            })
            .into_any()
        },
    )
    .style(move |s| {
        if open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

#[cfg(test)]
mod tests {
    use super::row_matches;

    #[test]
    fn an_empty_search_keeps_every_row() {
        assert!(row_matches("", "New query tab", "Ctrl+T"));
        assert!(row_matches("   ", "New query tab", "Ctrl+T"));
    }

    #[test]
    fn a_search_finds_a_row_by_what_it_does_or_by_its_keys() {
        assert!(row_matches("query tab", "New query tab", "Ctrl+T"));
        assert!(row_matches("NEW", "New query tab", "Ctrl+T"));
        assert!(row_matches("ctrl+t", "New query tab", "Ctrl+T"));
        assert!(!row_matches("palette", "New query tab", "Ctrl+T"));
    }

    #[test]
    fn keys_are_found_without_their_plus_signs() {
        assert!(row_matches("ctrlt", "New query tab", "Ctrl+T"));
        assert!(row_matches(
            "ctrl shift p",
            "Command palette",
            "Ctrl+Shift+P"
        ));
        assert!(!row_matches("ctrlp", "New query tab", "Ctrl+T"));
    }
}
