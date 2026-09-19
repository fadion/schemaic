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

use crate::consts::row_h;
use crate::settings::{focusable_dropdown, focusable_toggle_row};
use crate::widgets::{
    ACTION_TAB, ActionKind, ExitAction, FocusRing, MenuEntry, action_button, action_gap, autohide,
    control_button, exit_action, focus_root_with_ring, form_gap, form_hint, form_section,
    form_separator, form_setting, modal_footer, modal_footer_split, modal_h, modal_pad_h,
    modal_title_owned, modal_w, panel_style, shift_hscroll,
};
use crate::{
    ConnUi, FieldCfg, ImportFn, ImportProbeFn, ImportProbeRequest, ImportRunRequest, ImportStep,
    ImportTargetInfo, ImportUi, SchemaUi, edit_field, icons, theme,
};

/// Everything the import modal reaches out of the root bundle.
///
/// **The drivers take this; the renderers take [`ImportUi`].** Four of the views
/// below only read and write the modal's own signals, and those say so — a
/// context struct on a function that needs one bundle is a widening dressed as a
/// convenience. What genuinely needs gathering is the half that *acts*: a probe
/// or a load also reaches a fetch, the read-only flag, or the schema tree.
///
/// Holds no `Ui`, for `whole_ui_gate`'s reason and `users_view::UsersCtx`'s: it
/// is built by naming the reads at the one call site, so what this modal touches
/// is visible there rather than inside a constructor that can quietly grow.
#[derive(Clone)]
pub(crate) struct ImportCtx {
    /// The modal's own state — step, file, settings, sample, mapping, issues.
    import: ImportUi,
    /// The connection registry, for the read-only flag [`run_import`] asks
    /// **live** in the same step as the launch.
    conn: ConnUi,
    /// The schema tree, watched so a target whose database has gone from it
    /// closes the modal rather than importing into nothing.
    schema: SchemaUi,
    /// Read the file's opening records so the modal can show what it found.
    probe: ImportProbeFn,
    /// Check the file and, if it's clean, load it in one transaction.
    run: ImportFn,
    /// Stop the load, rolling it back. **Not the probe** — see the exit note in
    /// [`import_overlay`].
    cancel: Rc<dyn Fn()>,
}

impl ImportCtx {
    /// Gather the modal's reads where the root bundle is in reach.
    pub(crate) fn new(
        import: ImportUi,
        conn: ConnUi,
        schema: SchemaUi,
        actions: &crate::SchemaActions,
    ) -> Self {
        Self {
            import,
            conn,
            schema,
            probe: actions.import_probe.clone(),
            run: actions.import_run.clone(),
            cancel: actions.import_cancel.clone(),
        }
    }
}

/// Rows shown in the mapping step's preview. Enough to spot a wrong delimiter or
/// an off-by-one mapping; not so many that the panel becomes a grid.
const PREVIEW_ROWS: usize = 50;
/// Problem lines rendered before the list is cut. The section sits inside the
/// body's own scroll, so this was never a layout requirement — 20 was a bare
/// literal, and its whole cost was that the disclosure below it was wired to
/// core's cap instead of this one (`import::issue_tail`). 50 is the same number
/// as the preview's, for the same reason: enough to see the shape of what is
/// wrong without turning the panel into a report.
const ISSUE_LINES: usize = 50;
/// One width for every step, so the panel doesn't resize as you move through it.
fn panel_w() -> f64 {
    modal_w(620.0)
}
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
fn mapping_list_h() -> f64 {
    theme::scaled(216.0)
}
/// Text-field width, matching the connection form's fixed-width fields.
fn field_w() -> f64 {
    theme::scaled(220.0)
}

/// The settings the modal's controls describe, as the reader wants them.
fn read_config(ui: ImportUi) -> ReadConfig {
    let delim = ui.delimiter.get_untracked();
    // The control holds a display string ("\t" for tab) so a tab is typeable.
    let delimiter = match delim.as_str() {
        "\\t" | "\t" => b'\t',
        s => s.as_bytes().first().copied().unwrap_or(b','),
    };
    ReadConfig {
        dialect: CsvDialect {
            delimiter,
            quote: b'"',
            has_header: ui.has_header.get_untracked(),
        },
        nulls: null_rule(
            ui.empty_is_null.get_untracked(),
            &ui.null_tokens.get_untracked(),
        ),
        trim: ui.trim.get_untracked(),
        sheet: ui.sheet.get_untracked(),
    }
}

/// The two NULL controls as one [`NullRule`].
///
/// **Separate from [`read_config`] because the preview needs the same rule from
/// *tracked* reads.** `read_config` reads every control untracked, which is
/// right for launching a probe and wrong inside a `dyn_container` builder — a
/// second spelling here is how the preview and the load would come to disagree
/// about what a NULL is, which is exactly the class of defect the preview exists
/// to catch.
///
/// The empty-string token comes from its own toggle: it can't be written in a
/// comma-separated list, so an empty box would otherwise be read as "no tokens"
/// and quietly turn every blank field into an empty string.
fn null_rule(empty_is_null: bool, tokens: &str) -> NullRule {
    let mut out: Vec<String> = Vec::new();
    if empty_is_null {
        out.push(String::new());
    }
    out.extend(
        tokens
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    );
    NullRule { tokens: out }
}

fn delimiter_display(d: u8) -> String {
    match d {
        b'\t' => "\\t".to_string(),
        other => (other as char).to_string(),
    }
}

/// Reset everything and open the modal on `target`.
///
/// Takes the bundle rather than [`ImportCtx`]: this door only writes the modal's
/// own signals, so gathering the fetches for it would be three `Rc` clones spent
/// on nothing.
pub(crate) fn open_import(i: ImportUi, target: ImportTargetInfo) {
    i.step.set(ImportStep::Source);
    i.path.set(None);
    i.format.set(ImportFormat::Csv);
    i.delimiter.set(",".to_string());
    i.has_header.set(true);
    // The workbook is per-file, so both belong to the previous open. A sheet
    // list left standing would offer sheets from a file this import isn't
    // reading, and a sheet *name* left standing would be applied to the next
    // workbook the moment one is picked.
    i.sheets.set(Vec::new());
    i.sheet.set(None);
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
    i.reading.set(false);
    i.loading.set(false);
    i.applying.set(false);
    // Anything still in flight from a previous open belongs to that open.
    i.generation.update(|g| *g += 1);
    i.target.set(Some(target));
}

/// Probe the picked file and fold the result into the modal's state. `sniff` asks
/// for the dialect to be detected rather than taken from the controls — true on
/// the first look at a file, false when the user changes a setting.
fn probe(ctx: &ImportCtx, sniff: bool) {
    let i = ctx.import;
    let Some(path) = i.path.get_untracked() else {
        return;
    };
    let format = i.format.get_untracked();
    let cfg = (!sniff).then(|| read_config(i));
    i.reading.set(true);
    // A probe is a new question, so the previous read's answers go — the error
    // *and* the problem list, which this used to leave standing. See
    // `ImportUi::begin_probe` for what it deliberately keeps.
    i.begin_probe();
    let target = i.target.get_untracked();
    // Which question this answer belongs to: which *opening* of the modal, and
    // which *request* within it. Several probes of one file are routinely in
    // flight — see `import::probe_verdict`.
    i.probe_seq.update(|n| *n += 1);
    let mine = (i.generation.get_untracked(), i.probe_seq.get_untracked());
    (ctx.probe)(
        ImportProbeRequest { path, format, cfg },
        Rc::new(move |res| {
            let current = (i.generation.get_untracked(), i.probe_seq.get_untracked());
            // Discarded whole, `reading` included: a request still in flight is
            // still a reason the modal must not enable Next.
            if import::probe_verdict(mine, current) == import::ProbeVerdict::Discard {
                return;
            }
            i.reading.set(false);
            match res {
                Err(e) => {
                    i.error.set(Some(e));
                    i.file_bytes.set(0);
                    i.sample.set(None);
                    // Nothing is known about this file, so nothing is claimed
                    // about its sheets. Leaving them would describe the
                    // *previous* workbook beside an error about this one — and
                    // clearing the chosen sheet is also what unwedges the one
                    // failure this can cause on its own: a name carried over
                    // from another file that this one has no sheet for. The
                    // next read then takes the first sheet and succeeds.
                    //
                    // Under `applying`, so resetting the choice is not mistaken
                    // by the settings effect for the user changing it.
                    i.applying.set(true);
                    i.sheets.set(Vec::new());
                    i.sheet.set(None);
                    i.applying.set(false);
                }
                Ok(p) => {
                    // The picker's options, whatever else the probe found. Set
                    // before the settings below so a `sheet` written here is
                    // always one the list can show.
                    let sheets = p.sheets.clone();
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
                        // A new file is a new workbook: a sheet name carried
                        // over from the last one would either not exist (a hard
                        // read error the user never asked for) or, worse, exist
                        // and silently be a different table.
                        i.sheet.set(None);
                        i.applying.set(false);
                    }
                    i.publish_sheets(sheets);
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

/// The delimiter box: one or two characters, so it is sized to its content
/// rather than to the form's field column.
///
/// `width` is a `fn` rather than an `f64` for the reason `theme::scaled` states —
/// a captured number cannot re-run when the interface scale changes, and the box
/// would keep its old width while the character inside it grew.
fn small_field(
    value: RwSignal<String>,
    width: fn() -> f64,
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
    .style(move |s| s.width(width()))
}

/// The delimiter box's width — two characters of the form font plus its padding.
fn delimiter_w() -> f64 {
    theme::scaled(96.0)
}

/// Step 1 — the file and how to read it.
fn source_step(ctx: &ImportCtx, ring: FocusRing) -> impl IntoView {
    let i = ctx.import;
    let ctx_pick = ctx.clone();
    // The first stop in the step, ahead of the Format picker at 10: without it a
    // keyboard user could reach every reading setting and never pick a file,
    // which is the one thing this step is for.
    let pick = control_button("Choose file…", ring.clone(), 5, move || {
        let ctx = ctx_pick.clone();
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
                ctx.import.applying.set(true);
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && let Some(f) = import::infer_format(name)
                {
                    ctx.import.format.set(f);
                }
                ctx.import.path.set(Some(path));
                ctx.import.applying.set(false);
                probe(&ctx, true);
            },
        );
    });

    let chosen = dyn_container(
        move || i.path.get(),
        move |p| match p {
            None => text("No file chosen")
                .style(|s| s.color(theme::text_dim()).font_size(theme::font_body()))
                .into_any(),
            Some(p) => text(
                p.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("(file)")
                    .to_string(),
            )
            .style(|s| s.color(theme::text()).font_size(theme::font_body()))
            .into_any(),
        },
    );

    // Excel-only settings: which sheet, and whether its first row names the
    // columns. Built and torn down the same way `csv_settings` is, and for the
    // same reason — a `hide()`n control is still in the modal's Tab order.
    //
    // The header toggle is repeated here rather than lifted out of
    // `csv_settings` and shared: it is the only setting the two formats have in
    // common, and hoisting it would put it above the format-specific block for
    // CSV too, moving a control users already know the position of.
    // **The sheet list is *not* in this container's key.** It is written on
    // every probe, and a probe fires whenever a reading setting changes — so
    // keying on it rebuilt this whole block, header toggle included, the moment
    // the user pressed Space on that toggle, dropping keyboard focus. floem's
    // `dyn_container` has no `PartialEq` bound and `create_updater` does not
    // dedup, so an equal value rebuilds just as readily as a changed one. Only
    // the picker below depends on the list, so only the picker is keyed on it.
    let ring_xlsx = ring.clone();
    let xlsx_settings = dyn_container(
        move || i.format.get() == ImportFormat::Xlsx,
        move |is_xlsx| {
            if !is_xlsx {
                return empty().into_any();
            }
            let ring = ring_xlsx.clone();
            // Only worth a control when there is a choice to make: a
            // single-sheet workbook is the common case and a dropdown with one
            // entry is furniture. The list is empty until the probe returns,
            // which is also when there is nothing to choose from yet.
            //
            // `nothing()` and not `empty()`: a zero-sized child still *is* a
            // flex child, so taffy gives it a `form_gap` on each side and the
            // header toggle sits two gaps below "Reading" instead of one —
            // visibly further than Delimiter sits from it on the CSV side. A
            // `display:none` child is skipped entirely. Same trap the
            // `csv_settings` container below is written around.
            let ring_pick = ring.clone();
            let picker = dyn_container(
                move || i.sheets.get(),
                move |sheets| {
                    let ring = ring_pick.clone();
                    if sheets.len() < 2 {
                        crate::widgets::nothing()
                    } else {
                        let names = sheets.clone();
                        let current = {
                            let names = names.clone();
                            move || {
                                i.sheet
                                    .get()
                                    .unwrap_or_else(|| names.first().cloned().unwrap_or_default())
                            }
                        };
                        let entries = {
                            let names = names.clone();
                            move || {
                                let chosen = i.sheet.get_untracked();
                                names
                                    .iter()
                                    .enumerate()
                                    .map(|(n, name)| {
                                        let name = name.clone();
                                        let pick = {
                                            let name = name.clone();
                                            move || i.sheet.set(Some(name.clone()))
                                        };
                                        // The same tint-means-current vocabulary
                                        // `focusable_dropdown` uses. `None` is the first
                                        // sheet, so it tints that one.
                                        let holding = match &chosen {
                                            Some(c) => *c == name,
                                            None => n == 0,
                                        };
                                        match holding {
                                            true => {
                                                MenuEntry::action_colored(name, theme::accent, pick)
                                            }
                                            false => MenuEntry::action(name, pick),
                                        }
                                    })
                                    .collect()
                            }
                        };
                        form_setting(
                            "Sheet",
                            container(crate::settings::in_ring_picker(
                                crate::settings::picker_box_content(current),
                                entries,
                                ring.clone(),
                                15,
                            ))
                            .style(|s| s.width(field_w())),
                        )
                        .into_any()
                    }
                },
            )
            // The container is the flex child, so an absent picker has to step
            // aside rather than sit there at zero height: taffy charges a
            // `form_gap` on each side of a zero-sized child and skips a
            // `display:none` one, which is the difference between the header
            // toggle sitting one gap under "Reading" and two.
            .style(move |s| {
                let s = s.width_full();
                // `with`, not `get`: a length comparison does not need a copy of
                // the workbook's sheet list, and this is inside a style closure.
                if i.sheets.with(|v| v.len() < 2) {
                    s.hide()
                } else {
                    s
                }
            });
            v_stack((
                form_section("Reading"),
                picker,
                focusable_toggle_row(
                    "First row is a header",
                    "Use the first row as column names instead of data.",
                    i.has_header,
                    ring,
                    30,
                ),
            ))
            .style(|s| s.flex_col().gap(form_gap()).width_full())
            .into_any()
        },
    )
    .style(move |s| {
        let s = s.width_full();
        if i.format.get() == ImportFormat::Xlsx {
            s
        } else {
            s.hide()
        }
    });

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
                    small_field(i.delimiter, delimiter_w, ring.clone(), 20),
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
                        .style(|s| s.width(field_w())),
                        form_hint("Comma-separated, e.g. NULL, \\N, NA."),
                    ))
                    .style(|s| s.flex_col().gap(theme::scaled(4.0))),
                ),
            ))
            .style(|s| s.flex_col().gap(form_gap()).width_full())
            .into_any()
        },
    )
    // The *container* is the flex child, so hiding its inner view wouldn't be
    // enough: taffy counts a zero-sized child for the parent's gap and skips a
    // `display:none` one, so on JSON this whole node steps aside rather than
    // leaving a `form_gap` of dead space under the Format row. Nothing is
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
                .style(|s| s.items_center().gap(theme::scaled(10.0)).width_full()),
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
            .style(|s| s.width(theme::scaled(150.0))),
        ),
        xlsx_settings,
        csv_settings,
    ))
    .style(|s| s.flex_col().gap(form_gap()).width_full())
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
fn mapping_row(i: ImportUi, fi: usize, file_col: String, ring: FocusRing) -> impl IntoView {
    let Some(info) = i.target.get_untracked() else {
        return empty().into_any();
    };
    let table = info.table;

    let name = text(file_col).style(|s| {
        s.width(theme::scaled(150.0))
            .font_size(theme::font_body())
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
    // The box, with `Skip` dimmed: a half-mapped file then reads at a glance
    // rather than needing every row read. `picker_box_content` is the plain
    // version, so this one is spelled out — the dimming is the whole difference.
    let main = dyn_container(current, move |t| {
        let skipped = t == Target::Skip;
        let label = target_label(&t, &main_table);
        h_stack((
            text(label).style(move |s| {
                let s = s
                    .font_size(theme::font_body())
                    .text_ellipsis()
                    .min_width(0.0);
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
        .style(|s| s.items_center().width_full().gap(theme::scaled(8.0)))
        .into_any()
    })
    .style(|s| s.width_full().min_width(0.0))
    .into_any();

    let row_table = table.clone();
    let pick = move |chosen: Target| {
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
    };
    let entries = move || {
        options
            .iter()
            .map(|t| {
                let label = target_label(t, &row_table);
                let (t, held) = (t.clone(), current() == *t);
                let act = move || pick(t.clone());
                match held {
                    true => MenuEntry::action_colored(label, theme::accent, act),
                    false => MenuEntry::action(label, act),
                }
            })
            .collect()
    };
    // One Tab stop per file column, in the order they appear, through the shared
    // constants rather than a hand-rolled base and stride.
    //
    // `100 + fi * 10` was an unbounded block whose base sat *below* the
    // `assert!(110 < VALUE_TAB)` floor — the compile-time chain's own statement
    // of where fixed controls end — and whose stride was a literal beside a
    // named `ROW_TAB_STRIDE`. Nothing collided today; adding one fixed control
    // at ≥100 to this step would have made Tab order build order.
    let picker = container(crate::settings::in_ring_picker(
        main,
        entries,
        ring,
        crate::widgets::VALUE_TAB + fi as u32 * crate::widgets::ROW_TAB_STRIDE,
    ))
    .style(|s| s.width(theme::scaled(300.0)).flex_shrink(0.0_f32));

    h_stack((
        name,
        icons::icon(icons::CHEVRON_RIGHT, 14.0).style(|s| {
            s.color(theme::text_faint())
                .margin_horiz(theme::scaled(10.0))
        }),
        picker,
    ))
    .style(|s| {
        s.items_center()
            .width_full()
            .margin_bottom(theme::scaled(6.0))
    })
    .into_any()
}

/// Preview column width, matching the results grid's default feel.
fn preview_col_w() -> f64 {
    theme::scaled(140.0)
}
/// Preview body height — five grid rows, scrolling past that. Enough to see a
/// pattern (and a wrong delimiter) while still fitting the step without the
/// whole body needing to scroll.
fn preview_body_h() -> f64 {
    row_h() * 5.0
}
/// One preview cell, wearing the results grid's cell metrics: 8px horizontal
/// padding, the grid's row height, and faint italic for a NULL — so the preview
/// looks like the table the rows are going into.
fn preview_cell(label: String, is_null: bool) -> impl IntoView {
    text(label).style(move |s| {
        let s = s
            .width(preview_col_w())
            .padding_horiz(theme::scaled(8.0))
            .font_size(theme::font_body())
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
fn preview_table(i: ImportUi) -> impl IntoView {
    dyn_container(
        // **The NULL settings are part of the key**, and that is the whole fix
        // for the preview not being able to show a CSV NULL. They cannot change
        // what a *probe* returns — which is why the settings effect rightly does
        // not re-read the file for them — so the rule has to be applied here, at
        // render time, over the sample already in hand. That costs no file read,
        // which is what makes the tokens box live-verifiable for the first time.
        move || {
            (
                i.sample.get(),
                i.mapping.get(),
                i.empty_is_null.get(),
                i.null_tokens.get(),
                i.format.get(),
            )
        },
        move |(sample, mapping, empty_is_null, null_tokens, format)| {
            let Some(sample) = sample else {
                return empty().into_any();
            };
            // Built here rather than through `read_config`, which reads the
            // signals untracked — inside a `dyn_container` builder that would be
            // a stale answer on the very rebuild this key exists to cause.
            let nulls = null_rule(empty_is_null, &null_tokens);
            // Only mapped columns are previewed — the rest aren't going anywhere,
            // and showing them would imply otherwise.
            let shown: Vec<usize> = (0..sample.columns.len())
                .filter(|fi| matches!(mapping.targets.get(*fi), Some(Target::Column(_))))
                .collect();
            if shown.is_empty() {
                return container(
                    text("Nothing is mapped, so there's nothing to import.")
                        .style(|s| s.color(theme::plan_warn()).font_size(theme::font_body())),
                )
                .style(|s| s.padding(theme::scaled(12.0)))
                .into_any();
            }
            // **Columns without values.** A JSON file whose first record is
            // larger than the prefix the preview reads yields its column names
            // and nothing else (`import::read_json_columns`), which is enough to
            // map and import. Said here because the alternative is a header over
            // an empty body, which reads as an empty file — and `values_withheld`
            // rather than `rows.is_empty()`, because a file with a header and no
            // records has the same shape and a different explanation.
            if sample.values_withheld {
                return container(
                    text(
                        "This file's first record is too large to preview, so only the \
                         column names were read. Mapping and import read the whole file.",
                    )
                    .style(|s| s.color(theme::text_dim()).font_size(theme::font_body())),
                )
                .style(|s| s.padding(theme::scaled(12.0)))
                .into_any();
            }

            // Header row: bold dim labels on the grid's own header height, with
            // the same bottom rule the results grid draws.
            let header = h_stack_from_iter(shown.iter().map(|&fi| {
                text(sample.columns[fi].clone()).style(|s| {
                    s.width(preview_col_w())
                        .padding_horiz(theme::scaled(8.0))
                        .font_size(theme::font_label())
                        .font_bold()
                        .color(theme::text_dim())
                        .text_ellipsis()
                        .flex_shrink(0.0_f32)
                })
            }))
            .style(|s| {
                s.flex_row()
                    .items_center()
                    .height(theme::scaled(28.0))
                    .flex_shrink(0.0_f32)
                    .border_bottom(1.0)
                    .border_color(theme::border())
            });

            // Zebra striping and row height straight from the grid, so the
            // preview reads as the same surface.
            let body = v_stack_from_iter(sample.rows.iter().take(PREVIEW_ROWS).enumerate().map(
                |(pos, row)| {
                    h_stack_from_iter(shown.iter().map(|&fi| {
                        // Nullness is `import::preview_field`'s answer, not this
                        // view's: it asks `NullRule` and `has_own_nulls` the way
                        // the *load* does, so what is drawn faint is what will
                        // land as NULL. Deciding it from the `Field` alone — the
                        // only spelling this had — could never see a CSV null at
                        // all, because `read_csv_sample` builds every field as
                        // `Some(..)`.
                        let cell = row
                            .get(fi)
                            .map(|f| import::preview_field(f, &nulls, format))
                            // Short of the header: not a null, but there is
                            // nothing to draw and the row is already an issue.
                            .unwrap_or(import::PreviewCell::Null);
                        match cell {
                            import::PreviewCell::Text(v) => preview_cell(v.to_string(), false),
                            // Rendered the way the grid renders one, so it can't
                            // be mistaken for an empty string.
                            import::PreviewCell::Null => preview_cell("NULL".to_string(), true),
                        }
                    }))
                    .style(move |s| {
                        let s = s.flex_row().items_center().height(row_h());
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
                        autohide(scroll(body)).style(|s| s.height(preview_body_h()).width_full()),
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
fn issue_list(i: ImportUi) -> impl IntoView {
    dyn_container(
        move || (i.issues.get(), i.more_issues.get()),
        move |(issues, more)| {
            if issues.is_empty() {
                // `nothing()`, not `empty()`: taffy excludes a `display:none`
                // child from gap accounting but counts a zero-sized one, so a
                // bare `empty()` here leaves a whole gap of dead space in the
                // *common* case — this section usually has nothing to show.
                return crate::widgets::nothing();
            }
            let heading = text(format!(
                "{} problem{} in the file — nothing was imported.",
                issues.len(),
                if issues.len() == 1 { "" } else { "s" }
            ))
            .style(|s| {
                s.color(theme::error())
                    .font_size(theme::font_body())
                    .font_bold()
                    .margin_bottom(theme::scaled(6.0))
            });
            let shown = issues.len().min(ISSUE_LINES);
            let lines = v_stack_from_iter(issues.iter().take(ISSUE_LINES).map(|is: &Issue| {
                let where_ = if is.column.is_empty() {
                    format!("line {}", is.line)
                } else {
                    format!("line {}, {}", is.line, is.column)
                };
                text(format!("{where_}: {}", is.kind.message())).style(|s| {
                    s.font_size(theme::scaled_font(11.0))
                        .color(theme::text())
                        .margin_bottom(theme::scaled(2.0))
                })
            }));
            // Both caps, in one sentence, from core — see `issue_tail`.
            let tail_text = schemaic_core::import::issue_tail(shown, issues.len(), more);
            let showing = tail_text.is_some();
            let tail = text(tail_text.unwrap_or_default()).style(move |s| {
                if showing {
                    s.font_size(theme::scaled_font(11.0))
                        .color(theme::text_dim())
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
fn mapping_step(i: ImportUi, ring: FocusRing) -> impl IntoView {
    let rows = dyn_container(
        move || {
            i.sample
                .get()
                .map(|s| s.columns.clone())
                .unwrap_or_default()
        },
        move |cols| {
            let ring = ring.clone();
            v_stack_from_iter(
                cols.into_iter()
                    .enumerate()
                    .map(move |(fi, c)| mapping_row(i, fi, c, ring.clone())),
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
                return crate::widgets::nothing();
            }
            text(format!(
                "Not mapped, and can't be empty: {}. The import will fail unless \
                 the database fills them in.",
                cols.join(", ")
            ))
            .style(|s| {
                s.color(theme::plan_warn())
                    .font_size(theme::font_body())
                    .max_width(theme::scaled(560.0))
                    .margin_top(theme::scaled(20.0))
            })
            .into_any()
        },
    );

    // Two formats can't stream, for two reasons: a JSON file's columns are the
    // union of every object's keys, so nothing can be emitted before EOF, and an
    // `.xlsx` is a ZIP whose directory is at the end of the file, so it cannot
    // be read as a prefix even in principle. Both are therefore held in memory
    // at some multiple of the file's size. Said here, before the load, because
    // the alternative was the user discovering it when the app died; a CSV of
    // the same data is constant-memory, which both messages name as the way out.
    //
    // `memory_warning` rather than either format's own: one call site, so a
    // format that needs a warning cannot be given one nothing asks for.
    let size_note = dyn_container(
        move || import::memory_warning(i.format.get(), i.file_bytes.get()),
        move |warning| {
            let Some(warning) = warning else {
                return crate::widgets::nothing();
            };
            text(warning)
                .style(|s| {
                    s.color(theme::plan_warn())
                        .font_size(theme::font_body())
                        .max_width(theme::scaled(560.0))
                        .margin_top(theme::scaled(20.0))
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
                return crate::widgets::nothing();
            };
            text(format!(
                "This table's storage engine ({engine}) is not transactional, so a \
                 failed import can't be undone — the rows loaded before the failure \
                 will remain."
            ))
            .style(|s| {
                s.color(theme::plan_warn())
                    .font_size(theme::font_body())
                    .max_width(theme::scaled(560.0))
                    .margin_top(theme::scaled(20.0))
            })
            .into_any()
        },
    );

    const GAP: f64 = 8.0;
    v_stack((
        form_section("Columns"),
        autohide(scroll(rows)).style(|s| s.max_height(mapping_list_h()).width_full()),
        missing,
        size_note,
        engine_note,
        form_separator(|| GAP),
        form_section("Preview"),
        preview_table(i),
        issue_list(i).style(|s| s.margin_top(theme::scaled(16.0))),
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
fn run_import(ctx: &ImportCtx) {
    let i = ctx.import;
    let (Some(target), Some(path)) = (i.target.get_untracked(), i.path.get_untracked()) else {
        return;
    };
    // **The guard, in the same synchronous step as the launch.** The disabled
    // Import button is what *says* a load is running; it is not what stops a
    // second one, because it only takes effect on a later update pass. Two
    // launches within one key dispatch each opened their own transaction and
    // each committed — see `widgets::accept_launch`.
    //
    // **And the read-only half was the literal `false`**, which disabled exactly
    // half of it: Import was the one database write with no read-only check
    // anywhere on its path. The only refusal was `.disabled(read_only || …)` on
    // the context-menu entry — the disabled control the invariant names as
    // insufficient, and one whose value is fixed when the *menu* is built. So a
    // connection flipped read-only from the status bar while the modal stood,
    // or a menu opened before the flag was set, wrote the file anyway.
    //
    // Asked **live**, for the reason `ddl_preview::apply` gives in full: two
    // destructive modals must not answer the same question two different ways,
    // and the flag can move while the modal is on screen.
    let read_only = ctx
        .conn
        .connections
        .with_untracked(|cs| schemaic_core::connection::read_only_of(cs, target.conn_id));
    if !crate::widgets::accept_launch(i.loading.get_untracked(), read_only) {
        return;
    }
    i.loading.set(true);
    i.error.set(None);
    i.issues.set(Vec::new());
    let opened = i.generation.get_untracked();
    (ctx.run)(
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
            i.loading.set(false);
            match outcome {
                crate::ImportOutcome::Invalid(v) => {
                    i.issues.set(v.issues);
                    i.more_issues.set(v.more_issues);
                }
                crate::ImportOutcome::Done(n) => {
                    i.imported.set(n);
                    i.step.set(ImportStep::Done);
                }
                // `Cancelled` now *means* the rollback completed: the write path
                // rolls back through `rollback()` and downgrades a rollback that
                // didn't undo everything to `Failed`, carrying
                // `Rollback::note()`. This sentence used to be printed
                // unconditionally, which on `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV`
                // told the user nothing was written while ~250k rows sat in the
                // table — and the re-run then doubled them.
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
pub(crate) fn import_overlay(ctx: ImportCtx) -> impl IntoView {
    let i = ctx.import;
    // Every exit — footer, Escape, ✕ — goes through one decision. While a load
    // is running this cancels (rolling the transaction back) instead of closing:
    // closing would hide a bulk write that is still going and would leave its
    // outcome with no reader, since the modal's signals are the only channel
    // `import_run` reports to. The footer used to be the only exit that knew.
    //
    // **`loading`, not "anything is busy".** A *probe* is a read with no
    // transaction and no token — `import_cancel` cancels a slot only the load
    // ever writes — so routing an exit there during a read cancelled nothing and
    // left the modal on screen with no way out at all: a large file with one
    // unterminated quote reads for as long as the file is big, and Escape, ✕ and
    // Cancel were all inert for the duration.
    let exit: Rc<dyn Fn()> = {
        let stop = ctx.cancel.clone();
        Rc::new(move || match exit_action(i.loading.get_untracked(), true) {
            ExitAction::Close => i.target.set(None),
            ExitAction::Cancel => (stop)(),
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
        let ctx = ctx.clone();
        create_effect(
            move |prev: Option<(String, bool, bool, ImportFormat, Option<String>)>| {
                // The sheet belongs here for exactly the reason the delimiter does:
                // it changes which bytes the reader sees, so the preview and the
                // mapping both have to be rebuilt from the new one. It is the *one*
                // Excel setting that does — the NULL rules stay absent for the
                // reason above.
                let cur = (
                    i.delimiter.get(),
                    i.has_header.get(),
                    i.trim.get(),
                    i.format.get(),
                    i.sheet.get(),
                );
                let changed = prev.is_some_and(|p| p != cur);
                if changed
                    && !i.applying.get_untracked()
                    && i.target.get_untracked().is_some()
                    && i.path.get_untracked().is_some()
                {
                    probe(&ctx, false);
                }
                cur
            },
        );
    }

    // A refreshed schema shouldn't leave the modal editing a table that's gone —
    // but closing needs *positive* evidence, which is what
    // `import::target_survives` is for. `load_schema` empties `db_nodes` before
    // it fetches, so "not in the list" was true of every refresh and of every
    // connection switch: the per-node `Loading` arm below never ran, because
    // there was no node left to be loading.
    {
        let db_nodes = ctx.schema.db_nodes;
        let active_conn = ctx.conn.active_conn;
        let stop = ctx.cancel.clone();
        create_effect(move |_| {
            db_nodes.track();
            if let Some(t) = i.target.get_untracked()
                && i.step.get_untracked() != ImportStep::Done
            {
                // Reduced to plain data and decided in core: the decision was
                // already tested there while its *evidence* was assembled here,
                // untested — and two of the three inputs were the bug.
                let verdict = db_nodes.with_untracked(|nodes| {
                    let views: Vec<import::DbNodeView<'_>> = nodes
                        .iter()
                        .map(|n| import::DbNodeView {
                            database: &n.database,
                            has_table: match n.schema.get_untracked() {
                                schemaic_core::schema::SchemaState::Loaded(db) => Some(
                                    db.tables
                                        .iter()
                                        .any(|x| x.name == t.table.name && x.schema == t.schema),
                                ),
                                // Mid-refresh or failed: nothing was looked at,
                                // which is not a report that the table is gone.
                                _ => None,
                            },
                        })
                        .collect();
                    import::target_verdict(
                        &views,
                        active_conn.get_untracked() == t.conn_id,
                        &t.database,
                        i.loading.get_untracked(),
                    )
                });
                match verdict {
                    import::TargetVerdict::Keep => {}
                    import::TargetVerdict::Close => i.target.set(None),
                    // A load is running: cancelling rolls its transaction back,
                    // where closing would abandon a bulk write whose outcome has
                    // nowhere left to report.
                    import::TargetVerdict::Cancel => (stop)(),
                }
            }
        });
    }

    dyn_container(
        move || (i.target.with(Option::is_some), i.step.get()),
        move |(open, step)| {
            if !open {
                return empty().into_any();
            }
            let ctx = ctx.clone();
            let title = i
                .target
                .with_untracked(|t| t.as_ref().map(|t| t.display()).unwrap_or_default());

            // The Tab order belongs to the modal, and this rebuilds per step —
            // so each step gets its own ring, holding exactly the controls that
            // are on screen.
            let ring = FocusRing::new();
            let root_ring = ring.clone();
            let body = match step {
                ImportStep::Source => source_step(&ctx, ring.clone()).into_any(),
                ImportStep::Mapping => mapping_step(i, ring.clone()).into_any(),
                ImportStep::Done => text(format!(
                    "Imported {} row{} into {title}.",
                    i.imported.get_untracked(),
                    if i.imported.get_untracked() == 1 {
                        ""
                    } else {
                        "s"
                    }
                ))
                .style(|s| s.color(theme::text()).font_size(theme::font_body()))
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
                                .font_size(theme::font_body())
                                .max_width(theme::scaled(520.0))
                                .margin_top(theme::scaled(10.0))
                        })
                        .into_any(),
                },
            );

            // The footer wears the schema editors' filled actions: Cancel neutral,
            // the affirmative one last and filled, which is the pair every other
            // modal ends with. Back is the exception to the "actions on the right"
            // rule — it moves *backwards* through the modal, so it sits at the far
            // left rather than in the group deciding what happens next.
            let conns_ro = ctx.conn.connections;
            let ctx_run = ctx.clone();
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
                    move || (i.sample.with(Option::is_some), i.reading.get()),
                    move |(has_sample, busy)| {
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
                                    move || i.step.set(ImportStep::Mapping),
                                ),
                            ))
                            .style(|s| s.flex_row().items_center().gap(action_gap())),
                        )
                        .into_any()
                    },
                )
                .into_any(),
                ImportStep::Mapping => dyn_container(
                    // **`read_only` is in the key, not read in the builder.** A
                    // `dyn_container` builder is not a tracking scope, so a flag
                    // flipped from the status bar while the modal stands would
                    // be frozen at whatever the last rebuild saw — and this is
                    // the term that decides whether Import is offered.
                    move || {
                        let target = i.target.get();
                        let read_only = target.as_ref().is_some_and(|t| {
                            conns_ro
                                .with(|cs| schemaic_core::connection::read_only_of(cs, t.conn_id))
                        });
                        (i.loading.get(), i.mapping.get(), target, read_only)
                    },
                    move |(busy, mapping, target, read_only)| {
                        let ctx = ctx_run.clone();
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
                                move || i.step.set(ImportStep::Source),
                            ),
                            h_stack((
                                // Said where the disabled button is, rather than
                                // leaving it unexplained — the same sentence, in
                                // the same place, as `ddl_preview`'s.
                                text("This connection is read-only.").style(move |s| {
                                    let s = s
                                        .color(theme::plan_warn())
                                        .font_size(theme::font_label())
                                        .margin_right(theme::scaled(12.0));
                                    if read_only { s } else { s.hide() }
                                }),
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
                                    // **The enable term `run_import`'s guard was
                                    // copied without.** The guard is real —
                                    // `accept_launch` refuses the write — but it
                                    // refuses *silently*, so Import stayed lit,
                                    // said nothing and did nothing. That is the
                                    // failure `ddl_preview::plan_read_only`'s own
                                    // doc records, on the modal that copied its
                                    // guard.
                                    ready && !busy && !read_only,
                                    ring,
                                    ACTION_TAB + 20,
                                    move || run_import(&ctx),
                                ),
                            ))
                            .style(|s| s.flex_row().items_center().gap(action_gap())),
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
                modal_title_owned(format!("Import into {title}"), close_x, root_ring.clone()),
                // `autohide`, not a plain `scroll`: each section inside scrolls on
                // its own, so this outer one is only a safety net for the issue
                // list that appears after a failed check. A permanently-visible
                // bar for that would suggest the step has more content than it
                // does.
                autohide(scroll(v_stack((body, err)).style(|s| {
                    s.flex_col()
                        .width_full()
                        .padding_horiz(modal_pad_h())
                        .padding_vert(theme::scaled(18.0))
                })))
                .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0)),
                footer,
            ))
            .on_click_stop(|_| {})
            .style(move |s| {
                panel_style(s).width(panel_w()).height(modal_h(match step {
                    ImportStep::Mapping => PANEL_H_MAPPING,
                    _ => PANEL_H,
                }))
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
        if i.target.with(Option::is_some) {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

#[cfg(test)]
mod tests {
    /// **The other half of `ImportUi::begin_probe`, and the only half that was
    /// ever wrong.** Asserting that `begin_probe` empties `issues` proves
    /// nothing — it did the moment it was written, and a `set(vec![])` in this
    /// file would pass it too. The defect was that `probe` cleared `error` and
    /// not the problem list, so what has to be pinned is that `probe` asks the
    /// bundle rather than reaching for one signal it happens to remember.
    #[test]
    fn a_probe_invalidates_the_previous_read_through_the_bundle() {
        let src =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/import_view.rs"))
                .expect("this file");
        let body = crate::source_gate::production_code(&src);
        let at = body
            .find("fn probe(")
            .expect("probe is gone — this gate is stale");
        let end = body[at..].find("\n}\n").expect("the end of probe");
        let probe = &body[at..at + end];
        assert!(
            probe.contains("begin_probe()"),
            "`probe` no longer invalidates the previous read through the \
             bundle, so a problem list from another file — or from before the \
             setting that fixed it — stays on screen beside the new preview"
        );
        assert!(
            !probe.contains("i.error.set(None)"),
            "`probe` clears `error` by hand again. That is the spelling that \
             left `issues` behind: the two are one answer about one read, and \
             `begin_probe` is where they go together"
        );
    }

    /// **The gate.** The problem list's "there is more" line must come from
    /// `import::issue_tail`, not from this file's own reading of
    /// `more_issues`.
    ///
    /// There are two caps and they are not the same one: core stops
    /// *collecting* at `max_issues` (200), this view stops *rendering* at
    /// `ISSUE_LINES`. The tail used to be `if more { "…and more." }`, wired to
    /// the larger cap only — so the whole 21..=199 range reported as complete,
    /// and a file with 57 problems showed 20 lines under a heading that said
    /// 57. A unit test on `issue_tail` alone guards nothing here: the helper
    /// was correct in isolation the moment it was written, and the defect was
    /// entirely in which of the two numbers the view asked about.
    #[test]
    fn the_problem_list_discloses_its_own_cap_and_not_only_cores() {
        let src =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/import_view.rs"))
                .expect("this file");
        let body = crate::source_gate::production_code(&src);
        assert!(
            body.contains("issue_tail("),
            "the tail must be derived by `import::issue_tail`, which sees both caps"
        );
        assert!(
            !body.contains("…and more."),
            "the old wording names neither cap and fires only on core's"
        );
    }

    /// **A guard that refuses silently is not a disclosure.**
    ///
    /// `run_import` asks the live read-only flag and `accept_launch` refuses the
    /// write, which is the dangerous half and it is right. What was copied from
    /// `ddl_preview::apply` was the guard alone: Import stayed lit, said nothing
    /// and did nothing — the exact failure `ddl_preview::plan_read_only`'s own
    /// doc records, on the modal that copied its guard.
    ///
    /// Three things, because the defect is that they came apart. The flag has to
    /// be in the `dyn_container`'s **key** — a builder is not a tracking scope,
    /// so a flag flipped from the status bar while the modal stands would be
    /// frozen at whatever the last rebuild saw. It has to reach the Import
    /// button's enable term. And the sentence has to be in the footer, in the
    /// same words as the modal this one is modelled on.
    ///
    /// Read off the source because all three live in view closures. The floor is
    /// that the footer was located at all.
    #[test]
    fn the_import_button_says_a_read_only_connection_rather_than_going_quiet() {
        let src =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/import_view.rs"))
                .expect("this file");
        let body = crate::source_gate::production_code(&src);

        assert!(
            body.contains("if busy { \"Importing…\" } else { \"Import\" }"),
            "the Import button was not found — this gate is stale"
        );
        assert!(
            body.contains("ready && !busy && !read_only"),
            "the Import button's enable term dropped its read-only half, so it \
             stays lit on a connection whose write `run_import` will refuse"
        );
        assert!(
            body.contains("This connection is read-only."),
            "the footer no longer says why Import is disabled — the same \
             sentence `ddl_preview` puts beside its own disabled Apply"
        );
        // The guard itself, which the enable term does not replace: a disabled
        // button is not what stops the write.
        assert!(
            body.contains("crate::widgets::accept_launch(i.loading.get_untracked(), read_only)"),
            "`run_import` no longer guards its own launch"
        );
    }
}
