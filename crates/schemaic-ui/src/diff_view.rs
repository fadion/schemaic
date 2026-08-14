//! The Ctrl+K inline-AI diff preview view. The line-level diff *logic* lives in
//! `schemaic_core::diff`; this renders its hunked [`DiffRow`]s — each changed
//! line SQL-syntax-highlighted (via the shared `sql_highlight` lexer) with a
//! `+`/`-`/context gutter, and collapsed runs shown as "⋯ N unchanged lines".

use floem::AnyView;
use floem::prelude::*;
use schemaic_core::diff::{DiffRow, DiffTag};
use schemaic_core::intel::SqlDialect;

use crate::consts::MONO_FAMILY;
use crate::widgets::measure_mono_px_at;
use crate::{sql_highlight, theme};

// ===== moved from lib.rs (Ctrl+K diff view) =====
/// One diff line's text, SQL-syntax-highlighted the same way as the editor: the
/// shared `sql_highlight` lexer gives colored byte-spans, which we render as a
/// row of colored segments (uncolored gaps use the default text color). Kept as
/// one line — the row's parent stack decides width/overflow.
fn diff_line(line: String, dialect: SqlDialect) -> impl IntoView {
    let mono = |st: floem::style::Style| {
        st.font_family(MONO_FAMILY.to_string())
            .font_size(theme::FONT_TITLE)
    };
    let spans = sql_highlight::highlight_spans(&line, dialect);
    let mut segs: Vec<AnyView> = Vec::new();
    let mut push = |txt: &str, color: floem::peniko::Color| {
        if txt.is_empty() {
            return;
        }
        let t = txt.to_string();
        segs.push(text(t).style(move |st| mono(st).color(color)).into_any());
    };
    let mut pos = 0usize;
    for (s, e, color) in spans {
        // Span boundaries are at ASCII token edges = char boundaries; `.get`
        // guards the (SQL-improbable) non-boundary case instead of panicking.
        if s > pos {
            push(line.get(pos..s).unwrap_or(""), theme::text());
        }
        push(line.get(s..e).unwrap_or(""), color);
        pos = e;
    }
    if pos < line.len() {
        push(line.get(pos..).unwrap_or(""), theme::text());
    }
    // `flex_shrink(0)`: don't let the line compress to the viewport width. Its
    // full content width then pushes the row (which is `min_width_full`) past the
    // viewport, giving the diff scroll real horizontal overflow to pan — otherwise
    // a long line just clips at the edge with no scrollbar.
    h_stack_from_iter(segs).style(|s| s.flex_row().items_center().flex_shrink(0.0_f32))
}

/// Renders the hunked diff rows (see [`schemaic_core::diff::build_diff_rows`]): each line carries its
/// document line number, a +/- (or blank) marker, and syntax-highlighted text
/// over a tinted background; gaps render as a faint "⋯ N unchanged lines" row.
pub(crate) fn diff_view(rows: Vec<DiffRow>, dialect: SqlDialect) -> impl IntoView {
    let mono = |s: floem::style::Style| {
        s.font_family(MONO_FAMILY.to_string())
            .font_size(theme::FONT_TITLE)
    };
    // Give the diff a DEFINITE content width so the scroll can detect horizontal
    // overflow: Floem's scroll measures its child's laid-out size (clamped to the
    // viewport), not max-content, so variable-width text otherwise just clips with
    // no scrollbar. `min_width_full` keeps it ≥ viewport when lines are short;
    // rows stretch to this width (default `align-items: stretch`) so tints stay
    // uniform.
    //
    // **Measured, not counted.** This used to be `chars().count() * 8.43`, a
    // comment-documented advance for IBM Plex Mono at 14px — exact for ASCII and
    // wrong for anything else, because a monospace font renders a full-width
    // glyph at *two* advances while it counts as one `char`, and a combining mark
    // at zero. Ctrl+K sends the editor buffer, so a CJK string literal in the
    // user's own SQL reaches this path and the scrollbar stopped short of the end
    // of the line. Two advances of slack replace the old `+ 2.0` columns.
    const DIFF_GUTTER_W: f64 = 60.0; // line-number cell (46) + marker (14)
    let widest = rows
        .iter()
        .map(|r| match r {
            DiffRow::Line { text, .. } => measure_mono_px_at(text, theme::FONT_TITLE),
            DiffRow::Gap(_) => 0.0,
        })
        .fold(0.0f64, f64::max);
    let content_w = DIFF_GUTTER_W + widest + 2.0 * measure_mono_px_at("0", theme::FONT_TITLE);
    let views = rows.into_iter().map(move |row| match row {
        DiffRow::Line {
            tag,
            num,
            text: line,
        } => {
            let (bg, marker, mcolor) = match tag {
                DiffTag::Equal => (None, " ", theme::text_muted()),
                DiffTag::Del => (Some(theme::diff_del_bg()), "-", theme::diff_del_marker()),
                DiffTag::Ins => (Some(theme::diff_add_bg()), "+", theme::diff_add_marker()),
            };
            h_stack((
                container(text(num.to_string()).style(move |s| mono(s).color(theme::text_muted())))
                    .style(|s| {
                        s.width(46.0)
                            .flex_shrink(0.0_f32)
                            .justify_end()
                            .padding_right(10.0)
                    }),
                text(marker.to_string())
                    .style(move |s| mono(s).width(14.0).flex_shrink(0.0_f32).color(mcolor)),
                diff_line(line, dialect),
            ))
            // No explicit width: the row sizes to its content (gutter + marker +
            // the non-shrinking line). The parent v_stack is `min_width_full` with
            // the default `align-items: stretch`, so short rows still stretch to the
            // widest row's width (uniform tint) while a long line grows the whole
            // stack PAST the viewport — that overflow is what gives the diff scroll
            // an h-scrollbar. (A `width_full`/`min_width_full` here instead caps the
            // row at the viewport, so a long line just clips with no scrollbar.)
            .style(move |s| {
                let s = s.flex_row().items_center().padding_vert(1.0);
                match bg {
                    Some(c) => s.background(c),
                    None => s,
                }
            })
            .into_any()
        }
        // Collapsed unchanged run — a faint annotation, indented past the gutter.
        DiffRow::Gap(count) => {
            let label = if count == 1 {
                "⋯ 1 unchanged line".to_string()
            } else {
                format!("⋯ {count} unchanged lines")
            };
            container(
                text(label).style(|s| s.font_size(theme::FONT_BODY).color(theme::text_muted())),
            )
            .style(|s| {
                s.width_full()
                    .items_center()
                    .padding_left(60.0)
                    .padding_vert(3.0)
            })
            .into_any()
        }
    });
    // `width_full` resolves to the viewport; `min_width(content_w)` floors it at
    // the longest line. Net width = max(viewport, content_w): short diffs fill the
    // panel (uniform tint, no scrollbar), long lines overflow → h-scrollbar. (Note:
    // `min_width_full` here instead *caps* the child at the viewport — no overflow.)
    v_stack_from_iter(views).style(move |s| s.flex_col().width_full().min_width(content_w))
}
