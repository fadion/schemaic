//! The Manage Connections modal: the saved-connection list + the edit form
//! (`conn_form`), including the password fields' `*`-masking.
//!
//! Masking keeps the real secret out of the native input's buffer (which we can't
//! fully control): the field shows `MASK_CH × len`, and each edit is mapped back
//! onto the real value by diffing the masked display (`reconstruct_real`) — pure
//! logic, unit-tested below.

use std::rc::Rc;

use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;
use schemaic_core::connection::Connection;

use schemaic_core::connection::SshAuth;

use crate::consts::MASK_CH;
use crate::settings::{settings_dropdown, settings_toggle_row};
use crate::widgets::{
    autohide, loading_dots, measure_px, menu_item_style, modal_title, panel_style,
};
use crate::{DraftSignals, FieldCfg, Ui, edit_field, icons, theme};

// ===== moved from lib.rs (connection form + password masking) =====
// One labelled text field for the connection form.
fn field(lbl: &'static str, sig: RwSignal<String>) -> impl IntoView {
    v_stack((
        text(lbl).style(|s| s.color(theme::text_dim()).font_size(theme::FONT_LABEL)),
        edit_field(sig, FieldCfg::default()).style(|s| s.width_full()),
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
) -> impl IntoView {
    let host_field = field(host_lbl, host).style(|s| s.flex_grow(1.0_f32).min_width(0.0));
    let port_field = v_stack((
        text(port_lbl).style(|s| s.color(theme::text_dim()).font_size(theme::FONT_LABEL)),
        edit_field(port, FieldCfg::default()).style(|s| s.width(96.0)),
    ))
    .style(|s| s.flex_col().gap(6.0).flex_shrink(0.0_f32));
    h_stack((host_field, port_field)).style(|s| s.flex_row().items_start().gap(25.0).width_full())
}

// Key-pair credentials: a private-key path (with a native "Browse…" picker) and
// an optional passphrase. Shown when the SSH auth method is "Key pair".
fn key_pair_fields(draft: DraftSignals) -> impl IntoView {
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
        text("Private key").style(|s| s.color(theme::text_dim()).font_size(theme::FONT_LABEL)),
        h_stack((
            edit_field(draft.ssh_key_path, FieldCfg::default())
                .style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
            browse,
        ))
        .style(|s| s.flex_row().items_center().gap(8.0).width_full()),
    ))
    .style(|s| s.flex_col().gap(6.0).width_full());

    v_stack((
        key_row,
        masked_field("Passphrase", draft.ssh_key_passphrase),
    ))
    .style(|s| s.flex_col().gap(20.0).width_full())
}

fn mask_of_len(n: usize) -> String {
    std::iter::repeat_n(MASK_CH, n).collect()
}

/// A row of colour swatches that sets the draft's identity colour. Every
/// connection has a colour (new ones are auto-assigned), so there's no "none"
/// option; the selected swatch gets a 2px border in `text()`.
fn color_picker(color: RwSignal<Option<String>>) -> impl IntoView {
    let swatches = crate::CONN_COLOR_PRESETS.iter().map(move |(_, hex, _)| {
        let hex = (*hex).to_string();
        let hx = hex.clone();
        empty()
            .on_click_stop(move |_| color.set(Some(hex.clone())))
            .style(move |s| {
                let fill = theme::parse_hex(&hx).unwrap_or(floem::peniko::Color::TRANSPARENT);
                let s = s
                    .size(18.0, 18.0)
                    .flex_shrink(0.0_f32)
                    .border_radius(9.0)
                    .background(fill);
                if color.get().as_deref() == Some(hx.as_str()) {
                    s.border(2.0).border_color(floem::peniko::Color::WHITE)
                } else {
                    s
                }
            })
            .into_any()
    });

    v_stack((
        text("Colour").style(|s| s.color(theme::text_dim()).font_size(theme::FONT_LABEL)),
        h_stack_from_iter(swatches).style(|s| s.flex_row().items_center().gap(8.0)),
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
fn masked_field(lbl: &'static str, sig: RwSignal<String>) -> impl IntoView {
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
        text(lbl).style(|s| s.color(theme::text_dim()).font_size(theme::FONT_LABEL)),
        edit_field(
            disp,
            FieldCfg {
                background: theme::bg_deepest,
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
                    container(text(c.name.clone()).style(|s| s.font_size(theme::FONT_BODY)))
                        .on_click_stop(move |_| (select)(id))
                        // Full-width row: resting `conn_list_text`, hover text
                        // brightens (no bg), selected = bright text on a full-width
                        // `conn_list_sel_bg`.
                        .style(move |s| {
                            let selected = draft.id.get() == Some(id);
                            let s = s
                                .width_full()
                                .padding_horiz(12.0)
                                .padding_vert(11.0)
                                .border_radius(5.0);
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

            let right = conn_form(
                draft,
                save_conn.clone(),
                delete_conn.clone(),
                test_conn.clone(),
                conn_test,
            );

            let body = h_stack((left, right))
                .style(|s| s.width_full().flex_grow(1.0_f32).flex_row().min_height(0.0));

            let close: Rc<dyn Fn()> = Rc::new(move || open.set(false));
            let panel = v_stack((modal_title("Manage Connections", close), body))
                .on_click_stop(|_| {})
                .style(|s| panel_style(s).width(720.0).height(500.0));

            // Dark backdrop, centered panel, click-away or Escape closes.
            container(panel)
                .keyboard_navigable()
                .request_focus(|| {})
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

fn conn_form(
    draft: DraftSignals,
    save_conn: Rc<dyn Fn()>,
    delete_conn: Rc<dyn Fn(u64)>,
    test_conn: Rc<dyn Fn()>,
    conn_test: RwSignal<crate::TestState>,
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
    let ssh_toggle = settings_toggle_row(
        "SSH tunnel",
        "Reach the server through an SSH tunnel.",
        draft.ssh_enabled,
    );
    let ssh_fields = dyn_container(
        move || ssh_enabled.get(),
        move |on| {
            if !on {
                return empty().into_any();
            }
            // Authentication method picker (150px), matching the settings dropdowns.
            let auth_field = v_stack((
                text("Authentication")
                    .style(|s| s.color(theme::text_dim()).font_size(theme::FONT_LABEL)),
                settings_dropdown(draft.ssh_auth, SshAuth::ALL, SshAuth::label)
                    .style(|s| s.width(150.0)),
            ))
            .style(|s| s.flex_col().gap(6.0));

            // Method-specific credentials, swapped on the chosen auth.
            let ssh_auth = draft.ssh_auth;
            let auth_creds = dyn_container(
                move || ssh_auth.get(),
                move |auth| match auth {
                    SshAuth::Password => {
                        masked_field("SSH password", draft.ssh_password).into_any()
                    }
                    SshAuth::KeyPair => key_pair_fields(draft).into_any(),
                    SshAuth::Agent => text(
                        "Signing is delegated to your running SSH agent (OpenSSH \
                         agent / Pageant on Windows). No key is stored by Schemaic.",
                    )
                    .style(|s| {
                        s.color(theme::text_faint())
                            .font_size(theme::FONT_LABEL)
                            .width_full()
                    })
                    .into_any(),
                },
            )
            .style(|s| s.width_full());

            v_stack((
                host_port_row("SSH host", draft.ssh_host, "SSH port", draft.ssh_port),
                field("SSH user", draft.ssh_user),
                auth_field,
                auth_creds,
            ))
            // `width_full` so the inner fields fill the pane (the `dyn_container`
            // below must also fill, or these collapse to shrink-wrapped width).
            // Tinted, padded container groups the tunnel fields — the toggle above
            // stays outside it.
            .style(|s| {
                s.flex_col()
                    .gap(20.0)
                    .width_full()
                    .padding(10.0)
                    .background(theme::bg_header_row())
                    .border_radius(6.0)
            })
            .into_any()
        },
    )
    .style(|s| s.width_full());

    // "Prominent color in editor" — when on, the identity colour frames the
    // query+results editor (a guard-rail for e.g. production connections). Uses
    // the same themed switch as the AI/terminal settings toggles.
    let prominent_toggle = settings_toggle_row(
        "Prominent color in editor",
        "Frame the query editor in this connection's colour.",
        draft.prominent_color,
    );
    // Read-only guard-rail: disables cell edits and blocks write/DDL queries.
    let read_only_toggle = settings_toggle_row(
        "Read-only",
        "Disable cell edits and block write/DDL queries on this connection.",
        draft.read_only,
    );

    // Type: a dropdown-styled box (single "MySQL" value for now) — matches the
    // settings dropdowns' closed-box look (dark field + chevron).
    let type_field = v_stack((
        text("Type").style(|s| s.color(theme::text_dim()).font_size(theme::FONT_LABEL)),
        container(
            h_stack((
                label(move || draft.db_type.get())
                    .style(|s| s.color(theme::text()).font_size(theme::FONT_BODY)),
                empty().style(|s| s.flex_grow(1.0_f32)),
                icons::icon(icons::CHEVRON_DOWN, 16.0)
                    .style(|s| s.color(theme::text_dim()).flex_shrink(0.0_f32)),
            ))
            .style(|s| s.items_center().width_full().gap(8.0)),
        )
        .style(|s| {
            // 150px wide (not full width), matching the settings dropdowns' look.
            s.width(150.0)
                .height(32.0)
                .items_center()
                .padding_horiz(10.0)
                .background(theme::bg_editor())
                .border(1.0)
                .border_color(theme::field_border())
                .border_radius(6.0)
        }),
    ))
    .style(|s| s.flex_col().gap(6.0).width_full());

    // Name + Colour sit closer together (20px) than the rest of the form (25px).
    let name_color = v_stack((field("Name", draft.name), color_picker(draft.color)))
        .style(|s| s.flex_col().gap(20.0).width_full());

    let fields = v_stack((
        name_color,
        prominent_toggle,
        read_only_toggle,
        type_field,
        host_port_row("Host", draft.host, "Port", draft.port),
        field("User", draft.user),
        masked_field("Password", draft.password),
        ssh_toggle,
        ssh_fields,
    ))
    .style(|s| s.flex_col().gap(20.0).width_full().padding(14.0));

    // Delete / Save are plain clickable text (no `button()` — its default theme
    // restyles the font size on focus/active, which caused the size to jump on
    // click). Explicit `font_size` keeps them stable; colour is set on the row so
    // the trash svg's `currentColor` and the label share the hover tint.
    let del = delete_conn.clone();
    let delete_btn = dyn_container(
        move || draft.id.get(),
        move |id| match id {
            Some(cid) => {
                let del = del.clone();
                h_stack((
                    icons::icon(icons::TRASH_2, 16.0),
                    text("Delete").style(|s| s.font_size(theme::FONT_BODY)),
                ))
                .on_click_stop(move |_| (del)(cid))
                .style(|s| {
                    s.flex_row()
                        .items_center()
                        .gap(8.0)
                        .padding_horiz(6.0)
                        .padding_vert(4.0)
                        .border_radius(6.0)
                        .color(theme::conn_delete())
                        .hover(|s| s.color(theme::conn_delete_hover()))
                })
                .into_any()
            }
            None => empty().into_any(),
        },
    );

    // Test button: a fixed 16px icon slot (so the result icon never shifts the
    // "Test" label) + the label. The slot is empty until a result lands; the
    // icons carry their own fixed colours, so they don't follow the label hover.
    let test = test_conn.clone();
    let test_btn = h_stack((
        container(dyn_container(
            move || conn_test.get(),
            move |st| match st {
                crate::TestState::Ok => icons::icon(icons::CIRCLE_CHECK, 16.0)
                    .style(|s| s.color(theme::conn_test_ok()))
                    .into_any(),
                crate::TestState::Fail => icons::icon(icons::CIRCLE_X, 16.0)
                    .style(|s| s.color(theme::conn_delete()))
                    .into_any(),
                _ => empty().into_any(),
            },
        ))
        .style(|s| {
            s.width(16.0)
                .height(16.0)
                .flex_shrink(0.0_f32)
                .items_center()
                .justify_center()
        }),
        // While testing, the label animates "Test." → ".." → "..." (shared
        // `loading_dots`). The idle/result label reserves the same `Test...` width
        // (left-anchored) so entering/leaving the testing state — and the dots
        // cycling within it — never shift the text.
        {
            let reserved = measure_px("Test...", theme::FONT_BODY) + 2.0;
            dyn_container(
                move || conn_test.get() == crate::TestState::Testing,
                move |testing| {
                    if testing {
                        loading_dots("Test", theme::conn_test, theme::FONT_BODY).into_any()
                    } else {
                        text("Test")
                            .style(move |s| s.font_size(theme::FONT_BODY).min_width(reserved))
                            .into_any()
                    }
                },
            )
        },
    ))
    .on_click_stop(move |_| (test)())
    .style(|s| {
        s.flex_row()
            .items_center()
            .gap(8.0)
            .padding_horiz(6.0)
            .padding_vert(4.0)
            .border_radius(6.0)
            .color(theme::conn_test())
            .hover(|s| s.color(theme::conn_test_hover()))
    });

    let save = save_conn.clone();
    let save_btn = text("Save").on_click_stop(move |_| (save)()).style(|s| {
        s.font_size(theme::FONT_BODY)
            .padding_horiz(6.0)
            .padding_vert(4.0)
            .border_radius(6.0)
            .color(theme::conn_save())
            .hover(|s| s.color(theme::conn_save_hover()))
    });
    // Test sits 30px to the left of Save.
    let right_actions =
        h_stack((test_btn, save_btn)).style(|s| s.flex_row().items_center().gap(30.0));
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
    use super::{mask_of_len, reconstruct_real};

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
