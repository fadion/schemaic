//! **Used by** — the modal listing what depends on a table, a view, a column
//! or a routine (`core::used_by`), raised from the schema tree's menu.
//!
//! Read from the schema already in memory, so it opens with its answer — no
//! fetch, no loading state. A database whose schema has not loaded says so
//! rather than claiming nothing uses the object.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;

use schemaic_core::used_by::{Dependent, DependentKind, Target};

use crate::i18n::tr;
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, autohide, dismiss_layer,
    focus_root_with_ring, modal_body_h, modal_footer_split, modal_pad_h, modal_title_owned,
    panel_style,
};
use crate::{ConnUi, OverlayUi, SchemaUi, icons, theme};

/// The object the modal describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsedByTarget {
    pub database: String,
    pub schema: Option<String>,
    pub name: String,
    /// A column of the table `name`.
    pub column: Option<String>,
    /// `name` is a routine, not a table or view.
    pub routine: bool,
}

impl UsedByTarget {
    /// The object as the title names it.
    fn display(&self) -> String {
        let base = schemaic_core::schema::display_name(self.schema.as_deref(), &self.name);
        match &self.column {
            Some(c) => format!("{base}.{c}"),
            None => base,
        }
    }

    /// As `core::used_by` asks it.
    fn target(&self) -> Target<'_> {
        if self.routine {
            Target::Routine {
                ns: self.schema.as_deref(),
                name: &self.name,
            }
        } else {
            Target::Relation {
                ns: self.schema.as_deref(),
                name: &self.name,
                column: self.column.as_deref(),
            }
        }
    }
}

/// What uses `t`, from the loaded schema of its database — `None` while that
/// schema is not loaded. Read in the active connection's dialect, which is
/// the tree's: every database the tree shows is that connection's. Shared
/// with the drop preview's warning.
pub(crate) fn dependents_of(
    schema: SchemaUi,
    conn: ConnUi,
    t: &UsedByTarget,
) -> Option<Vec<Dependent>> {
    let db = crate::table_designer::loaded_schema(schema, &t.database)?;
    Some(schemaic_core::used_by::used_by(
        &db,
        t.target(),
        conn.dialect.get_untracked(),
        &t.database,
    ))
}

/// Modal width — a qualified name and a table beside it on one line.
fn panel_w() -> f64 {
    theme::scaled(520.0)
}

pub(crate) fn used_by_overlay(overlay: OverlayUi, schema: SchemaUi, conn: ConnUi) -> impl IntoView {
    let target = overlay.used_by;
    dyn_container(
        move || target.get(),
        move |open| {
            let Some(t) = open else {
                return empty().into_any();
            };
            let close: Rc<dyn Fn()> = Rc::new(move || target.set(None));
            let ring = FocusRing::new();
            let found = dependents_of(schema, conn, &t);
            let body = body(&t, found.as_deref());
            let copy_text = found.as_deref().map(|d| as_markdown(&t, d));
            let title = modal_title_owned(
                tr!("used-by-title", object = t.display()),
                close.clone(),
                ring.clone(),
            );
            let done = close.clone();
            let footer = modal_footer_split(
                action_button(
                    tr!("used-by-copy"),
                    ActionKind::Quiet,
                    copy_text.is_some(),
                    ring.clone(),
                    ACTION_TAB,
                    move || {
                        if let Some(text) = &copy_text {
                            let _ = floem::Clipboard::set_contents(text.clone());
                        }
                    },
                ),
                action_button(
                    tr!("dialog-close"),
                    ActionKind::Primary,
                    true,
                    ring.clone(),
                    ACTION_TAB + 10,
                    move || (done)(),
                ),
            );
            let panel = v_stack((title, body, footer))
                .on_click_stop(|_| {})
                .style(|s| {
                    panel_style(s)
                        .background(theme::bg_panel())
                        .width(panel_w())
                });
            let esc = close.clone();
            focus_root_with_ring(stack((dismiss_layer(move || close()), panel)), ring)
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
        if target.get().is_some() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// The list, grouped by kind under a counted heading; or why there is none.
fn body(t: &UsedByTarget, found: Option<&[Dependent]>) -> AnyView {
    let note = |icon, color: fn() -> floem::peniko::Color, msg: String| {
        crate::widgets::fact_note(icon, color, msg)
    };
    let mut sections: Vec<AnyView> = Vec::new();
    match found {
        None => sections.push(note(
            icons::TRIANGLE_ALERT,
            theme::text_dim,
            tr!("used-by-not-loaded", database = t.database.clone()),
        )),
        Some([]) => sections.push(note(
            icons::CIRCLE_CHECK,
            theme::text_dim,
            tr!("used-by-nothing-in", database = t.database.clone()),
        )),
        Some(deps) => {
            let mut start = 0;
            while start < deps.len() {
                let kind = deps[start].kind;
                let end = deps[start..]
                    .iter()
                    .position(|d| d.kind != kind)
                    .map_or(deps.len(), |n| start + n);
                let group = &deps[start..end];
                let rows: Vec<AnyView> = group.iter().map(row).collect();
                sections.push(crate::widgets::fact_section_owned(
                    dependent_heading(kind, group.len()),
                    rows,
                    || theme::scaled(6.0),
                ));
                start = end;
            }
        }
    }
    // What a scan of definitions cannot see, said once under the answer.
    if found.is_some() {
        sections.push(
            text(tr!("used-by-limits"))
                .style(|s| {
                    s.font_size(theme::font_hint())
                        .color(theme::text_faint())
                        .width_full()
                })
                .into_any(),
        );
    }
    let content =
        v_stack_from_iter(sections).style(|s| s.flex_col().gap(theme::scaled(16.0)).width_full());
    autohide(scroll(
        container(content).style(|s| s.width_full().padding(modal_pad_h())),
    ))
    .style(|s| s.width_full().max_height(modal_body_h(420.0)))
    .into_any()
}

/// One dependent: its name, and what else there is to say beside it.
fn row(d: &Dependent) -> AnyView {
    h_stack((
        text(d.name.clone()).style(|s| {
            s.font_family(crate::MONO_FAMILY.to_string())
                .font_size(theme::font_body())
                .color(theme::text())
                .min_width(0.0)
        }),
        // A trigger on the table, a key onto itself: it belongs to the object
        // and goes with a drop, so it is no breakage — said, since the list
        // is read before one.
        text(if d.goes_with_it {
            tr!("used-by-dropped-with-it", detail = d.detail.clone())
        } else {
            d.detail.clone()
        })
        .style(|s| {
            s.font_size(theme::font_label())
                .color(theme::text_faint())
                .min_width(0.0)
        }),
    ))
    .style(|s| {
        s.items_center()
            .gap(theme::scaled(10.0))
            .width_full()
            .min_width(0.0)
    })
    .into_any()
}

/// A kind's group heading with its count — `DependentKind::noun`'s word,
/// capitalised, drawn here so it can be translated: one whole message per
/// kind, a plural on `$count`.
fn dependent_heading(kind: DependentKind, n: usize) -> String {
    match kind {
        DependentKind::View => tr!("used-by-heading-view", count = n),
        DependentKind::Routine => tr!("used-by-heading-routine", count = n),
        DependentKind::Trigger => tr!("used-by-heading-trigger", count = n),
        DependentKind::Event => tr!("used-by-heading-event", count = n),
        DependentKind::ForeignKey => tr!("used-by-heading-foreign-key", count = n),
        DependentKind::Expression => tr!("used-by-heading-expression", count = n),
    }
}

/// A kind in the singular, as the Copy text's list item names it —
/// `DependentKind::noun(1)`'s word, drawn here.
fn dependent_noun(kind: DependentKind) -> String {
    match kind {
        DependentKind::View => tr!("used-by-noun-view"),
        DependentKind::Routine => tr!("used-by-noun-routine"),
        DependentKind::Trigger => tr!("used-by-noun-trigger"),
        DependentKind::Event => tr!("used-by-noun-event"),
        DependentKind::ForeignKey => tr!("used-by-noun-foreign-key"),
        DependentKind::Expression => tr!("used-by-noun-expression"),
    }
}

/// The list as Markdown, for Copy.
fn as_markdown(t: &UsedByTarget, deps: &[Dependent]) -> String {
    let mut out = format!("{}\n", tr!("used-by-title", object = t.display()));
    if deps.is_empty() {
        out.push_str(&format!("\n{}\n", tr!("used-by-nothing")));
    }
    for d in deps {
        let detail = if d.detail.is_empty() {
            String::new()
        } else {
            format!(" — {}", d.detail)
        };
        out.push_str(&format!(
            "- {} `{}`{detail}\n",
            dependent_noun(d.kind),
            d.name
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{dependent_heading, dependent_noun};
    use crate::i18n::tr;
    use schemaic_core::used_by::DependentKind;

    /// The headings and nouns are core's in English: each heading reads as
    /// `DependentKind::noun` capitalised with the count beside it, in both
    /// numbers, and each noun as its singular.
    #[test]
    fn english_words_every_dependent_kind_as_core_does() {
        let capitalised = |s: &str| {
            let mut c = s.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        };
        for kind in [
            DependentKind::View,
            DependentKind::Routine,
            DependentKind::Trigger,
            DependentKind::Event,
            DependentKind::ForeignKey,
            DependentKind::Expression,
        ] {
            for n in [1, 3] {
                assert_eq!(
                    dependent_heading(kind, n),
                    format!("{} ({n})", capitalised(kind.noun(n))),
                    "{kind:?}"
                );
            }
            assert_eq!(dependent_noun(kind), kind.noun(1), "{kind:?}");
        }
    }

    /// The composed lines read as they did in English before the catalogue.
    #[test]
    fn the_composed_lines_read_as_they_did_in_english() {
        assert_eq!(
            tr!("used-by-title", object = "shop.orders"),
            "Used by \u{2014} shop.orders"
        );
        assert_eq!(
            tr!("used-by-not-loaded", database = "shop"),
            "shop's schema has not loaded yet \u{2014} expand it in the tree, then ask again."
        );
        assert_eq!(
            tr!("used-by-nothing-in", database = "shop"),
            "Nothing in shop uses it."
        );
        assert_eq!(
            tr!("used-by-dropped-with-it", detail = "on orders"),
            "on orders · dropped with it"
        );
    }
}
