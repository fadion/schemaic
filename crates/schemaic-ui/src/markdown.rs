//! AI-chat markdown rendering: a `pulldown-cmark` (CommonMark + tables +
//! strikethrough) event stream mapped onto Floem views. Renders tables, nested
//! lists, blockquotes, links, strikethrough, and backslash escapes correctly;
//! fenced code blocks become `code_block`s with a standing header bar — the
//! language on the left, Copy and (for SQL) Insert / Run on the right, via
//! [`CodeActions`].
//!
//! Entry point: [`render_markdown`]; the AI panel builds a [`CodeActions`] and
//! calls it per assistant text segment.

use std::rc::Rc;

use floem::AnyView;
use floem::prelude::*;
use floem::reactive::Memo;
use floem::text::{
    Attrs, AttrsList, FamilyOwned, LineHeightValue, Style as FontStyle, TextLayout, Weight,
};
use floem::views::{RichText, rich_text};

use schemaic_core::intel::SqlDialect;

use crate::{icons, theme};

// ===== moved from lib.rs (markdown cluster) =====
/// Line height of every text run the markdown renderer emits, as a multiple of
/// the font size.
///
/// Shared rather than repeated because cosmic-text centres a glyph in its line
/// box — `centering_offset = (line_height - glyph_height) / 2` — so two views
/// top-aligned beside each other only line up while their line heights agree.
/// A list marker is exactly that: a plain label next to a `rich_text`, and at
/// the default (the font's own metrics, ~1.2) it floated a couple of pixels
/// above the item it belongs to.
const MD_LINE_HEIGHT: f32 = 1.4;

/// Body font size for markdown prose — list items, paragraphs, table cells.
/// Headings scale from `heading_size` instead.
///
/// Baked into a text `Attrs` list rather than read from a style closure, which is
/// why this module's views are keyed on `theme::ui_generation` — the generation
/// the interface scale bumps, so a scale change rebuilds them (see
/// `theme::set_ui_scale`).
fn md_font_size() -> f32 {
    theme::scaled_font(14.0)
}

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
        .line_height(LineHeightValue::Normal(MD_LINE_HEIGHT));
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
                    .line_height(LineHeightValue::Normal(MD_LINE_HEIGHT)),
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
                .line_height(LineHeightValue::Normal(MD_LINE_HEIGHT));
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
        // Same size *and* line height as the content beside it — the marker is a
        // plain label rather than a `rich_text`, so nothing else makes the two
        // line boxes agree, and only equal boxes put the two glyphs on one
        // baseline under `items_start` (see [`MD_LINE_HEIGHT`]).
        text(marker).style(|s| {
            s.flex_shrink(0.0_f32)
                .min_width(theme::scaled(16.0))
                .color(theme::text_dim())
                .font_size(md_font_size())
                .line_height(MD_LINE_HEIGHT)
                .margin_right(theme::scaled(4.0))
        }),
        inline_text(runs, base, false, md_font_size())
            .style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
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
                    inline_text(runs, base, is_head, md_font_size())
                        .style(|s| {
                            s.flex_grow(1.0_f32)
                                .flex_basis(0.0)
                                .min_width(0.0)
                                .padding_horiz(theme::scaled(8.0))
                                .padding_vert(theme::scaled(4.0))
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
                .margin_vert(theme::scaled(2.0))
        })
        .into_any()
}

/// Render Claude's markdown into Floem views via pulldown-cmark (CommonMark +
/// tables + strikethrough), so tables, nested lists, blockquotes, links, and
/// backslash escapes render correctly. Fenced code blocks become `code_block`s
/// (with the action bar); everything else maps onto `inline_text`/`md_item`/
/// `md_table`.
///
/// `settled` is whether the turn has finished streaming, and it gates the
/// proposal card. Mid-stream the fence is still open, and pulldown-cmark closes
/// an unterminated block at the end of input — so a proposal would render as a
/// card full of half-arrived JSON, flickering "couldn't read this" on every
/// chunk. Until the turn settles, a proposal block is just a code block.
pub(crate) fn render_markdown(src: &str, actions: CodeActions, settled: bool) -> impl IntoView {
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
                            .style(move |s| {
                                md_quote_wrap(s.width_full().padding_top(theme::scaled(2.0)), quote)
                            })
                            .into_any();
                        out.push(block);
                    }
                }
                TagEnd::Paragraph => {
                    // Inside a list item, the item's text flushes on End(Item) (or
                    // before a nested list); a top-level paragraph flushes here.
                    if item_stack.is_empty() && !runs.is_empty() {
                        let block =
                            inline_text(std::mem::take(&mut runs), base, false, md_font_size())
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
                        if settled && schemaic_core::propose::is_proposal_tag(&code_lang) {
                            out.push(proposal_card(trimmed, actions.clone()).into_any());
                        } else {
                            let is_sql = code_is_sql(&code_lang, &trimmed);
                            out.push(
                                code_block(trimmed, actions.clone(), &code_lang, is_sql, settled)
                                    .into_any(),
                            );
                        }
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
                                .margin_vert(theme::scaled(4.0))
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
    v_stack_from_iter(out).style(|s| s.flex_col().gap(theme::scaled(6.0)).width_full())
}

/// Callbacks the code-block action bar needs: insert the code as a new query tab,
/// and run it. (Copy is self-contained via the clipboard.)
#[derive(Clone)]
pub(crate) struct CodeActions {
    pub insert: Rc<dyn Fn(String)>,
    pub run: Rc<dyn Fn(String)>,
    /// Send a proposed table change to the DDL preview. `Err` is what to show on
    /// the card — every failure here is the model being wrong about the table,
    /// and the card is where the user can see what it asked for.
    pub propose: Rc<dyn Fn(schemaic_core::propose::Proposal) -> Result<(), String>>,
    /// Which lexer colours a SQL block — **the tab's** connection, for the same
    /// reason `insert` and `propose` use it: the chat is about the tab the user
    /// is looking at, so a code block in it is about that tab's database.
    ///
    /// Read untracked at build, like `InlineDiffDoc`'s: a block keeps the
    /// dialect it was rendered with until its message rebuilds. Syntax colouring
    /// only — no wrong text and no wrong action — and subscribing here would put
    /// a dependency on the connection into every bubble in the conversation.
    pub dialect: Memo<SqlDialect>,
}

/// One header action: a text link, dim until hovered, then accent. Words rather
/// than icons because the header is always on screen — an icon row standing
/// permanently over every code block is noise, and "Run" said in a word can't be
/// mistaken for "Insert".
fn code_action_link(label: &'static str, on_click: impl Fn() + 'static) -> impl IntoView {
    text(label).on_click_stop(move |_| on_click()).style(|s| {
        s.font_size(theme::font_label())
            .color(theme::text_dim())
            .hover(|s| s.color(theme::accent()))
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

/// The card's own action button — the small-toolbar chrome, without the modal
/// focus machinery `widgets::control_button` carries. A chat bubble is not a
/// focus root and has no tab order to join.
fn card_button(label: &'static str, on_click: impl Fn() + 'static) -> impl IntoView {
    text(label).on_click_stop(move |_| on_click()).style(|s| {
        crate::widgets::control_surface(s)
            .font_size(crate::widgets::toolbar_font())
            .padding_horiz(theme::scaled(10.0))
            .padding_vert(theme::scaled(5.0))
            .flex_shrink(0.0_f32)
            .color(theme::text())
            .hover(|s| s.background(theme::control_hover()))
    })
}

/// A `schemaic-proposal` block, once the turn has settled: the change the model
/// is offering, and a button that opens it in the DDL preview.
///
/// **The card is the offer; the preview is the decision.** Review runs nothing —
/// it opens the same modal the table designer opens, with the same warnings and
/// the same Apply. So the model can propose freely and still cannot write.
fn proposal_card(json: String, actions: CodeActions) -> AnyView {
    let proposal = match schemaic_core::propose::parse(&json) {
        Ok(p) => p,
        // Neither dropped nor hidden. The model said it was proposing something,
        // so the user is told it couldn't be read *and* still gets to see what it
        // wrote — which is what they need to tell it what went wrong.
        Err(e) => {
            return v_stack((
                text(format!(
                    "Claude proposed a change that couldn't be read — {e}"
                ))
                .style(|s| {
                    s.width_full()
                        .font_size(theme::font_label())
                        .color(theme::text_muted())
                }),
                code_block(json, actions, "json", false, true),
            ))
            .style(|s| s.flex_col().width_full().gap(theme::scaled(5.0)))
            .into_any();
        }
    };

    let error = RwSignal::new(None::<String>);
    let ops = proposal.ops.len();

    let head = h_stack((
        icons::icon(icons::TABLE_PROPERTIES, 15.0)
            .style(|s| s.color(theme::text_muted()).flex_shrink(0.0_f32)),
        text(format!("Proposed change to {}", proposal.table)).style(|s| {
            s.font_size(theme::font_body())
                .color(theme::text())
                .flex_grow(1.0_f32)
        }),
        text(format!(
            "{ops} {}",
            schemaic_core::text::plural(ops, "change", "changes")
        ))
        .style(|s| {
            s.font_size(theme::font_label())
                .color(theme::text_faint())
                .flex_shrink(0.0_f32)
        }),
    ))
    .style(|s| {
        s.flex_row()
            .items_center()
            .width_full()
            .gap(theme::scaled(7.0))
    });

    // The model's own line about what the change is for, when it wrote one. Not
    // load-bearing — the preview lists every change in the app's words — so it
    // is dim, and absent rather than an empty row when there is none.
    let summary: AnyView = match proposal.summary.clone() {
        Some(s) if !s.trim().is_empty() => text(s)
            .style(|s| {
                s.width_full()
                    .font_size(theme::font_label())
                    .color(theme::text_muted())
            })
            .into_any(),
        _ => empty().into_any(),
    };

    let review = {
        let propose = actions.propose.clone();
        card_button("Review change…", move || {
            // The failure lands on the card, not in a modal: it is the model
            // being wrong about the table, and this is where the user can see
            // what it asked for.
            error.set((propose)(proposal.clone()).err());
        })
    };
    let footer = h_stack((empty().style(|s| s.flex_grow(1.0_f32)), review))
        .style(|s| s.flex_row().items_center().width_full());

    let error_line = dyn_container(
        move || error.get(),
        move |e| match e {
            Some(msg) => text(msg)
                .style(|s| {
                    s.width_full()
                        .font_size(theme::font_label())
                        .color(theme::error())
                })
                .into_any(),
            None => empty().into_any(),
        },
    )
    .style(|s| s.width_full());

    v_stack((head, summary, error_line, footer))
        .style(|s| {
            s.flex_col()
                .width_full()
                .gap(theme::scaled(7.0))
                .padding(theme::scaled(10.0))
                .background(theme::bg_deepest())
                .border(1.0)
                .border_color(theme::border())
                .border_radius(6.0)
        })
        .into_any()
}

fn code_block(
    code: String,
    actions: CodeActions,
    lang: &str,
    is_sql: bool,
    settled: bool,
) -> impl IntoView {
    let shown = code.trim_end().to_string();
    // **Syntax-coloured, on the editor's own surface** — the same pairing the
    // History and Snippet previews take, through the same two accessors, because
    // this is the same thing: SQL, read outside the editor. `theme::preview_bg`
    // holds why both come off the editor axis rather than the UI one.
    //
    // **Only once the turn has settled**, and that is not a perf hedge. The
    // bubbles rebuild on every streamed token, and mid-stream the block is
    // syntactically incomplete: one arrived-but-unclosed quote paints every line
    // after it as a string literal, so the block would flicker through wrong
    // colourings on the way to the right one. Plain until it is whole, then
    // coloured — and the per-token lex goes with it.
    //
    // A non-SQL block (shell, json) stays plain in any case, but takes the same
    // surface and the same base: it is still code, and a second background for
    // it would be two kinds of block in one conversation.
    let base_style = |s: floem::style::Style| {
        s.width_full()
            .font_size(theme::font_body())
            .padding_horiz(theme::scaled(9.0))
            .padding_vert(theme::scaled(7.0))
    };
    let body: AnyView = match is_sql && settled {
        true => crate::widgets::highlight_sql_mono(
            shown,
            None,
            theme::font_body,
            theme::preview_fg,
            CODE_LINE_H,
            actions.dialect.get_untracked(),
        )
        .style(base_style)
        .into_any(),
        // The family and the line height are spelled the same on both paths, so
        // a block does not change shape when it settles — only colour.
        false => text(shown)
            .style(move |s| {
                base_style(s)
                    .font_family(crate::consts::MONO_FAMILY.to_string())
                    .line_height(CODE_LINE_H)
                    .color(theme::preview_fg())
            })
            .into_any(),
    };

    // What the block *is*, said once on the left. `is_sql` is the authority
    // rather than the tag, because an untagged block that starts `SELECT` is
    // treated as SQL everywhere else here (`code_is_sql`) — labelling it "CODE"
    // while offering it Run would be the header contradicting the buttons.
    let kind = if is_sql {
        "SQL".to_string()
    } else if lang.trim().is_empty() {
        "CODE".to_string()
    } else {
        lang.trim().to_ascii_uppercase()
    };

    // Copy for any block; Insert and Run only for SQL, since they target the SQL
    // editor. Run is Insert-&-Run: it lands in a new tab and executes there, so
    // what ran is on screen afterwards rather than having happened invisibly.
    let mut links: Vec<AnyView> = Vec::new();
    let copy_code = code.clone();
    links.push(
        code_action_link("Copy", move || {
            let _ = floem::Clipboard::set_contents(copy_code.clone());
        })
        .into_any(),
    );
    if is_sql {
        let insert_code = code.clone();
        let insert = actions.insert.clone();
        links.push(
            code_action_link("Insert", move || {
                (insert)(insert_code.clone());
            })
            .into_any(),
        );
        let run_code = code.clone();
        let run_insert = actions.insert.clone();
        let run = actions.run.clone();
        links.push(
            code_action_link("Run", move || {
                (run_insert)(run_code.clone());
                (run)(run_code.clone());
            })
            .into_any(),
        );
    }

    // The header carries the block's own radius, not just the wrapper's: floem
    // does not clip a child to a rounded parent, so a square-cornered fill here
    // would paint over the top of the wrapper's arc and square the block off.
    let header = h_stack((
        text(kind).style(|s| {
            s.font_size(theme::font_label())
                .font_bold()
                .color(theme::text_muted())
        }),
        empty().style(|s| s.flex_grow(1.0_f32)),
        h_stack_from_iter(links).style(|s| s.flex_row().items_center().gap(theme::scaled(10.0))),
    ))
    .style(|s| {
        s.width_full()
            .flex_row()
            .items_center()
            .height(theme::scaled(24.0))
            .padding_horiz(theme::scaled(8.0))
            .gap(theme::scaled(10.0))
            .background(theme::group_header_bg())
            .border_radius(CODE_RADIUS)
            .border_bottom(1.0)
            .border_color(theme::border())
    });

    v_stack((header, body)).style(|s| {
        s.flex_col()
            .width_full()
            .background(theme::preview_bg())
            .border(1.0)
            .border_color(theme::border())
            .border_radius(CODE_RADIUS)
    })
}

/// Corner radius of a code block, shared by the block and its header so the two
/// arcs agree — see the note in [`code_block`].
const CODE_RADIUS: f64 = 5.0;
/// Line height of a code block's text, on **both** the coloured and the plain
/// path so a block does not reflow when the turn settles. The figure the History
/// and Snippet previews use, for the same text at the same size.
const CODE_LINE_H: f32 = 1.4;

#[cfg(test)]
mod tests {
    use super::{code_is_sql, sql_leading_keyword};

    /// The decision this pair encodes is *which code blocks get a Run button*,
    /// and it is the only place in the chat where a model's output becomes
    /// something the user can execute against their database in one click. A
    /// false positive puts Run on a shell command; a false negative hides it
    /// from the SQL the whole conversation was about.

    #[test]
    fn every_sql_dialect_tag_is_authoritative() {
        for lang in [
            "sql",
            "mysql",
            "mariadb",
            "postgres",
            "postgresql",
            "psql",
            "sqlite",
            "tsql",
        ] {
            assert!(code_is_sql(lang, "anything at all"), "{lang}");
        }
    }

    /// Claude writes the fence tag, and it is not consistent about case or a
    /// stray space after the backticks.
    #[test]
    fn a_tag_is_matched_case_insensitively_and_trimmed() {
        assert!(code_is_sql("SQL", "x"));
        assert!(code_is_sql("  PostgreSQL  ", "x"));
    }

    /// **The tag wins over the body.** A block tagged `bash` holding something
    /// that reads like SQL is still not SQL — `DROP TABLE` inside a heredoc in
    /// a shell script is exactly the case where offering Run would be worst.
    #[test]
    fn a_non_sql_tag_beats_a_sql_looking_body() {
        assert!(!code_is_sql("bash", "SELECT 1"));
        assert!(!code_is_sql("json", "SELECT 1"));
        assert!(!code_is_sql("python", "DROP TABLE t"));
    }

    #[test]
    fn an_untagged_block_falls_back_to_its_first_word() {
        assert!(code_is_sql("", "SELECT 1"));
        assert!(!code_is_sql("", "npm install"));
    }

    /// Every statement kind the fallback claims to know, in the case Claude
    /// actually writes them in. A keyword quietly dropped from the list is a
    /// Run button that stops appearing, with nothing else to notice it.
    #[test]
    fn the_fallback_knows_each_statement_kind_it_lists() {
        for code in [
            "SELECT 1",
            "WITH x AS (SELECT 1) SELECT * FROM x",
            "INSERT INTO t VALUES (1)",
            "UPDATE t SET a = 1",
            "DELETE FROM t",
            "REPLACE INTO t VALUES (1)",
            "CREATE TABLE t (a int)",
            "ALTER TABLE t ADD b int",
            "DROP TABLE t",
            "TRUNCATE TABLE t",
            "SHOW TABLES",
            "DESCRIBE t",
            "DESC t",
            "EXPLAIN SELECT 1",
            "USE shop",
            "SET @x = 1",
            "CALL p()",
            "GRANT ALL ON *.* TO u",
            "REVOKE ALL ON *.* FROM u",
            "RENAME TABLE a TO b",
            "ANALYZE TABLE t",
            "OPTIMIZE TABLE t",
            "START TRANSACTION",
            "BEGIN",
            "COMMIT",
            "ROLLBACK",
        ] {
            assert!(sql_leading_keyword(code), "{code}");
        }
    }

    #[test]
    fn the_fallback_ignores_case_and_leading_whitespace() {
        assert!(sql_leading_keyword("select 1"));
        assert!(sql_leading_keyword("\n\n   update t set a = 1"));
    }

    /// A whole-word match, not a prefix one: the keyword is read by taking
    /// letters until the first non-letter, so `SELECTED` is its own word and
    /// not a `SELECT`.
    #[test]
    fn the_fallback_matches_a_whole_word_rather_than_a_prefix() {
        assert!(!sql_leading_keyword("SELECTED rows are shown below"));
        assert!(!sql_leading_keyword("CREATED_AT is the column"));
        // …and the word still ends at a non-letter that is not a space.
        assert!(sql_leading_keyword("SELECT(1)"));
    }

    /// The known limits, pinned so a change to them is a decision rather than a
    /// surprise: the fallback reads the *first* word of the block, so a leading
    /// comment, an opening parenthesis or an empty block all read as not-SQL.
    /// An untagged block is the uncommon case and a missing Run button is the
    /// safe way to be wrong.
    #[test]
    fn the_fallback_declines_what_does_not_start_with_a_keyword() {
        assert!(!sql_leading_keyword(""));
        assert!(!sql_leading_keyword("   "));
        assert!(!sql_leading_keyword("-- a comment\nSELECT 1"));
        assert!(!sql_leading_keyword("/* note */ SELECT 1"));
        assert!(!sql_leading_keyword("(SELECT 1)"));
        assert!(!sql_leading_keyword("1 + 1"));
    }
}
