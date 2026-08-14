//! The Manage Connections modal: the saved-connection list + the edit form
//! (`conn_form`), including the password fields' `*`-masking.
//!
//! Masking keeps the real secret out of the native input's buffer (which we can't
//! fully control): the field shows `MASK_CH × len`, and each edit is mapped back
//! onto the real value by diffing the masked display (`reconstruct_real`) — pure
//! logic, unit-tested below.

use std::rc::Rc;

use floem::event::{Event, EventListener, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;
use schemaic_core::connection::Connection;
use schemaic_core::connection::Environment;

use schemaic_core::connection::SshAuth;

use crate::consts::MASK_CH;
use crate::settings::{focusable_dropdown, focusable_toggle_row};
use crate::widgets::{
    ACTION_GAP, ActionKind, FocusRing, action_button_icon, action_face, autohide,
    focus_root_with_ring, form_hint, form_label_style, loading_dots, menu_item_style, modal_title,
    panel_style,
};
use crate::{DraftSignals, FieldCfg, Ui, edit_field, icons, theme};

/// How long Save's check stands in for its label. Long enough to be read on a
/// glance away from the button, short enough that it can't still be showing when
/// the next edit is made and would then be confirming the wrong write.
const SAVE_FLASH: std::time::Duration = std::time::Duration::from_millis(2000);
/// The same for Test's result icon, held longer: a failure is worth going back
/// and looking at, and unlike Save there is nothing else on screen that says how
/// it went.
const TEST_FLASH: std::time::Duration = std::time::Duration::from_millis(4000);

/// The engine choice in the connection form's **Type** picker. Backs a
/// [`crate::settings::focusable_dropdown`]; the selection is persisted into `Connection::db_type`
/// (as the label string) and drives the DB layer's engine + the editor's SQL
/// dialect via `schemaic_db::Engine::from_db_type` / `SqlDialect::from_db_type`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DbKind {
    MySql,
    Postgres,
}

impl DbKind {
    const ALL: [DbKind; 2] = [DbKind::MySql, DbKind::Postgres];

    fn label(self) -> &'static str {
        match self {
            DbKind::MySql => "MySQL",
            DbKind::Postgres => "PostgreSQL",
        }
    }

    /// The default TCP port for this engine (offered when switching engines).
    /// Shared with the port field's parse fallback, so the two can't disagree.
    fn default_port(self) -> u16 {
        schemaic_core::connection::default_port(self.label())
    }

    /// Map a persisted `db_type` label back to a picker value (anything not
    /// recognizably Postgres is MySQL — the historical default).
    fn from_db_type(s: &str) -> DbKind {
        if schemaic_core::connection::is_postgres(s) {
            DbKind::Postgres
        } else {
            DbKind::MySql
        }
    }
}

// Fixed width for credential inputs (user/password + SSH user/password/passphrase)
// — narrower than the full-width host/name fields, since credentials are short.
const CONN_FIELD_W: f64 = 200.0;

// ===== moved from lib.rs (connection form + password masking) =====
// One labelled text field for the connection form.
/// One labelled text field, placed in the modal's Tab order at `tabindex`.
///
/// Indices are explicit and spaced rather than implied by build order, because
/// the SSH block is built only when its toggle is on and would otherwise
/// register after the fields that follow it on screen.
fn field(
    lbl: &'static str,
    sig: RwSignal<String>,
    ring: FocusRing,
    tabindex: u32,
) -> impl IntoView {
    v_stack((
        text(lbl).style(form_label_style),
        edit_field(
            sig,
            FieldCfg {
                focus: Some((ring.clone(), tabindex)),
                ..Default::default()
            },
        )
        .style(|s| s.width_full()),
    ))
    .style(|s| s.flex_col().gap(6.0).width_full())
}

// Host (fills) + Port (fixed, ~8 chars) on one line, 25px apart. Shared by the
// normal and SSH field groups so they lay out identically.
fn host_port_row(
    host_lbl: &'static str,
    host: RwSignal<String>,
    port_lbl: &'static str,
    port: RwSignal<String>,
    ring: FocusRing,
    tabindex: u32,
) -> impl IntoView {
    let host_field = field(host_lbl, host, ring.clone(), tabindex)
        .style(|s| s.flex_grow(1.0_f32).min_width(0.0));
    let port_field = v_stack((
        text(port_lbl).style(form_label_style),
        edit_field(
            port,
            FieldCfg {
                // Port follows Host on the same row, so it takes the next slot.
                focus: Some((ring.clone(), tabindex + 1)),
                ..Default::default()
            },
        )
        .style(|s| s.width(96.0)),
    ))
    .style(|s| s.flex_col().gap(6.0).flex_shrink(0.0_f32));
    h_stack((host_field, port_field)).style(|s| s.flex_row().items_start().gap(25.0).width_full())
}

// Key-pair credentials: a private-key path (with a native "Browse…" picker) and
// an optional passphrase. Shown when the SSH auth method is "Key pair".
fn key_pair_fields(draft: DraftSignals, ring: FocusRing, tabindex: u32) -> impl IntoView {
    use floem::file::FileDialogOptions;
    use floem::file_action::open_file;

    let key_sig = draft.ssh_key_path;
    let browse = text("Browse…")
        .on_click_stop(move |_| {
            open_file(
                FileDialogOptions::new().title("Select private key"),
                move |file| {
                    if let Some(info) = file
                        && let Some(path) = info.path.first()
                    {
                        key_sig.set(path.to_string_lossy().to_string());
                    }
                },
            )
        })
        .style(|s| {
            s.flex_shrink(0.0_f32)
                .font_size(theme::FONT_BODY)
                .padding_horiz(10.0)
                .padding_vert(6.0)
                .border_radius(6.0)
                .background(theme::bg_editor())
                .border(1.0)
                .border_color(theme::field_border())
                .color(theme::text())
                .hover(|s| s.border_color(theme::field_border_active()))
        });

    let key_row = v_stack((
        text("Private key").style(form_label_style),
        h_stack((
            edit_field(
                draft.ssh_key_path,
                FieldCfg {
                    focus: Some((ring.clone(), tabindex)),
                    ..Default::default()
                },
            )
            .style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
            browse,
        ))
        .style(|s| s.flex_row().items_center().gap(8.0).width_full()),
    ))
    .style(|s| s.flex_col().gap(6.0).width_full());

    v_stack((
        key_row,
        masked_field("Passphrase", draft.ssh_key_passphrase, ring, tabindex + 1)
            .style(|s| s.width(CONN_FIELD_W)),
    ))
    .style(|s| s.flex_col().gap(20.0).width_full())
}

fn mask_of_len(n: usize) -> String {
    std::iter::repeat_n(MASK_CH, n).collect()
}

/// A small identity-colour dot for the connection list (`[dot] [name]`). An 8px
/// circle filled with the connection's `#rrggbb`; a missing/unparsable colour
/// renders as a same-size transparent spacer so names stay aligned.
fn conn_color_dot(color: Option<String>) -> impl IntoView {
    empty().style(move |s| {
        let s = s.size(8.0, 8.0).flex_shrink(0.0_f32).border_radius(4.0);
        match color.as_deref().and_then(theme::parse_hex) {
            Some(c) => s.background(c),
            None => s,
        }
    })
}

/// Where the keyboard cursor lands when the swatch row is Tab-ed into: on the
/// preset the connection's colour already names, or **nowhere** when it names
/// none of them.
///
/// Nowhere, not the first swatch. A colour outside the preset table is
/// reachable — a hand-edited `connections.json`, or a preset retuned in a later
/// build — and answering "Red" put the halo on a swatch the selection border was
/// not on, which the next Space *wrote*: a connection silently repainted by two
/// keys that look like navigation. A `None` cursor paints no halo, and the first
/// arrow enters the row at its head as it does for a colourless connection.
///
/// The match is case-insensitive on purpose: `#e05252` and `#E05252` are the
/// same colour to every renderer here, so they must be the same swatch.
pub(crate) fn preset_cursor(presets: &[crate::ColorPreset], color: Option<&str>) -> Option<usize> {
    let c = color?.trim();
    presets
        .iter()
        .position(|(_, hex, _)| hex.eq_ignore_ascii_case(c))
}

/// A row of colour swatches that sets the draft's identity colour. Every
/// connection has a colour (new ones are auto-assigned), so there's no "none"
/// option; the selected swatch gets a 2px border in `text()`.
fn color_picker(color: RwSignal<Option<String>>, ring: FocusRing, tabindex: u32) -> impl IntoView {
    let presets = crate::CONN_COLOR_PRESETS;
    // The keyboard's position in the row, and *only* the keyboard's: cleared on
    // blur, so the halo it paints never outlives the focus it stands for. It is
    // separate from `color` because a row that has focus but no colour yet still
    // has to say where the first arrow will land.
    let cursor = RwSignal::new(None::<usize>);

    let swatches = presets.iter().enumerate().map(move |(i, (_, hex, _))| {
        let hex = (*hex).to_string();
        let hx = hex.clone();
        empty()
            .on_click_stop(move |_| {
                cursor.set(Some(i));
                color.set(Some(hex.clone()));
            })
            .style(move |s| {
                let fill = theme::parse_hex(&hx).unwrap_or(floem::peniko::Color::TRANSPARENT);
                let s = s
                    .size(18.0, 18.0)
                    .flex_shrink(0.0_f32)
                    .border_radius(9.0)
                    .background(fill);
                let s = if color.get().as_deref() == Some(hx.as_str()) {
                    s.border(2.0).border_color(floem::peniko::Color::WHITE)
                } else {
                    s
                };
                // `outline`, not a border: an 18px circle that grew by 2px when
                // the keyboard arrived would nudge every swatch after it along
                // the row. An outline is painted outside the box and costs no
                // layout — the same reason the dropdowns suppress floem's with
                // `outline(0)` rather than restyling it.
                if cursor.get() == Some(i) {
                    s.outline(2.0).outline_color(theme::field_border_active())
                } else {
                    s
                }
            })
            .into_any()
    });

    let cursor_start = move || preset_cursor(presets, color.get_untracked().as_deref());
    // One step along the row, selecting as it goes — a radio group, not a list
    // you then have to confirm. `ring_step` because the wrap-at-both-ends rule
    // is the same one the modal's Tab order follows.
    let step = move |backwards: bool| {
        let next = crate::widgets::ring_step(presets.len(), cursor.get_untracked(), backwards)
            .unwrap_or(0);
        cursor.set(Some(next));
        color.set(Some(presets[next].1.to_string()));
    };

    let row = h_stack_from_iter(swatches).style(|s| {
        s.flex_row()
            .items_center()
            .gap(8.0)
            .flex_shrink(0.0_f32)
            // Floem's own focus ring is a magenta outline belonging to no
            // palette here; the cursor halo above is this row's focus signal.
            .focus_visible(|s| s.outline(0.0))
    });
    let row = crate::widgets::in_focus_ring(row, ring, tabindex)
        .on_event(EventListener::FocusGained, move |_| {
            if cursor.get_untracked().is_none() {
                cursor.set(cursor_start());
            }
            EventPropagation::Continue
        })
        .on_event(EventListener::FocusLost, move |_| {
            cursor.set(None);
            EventPropagation::Continue
        })
        .on_event(EventListener::KeyDown, move |e| {
            let Event::KeyDown(ke) = e else {
                return EventPropagation::Continue;
            };
            match ke.key.logical_key {
                Key::Named(NamedKey::ArrowRight) | Key::Named(NamedKey::ArrowDown) => {
                    step(false);
                    EventPropagation::Stop
                }
                Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowUp) => {
                    step(true);
                    EventPropagation::Stop
                }
                // Space/Enter take the swatch the cursor is already on. It is
                // normally selected already — the arrows select as they move —
                // but not on the row's very first focus, where the cursor sits
                // on a colour nobody has chosen yet. With the cursor on nothing
                // (a colour outside the presets) there is no swatch to take, so
                // the key does nothing rather than picking one.
                Key::Named(NamedKey::Space) | Key::Named(NamedKey::Enter) => {
                    if let Some(at) = cursor.get_untracked().or_else(cursor_start) {
                        cursor.set(Some(at));
                        color.set(Some(presets[at].1.to_string()));
                    }
                    EventPropagation::Stop
                }
                _ => EventPropagation::Continue,
            }
        });

    v_stack((
        text("Colour").style(form_label_style),
        // The row shrink-wraps inside a full-width line, so the focus halo hugs
        // the swatches instead of stretching across the form.
        h_stack((row, empty().style(|s| s.flex_grow(1.0_f32))))
            .style(|s| s.flex_row().width_full()),
    ))
    .style(|s| s.flex_col().gap(6.0).width_full())
}

/// Char-diff `old` → `new`: shared-prefix char count, shared-suffix char count,
/// and the text inserted between them. Used to turn a masked-buffer edit back
/// into the same structural change on the real (unmasked) value.
fn diff_edit(old: &str, new: &str) -> (usize, usize, String) {
    let o: Vec<char> = old.chars().collect();
    let n: Vec<char> = new.chars().collect();
    let mut p = 0;
    while p < o.len() && p < n.len() && o[p] == n[p] {
        p += 1;
    }
    let mut s = 0;
    while s < o.len() - p && s < n.len() - p && o[o.len() - 1 - s] == n[n.len() - 1 - s] {
        s += 1;
    }
    let inserted: String = n[p..n.len() - s].iter().collect();
    (p, s, inserted)
}

/// Apply a masked-buffer edit (`prev_disp` → `cur_disp`) to the real value.
/// `prev_disp` and `prev_real` always share a char count, so the diff's prefix/
/// suffix offsets index both. Insertions are localized exactly (the typed char
/// is non-mask, so the diff pins its position); a **pure deletion** of identical
/// mask chars can't be localized, so it collapses at the prefix/suffix boundary
/// — a mid-string backspace removes a boundary char rather than the one under
/// the caret. Acceptable for password fields (end-editing is the common case),
/// and selection-replace stays correct because the inserted char re-anchors it.
fn reconstruct_real(prev_real: &str, prev_disp: &str, cur_disp: &str) -> String {
    let (p, s, inserted) = diff_edit(prev_disp, cur_disp);
    let rc: Vec<char> = prev_real.chars().collect();
    let p = p.min(rc.len());
    let keep = rc.len().saturating_sub(s).max(p);
    let mut next = String::with_capacity(rc.len() + inserted.len());
    next.extend(rc[..p].iter());
    next.push_str(&inserted);
    next.extend(rc[keep..].iter());
    next
}

// A password field on the shared `edit_field`: the editor doc holds only `*`s
// (a hidden `disp` buffer mirrors it), and each masked edit is diffed back onto
// the real value (`sig`). The real characters never enter the doc, so copy/cut
// only ever yield `*`s and the password can't leak via the clipboard. External
// changes to `sig` (loading a saved connection) re-mask the buffer to match.
// `disp` and `real` always share a char count, which is what makes the diff in
// `reconstruct_real` map masked-buffer edits back to the right real positions.
fn masked_field(
    lbl: &'static str,
    sig: RwSignal<String>,
    ring: FocusRing,
    tabindex: u32,
) -> impl IntoView {
    let real = sig;
    let disp = RwSignal::new(mask_of_len(real.get_untracked().chars().count()));
    // Untracked mirrors of the last state each effect committed, so an effect
    // can tell an external change from its own write and not loop.
    let mirror_disp = RwSignal::new(disp.get_untracked());
    let mirror_real = RwSignal::new(real.get_untracked());

    // Masked buffer edited (via the editor) → apply the same structural edit to
    // the real value, then re-mask.
    create_effect(move |_| {
        let cur = disp.get();
        if cur == mirror_disp.get_untracked() {
            return; // our own re-mask, or genuinely unchanged
        }
        let next = reconstruct_real(
            &mirror_real.get_untracked(),
            &mirror_disp.get_untracked(),
            &cur,
        );
        let masked = mask_of_len(next.chars().count());
        mirror_real.set(next.clone());
        mirror_disp.set(masked.clone());
        real.set(next);
        if cur != masked {
            disp.set(masked);
        }
    });

    // Real value changed from the outside (e.g. loading a saved connection) →
    // re-mask the buffer to its new length.
    create_effect(move |_| {
        let r = real.get();
        if r == mirror_real.get_untracked() {
            return; // our own write
        }
        let masked = mask_of_len(r.chars().count());
        mirror_real.set(r);
        mirror_disp.set(masked.clone());
        disp.set(masked);
    });

    v_stack((
        text(lbl).style(form_label_style),
        edit_field(
            disp,
            FieldCfg {
                background: theme::bg_deepest,
                focus: Some((ring.clone(), tabindex)),
                ..Default::default()
            },
        )
        .style(|s| s.width_full()),
    ))
    .style(|s| s.flex_col().gap(6.0).width_full())
}

// Manage Connections: list + editable form (create / update / delete).
pub(crate) fn manage_modal(ui: Ui) -> impl IntoView {
    let open = ui.conn.manage_open;
    let connections = ui.conn.connections;
    let draft = ui.conn.draft;
    let select_conn = ui.conn_actions.select_conn.clone();
    let new_conn = ui.conn_actions.new_conn.clone();
    let save_conn = ui.conn_actions.save_conn.clone();
    let delete_conn = ui.conn_actions.delete_conn.clone();
    let test_conn = ui.conn_actions.test_conn.clone();
    let conn_test = ui.conn.conn_test;
    // Transient confirmations standing in for the two safe actions' labels: a
    // check on Save, the result icon on Test. Created here (once, in the stable
    // workspace scope), not inside the open/close `dyn_container`, so a deferred
    // reset never fires on a disposed signal.
    let save_flash = RwSignal::new(false);
    let test_flash = RwSignal::new(false);
    // Test's flash is driven by the *result arriving*, not by the click — the
    // click only starts the round trip, and how long that takes is the server's
    // business. `test_gen` is what keeps an earlier result's timer from cutting a
    // later one short, the same guard `autohide_state` uses; `try_get_untracked`
    // because the window can close while a timer is out.
    let test_gen: RwSignal<u64> = RwSignal::new(0);
    floem::reactive::create_effect(move |_| {
        match conn_test.get() {
            crate::TestState::Ok | crate::TestState::Fail => {
                let g = test_gen.get_untracked().wrapping_add(1);
                test_gen.set(g);
                test_flash.set(true);
                floem::action::exec_after(TEST_FLASH, move |_| {
                    if test_gen.try_get_untracked() == Some(g) {
                        test_flash.set(false);
                    }
                });
            }
            // Editing any field resets the state, which withdraws the result the
            // icon was reporting.
            _ => {
                if test_flash.get_untracked() {
                    test_flash.set(false);
                }
            }
        }
    });

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let select = select_conn.clone();
            let list = dyn_stack(
                move || connections.get(),
                |c: &Connection| c.id,
                move |c| {
                    let id = c.id;
                    let select = select.clone();
                    // `[colour dot] [name]`, reflecting this connection's *stored*
                    // name/colour. Reads `connections` (not the draft) so the row
                    // refreshes only when Save writes the edit back — never live while
                    // the form is being typed (which would falsely imply a real-time
                    // update). Reactive because `dyn_stack` reuses a row's view for an
                    // unchanged id, so a static label would go stale after Save.
                    let label = dyn_container(
                        move || {
                            connections.with(|cs| {
                                cs.iter()
                                    .find(|c| c.id == id)
                                    .map(|c| (c.name.clone(), c.color.clone()))
                                    .unwrap_or_default()
                            })
                        },
                        |(name, color)| {
                            h_stack((
                                conn_color_dot(color),
                                text(name).style(|s| s.font_size(theme::FONT_BODY)),
                            ))
                            .style(|s| s.flex_row().items_center().gap(8.0))
                            .into_any()
                        },
                    );
                    container(label)
                        .on_click_stop(move |_| (select)(id))
                        // Full-width row: resting `conn_list_text`, hover text
                        // brightens (no bg), selected = bright text on a full-width
                        // `conn_list_sel_bg`.
                        .style(move |s| {
                            let selected = draft.id.get() == Some(id);
                            let s = s.width_full().padding_horiz(12.0).padding_vert(11.0);
                            if selected {
                                s.color(theme::conn_list_sel_text())
                                    .background(theme::conn_list_sel_bg())
                            } else {
                                s.color(theme::conn_list_text())
                                    .hover(|s| s.color(theme::conn_list_sel_text()))
                            }
                        })
                },
            )
            // Full width so the selected row's background spans the pane; +5px gap
            // between rows.
            .style(|s| s.flex_col().width_full().gap(5.0));

            let new_c = new_conn.clone();
            let add = container(
                h_stack((
                    icons::icon(icons::CIRCLE_PLUS, 16.0),
                    text("New connection").style(|s| s.font_size(theme::FONT_BODY)),
                ))
                .style(|s| s.flex_row().items_center().gap(8.0).color(theme::accent())),
            )
            .on_click_stop(move |_| (new_c)())
            .style(|s| menu_item_style(s).justify_center());

            let left = v_stack((
                autohide(scroll(list)).style(|s| s.flex_grow(1.0_f32).width_full().min_height(0.0)),
                add,
            ))
            .style(|s| {
                s.width(210.0)
                    .flex_shrink(0.0_f32)
                    .height_full()
                    .flex_col()
                    .border_right(1.0)
                    .border_color(theme::border())
                    .padding_vert(6.0)
            });

            // The Tab order belongs to the modal, not the form: Escape is
            // answered here at the root, and it has to be able to ask the ring
            // whether a control has a popup open before closing anything.
            let ring = FocusRing::new();
            let right = conn_form(
                draft,
                save_conn.clone(),
                delete_conn.clone(),
                test_conn.clone(),
                conn_test,
                save_flash,
                test_flash,
                ring.clone(),
            );
            let root_ring = ring;

            let body = h_stack((left, right))
                .style(|s| s.width_full().flex_grow(1.0_f32).flex_row().min_height(0.0));

            let close: Rc<dyn Fn()> = Rc::new(move || open.set(false));
            let panel = v_stack((modal_title("Manage Connections", close), body))
                .on_click_stop(|_| {})
                .style(|s| panel_style(s).width(720.0).height(500.0));

            // Dark backdrop, centered panel, click-away or Escape closes.
            //
            // Escape peels one layer at a time and this is the last of them: an
            // open dropdown popup is closed at the *window root* (it holds the
            // keyboard, so neither its own box nor this modal sees the key), then
            // `in_focus_ring` blurs whatever control was focused, and only then
            // does this run.
            //
            // The root also answers Tab, by entering the ring — it is where focus
            // sits when the modal opens and where Escape hands it back.
            focus_root_with_ring(container(panel), root_ring)
                .on_key_down(
                    Key::Named(NamedKey::Escape),
                    |_| true,
                    move |_| open.set(false),
                )
                .on_click_stop(move |_| open.set(false))
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
        if open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

#[allow(clippy::too_many_arguments)] // a UI builder; grouping into a struct adds no clarity
fn conn_form(
    draft: DraftSignals,
    save_conn: Rc<dyn Fn()>,
    delete_conn: Rc<dyn Fn(u64)>,
    test_conn: Rc<dyn Fn()>,
    conn_test: RwSignal<crate::TestState>,
    save_flash: RwSignal<bool>,
    test_flash: RwSignal<bool>,
    // Owned by the modal (Escape consults it there). Indices below are spaced by
    // 10 so a field can be inserted between two without renumbering, and the SSH
    // block — built only once its toggle is on — claims the 100s so it lands
    // between the toggle that reveals it and the fields below.
    ring: FocusRing,
) -> impl IntoView {
    // Editing any connection parameter invalidates a prior Test result, so reset
    // the indicator whenever host/port/user/password or the SSH fields change.
    create_effect(move |_| {
        draft.host.track();
        draft.port.track();
        draft.user.track();
        draft.password.track();
        draft.ssh_enabled.track();
        draft.ssh_host.track();
        draft.ssh_port.track();
        draft.ssh_user.track();
        draft.ssh_password.track();
        draft.ssh_auth.track();
        draft.ssh_key_path.track();
        draft.ssh_key_passphrase.track();
        conn_test.set(crate::TestState::Idle);
    });

    // SSH tunnel fields, shown only when enabled. The toggle is the themed switch
    // (like the other toggles); the fields lay out exactly like the normal ones.
    let ssh_enabled = draft.ssh_enabled;
    let ssh_toggle = focusable_toggle_row(
        "SSH tunnel",
        "Reach the server through an SSH tunnel.",
        draft.ssh_enabled,
        ring.clone(),
        90,
    );
    let ring_ssh = ring.clone();
    let ssh_fields = dyn_container(
        move || ssh_enabled.get(),
        move |on| {
            if !on {
                return empty().into_any();
            }
            let ring = ring_ssh.clone();
            // Authentication method picker (150px), matching the settings dropdowns.
            let auth_field = v_stack((
                text("Authentication").style(form_label_style),
                focusable_dropdown(
                    draft.ssh_auth,
                    SshAuth::ALL,
                    SshAuth::label,
                    ring.clone(),
                    130,
                )
                .style(|s| s.width(150.0)),
            ))
            .style(|s| s.flex_col().gap(6.0));

            // Method-specific credentials, swapped on the chosen auth.
            let ssh_auth = draft.ssh_auth;
            let ring_creds = ring.clone();
            let auth_creds = dyn_container(
                move || ssh_auth.get(),
                move |auth| match auth {
                    SshAuth::Password => {
                        masked_field("SSH password", draft.ssh_password, ring_creds.clone(), 140)
                            .style(|s| s.width(CONN_FIELD_W))
                            .into_any()
                    }
                    SshAuth::KeyPair => key_pair_fields(draft, ring_creds.clone(), 140).into_any(),
                    SshAuth::Agent => form_hint(
                        "Signing is delegated to your running SSH agent (OpenSSH \
                         agent / Pageant on Windows). No key is stored by Schemaic.",
                    )
                    .style(|s| s.width_full())
                    .into_any(),
                },
            )
            .style(|s| s.width_full());

            v_stack((
                host_port_row(
                    "SSH host",
                    draft.ssh_host,
                    "SSH port",
                    draft.ssh_port,
                    ring.clone(),
                    100,
                ),
                field("SSH user", draft.ssh_user, ring.clone(), 120)
                    .style(|s| s.width(CONN_FIELD_W)),
                auth_field,
                auth_creds,
            ))
            // `width_full` so the inner fields fill the pane (the `dyn_container`
            // below must also fill, or these collapse to shrink-wrapped width).
            // A bordered, padded container groups the tunnel fields — the toggle
            // above stays outside it. Outlined rather than tinted, matching the
            // designer's list pane: a fill reads as a *state* (a selected row, a
            // header) everywhere else in the app, and this is only a grouping.
            .style(|s| {
                s.flex_col()
                    .gap(20.0)
                    .width_full()
                    .padding(10.0)
                    .border(1.0)
                    .border_color(theme::border())
                    .border_radius(6.0)
            })
            .into_any()
        },
    )
    .style(|s| s.width_full());

    // "Prominent color in editor" — when on, the identity colour frames the
    // query+results editor (a guard-rail for e.g. production connections). Uses
    // the same themed switch as the AI/terminal settings toggles.
    let prominent_toggle = focusable_toggle_row(
        "Prominent color in editor",
        "Frame the query editor in this connection's colour.",
        draft.prominent_color,
        ring.clone(),
        20,
    );
    // Read-only guard-rail: disables cell edits and blocks write/DDL queries.
    let read_only_toggle = focusable_toggle_row(
        "Read-only",
        "Disable cell edits and block write/DDL queries on this connection.",
        draft.read_only,
        ring.clone(),
        30,
    );

    // Type: engine picker (MySQL / PostgreSQL). The selection drives `draft.db_type`;
    // switching engine also swaps in that engine's default port — but only when the
    // port is still the *other* engine's default (or empty), so a port the user typed
    // is never clobbered. `create_effect`'s `prev` is `None` on the first (load) run,
    // so opening an existing connection never rewrites its saved port.
    let engine = RwSignal::new(DbKind::from_db_type(&draft.db_type.get_untracked()));
    create_effect(move |prev: Option<DbKind>| {
        let k = engine.get();
        draft.db_type.set(k.label().to_string());
        if let Some(prev) = prev
            && prev != k
        {
            let cur = draft.port.get_untracked();
            let c = cur.trim();
            if c.is_empty() || c == prev.default_port().to_string() {
                draft.port.set(k.default_port().to_string());
            }
        }
        k
    });
    let type_field = v_stack((
        text("Type").style(form_label_style),
        // 150px wide (not full width), matching the settings dropdowns' look.
        container(focusable_dropdown(
            engine,
            DbKind::ALL,
            DbKind::label,
            ring.clone(),
            50,
        ))
        .style(|s| s.width(150.0)),
    ))
    .style(|s| s.flex_col().gap(6.0).width_full());

    // Environment picker → the top-bar badge. Defaults to None (no badge).
    let env_field = v_stack((
        text("Environment").style(form_label_style),
        container(focusable_dropdown(
            draft.environment,
            Environment::ALL,
            Environment::label,
            ring.clone(),
            40,
        ))
        .style(|s| s.width(150.0)),
    ))
    .style(|s| s.flex_col().gap(6.0).width_full());

    // Name + Colour sit closer together (20px) than the rest of the form (25px).
    // The swatch row is one Tab stop, between Name and the toggles below it —
    // a radio group, so Left/Right walk it once the keyboard is there.
    let name_color = v_stack((
        field("Name", draft.name, ring.clone(), 10),
        color_picker(draft.color, ring.clone(), 15),
    ))
    .style(|s| s.flex_col().gap(20.0).width_full());

    let fields = v_stack((
        name_color,
        prominent_toggle,
        read_only_toggle,
        env_field,
        type_field,
        host_port_row("Host", draft.host, "Port", draft.port, ring.clone(), 60),
        field("User", draft.user, ring.clone(), 70).style(|s| s.width(CONN_FIELD_W)),
        masked_field("Password", draft.password, ring.clone(), 80).style(|s| s.width(CONN_FIELD_W)),
        ssh_toggle,
        ssh_fields,
    ))
    .style(|s| s.flex_col().gap(20.0).width_full().padding(14.0));

    // The three footer actions are the filled buttons every other modal ends
    // with: Save is the affirmative one, Test its own `Verify` hue (it does real
    // work but changes nothing), Delete red like the preview's Apply on a
    // destructive plan. Delete keeps its trash glyph through the shared
    // icon+label form, so its spacing is the preview footer's Copy and Open in
    // editor rather than a third answer to the same question.
    let del = delete_conn.clone();
    let delete_btn = dyn_container(
        move || draft.id.get(),
        move |id| match id {
            Some(cid) => {
                let del = del.clone();
                action_button_icon(
                    "Delete",
                    icons::TRASH_2,
                    ActionKind::Danger,
                    true,
                    move || (del)(cid),
                )
                .into_any()
            }
            None => empty().into_any(),
        },
    );

    // Test: `Neutral`, the same grey as every modal's Cancel — it commits
    // nothing, and a fill of its own said otherwise. Its label is replaced, in
    // place, by the result icon, which keeps its green or red: with the button no
    // longer carrying a colour, the glyph is the only thing that says how it went.
    // The width is pinned to `Test...` so none of the three faces — label,
    // animated dots, icon — moves the button.
    let test = test_conn.clone();
    let test_btn = action_face(
        "Test...",
        ActionKind::Neutral,
        true,
        dyn_container(
            move || (conn_test.get(), test_flash.get()),
            move |(st, flashing)| match st {
                crate::TestState::Testing => {
                    loading_dots("Test", theme::btn_neutral_text, theme::FONT_BODY).into_any()
                }
                crate::TestState::Ok if flashing => icons::icon(icons::CIRCLE_CHECK, 16.0)
                    .style(|s| s.color(theme::conn_test_ok()))
                    .into_any(),
                crate::TestState::Fail if flashing => icons::icon(icons::CIRCLE_X, 16.0)
                    .style(|s| s.color(theme::conn_test_fail()))
                    .into_any(),
                // Centred, like every other face here. It deliberately does *not*
                // claim `loading_dots`' reserved `Test...` box: doing so pinned
                // the word to the left of a button sized for three dots it isn't
                // showing, which is a permanently off-centre label bought to
                // avoid a one-off shift as the test starts.
                _ => text("Test")
                    .style(move |s| s.font_size(theme::FONT_BODY))
                    .into_any(),
            },
        ),
        move || (test)(),
    );

    // Save: the write is otherwise silent, so the button flashes a check in place
    // of its label. Same pinned width, so the confirmation can't nudge Test.
    // `save_gen` is Save's counterpart to `test_gen`, for the identical reason.
    let save_gen: RwSignal<u64> = RwSignal::new(0);
    let save = save_conn.clone();
    let save_btn = action_face(
        "Save",
        ActionKind::Primary,
        true,
        dyn_container(
            move || save_flash.get(),
            move |flashed| {
                if flashed {
                    icons::icon(icons::CHECK, 16.0).into_any()
                } else {
                    text("Save")
                        .style(move |s| s.font_size(theme::FONT_BODY))
                        .into_any()
                }
            },
        ),
        move || {
            (save)();
            // The same generation guard the Test flash beside this one already
            // has, and for the same reason: unguarded, a second Save inside
            // `SAVE_FLASH` has its confirmation cleared early by the *first*
            // click's timer, so the tick vanishes while the save it reports is
            // the one that just happened. Widening `SAVE_FLASH` to 2000 ms
            // widened that window rather than closing it.
            let g = save_gen.get_untracked().wrapping_add(1);
            save_gen.set(g);
            save_flash.set(true);
            floem::action::exec_after(SAVE_FLASH, move |_| {
                if save_gen.try_get_untracked() == Some(g) {
                    save_flash.set(false);
                }
            });
        },
    );
    let right_actions =
        h_stack((test_btn, save_btn)).style(|s| s.flex_row().items_center().gap(ACTION_GAP));
    let buttons = h_stack((
        delete_btn,
        empty().style(|s| s.flex_grow(1.0_f32)),
        right_actions,
    ))
    .style(|s| {
        s.width_full()
            .flex_row()
            .items_center()
            .padding_horiz(14.0)
            .padding_vert(10.0)
            .border_top(1.0)
            .border_color(theme::border())
    });

    v_stack((
        autohide(scroll(fields)).style(|s| s.flex_grow(1.0_f32).width_full().min_height(0.0)),
        buttons,
    ))
    .style(|s| s.flex_grow(1.0_f32).height_full().flex_col().min_width(0.0))
}

#[cfg(test)]
mod tests {
    use super::{mask_of_len, preset_cursor, reconstruct_real};

    #[test]
    fn the_swatch_cursor_starts_on_the_colour_in_effect() {
        let presets = crate::CONN_COLOR_PRESETS;
        let first = presets[0].1;
        let last = presets[presets.len() - 1].1;
        assert_eq!(preset_cursor(presets, Some(first)), Some(0));
        assert_eq!(preset_cursor(presets, Some(last)), Some(presets.len() - 1));
    }

    /// Both spellings are the same colour to every renderer here, so they have
    /// to be the same swatch.
    #[test]
    fn the_swatch_cursor_reads_hex_case_insensitively() {
        let presets = crate::CONN_COLOR_PRESETS;
        let upper = presets[1].1.to_ascii_uppercase();
        assert_eq!(preset_cursor(presets, Some(&upper)), Some(1));
    }

    /// The defect this replaced: an unrecognised colour answered "the first
    /// swatch", so the halo sat where the selection border did not — and the
    /// next Space wrote it, repainting the connection with two keys that look
    /// like navigation.
    #[test]
    fn a_colour_outside_the_presets_puts_the_cursor_nowhere() {
        let presets = crate::CONN_COLOR_PRESETS;
        assert_eq!(preset_cursor(presets, Some("#123456")), None);
        assert_eq!(preset_cursor(presets, Some("")), None);
        assert_eq!(preset_cursor(presets, None), None);
    }

    #[test]
    fn mask_lengths() {
        assert_eq!(mask_of_len(0), "");
        assert_eq!(mask_of_len(3), "***");
    }

    // `cur_disp` models what the native input's buffer becomes after an edit:
    // an inserted (typed/pasted) char appears verbatim; deletions just shorten
    // the all-mask string. `reconstruct_real` maps that back onto the real value.
    #[test]
    fn insertions_are_localized() {
        // Insertions carry the real char, so their position is exact.
        assert_eq!(reconstruct_real("secret", "******", "******X"), "secretX"); // append
        assert_eq!(reconstruct_real("secret", "******", "X******"), "Xsecret"); // prepend
        assert_eq!(reconstruct_real("secret", "******", "***Z***"), "secZret"); // middle
        assert_eq!(reconstruct_real("", "", "a"), "a"); // first char into empty
    }

    #[test]
    fn selection_replace_is_localized() {
        // The inserted char re-anchors the edit even when it replaces a range.
        assert_eq!(reconstruct_real("secret", "******", "N"), "N"); // select-all + type
        assert_eq!(reconstruct_real("secret", "******", "*Z**"), "sZet"); // replace [1..4]
    }

    #[test]
    fn end_deletion_is_correct() {
        assert_eq!(reconstruct_real("secret", "******", "*****"), "secre"); // backspace at end
        assert_eq!(reconstruct_real("s", "*", ""), ""); // delete last char
    }

    #[test]
    fn mid_deletion_collapses_to_boundary() {
        // Documented limitation: identical mask chars can't localize a pure
        // deletion, so a mid-string backspace removes a boundary char (here the
        // trailing one) instead of the char under the caret.
        assert_eq!(reconstruct_real("secret", "******", "*****"), "secre");
    }
}
