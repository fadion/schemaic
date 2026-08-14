//! The file-import modal, opened from a table's schema context menu.
//!
//! Two steps in one panel. **Source** picks the file and shows how it's being
//! read — the delimiter and header were sniffed, and both are editable, because a
//! wrong delimiter is the most common way an import goes wrong and it's obvious
//! the moment the preview renders. **Mapping** pairs the file's columns with the
//! table's and previews the first rows as they'd land, so the mapping is verified
//! by looking rather than by trusting.
//!
//! The heavy work isn't here: reading, checking and loading all happen off the UI
//! thread through `SchemaActions::import_probe` / `import_run`. This module owns
//! the panel and the state transitions.

use std::rc::Rc;

use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::import::{
    self, CsvDialect, ImportFormat, Issue, Mapping, NullRule, ReadConfig, Target,
};
use schemaic_core::model::engine_is_transactional;

use crate::consts::ROW_H;
use crate::settings::{dropdown_box_style, focusable_dropdown, focusable_toggle_row};
use crate::widgets::{
    ACTION_GAP, ACTION_TAB, ActionKind, ExitAction, FORM_GAP, FocusRing, MODAL_PAD_H,
    action_button, autohide, control_button, exit_action, focus_root_with_ring, form_hint,
    form_section, form_separator, form_setting, modal_footer, modal_footer_split,
    modal_title_owned, panel_style, shift_hscroll,
};
use crate::{
    FieldCfg, ImportProbeRequest, ImportRunRequest, ImportStep, ImportTargetInfo, ImportUi, Ui,
    edit_field, icons, theme,
};

/// Rows shown in the mapping step's preview. Enough to spot a wrong delimiter or
/// an off-by-one mapping; not so many that the panel becomes a grid.
const PREVIEW_ROWS: usize = 50;
/// One width for every step, so the panel doesn't resize as you move through it.
const PANEL_W: f64 = 620.0;
/// The source step's height. Fixed rather than content-sized so the footer sits
/// in the same place on both steps.
const PANEL_H: f64 = 520.0;
/// The mapping step needs more room — a column list, a preview table, and enough
/// slack for the unmapped-required warning to appear without the body scrolling.
/// An offset rather than a ratio, since what it absorbs is a fixed amount of
/// content, not a proportion of the first step.
const PANEL_H_MAPPING: f64 = PANEL_H + 134.0;
/// The column list's height before it scrolls. Deep enough that a typical table's
/// columns are visible without scrolling at all.
const MAPPING_LIST_H: f64 = 216.0;
/// Text-field width, matching the connection form's fixed-width fields.
const FIELD_W: f64 = 220.0;

/// The settings the modal's controls describe, as the reader wants them.
fn read_config(ui: ImportUi) -> ReadConfig {
    let delim = ui.delimiter.get_untracked();
    // The control holds a display string ("\t" for tab) so a tab is typeable.
    let delimiter = match delim.as_str() {
        "\\t" | "\t" => b'\t',
        s => s.as_bytes().first().copied().unwrap_or(b','),
    };
    // The empty-string token comes from its own toggle: it can't be written in a
    // comma-separated list, so an empty box would otherwise be read as "no
    // tokens" and quietly turn every blank field into an empty string.
    let mut tokens: Vec<String> = Vec::new();
    if ui.empty_is_null.get_untracked() {
        tokens.push(String::new());
    }
    tokens.extend(
        ui.null_tokens
            .get_untracked()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    );
    ReadConfig {
        dialect: CsvDialect {
            delimiter,
            quote: b'"',
            has_header: ui.has_header.get_untracked(),
        },
        nulls: NullRule { tokens },
        trim: ui.trim.get_untracked(),
    }
}

fn delimiter_display(d: u8) -> String {
    match d {
        b'\t' => "\\t".to_string(),
        other => (other as char).to_string(),
    }
}

/// Reset everything and open the modal on `target`.
pub(crate) fn open_import(ui: &Ui, target: ImportTargetInfo) {
    let i = ui.import;
    i.step.set(ImportStep::Source);
    i.path.set(None);
    i.format.set(ImportFormat::Csv);
    i.delimiter.set(",".to_string());
    i.has_header.set(true);
    i.empty_is_null.set(true);
    i.null_tokens.set(String::new());
    i.trim.set(false);
    i.file_bytes.set(0);
    i.sample.set(None);
    i.mapping.set(Mapping {
        targets: Vec::new(),
    });
    i.issues.set(Vec::new());
    i.more_issues.set(false);
    i.error.set(None);
    i.imported.set(0);
    i.busy.set(false);
    i.applying.set(false);
    // Anything still in flight from a previous open belongs to that open.
    i.generation.update(|g| *g += 1);
    i.target.set(Some(target));
}

/// Probe the picked file and fold the result into the modal's state. `sniff` asks
/// for the dialect to be detected rather than taken from the controls — true on
/// the first look at a file, false when the user changes a setting.
fn probe(ui: Ui, sniff: bool) {
    let i = ui.import;
    let Some(path) = i.path.get_untracked() else {
        return;
    };
    let format = i.format.get_untracked();
    let cfg = (!sniff).then(|| read_config(i));
    i.busy.set(true);
    i.error.set(None);
    let target = i.target.get_untracked();
    // Which question this answer belongs to: which *opening* of the modal, and
    // which *request* within it. Several probes of one file are routinely in
    // flight — see `import::probe_verdict`.
    i.probe_seq.update(|n| *n += 1);
    let mine = (i.generation.get_untracked(), i.probe_seq.get_untracked());
    (ui.schema_actions.import_probe)(
        ImportProbeRequest { path, format, cfg },
        Rc::new(move |res| {
            let current = (i.generation.get_untracked(), i.probe_seq.get_untracked());
            // Discarded whole, `busy` included: a request still in flight is
            // still a reason the modal must not enable Next.
            if import::probe_verdict(mine, current) == import::ProbeVerdict::Discard {
                return;
            }
            i.busy.set(false);
            match res {
                Err(e) => {
                    i.error.set(Some(e));
                    i.file_bytes.set(0);
                    i.sample.set(None);
                }
                Ok(p) => {
                    if sniff {
                        // The settings effect watches these; writing them here is
                        // the app answering itself, so it must not bounce back
                        // into another probe.
                        i.applying.set(true);
                        i.delimiter.set(delimiter_display(p.cfg.dialect.delimiter));
                        i.has_header.set(p.cfg.dialect.has_header);
                        i.empty_is_null
                            .set(p.cfg.nulls.tokens.iter().any(|t| t.is_empty()));
                        i.null_tokens.set(
                            p.cfg
                                .nulls
                                .tokens
                                .iter()
                                .filter(|t| !t.is_empty())
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(","),
                        );
                        i.applying.set(false);
                    }
                    // Re-propose the mapping whenever the columns change: after a
                    // delimiter fix the old mapping refers to columns that no
                    // longer exist.
                    if let Some(t) = &target {
                        i.mapping.set(import::auto_map(
                            &p.sample.columns,
                            &t.table,
                            p.cfg.dialect.has_header || format == ImportFormat::Json,
                        ));
                    }
                    i.file_bytes.set(p.file_bytes);
                    i.sample.set(Some(p.sample));
                }
            }
        }),
    );
}

fn small_field(
    value: RwSignal<String>,
    width: f64,
    ring: FocusRing,
    tabindex: u32,
) -> impl IntoView {
    edit_field(
        value,
        FieldCfg {
            focus: Some((ring, tabindex)),
            ..Default::default()
        },
    )
    .style(move |s| s.width(width))
}

/// Step 1 — the file and how to read it.
fn source_step(ui: Ui, ring: FocusRing) -> impl IntoView {
    let i = ui.import;
    let ui_pick = ui.clone();
    // The first stop in the step, ahead of the Format picker at 10: without it a
    // keyboard user could reach every reading setting and never pick a file,
    // which is the one thing this step is for.
    let pick = control_button("Choose file…", ring.clone(), 5, move || {
        let ui = ui_pick.clone();
        floem::action::open_file(
            floem::file::FileDialogOptions::new().title("Import into table"),
            move |file| {
                let Some(path) = file.and_then(|f| f.path.first().cloned()) else {
                    return;
                };
                // The extension is a hint, not a decision — the dropdown can
                // override it, and an unknown extension leaves the choice alone.
                //
                // `applying` around the writes: the settings effect watches
                // `format`, and it would otherwise fire here — while `path` still
                // points at the *previous* file — racing a probe of the old file
                // under the new format against the real one below.
                ui.import.applying.set(true);
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && let Some(f) = import::infer_format(name)
                {
                    ui.import.format.set(f);
                }
                ui.import.path.set(Some(path));
                ui.import.applying.set(false);
                probe(ui.clone(), true);
            },
        );
    });

    let chosen = dyn_container(
        move || i.path.get(),
        move |p| match p {
            None => text("No file chosen")
                .style(|s| s.color(theme::text_dim()).font_size(theme::FONT_BODY))
                .into_any(),
            Some(p) => text(
                p.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("(file)")
                    .to_string(),
            )
            .style(|s| s.color(theme::text()).font_size(theme::FONT_BODY))
            .into_any(),
        },
    );

    // CSV-only settings — a JSON file has no delimiter, and its nulls are its own.
    //
    // Built only for CSV rather than built-and-hidden: a `hide()`n view is still
    // in the tree, so its controls would still be in the modal's Tab order and
    // Tab would move focus onto something nobody can see. Nothing is lost by
    // rebuilding — every control here binds straight to an `ImportUi` signal.
    let ring_csv = ring.clone();
    let csv_settings = dyn_container(
        move || i.format.get() == ImportFormat::Csv,
        move |is_csv| {
            if !is_csv {
                return empty().into_any();
            }
            let ring = ring_csv.clone();
            v_stack((
                form_section("Reading"),
                form_setting(
                    "Delimiter",
                    small_field(i.delimiter, 96.0, ring.clone(), 20),
                ),
                focusable_toggle_row(
                    "First row is a header",
                    "Use the first row as column names instead of data.",
                    i.has_header,
                    ring.clone(),
                    30,
                ),
                focusable_toggle_row(
                    "Trim whitespace",
                    "Strip spaces around every value — including inside quotes.",
                    i.trim,
                    ring.clone(),
                    40,
                ),
                focusable_toggle_row(
                    "Empty field is NULL",
                    "An empty field means NULL rather than an empty string.",
                    i.empty_is_null,
                    ring.clone(),
                    50,
                ),
                form_setting(
                    "Other values meaning NULL",
                    v_stack((
                        edit_field(
                            i.null_tokens,
                            FieldCfg {
                                focus: Some((ring, 60)),
                                ..Default::default()
                            },
                        )
                        .style(|s| s.width(FIELD_W)),
                        form_hint("Comma-separated, e.g. NULL, \\N, NA."),
                    ))
                    .style(|s| s.flex_col().gap(4.0)),
                ),
            ))
            .style(|s| s.flex_col().gap(FORM_GAP).width_full())
            .into_any()
        },
    )
    // The *container* is the flex child, so hiding its inner view wouldn't be
    // enough: taffy counts a zero-sized child for the parent's gap and skips a
    // `display:none` one, so on JSON this whole node steps aside rather than
    // leaving a `FORM_GAP` of dead space under the Format row. Nothing is
    // hidden-but-reachable — the branch builds no controls at all (see
    // `widgets::nothing`).
    .style(move |s| {
        let s = s.width_full();
        if i.format.get() == ImportFormat::Csv {
            s
        } else {
            s.hide()
        }
    });

    v_stack((
        form_section("Source"),
        form_setting(
            "File",
            h_stack((pick, chosen.style(|s| s.flex_grow(1.0_f32).min_width(0.0))))
                .style(|s| s.items_center().gap(10.0).width_full()),
        ),
        form_setting(
            "Format",
            container(focusable_dropdown(
                i.format,
                ImportFormat::ALL,
                ImportFormat::label,
                ring,
                10,
            ))
            .style(|s| s.width(150.0)),
        ),
        csv_settings,
    ))
    .style(|s| s.flex_col().gap(FORM_GAP).width_full())
}

/// How a target reads in the dropdown.
fn target_label(target: &Target, table: &schemaic_core::schema::TableInfo) -> String {
    match target {
        Target::Skip => "Skip".to_string(),
        Target::Column(ci) => table
            .columns
            .get(*ci)
            .map(|c| format!("{}  ({})", c.name, c.type_name))
            .unwrap_or_else(|| "—".to_string()),
    }
}

/// One file column's row in the mapping step: its name, and a dropdown for the
/// table column it feeds. The same dropdown chrome as the settings modals.
///
/// The dropdown reads and writes `mapping` directly instead of holding its own
/// signal — one source of truth, so re-proposing the mapping after a settings
/// change is a single `set` and every row follows it.
fn mapping_row(ui: Ui, fi: usize, file_col: String, ring: FocusRing) -> impl IntoView {
    use floem::views::dropdown::Dropdown;

    let i = ui.import;
    let Some(info) = i.target.get_untracked() else {
        return empty().into_any();
    };
    let table = info.table;

    let name = text(file_col).style(|s| {
        s.width(150.0)
            .font_size(theme::FONT_BODY)
            .color(theme::text())
            .text_ellipsis()
            .flex_shrink(0.0_f32)
    });

    // `Skip` first, so "don't import this" is always in the same place.
    let options: Vec<Target> = std::iter::once(Target::Skip)
        .chain((0..table.columns.len()).map(Target::Column))
        .collect();
    let current = move || {
        i.mapping
            .with(|m| m.targets.get(fi).cloned().unwrap_or(Target::Skip))
    };

    let main_table = table.clone();
    let main = move |t: Target| {
        let skipped = t == Target::Skip;
        let label = target_label(&t, &main_table);
        h_stack((
            text(label).style(move |s| {
                let s = s.font_size(theme::FONT_BODY);
                // A skipped column is dimmed, so a half-mapped file reads at a
                // glance rather than needing every row read.
                if skipped {
                    s.color(theme::text_faint())
                } else {
                    s.color(theme::text())
                }
            }),
            empty().style(|s| s.flex_grow(1.0_f32)),
            icons::icon(icons::CHEVRON_DOWN, 16.0)
                .style(|s| s.color(theme::text_dim()).flex_shrink(0.0_f32)),
        ))
        .style(|s| s.items_center().width_full().gap(8.0))
        .into_any()
    };

    let row_table = table.clone();
    let row = move |t: Target| {
        let label = target_label(&t, &row_table);
        let this = t.clone();
        text(label)
            .style(move |s| {
                let s = s
                    .width_full()
                    .padding_horiz(12.0)
                    .padding_vert(6.0)
                    .color(theme::text())
                    .font_size(theme::FONT_BODY)
                    .hover(|s| s.background(theme::dropdown_hover()));
                if current() == this {
                    s.background(theme::dropdown_active())
                } else {
                    s
                }
            })
            .into_any()
    };

    let picker = Dropdown::custom(current, main, options, row)
        .style(|s| dropdown_box_style(s).width(300.0).flex_shrink(0.0_f32));
    // One Tab stop per file column, in the order they appear. The row index is
    // the index, spaced so the step's own controls (if any are ever added above)
    // can sit in front of them.
    let picker = crate::settings::in_ring_dropdown(
        picker,
        ring,
        100 + fi as u32 * 10,
        move |chosen: Target| {
            i.mapping.update(|m| {
                // One target per column: claiming a column frees it from whoever
                // had it, so two file columns can't both write to the same place.
                if let Target::Column(ci) = chosen {
                    for (j, t) in m.targets.iter_mut().enumerate() {
                        if j != fi && *t == Target::Column(ci) {
                            *t = Target::Skip;
                        }
                    }
                }
                if let Some(slot) = m.targets.get_mut(fi) {
                    *slot = chosen;
                }
            });
        },
    );

    h_stack((
        name,
        icons::icon(icons::CHEVRON_RIGHT, 14.0)
            .style(|s| s.color(theme::text_faint()).margin_horiz(10.0)),
        picker,
    ))
    .style(|s| s.items_center().width_full().margin_bottom(6.0))
    .into_any()
}

/// Preview column width, matching the results grid's default feel.
const PREVIEW_COL_W: f64 = 140.0;
/// Preview body height — five grid rows, scrolling past that. Enough to see a
/// pattern (and a wrong delimiter) while still fitting the step without the
/// whole body needing to scroll.
const PREVIEW_BODY_H: f64 = ROW_H * 5.0;
/// One preview cell, wearing the results grid's cell metrics: 8px horizontal
/// padding, the grid's row height, and faint italic for a NULL — so the preview
/// looks like the table the rows are going into.
fn preview_cell(label: String, is_null: bool) -> impl IntoView {
    text(label).style(move |s| {
        let s = s
            .width(PREVIEW_COL_W)
            .padding_horiz(8.0)
            .font_size(theme::FONT_BODY)
            .text_ellipsis()
            .flex_shrink(0.0_f32);
        if is_null {
            s.color(theme::text_faint())
                .font_style(floem::text::Style::Italic)
        } else {
            s.color(theme::text())
        }
    })
}

/// The sampled rows as a real table: a pinned header over a scrolling body,
/// like the results grid.
///
/// Header and body sit inside **one** horizontal scroll, so they can't drift out
/// of alignment — the results grid needs a strict one-writer/one-reader rule
/// because its two panes scroll separately, and nesting sidesteps that entirely.
fn preview_table(ui: Ui) -> impl IntoView {
    let i = ui.import;
    dyn_container(
        move || (i.sample.get(), i.mapping.get()),
        move |(sample, mapping)| {
            let Some(sample) = sample else {
                return empty().into_any();
            };
            // Only mapped columns are previewed — the rest aren't going anywhere,
            // and showing them would imply otherwise.
            let shown: Vec<usize> = (0..sample.columns.len())
                .filter(|fi| matches!(mapping.targets.get(*fi), Some(Target::Column(_))))
                .collect();
            if shown.is_empty() {
                return container(
                    text("Nothing is mapped, so there's nothing to import.")
                        .style(|s| s.color(theme::plan_warn()).font_size(theme::FONT_BODY)),
                )
                .style(|s| s.padding(12.0))
                .into_any();
            }

            // Header row: bold dim labels on the grid's own header height, with
            // the same bottom rule the results grid draws.
            let header = h_stack_from_iter(shown.iter().map(|&fi| {
                text(sample.columns[fi].clone()).style(|s| {
                    s.width(PREVIEW_COL_W)
                        .padding_horiz(8.0)
                        .font_size(theme::FONT_LABEL)
                        .font_bold()
                        .color(theme::text_dim())
                        .text_ellipsis()
                        .flex_shrink(0.0_f32)
                })
            }))
            .style(|s| {
                s.flex_row()
                    .items_center()
                    .height(28.0)
                    .flex_shrink(0.0_f32)
                    .border_bottom(1.0)
                    .border_color(theme::border())
            });

            // Zebra striping and row height straight from the grid, so the
            // preview reads as the same surface.
            let body = v_stack_from_iter(sample.rows.iter().take(PREVIEW_ROWS).enumerate().map(
                |(pos, row)| {
                    h_stack_from_iter(shown.iter().map(|&fi| match row.get(fi) {
                        Some(Some(v)) => preview_cell(v.clone(), false),
                        // A format-level null, rendered the way the grid renders
                        // one, so it can't be mistaken for an empty string.
                        _ => preview_cell("NULL".to_string(), true),
                    }))
                    .style(move |s| {
                        let s = s.flex_row().items_center().height(ROW_H);
                        if pos % 2 == 1 {
                            s.background(theme::bg_editor())
                        } else {
                            s
                        }
                    })
                },
            ));

            // Header and body share ONE horizontal scroll, so they can't drift
            // apart; the vertical scroll is nested inside it around the body
            // alone. Both auto-hide their bars like every other scroll in the app,
            // and Shift+wheel pans horizontally as it does everywhere else.
            container(
                autohide(shift_hscroll(
                    v_stack((
                        header,
                        autohide(scroll(body)).style(|s| s.height(PREVIEW_BODY_H).width_full()),
                    ))
                    .style(|s| s.flex_col()),
                ))
                .style(|s| s.width_full()),
            )
            .style(|s| {
                s.width_full()
                    .border(1.0)
                    .border_color(theme::border())
                    .border_radius(6.0)
            })
            .into_any()
        },
    )
}

/// Problems the full check found. Present ⇒ the import was refused and nothing
/// was written, which the heading has to say plainly.
fn issue_list(ui: Ui) -> impl IntoView {
    let i = ui.import;
    dyn_container(
        move || (i.issues.get(), i.more_issues.get()),
        move |(issues, more)| {
            if issues.is_empty() {
                return empty().into_any();
            }
            let heading = text(format!(
                "{} problem{} in the file — nothing was imported.",
                issues.len(),
                if issues.len() == 1 { "" } else { "s" }
            ))
            .style(|s| {
                s.color(theme::error())
                    .font_size(theme::FONT_BODY)
                    .font_bold()
                    .margin_bottom(6.0)
            });
            let lines = v_stack_from_iter(issues.iter().take(20).map(|is: &Issue| {
                let where_ = if is.column.is_empty() {
                    format!("line {}", is.line)
                } else {
                    format!("line {}, {}", is.line, is.column)
                };
                text(format!("{where_}: {}", is.kind.message()))
                    .style(|s| s.font_size(11.0).color(theme::text()).margin_bottom(2.0))
            }));
            let tail = text(if more {
                "…and more.".to_string()
            } else {
                String::new()
            })
            .style(move |s| {
                if more {
                    s.font_size(11.0).color(theme::text_dim())
                } else {
                    s.hide()
                }
            });
            v_stack((heading, lines, tail))
                .style(|s| s.flex_col())
                .into_any()
        },
    )
}

/// Step 2 — mapping, preview, and whatever the last check said.
fn mapping_step(ui: Ui, ring: FocusRing) -> impl IntoView {
    let i = ui.import;
    let ui_rows = ui.clone();
    let rows = dyn_container(
        move || {
            i.sample
                .get()
                .map(|s| s.columns.clone())
                .unwrap_or_default()
        },
        move |cols| {
            let ui = ui_rows.clone();
            let ring = ring.clone();
            v_stack_from_iter(
                cols.into_iter()
                    .enumerate()
                    .map(move |(fi, c)| mapping_row(ui.clone(), fi, c, ring.clone())),
            )
            .style(|s| s.flex_col())
            .into_any()
        },
    );

    // Unmapped NOT NULL columns will fail on insert unless the server has a
    // default — which the introspected schema doesn't record, so this warns
    // rather than blocks.
    let missing = dyn_container(
        move || {
            let (t, m) = (i.target.get(), i.mapping.get());
            t.map(|t| m.missing_required(&t.table)).unwrap_or_default()
        },
        move |cols| {
            if cols.is_empty() {
                return empty().into_any();
            }
            text(format!(
                "Not mapped, and can't be empty: {}. The import will fail unless \
                 the database fills them in.",
                cols.join(", ")
            ))
            .style(|s| {
                s.color(theme::plan_warn())
                    .font_size(theme::FONT_BODY)
                    .max_width(560.0)
                    .margin_top(20.0)
            })
            .into_any()
        },
    );

    // A JSON load can't stream — the columns are the union of every object's
    // keys — so it is held in memory at roughly five times the file's size,
    // measured. Said here, before the load, because the alternative was the user
    // discovering it when the app died; CSV of the same data is constant-memory,
    // which the message names as the way out.
    let json_size_note = dyn_container(
        move || import::json_memory_warning(i.format.get(), i.file_bytes.get()),
        move |warning| {
            let Some(warning) = warning else {
                return empty().into_any();
            };
            text(warning)
                .style(|s| {
                    s.color(theme::plan_warn())
                        .font_size(theme::FONT_BODY)
                        .max_width(560.0)
                        .margin_top(20.0)
                })
                .into_any()
        },
    );

    // The load runs in one transaction and undoes itself on any failure — but
    // MyISAM/MEMORY/ARCHIVE/CSV ignore `BEGIN`/`ROLLBACK`, so on those the rows
    // written before a bad record stay. Said here, in the same place the preview
    // states its other consequences, rather than only in the error after the
    // fact. `engine` is `None` on PostgreSQL, where every table is
    // transactional, so this is MySQL-only by construction.
    let engine_note = dyn_container(
        move || {
            i.target
                .get()
                .and_then(|t| t.table.engine.clone())
                .filter(|e| !engine_is_transactional(e))
        },
        move |engine| {
            let Some(engine) = engine else {
                return empty().into_any();
            };
            text(format!(
                "This table's storage engine ({engine}) is not transactional, so a \
                 failed import can't be undone — the rows loaded before the failure \
                 will remain."
            ))
            .style(|s| {
                s.color(theme::plan_warn())
                    .font_size(theme::FONT_BODY)
                    .max_width(560.0)
                    .margin_top(20.0)
            })
            .into_any()
        },
    );

    const GAP: f64 = 8.0;
    v_stack((
        form_section("Columns"),
        autohide(scroll(rows)).style(|s| s.max_height(MAPPING_LIST_H).width_full()),
        missing,
        json_size_note,
        engine_note,
        form_separator(GAP),
        form_section("Preview"),
        preview_table(ui.clone()),
        issue_list(ui).style(|s| s.margin_top(16.0)),
    ))
    .style(|s| s.flex_col().gap(GAP).width_full())
}

/// Run the check-then-load, and fold the outcome back into the modal.
///
/// It sends a **fresh** `read_config` beside the **stored** mapping, which is
/// only sound because the two can no longer describe different settings: every
/// probe but the newest is discarded whole (`import::probe_verdict`), `busy`
/// stays set until that newest one lands, and Import is disabled while `busy`.
/// So the mapping on screen was built from the config on screen. Loosen any one
/// of those three and this becomes the mismatch that writes a file's `name`
/// column into `email`.
fn run_import(ui: Ui) {
    let i = ui.import;
    let (Some(target), Some(path)) = (i.target.get_untracked(), i.path.get_untracked()) else {
        return;
    };
    i.busy.set(true);
    i.error.set(None);
    i.issues.set(Vec::new());
    let opened = i.generation.get_untracked();
    (ui.schema_actions.import_run)(
        ImportRunRequest {
            target,
            path,
            format: i.format.get_untracked(),
            cfg: read_config(i),
            mapping: i.mapping.get_untracked(),
        },
        Rc::new(move |outcome| {
            // Closing the modal doesn't stop the import, so by the time it reports
            // the modal may be open on another table — reporting into that would
            // claim rows landed somewhere they didn't.
            if i.generation.get_untracked() != opened {
                return;
            }
            i.busy.set(false);
            match outcome {
                crate::ImportOutcome::Invalid(v) => {
                    i.issues.set(v.issues);
                    i.more_issues.set(v.more_issues);
                }
                crate::ImportOutcome::Done(n) => {
                    i.imported.set(n);
                    i.step.set(ImportStep::Done);
                }
                crate::ImportOutcome::Cancelled => i.error.set(Some(
                    "Import cancelled — the transaction rolled back, so nothing was written."
                        .to_string(),
                )),
                crate::ImportOutcome::Failed(e) => i.error.set(Some(e)),
            }
        }),
    );
}

/// The import modal. Absolutely positioned over the workspace when
/// `ui.import.target` is `Some`.
pub(crate) fn import_overlay(ui: Ui) -> impl IntoView {
    let i = ui.import;
    // Every exit — footer, Escape, ✕ — goes through one decision. While a load
    // is running this cancels (rolling the transaction back) instead of closing:
    // closing would hide a bulk write that is still going and would leave its
    // outcome with no reader, since the modal's signals are the only channel
    // `import_run` reports to. The footer used to be the only exit that knew.
    let exit: Rc<dyn Fn()> = {
        let stop = ui.schema_actions.clone();
        Rc::new(move || match exit_action(i.busy.get_untracked(), true) {
            ExitAction::Close => i.target.set(None),
            ExitAction::Cancel => (stop.import_cancel)(),
            // Unreachable for this modal (an import is always cancellable), but
            // matched explicitly so a future caller can't fall through to close.
            ExitAction::Ignore => {}
        })
    };
    // Handed to each exit site. `Rc` rather than a plain closure because it now
    // captures the cancel action; the sites clone it.
    let exit_at = move |e: &Rc<dyn Fn()>| {
        let e = e.clone();
        move || (e)()
    };

    // A setting that changes how the file *parses* re-reads it. One effect over
    // all of them, created once here rather than per rebuild, so switching steps
    // doesn't re-probe. `applying` skips the writes the modal itself makes, which
    // would otherwise loop.
    //
    // The NULL rules are deliberately absent: the sample holds raw field text and
    // nullness is decided at coercion time, so they can't change what a probe
    // returns. Tracking them would re-read the file on every keystroke in the
    // tokens box — and each read re-proposes the mapping, so it would also stamp
    // over a hand-edited one.
    {
        let ui = ui.clone();
        create_effect(move |prev: Option<(String, bool, bool, ImportFormat)>| {
            let cur = (
                i.delimiter.get(),
                i.has_header.get(),
                i.trim.get(),
                i.format.get(),
            );
            let changed = prev.is_some_and(|p| p != cur);
            if changed
                && !i.applying.get_untracked()
                && i.target.get_untracked().is_some()
                && i.path.get_untracked().is_some()
            {
                probe(ui.clone(), false);
            }
            cur
        });
    }

    // A refreshed schema shouldn't leave the modal editing a table that's gone —
    // but closing needs *positive* evidence, which is what
    // `import::target_survives` is for. `load_schema` empties `db_nodes` before
    // it fetches, so "not in the list" was true of every refresh and of every
    // connection switch: the per-node `Loading` arm below never ran, because
    // there was no node left to be loading.
    {
        let db_nodes = ui.schema.db_nodes;
        let stop = ui.schema_actions.clone();
        create_effect(move |_| {
            db_nodes.track();
            if let Some(t) = i.target.get_untracked()
                && i.step.get_untracked() != ImportStep::Done
            {
                let (empty, found) = db_nodes.with_untracked(|nodes| {
                    let found = nodes.iter().any(|n| {
                        n.database == t.database
                            && match n.schema.get_untracked() {
                                schemaic_core::schema::SchemaState::Loaded(db) => db
                                    .tables
                                    .iter()
                                    .any(|x| x.name == t.table.name && x.schema == t.schema),
                                // Mid-refresh: no evidence it's gone, so leave the
                                // modal alone rather than closing it under the user.
                                _ => true,
                            }
                    });
                    (nodes.is_empty(), found)
                });
                match import::target_survives(empty, found, i.busy.get_untracked()) {
                    import::TargetVerdict::Keep => {}
                    import::TargetVerdict::Close => i.target.set(None),
                    // A load is running: cancelling rolls its transaction back,
                    // where closing would abandon a bulk write whose outcome has
                    // nowhere left to report.
                    import::TargetVerdict::Cancel => (stop.import_cancel)(),
                }
            }
        });
    }

    dyn_container(
        move || (i.target.get().is_some(), i.step.get()),
        move |(open, step)| {
            if !open {
                return empty().into_any();
            }
            let ui = ui.clone();
            let title = i
                .target
                .with_untracked(|t| t.as_ref().map(|t| t.display()).unwrap_or_default());

            // The Tab order belongs to the modal, and this rebuilds per step —
            // so each step gets its own ring, holding exactly the controls that
            // are on screen.
            let ring = FocusRing::new();
            let root_ring = ring.clone();
            let body = match step {
                ImportStep::Source => source_step(ui.clone(), ring.clone()).into_any(),
                ImportStep::Mapping => mapping_step(ui.clone(), ring.clone()).into_any(),
                ImportStep::Done => text(format!(
                    "Imported {} row{} into {title}.",
                    i.imported.get_untracked(),
                    if i.imported.get_untracked() == 1 {
                        ""
                    } else {
                        "s"
                    }
                ))
                .style(|s| s.color(theme::text()).font_size(theme::FONT_BODY))
                .into_any(),
            };

            // One error line for a read/transaction failure — distinct from the
            // per-row issue list, which means "we refused", not "it broke".
            let err = dyn_container(
                move || i.error.get(),
                move |e| match e {
                    None => empty().into_any(),
                    Some(e) => text(e)
                        .style(|s| {
                            s.color(theme::error())
                                .font_size(theme::FONT_BODY)
                                .max_width(520.0)
                                .margin_top(10.0)
                        })
                        .into_any(),
                },
            );

            // The footer wears the schema editors' filled actions: Cancel neutral,
            // the affirmative one last and filled, which is the pair every other
            // modal ends with. Back is the exception to the "actions on the right"
            // rule — it moves *backwards* through the modal, so it sits at the far
            // left rather than in the group deciding what happens next.
            let ui_back = ui.clone();
            let ui_next = ui.clone();
            let ui_run = ui.clone();
            let (exit_src, exit_map, exit_done, exit_x, exit_esc) = (
                exit.clone(),
                exit.clone(),
                exit.clone(),
                exit.clone(),
                exit.clone(),
            );
            let ring_src = ring.clone();
            let ring_map = ring.clone();
            let ring_done = ring.clone();
            let footer = match step {
                ImportStep::Source => dyn_container(
                    move || (i.sample.get().is_some(), i.busy.get()),
                    move |(has_sample, busy)| {
                        let ui = ui_next.clone();
                        let ring = ring_src.clone();
                        modal_footer(
                            h_stack((
                                action_button(
                                    "Cancel",
                                    ActionKind::Neutral,
                                    true,
                                    ring.clone(),
                                    ACTION_TAB,
                                    exit_at(&exit_src),
                                ),
                                action_button(
                                    if busy { "Reading…" } else { "Next" },
                                    ActionKind::Primary,
                                    has_sample && !busy,
                                    ring,
                                    ACTION_TAB + 10,
                                    move || ui.import.step.set(ImportStep::Mapping),
                                ),
                            ))
                            .style(|s| s.flex_row().items_center().gap(ACTION_GAP)),
                        )
                        .into_any()
                    },
                )
                .into_any(),
                ImportStep::Mapping => dyn_container(
                    move || (i.busy.get(), i.mapping.get(), i.target.get()),
                    move |(busy, mapping, target)| {
                        let ui = ui_run.clone();
                        let back = ui_back.clone();
                        let ring = ring_map.clone();
                        let ready = target
                            .map(|t| !import::insert_columns(&mapping, &t.table).is_empty())
                            .unwrap_or(false);
                        modal_footer_split(
                            action_button(
                                "Back",
                                ActionKind::Neutral,
                                !busy,
                                ring.clone(),
                                ACTION_TAB,
                                move || back.import.step.set(ImportStep::Source),
                            ),
                            h_stack((
                                // While a load is running this stops it (rolling
                                // the transaction back) instead of closing —
                                // closing would hide a write that's still going,
                                // which is the one thing the user pressing Cancel
                                // doesn't want. That rule now lives in `exit`,
                                // which Escape and the ✕ share; this button used
                                // to be the only one that had it.
                                action_button(
                                    "Cancel",
                                    ActionKind::Neutral,
                                    true,
                                    ring.clone(),
                                    ACTION_TAB + 10,
                                    exit_at(&exit_map),
                                ),
                                action_button(
                                    if busy { "Importing…" } else { "Import" },
                                    ActionKind::Primary,
                                    ready && !busy,
                                    ring,
                                    ACTION_TAB + 20,
                                    move || run_import(ui.clone()),
                                ),
                            ))
                            .style(|s| s.flex_row().items_center().gap(ACTION_GAP)),
                        )
                        .into_any()
                    },
                )
                .into_any(),
                ImportStep::Done => modal_footer(action_button(
                    "Close",
                    ActionKind::Primary,
                    true,
                    ring_done,
                    ACTION_TAB,
                    exit_at(&exit_done),
                ))
                .into_any(),
            };

            let close_x: Rc<dyn Fn()> = exit_x.clone();
            let panel = v_stack((
                modal_title_owned(format!("Import into {title}"), close_x),
                // `autohide`, not a plain `scroll`: each section inside scrolls on
                // its own, so this outer one is only a safety net for the issue
                // list that appears after a failed check. A permanently-visible
                // bar for that would suggest the step has more content than it
                // does.
                autohide(scroll(v_stack((body, err)).style(|s| {
                    s.flex_col()
                        .width_full()
                        .padding_horiz(MODAL_PAD_H)
                        .padding_vert(18.0)
                })))
                .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0)),
                footer,
            ))
            .on_click_stop(|_| {})
            .style(move |s| {
                panel_style(s).width(PANEL_W).height(match step {
                    ImportStep::Mapping => PANEL_H_MAPPING,
                    _ => PANEL_H,
                })
            });

            focus_root_with_ring(container(panel), root_ring)
                .on_key_down(
                    Key::Named(NamedKey::Escape),
                    |_| true,
                    move |_| (exit_esc)(),
                )
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
        if i.target.get().is_some() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}
