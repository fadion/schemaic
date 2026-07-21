//! AI-chat markdown rendering: a `pulldown-cmark` (CommonMark + tables +
//! strikethrough) event stream mapped onto Floem views. Renders tables, nested
//! lists, blockquotes, links, strikethrough, and backslash escapes correctly;
//! fenced code blocks become `code_block`s with a hover action bar (Copy, and —
//! for SQL — Insert / Insert-&-Run via [`CodeActions`]).
//!
//! Entry point: [`render_markdown`]; the AI panel builds a [`CodeActions`] and
//! calls it per assistant text segment.

use std::rc::Rc;

use floem::AnyView;
use floem::event::{EventListener, EventPropagation};
use floem::prelude::*;
use floem::text::{
    Attrs, AttrsList, FamilyOwned, LineHeightValue, Style as FontStyle, TextLayout, Weight,
};
use floem::views::{RichText, rich_text};

use crate::{icons, theme};

// ===== moved from lib.rs (markdown cluster) =====
/// Inline run style flags — emphasis nests, so these compose (bold *and* italic,
/// code inside a link, …). Built from the CommonMark event stream.
#[derive(Clone, Copy, Default)]
struct Inline {
    bold: bool,
    italic: bool,
    code: bool,
    strike: bool,
    link: bool,
}

/// A wrapping `rich_text` view for inline runs. `bold_all` forces the base weight
/// bold (headers). Code runs render mono in the code color; links in the accent;
/// struck-through text dimmed.
fn inline_text(
    runs: Vec<(String, Inline)>,
    base: floem::peniko::Color,
    bold_all: bool,
    font_size: f32,
) -> RichText {
    let full: String = runs.iter().map(|(t, _)| t.as_str()).collect();
    let sans = [FamilyOwned::Name("IBM Plex Sans".to_string())];
    let mono = [FamilyOwned::Name("IBM Plex Mono".to_string())];
    let base_weight = if bold_all {
        Weight::BOLD
    } else {
        Weight::NORMAL
    };
    let base_attrs = Attrs::new()
        .family(&sans)
        .font_size(font_size)
        .color(base)
        .weight(base_weight)
        .line_height(LineHeightValue::Normal(1.4));
    let mut list = AttrsList::new(base_attrs);
    let mut pos = 0usize;
    for (t, st) in &runs {
        let range = pos..pos + t.len();
        if st.code {
            list.add_span(
                range,
                Attrs::new()
                    .family(&mono)
                    .font_size(font_size - 1.0)
                    .color(theme::text())
                    .line_height(LineHeightValue::Normal(1.4)),
            );
        } else if st.bold || st.italic || st.strike || st.link || bold_all {
            let color = if st.link {
                theme::accent()
            } else if st.strike {
                theme::text_dim()
            } else {
                base
            };
            let weight = if st.bold || bold_all {
                Weight::BOLD
            } else {
                Weight::NORMAL
            };
            let mut a = Attrs::new()
                .family(&sans)
                .font_size(font_size)
                .color(color)
                .weight(weight)
                .line_height(LineHeightValue::Normal(1.4));
            if st.italic {
                a = a.style(FontStyle::Italic);
            }
            list.add_span(range, a);
        }
        pos += t.len();
    }
    let mut layout = TextLayout::new();
    layout.set_text(&full, list);
    rich_text(move || layout.clone())
}

/// Apply blockquote indentation + a left stripe to a block's style.
fn md_quote_wrap(s: floem::style::Style, quote: usize) -> floem::style::Style {
    if quote > 0 {
        s.padding_left(10.0 * quote as f64)
            .border_left(2.0)
            .border_color(theme::border())
    } else {
        s
    }
}

/// One list item: a dim marker beside the (flex-growing) inline content, indented
/// by nesting `depth`.
fn md_item(
    runs: Vec<(String, Inline)>,
    marker: String,
    depth: f64,
    base: floem::peniko::Color,
    quote: usize,
) -> AnyView {
    h_stack((
        text(marker).style(|s| {
            s.flex_shrink(0.0_f32)
                .min_width(16.0)
                .color(theme::text_dim())
                .font_size(14.0)
                .margin_right(4.0)
        }),
        inline_text(runs, base, false, 14.0).style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
    ))
    .style(move |s| {
        md_quote_wrap(
            s.flex_row()
                .items_start()
                .width_full()
                .padding_left(depth * 18.0),
            quote,
        )
    })
    .into_any()
}

/// Render a markdown table (header rows bold, over a bordered grid).
fn md_table(
    rows: Vec<Vec<Vec<(String, Inline)>>>,
    head_rows: usize,
    base: floem::peniko::Color,
) -> AnyView {
    let row_views: Vec<AnyView> = rows
        .into_iter()
        .enumerate()
        .map(|(ri, cells)| {
            let is_head = ri < head_rows;
            let cell_views: Vec<AnyView> = cells
                .into_iter()
                .map(|runs| {
                    inline_text(runs, base, is_head, 14.0)
                        .style(|s| {
                            s.flex_grow(1.0_f32)
                                .flex_basis(0.0)
                                .min_width(0.0)
                                .padding_horiz(8.0)
                                .padding_vert(4.0)
                        })
                        .into_any()
                })
                .collect();
            h_stack_from_iter(cell_views)
                .style(move |s| {
                    let s = s
                        .flex_row()
                        .width_full()
                        .border_bottom(1.0)
                        .border_color(theme::border());
                    if is_head {
                        s.background(theme::bg_deepest())
                    } else {
                        s
                    }
                })
                .into_any()
        })
        .collect();
    v_stack_from_iter(row_views)
        .style(|s| {
            s.flex_col()
                .width_full()
                .border(1.0)
                .border_color(theme::border())
                .border_radius(6.0)
                .margin_vert(2.0)
        })
        .into_any()
}

/// Render Claude's markdown into Floem views via pulldown-cmark (CommonMark +
/// tables + strikethrough), so tables, nested lists, blockquotes, links, and
/// backslash escapes render correctly. Fenced code blocks become `code_block`s
/// (with the action bar); everything else maps onto `inline_text`/`md_item`/
/// `md_table`.
pub(crate) fn render_markdown(src: &str, actions: CodeActions) -> impl IntoView {
    use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

    let base = theme::bubble_claude_text();
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);

    let mut out: Vec<AnyView> = Vec::new();

    // Inline accumulation for the current leaf block (paragraph / heading / item /
    // table cell). Emphasis counters compose into `Inline` flags per run.
    let mut runs: Vec<(String, Inline)> = Vec::new();
    let (mut bold, mut italic, mut strike, mut link) = (0u32, 0u32, 0u32, 0u32);
    let inline_now = |bold: u32, italic: u32, strike: u32, link: u32| Inline {
        bold: bold > 0,
        italic: italic > 0,
        code: false,
        strike: strike > 0,
        link: link > 0,
    };

    // Block context.
    let mut heading: Option<HeadingLevel> = None;
    let mut list_stack: Vec<Option<u64>> = Vec::new(); // per-list next ordinal (None = bullet)
    let mut item_stack: Vec<String> = Vec::new(); // markers of open items (nesting depth)
    let mut quote: usize = 0;

    // Fenced code block.
    let mut in_code = false;
    let mut code_buf = String::new();
    let mut code_lang = String::new();

    // Table.
    let mut table_rows: Vec<Vec<Vec<(String, Inline)>>> = Vec::new();
    let mut table_head_rows = 0usize;
    let mut cur_row: Vec<Vec<(String, Inline)>> = Vec::new();

    let heading_size = |lvl: HeadingLevel| match lvl {
        HeadingLevel::H1 => 18.0_f32,
        HeadingLevel::H2 => 16.0,
        _ => 15.0,
    };

    for ev in Parser::new_ext(src, opts) {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Strong => bold += 1,
                Tag::Emphasis => italic += 1,
                Tag::Strikethrough => strike += 1,
                Tag::Link { .. } => link += 1,
                Tag::Heading { level, .. } => heading = Some(level),
                Tag::BlockQuote(_) => quote += 1,
                Tag::CodeBlock(kind) => {
                    in_code = true;
                    code_buf.clear();
                    code_lang = match kind {
                        CodeBlockKind::Fenced(l) => l.to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                }
                Tag::List(start) => list_stack.push(start),
                Tag::Item => {
                    let marker = match list_stack.last_mut() {
                        Some(Some(n)) => {
                            let m = format!("{n}.");
                            *n += 1;
                            m
                        }
                        _ => "•".to_string(),
                    };
                    item_stack.push(marker);
                }
                Tag::Table(_) => {
                    table_rows.clear();
                    table_head_rows = 0;
                }
                Tag::TableHead | Tag::TableRow => cur_row = Vec::new(),
                Tag::TableCell => {
                    runs = Vec::new();
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Strong => bold = bold.saturating_sub(1),
                TagEnd::Emphasis => italic = italic.saturating_sub(1),
                TagEnd::Strikethrough => strike = strike.saturating_sub(1),
                TagEnd::Link => link = link.saturating_sub(1),
                TagEnd::Heading(_) => {
                    if let Some(lvl) = heading.take() {
                        let fs = heading_size(lvl);
                        let block = inline_text(std::mem::take(&mut runs), base, true, fs)
                            .style(move |s| md_quote_wrap(s.width_full().padding_top(2.0), quote))
                            .into_any();
                        out.push(block);
                    }
                }
                TagEnd::Paragraph => {
                    // Inside a list item, the item's text flushes on End(Item) (or
                    // before a nested list); a top-level paragraph flushes here.
                    if item_stack.is_empty() && !runs.is_empty() {
                        let block = inline_text(std::mem::take(&mut runs), base, false, 14.0)
                            .style(move |s| md_quote_wrap(s.width_full(), quote))
                            .into_any();
                        out.push(block);
                    }
                }
                TagEnd::CodeBlock => {
                    in_code = false;
                    let code = std::mem::take(&mut code_buf);
                    let trimmed = code.trim_end_matches('\n').to_string();
                    if !trimmed.trim().is_empty() {
                        let is_sql = code_is_sql(&code_lang, &trimmed);
                        out.push(code_block(trimmed, actions.clone(), is_sql).into_any());
                    }
                }
                TagEnd::Item => {
                    if !runs.is_empty() {
                        let depth = item_stack.len().saturating_sub(1) as f64;
                        let marker = item_stack.last().cloned().unwrap_or_default();
                        out.push(md_item(
                            std::mem::take(&mut runs),
                            marker,
                            depth,
                            base,
                            quote,
                        ));
                    }
                    item_stack.pop();
                }
                TagEnd::List(_) => {
                    list_stack.pop();
                }
                TagEnd::BlockQuote(_) => quote = quote.saturating_sub(1),
                TagEnd::TableCell => {
                    cur_row.push(std::mem::take(&mut runs));
                }
                TagEnd::TableHead => {
                    table_rows.push(std::mem::take(&mut cur_row));
                    table_head_rows = 1;
                }
                TagEnd::TableRow => table_rows.push(std::mem::take(&mut cur_row)),
                TagEnd::Table if !table_rows.is_empty() => {
                    out.push(md_table(
                        std::mem::take(&mut table_rows),
                        table_head_rows,
                        base,
                    ));
                }
                _ => {}
            },
            Event::Text(t) => {
                if in_code {
                    code_buf.push_str(&t);
                } else {
                    runs.push((t.to_string(), inline_now(bold, italic, strike, link)));
                }
            }
            Event::Code(t) => {
                let mut st = inline_now(bold, italic, strike, link);
                st.code = true;
                runs.push((t.to_string(), st));
            }
            Event::SoftBreak => {
                if !in_code {
                    runs.push((" ".to_string(), inline_now(bold, italic, strike, link)));
                }
            }
            Event::HardBreak => {
                if !in_code {
                    runs.push(("\n".to_string(), inline_now(bold, italic, strike, link)));
                }
            }
            Event::Rule => {
                out.push(
                    empty()
                        .style(|s| {
                            s.width_full()
                                .height(1.0)
                                .background(theme::border())
                                .margin_vert(4.0)
                        })
                        .into_any(),
                );
            }
            _ => {}
        }
        // When a nested list opens while an item still has un-flushed lead text,
        // emit that text as the item row first.
        if !item_stack.is_empty() && !runs.is_empty() && list_stack.len() > item_stack.len() {
            let depth = item_stack.len().saturating_sub(1) as f64;
            let marker = item_stack.last().cloned().unwrap_or_default();
            out.push(md_item(
                std::mem::take(&mut runs),
                marker,
                depth,
                base,
                quote,
            ));
        }
    }
    v_stack_from_iter(out).style(|s| s.flex_col().gap(6.0).width_full())
}

/// Callbacks the code-block action bar needs: insert the code as a new query tab,
/// and run it. (Copy is self-contained via the clipboard.)
#[derive(Clone)]
pub(crate) struct CodeActions {
    pub insert: Rc<dyn Fn(String)>,
    pub run: Rc<dyn Fn(String)>,
}

/// One action-bar icon: 16px, code-text colored, white on hover.
fn code_action_icon(svg: &'static str, on_click: impl Fn() + 'static) -> impl IntoView {
    container(icons::icon(svg, 16.0))
        .on_click_stop(move |_| on_click())
        .style(|s| {
            s.items_center()
                .color(theme::text())
                .hover(|s| s.color(floem::peniko::Color::WHITE))
        })
}

/// Is a fenced block SQL? An explicit language tag is authoritative; an untagged
/// block falls back to a leading-keyword check (Claude usually tags SQL as ```sql
/// but not always). Non-SQL blocks (shell, json, …) only get the Copy action.
fn code_is_sql(lang: &str, code: &str) -> bool {
    match lang.trim().to_ascii_lowercase().as_str() {
        "" => sql_leading_keyword(code),
        "sql" | "mysql" | "mariadb" | "postgres" | "postgresql" | "psql" | "sqlite" | "tsql" => {
            true
        }
        _ => false,
    }
}

fn sql_leading_keyword(code: &str) -> bool {
    let word: String = code
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_uppercase();
    matches!(
        word.as_str(),
        "SELECT"
            | "WITH"
            | "INSERT"
            | "UPDATE"
            | "DELETE"
            | "REPLACE"
            | "CREATE"
            | "ALTER"
            | "DROP"
            | "TRUNCATE"
            | "SHOW"
            | "DESCRIBE"
            | "DESC"
            | "EXPLAIN"
            | "USE"
            | "SET"
            | "CALL"
            | "GRANT"
            | "REVOKE"
            | "RENAME"
            | "ANALYZE"
            | "OPTIMIZE"
            | "START"
            | "BEGIN"
            | "COMMIT"
            | "ROLLBACK"
    )
}

fn code_block(code: String, actions: CodeActions, is_sql: bool) -> impl IntoView {
    let hovered = RwSignal::new(false);

    let body = text(code.trim_end().to_string()).style(|s| {
        s.width_full()
            .font_family("monospace".to_string())
            .font_size(theme::FONT_BODY)
            .color(theme::text())
    });

    // Bar icons: Copy for any block; Insert (square-pen) and Insert-&-Run
    // (circle-play) only for SQL, since they target the SQL editor.
    let mut buttons: Vec<AnyView> = Vec::new();
    let copy_code = code.clone();
    buttons.push(
        code_action_icon(icons::COPY, move || {
            let _ = floem::Clipboard::set_contents(copy_code.clone());
        })
        .into_any(),
    );
    if is_sql {
        let insert_code = code.clone();
        let insert = actions.insert.clone();
        buttons.push(
            code_action_icon(icons::SQUARE_PEN, move || {
                (insert)(insert_code.clone());
            })
            .into_any(),
        );
        let run_code = code.clone();
        let run_insert = actions.insert.clone();
        let run = actions.run.clone();
        buttons.push(
            code_action_icon(icons::CIRCLE_PLAY, move || {
                // Insert into a new tab, then run it there.
                (run_insert)(run_code.clone());
                (run)(run_code.clone());
            })
            .into_any(),
        );
    }

    // Floated top-right inside the block; revealed only while the block is hovered.
    let bar = h_stack_from_iter(buttons).style(move |s| {
        let s = s
            .absolute()
            .inset_top(3.0)
            .inset_right(3.0)
            .flex_row()
            .items_center()
            .gap(10.0)
            .padding_horiz(8.0)
            .padding_vert(5.0)
            .border_radius(6.0)
            .background(theme::code_action_bar());
        if hovered.get() { s } else { s.hide() }
    });

    stack((body, bar))
        .on_event(EventListener::PointerEnter, move |_| {
            hovered.set(true);
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerLeave, move |_| {
            hovered.set(false);
            EventPropagation::Continue
        })
        .style(|s| {
            s.width_full()
                .padding(8.0)
                .background(theme::bg_deepest())
                .border(1.0)
                .border_color(theme::border())
                .border_radius(6.0)
        })
}
