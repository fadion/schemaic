//! The table designer: edit a table's shape, see what that means, hand it to the
//! preview.
//!
//! One `TableDraft` is the whole state ([`DdlUi::draft`]). Every control writes
//! into it and nothing else, so the change count in the footer is literally
//! [`ddl::diff`] of the draft against the introspected table — the same function
//! that generates the SQL. There's no second model of "what the user changed"
//! that could disagree with what gets emitted.
//!
//! Layout is a list on the left and a form on the right, per section. The form
//! is rebuilt on selection (and on any structural edit), and seeds fresh local
//! signals from the draft; each field writes back through an effect. That split
//! matters: the form must **not** rebuild when the draft changes, or typing a
//! name would tear down the field being typed into.
//!
//! Types, defaults and generated expressions are free-form text. The server is
//! the authority on what those mean — the same call as import's coercion — and a
//! curated picker would only be a list of the types we happened to think of.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::ddl::{
    self, CheckDraft, ColumnDraft, ForeignKeyDraft, IndexDraft, TableDraft, key_list_text,
    parse_key_list, parse_name_list,
};
use schemaic_core::intel::SqlDialect;
use schemaic_core::schema::{CheckInfo, ColumnInfo, ForeignKeyInfo, IndexInfo};

use crate::settings::{dropdown_box_style, settings_toggle_row};
use crate::widgets::{
    ACTION_GAP, ActionKind, FORM_GAP, MODAL_PAD_H, MenuEntry, action_button, autohide, focus_root,
    form_hint, form_setting, modal_footer_split, modal_title_owned, panel_style,
};
use crate::{
    DdlPreview, DesignerTab, DesignerTarget, FieldCfg, Ui, ddl_preview, edit_field, icons,
    object_location, theme,
};

const PANEL_W: f64 = 900.0;
const PANEL_H: f64 = 550.0;
/// The item list's width, shared by all three sections so switching between them
/// doesn't shift the form. Wide enough that a long name and a long type
/// (`timestamp without time zone`) can both be read — the detail pane has the
/// slack to give.
const LIST_W: f64 = 320.0;
/// One row of the item list.
const ROW_H: f64 = 30.0;
/// Text-field width in the detail form, matching the connection form's fields.
const FIELD_W: f64 = 260.0;

// ── opening ──────────────────────────────────────────────────────────────────

/// Open the designer on `target`, seeding the draft from the table it names (or
/// a blank one for a new table).
pub(crate) fn open_designer(ui: &Ui, target: DesignerTarget) {
    let d = ui.ddl;
    let draft = match &target.current {
        Some(t) => TableDraft::from_table(t),
        None => TableDraft::blank("new_table", target.schema.clone()),
    };
    d.draft.set(draft);
    // Always Table first — for an existing table it's the summary, and for a new
    // one it's where the name is set, which has to come before there's anything
    // worth putting columns on.
    d.tab.set(DesignerTab::Table);
    d.selected.set(0);
    d.rev.update(|r| *r += 1);
    d.error.set(None);
    d.preview.set(None);
    // See `view_editor::open_editor`: each overlay knows only its own flag.
    d.view.set(None);
    d.designer.set(Some(target));
}

/// What the schema-editing entries need to know about the active connection:
/// which one it is, what SQL it speaks, and whether it's allowed to write.
pub(crate) struct EditCtx {
    pub conn_id: u64,
    pub dialect: SqlDialect,
    pub read_only: bool,
}

pub(crate) fn edit_ctx(ui: &Ui) -> EditCtx {
    let conn_id = ui.conn.active_conn.get_untracked();
    let conn = ui
        .conn
        .connections
        .with_untracked(|cs| cs.iter().find(|c| c.id == conn_id).cloned());
    EditCtx {
        conn_id,
        dialect: conn
            .as_ref()
            .map(|c| SqlDialect::from_db_type(&c.db_type))
            .unwrap_or_default(),
        read_only: conn.is_some_and(|c| c.read_only),
    }
}

/// The introspected table behind a schema-tree node, when its database's schema
/// has loaded. `None` ⇒ nothing to design, and the menu entry is disabled.
pub(crate) fn loaded_table(
    ui: &Ui,
    database: &str,
    schema: Option<&str>,
    table: &str,
) -> Option<schemaic_core::schema::TableInfo> {
    ui.schema.db_nodes.with_untracked(|nodes| {
        nodes
            .iter()
            .find(|n| n.database == database)
            .and_then(|n| match n.schema.get_untracked() {
                schemaic_core::schema::SchemaState::Loaded(db) => db
                    .tables
                    .iter()
                    .find(|t| t.name == table && t.schema.as_deref() == schema)
                    .cloned(),
                _ => None,
            })
    })
}

/// The namespace a *new* object in `database` should land in: `public` on
/// PostgreSQL, `None` on MySQL (which has no level between database and table).
///
/// Read off the loaded schema rather than the connection's engine, because it's
/// the same question the tree answers by *showing* namespace nodes — a database
/// node stands for `public`, and any other namespace has its own node.
pub(crate) fn default_schema(ui: &Ui, database: &str) -> Option<String> {
    let has_namespaces =
        ui.schema
            .db_nodes
            .with_untracked(|nodes| {
                nodes.iter().find(|n| n.database == database).map(|n| {
                    match n.schema.get_untracked() {
                        schemaic_core::schema::SchemaState::Loaded(s) => !s.schemas().is_empty(),
                        _ => false,
                    }
                })
            })
            .unwrap_or(false);
    has_namespaces.then(|| schemaic_core::schema::PG_DEFAULT_SCHEMA.to_string())
}

/// Every table name in a database, for the foreign-key target picker. Views are
/// left out — a foreign key can't reference one.
fn table_names(ui: &Ui, database: &str, schema: Option<&str>) -> Vec<String> {
    ui.schema.db_nodes.with_untracked(|nodes| {
        nodes
            .iter()
            .find(|n| n.database == database)
            .map(|n| match n.schema.get_untracked() {
                schemaic_core::schema::SchemaState::Loaded(db) => db
                    .tables
                    .iter()
                    .filter(|t| !t.is_view && t.schema.as_deref() == schema)
                    .map(|t| t.name.clone())
                    .collect(),
                _ => Vec::new(),
            })
            .unwrap_or_default()
    })
}

/// Open the designer on an existing table, optionally with one column selected
/// (the schema tree's column right-click lands you on the column you clicked).
pub(crate) fn open_for_table(
    ui: &Ui,
    database: &str,
    schema: Option<&str>,
    table: &str,
    focus_column: Option<&str>,
) {
    let Some(info) = loaded_table(ui, database, schema, table) else {
        return;
    };
    let ctx = edit_ctx(ui);
    let column_at = focus_column.and_then(|c| info.columns.iter().position(|x| x.name == c));
    open_designer(
        ui,
        DesignerTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            schema: info.schema.clone(),
            dialect: ctx.dialect,
            current: Some(info),
            tables: table_names(ui, database, schema),
            read_only: ctx.read_only,
        },
    );
    if let Some(i) = column_at {
        ui.ddl.tab.set(DesignerTab::Columns);
        ui.ddl.selected.set(i);
        ui.ddl.rev.update(|r| *r += 1);
    }
}

/// Send the result of one draft edit straight to the preview, without opening
/// the designer.
///
/// Not the same as handing [`ddl_preview::preview_change`] a lone `Change`:
/// going through the draft means the *dependent* changes come too. Dropping a
/// column takes the index over it and any foreign key standing on it — emit the
/// `DROP COLUMN` on its own and the server refuses it.
pub(crate) fn preview_draft_edit(
    ui: &Ui,
    database: &str,
    schema: Option<&str>,
    table: &str,
    edit: impl FnOnce(&mut TableDraft),
) {
    let Some(info) = loaded_table(ui, database, schema, table) else {
        return;
    };
    let ctx = edit_ctx(ui);
    let mut draft = TableDraft::from_table(&info);
    edit(&mut draft);
    let cs = ddl::diff(&info, &draft, ctx.dialect);
    if cs.is_empty() {
        return;
    }
    ddl_preview::open_preview(
        ui,
        ddl_preview::preview_of(
            ctx.conn_id,
            database,
            schemaic_core::schema::display_name(schema, table),
            &cs,
            ctx.read_only,
        ),
    );
}

/// Open the designer on a blank draft — Create table.
pub(crate) fn open_for_new(ui: &Ui, database: &str, schema: Option<&str>) {
    let ctx = edit_ctx(ui);
    open_designer(
        ui,
        DesignerTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            schema: schema.map(str::to_string),
            dialect: ctx.dialect,
            current: None,
            tables: table_names(ui, database, schema),
            read_only: ctx.read_only,
        },
    );
}

// ── small shared pieces ──────────────────────────────────────────────────────

/// The section switcher — one row of quiet buttons, the active one carrying the
/// selected-row background the schema tree and dropdowns use.
fn tab_strip(ui: Ui) -> impl IntoView {
    let d = ui.ddl;
    h_stack_from_iter(DesignerTab::ALL.into_iter().map(move |t| {
        text(t.label())
            .on_click_stop(move |_| {
                if d.tab.get_untracked() != t {
                    d.tab.set(t);
                    d.selected.set(0);
                    d.rev.update(|r| *r += 1);
                }
            })
            .style(move |s| {
                let s = s
                    .font_size(theme::FONT_BODY)
                    .padding_horiz(12.0)
                    .padding_vert(7.0)
                    .border_radius(6.0);
                if d.tab.get() == t {
                    s.background(theme::pill_active_bg())
                        .color(theme::pill_active_text())
                } else {
                    s.color(theme::text_dim())
                        .hover(|s| s.background(theme::pill_hover_bg()).color(theme::text()))
                }
            })
    }))
    .style(|s| {
        s.flex_row()
            .items_center()
            .gap(4.0)
            .width_full()
            .padding_horiz(MODAL_PAD_H)
            .padding_vert(8.0)
            .border_bottom(1.0)
            .border_color(theme::border())
    })
}

/// A text field bound to one place in the draft.
///
/// The local signal is seeded once, on build; the effect writes back only on a
/// genuine change. Seeding through the effect instead would set the draft to the
/// value it already holds — and `RwSignal::set` never dedups, so every rebuild of
/// the form would look like an edit and re-render the list.
///
/// Returns an erased `AnyView` rather than `impl IntoView`: the opaque form would
/// capture the `&Ui` borrow, which every caller outlives.
fn bound_field(
    ui: &Ui,
    initial: String,
    width: f64,
    placeholder: &'static str,
    apply: impl Fn(&mut TableDraft, &str) + 'static,
) -> AnyView {
    field_view(bound_signal(ui, initial, apply), width, placeholder, false)
}

/// [`bound_field`] for a field whose content is **SQL**, not prose — a type, a
/// default expression, a generated expression. Monospace for the same reason the
/// DDL preview and the view editor's body are: it's the text that ends up in the
/// generated statement verbatim, and `varchar(255)` / `CURRENT_TIMESTAMP(3)`
/// read as code.
fn sql_field(
    ui: &Ui,
    initial: String,
    width: f64,
    placeholder: &'static str,
    apply: impl Fn(&mut TableDraft, &str) + 'static,
) -> AnyView {
    field_view(bound_signal(ui, initial, apply), width, placeholder, true)
}

/// The signal behind a bound field: seeded once, on build; the effect writes
/// back only on a genuine change. Seeding through the effect instead would set
/// the draft to the value it already holds — and `RwSignal::set` never dedups,
/// so every rebuild of the form would look like an edit and re-render the list.
fn bound_signal(
    ui: &Ui,
    initial: String,
    apply: impl Fn(&mut TableDraft, &str) + 'static,
) -> RwSignal<String> {
    let draft = ui.ddl.draft;
    let sig = floem::reactive::create_rw_signal(initial);
    create_effect(move |prev: Option<String>| {
        let v = sig.get();
        if prev.is_some_and(|p| p != v) {
            draft.update(|d| apply(d, &v));
        }
        v
    });
    sig
}

fn field_view(sig: RwSignal<String>, width: f64, placeholder: &'static str, mono: bool) -> AnyView {
    edit_field(
        sig,
        FieldCfg {
            placeholder,
            mono,
            ..Default::default()
        },
    )
    .style(move |s| s.width(width))
    .into_any()
}

/// The chevron that sits beside a free-text field and offers a menu of
/// suggestions, each writing itself into that field.
///
/// A **shortcut, not a picker** — which is why it isn't a [`owned_dropdown`]. A
/// type box has to stay free text, because the answer worth typing is often one
/// no fixed list holds (a length, a domain built on another domain, an array, an
/// extension's type), and a `Dropdown` parked beside a field reads as a second
/// control asking a second question. Shared so the two type fields in the app —
/// the designer's column type and the object editor's domain base type — are one
/// control rather than two that merely offer the same list.
// `use<>`: the view captures only the two `Copy` overlay signals, not the `&Ui`
// it read them off, so it can outlive the borrow and be returned from a form
// builder that took `ui` by reference.
pub(crate) fn suggest_chevron(
    ui: &Ui,
    sig: RwSignal<String>,
    options: Vec<String>,
) -> impl IntoView + use<> {
    let popup = ui.overlay.popup_menu;
    let anchor = ui.overlay.popup_anchor;
    container(icons::icon(icons::CHEVRON_DOWN, 16.0))
        .on_click_stop(move |_| {
            anchor.set(None);
            popup.set(Some(
                options
                    .iter()
                    .map(|o| {
                        let o = o.clone();
                        MenuEntry::action(o.clone(), move || sig.set(o.clone()))
                    })
                    .collect(),
            ));
        })
        .style(|s| {
            s.padding(6.0)
                .color(theme::text_dim())
                .hover(|s| s.color(theme::text()))
        })
}

/// [`bound_field`] plus a [`suggest_chevron`] writing into it.
fn bound_field_with_menu(
    ui: &Ui,
    initial: String,
    width: f64,
    placeholder: &'static str,
    mono: bool,
    options: Vec<String>,
    apply: impl Fn(&mut TableDraft, &str) + 'static,
) -> AnyView {
    let sig = bound_signal(ui, initial, apply);
    h_stack((
        field_view(sig, width, placeholder, mono),
        suggest_chevron(ui, sig, options),
    ))
    .style(|s| s.flex_row().items_center().gap(2.0))
    .into_any()
}

/// A `<select>`-style dropdown over owned values (the settings one needs `Copy`,
/// and a table name isn't). Same chrome, so it reads as the same control.
pub(crate) fn owned_dropdown(
    current: impl Fn() -> String + Copy + 'static,
    options: Vec<String>,
    width: f64,
    on_pick: impl Fn(String) + 'static,
) -> impl IntoView {
    use floem::views::dropdown::Dropdown;
    let main = move |cur: String| {
        h_stack((
            text(if cur.is_empty() {
                "—".to_string()
            } else {
                cur
            })
            .style(|s| s.color(theme::text()).font_size(theme::FONT_BODY)),
            empty().style(|s| s.flex_grow(1.0_f32)),
            icons::icon(icons::CHEVRON_DOWN, 16.0)
                .style(|s| s.color(theme::text_dim()).flex_shrink(0.0_f32)),
        ))
        .style(|s| s.items_center().width_full().gap(8.0))
        .into_any()
    };
    let row = move |item: String| {
        let this = item.clone();
        text(item)
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
    Dropdown::custom(current, main, options, row)
        .on_accept(move |v: String| on_pick(v))
        .style(move |s| dropdown_box_style(s).width(width))
}

/// A toggle row bound to the draft, same shape as the settings modals'.
fn bound_toggle(
    ui: &Ui,
    title: &'static str,
    hint: &'static str,
    initial: bool,
    apply: impl Fn(&mut TableDraft, bool) + 'static,
) -> AnyView {
    let draft = ui.ddl.draft;
    let sig = floem::reactive::create_rw_signal(initial);
    create_effect(move |prev: Option<bool>| {
        let v = sig.get();
        if prev.is_some_and(|p| p != v) {
            draft.update(|d| apply(d, v));
        }
        v
    });
    settings_toggle_row(title, hint, sig).into_any()
}

/// The +/−/↑/↓ bar under an item list.
pub(crate) fn list_actions(
    add: impl Fn() + 'static,
    remove: impl Fn() + 'static,
    move_up: Option<Rc<dyn Fn()>>,
    move_down: Option<Rc<dyn Fn()>>,
) -> impl IntoView {
    let btn = |glyph: &'static str, tip: &'static str, act: Rc<dyn Fn()>| {
        container(icons::icon(glyph, 15.0))
            .on_click_stop(move |_| (act)())
            // Colour is the whole affordance, as it is for every icon button in
            // the app (`toolbar_icon`, the modal ✕, the grid's toolbar). The
            // padding stays — it's the hitbox — but nothing paints behind it.
            .style(|s| {
                s.padding(5.0)
                    .color(theme::text_dim())
                    .hover(|s| s.color(theme::text()))
            })
            .tooltip(move || text(tip).style(crate::widgets::tooltip_style))
    };
    let arrows: Vec<AnyView> = match (move_up, move_down) {
        (Some(u), Some(d)) => vec![
            btn(icons::CHEVRON_UP, "Move up", u).into_any(),
            btn(icons::CHEVRON_DOWN, "Move down", d).into_any(),
        ],
        _ => Vec::new(),
    };
    h_stack((
        btn(icons::PLUS, "Add", Rc::new(add)),
        btn(icons::TRASH_2, "Remove", Rc::new(remove)),
        h_stack_from_iter(arrows).style(|s| s.flex_row().gap(2.0)),
    ))
    .style(|s| {
        s.flex_row()
            .items_center()
            .gap(2.0)
            .width_full()
            .padding(4.0)
            .border_top(1.0)
            .border_color(theme::border())
    })
}

/// One row of an item list.
pub(crate) fn list_row(
    ui: Ui,
    idx: usize,
    label: String,
    detail: String,
    // The same pair the popup menus use: glyph plus a colour *accessor*, so the
    // tint follows a live theme switch.
    icon: Option<crate::widgets::MenuIcon>,
) -> impl IntoView {
    list_row_inner(ui, idx, label, detail, icon, true)
}

/// [`list_row`] for a list where **nothing** carries an icon.
///
/// The icon slot is reserved even when a row has none, so a keyed and an unkeyed
/// column line up. A list where no row can ever have one — triggers — would just
/// be indented by a gutter that never fills, so it doesn't reserve it.
pub(crate) fn list_row_plain(ui: Ui, idx: usize, label: String, detail: String) -> impl IntoView {
    list_row_inner(ui, idx, label, detail, None, false)
}

fn list_row_inner(
    ui: Ui,
    idx: usize,
    label: String,
    detail: String,
    icon: Option<crate::widgets::MenuIcon>,
    reserve_icon: bool,
) -> impl IntoView {
    let selected = ui.ddl.selected;
    let mark: AnyView = match icon {
        Some((glyph, color)) => icons::icon(glyph, 13.0)
            .style(move |s| s.color(color()).flex_shrink(0.0_f32))
            .into_any(),
        None if reserve_icon => empty()
            .style(|s| s.width(13.0).flex_shrink(0.0_f32))
            .into_any(),
        None => empty()
            .style(|s| s.width(0.0).flex_shrink(0.0_f32))
            .into_any(),
    };
    h_stack((
        mark,
        text(label).style(|s| {
            s.font_size(theme::FONT_BODY)
                .color(theme::text())
                .text_ellipsis()
                .flex_shrink(1.0_f32)
                .min_width(0.0)
        }),
        empty().style(|s| s.flex_grow(1.0_f32).min_width(6.0)),
        // The type shrinks *first* and four times as fast as the name: which
        // column you're looking at is the thing you can't work out from the
        // other, and `timestamp without time zone` would otherwise squeeze the
        // name down to a couple of characters.
        text(detail).style(|s| {
            s.font_size(theme::FONT_LABEL)
                .color(theme::text_faint())
                .text_ellipsis()
                .flex_shrink(4.0_f32)
                .min_width(0.0)
        }),
    ))
    .on_click_stop(move |_| {
        if selected.get_untracked() != idx {
            selected.set(idx);
        }
    })
    .style(move |s| {
        let s = s
            .flex_row()
            .items_center()
            .gap(6.0)
            .width_full()
            .height(ROW_H)
            .padding_horiz(8.0)
            .flex_shrink(0.0_f32);
        if selected.get() == idx {
            s.background(theme::row_selected())
        } else {
            s.hover(|s| s.background(theme::row_hover()))
        }
    })
}

/// The list + its action bar, boxed like the import preview's table.
pub(crate) fn list_pane(
    rows: impl IntoView + 'static,
    actions: impl IntoView + 'static,
) -> impl IntoView {
    v_stack((
        autohide(scroll(rows)).style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0)),
        actions,
    ))
    .style(|s| {
        s.flex_col()
            .width(LIST_W)
            .flex_shrink(0.0_f32)
            .height_full()
            .border(1.0)
            .border_color(theme::border())
            .border_radius(6.0)
    })
}

/// The detail form beside a list: scrolls on its own, so a long column form
/// never pushes the footer off the panel.
///
/// The left inset is the gap to the list — so on the Table section, which has no
/// list, the form starts flush with the body padding like every other modal's.
///
/// **`width_full` on the scroll is one link of the chain that makes the form fill
/// the pane** — the others are the `container` here, the `dyn_container` the
/// caller passes in, and the form's own column. A percentage width resolves only
/// against a definite one, so a single link left at its content size collapses
/// every link below it back to content, and the switches end up wherever the
/// longest hint happens to end. Every other scrolled modal body in the app
/// (Settings, the object editor, Import) states a width here for the same reason.
fn detail_pane(tab: RwSignal<DesignerTab>, body: impl IntoView + 'static) -> impl IntoView {
    autohide(scroll(container(body).style(move |s| {
        s.width_full()
            .padding_left(if tab.get() == DesignerTab::Table {
                0.0
            } else {
                18.0
            })
            // Clear of the scrollbar, which floats over the content at the pane's
            // edge rather than insetting it. Now that the form really is as wide
            // as the pane, a toggle's switch is the rightmost thing in it and was
            // sitting under the bar.
            .padding_right(10.0)
    })))
    .style(|s| {
        s.width_full()
            .flex_grow(1.0_f32)
            .min_width(0.0)
            .height_full()
    })
}

fn hint(t: &'static str) -> impl IntoView {
    form_hint(t)
}

/// A field with a hint under it.
fn field_with_hint(field: impl IntoView + 'static, h: &'static str) -> impl IntoView {
    v_stack((field, hint(h))).style(|s| s.flex_col().gap(4.0))
}

// ── the Table section ────────────────────────────────────────────────────────

fn table_section(ui: Ui, target: &DesignerTarget) -> AnyView {
    let d = ui.ddl.draft;
    let mysql = target.dialect != SqlDialect::Postgres;
    let draft = d.get_untracked();
    let name = form_setting(
        "Name",
        bound_field(&ui, draft.name.clone(), FIELD_W, "table_name", |d, v| {
            d.name = v.trim().to_string()
        }),
    );
    let comment = form_setting(
        "Comment",
        bound_field(
            &ui,
            draft.comment.clone().unwrap_or_default(),
            FIELD_W * 1.6,
            "What this table is for",
            |d, v| d.comment = Some(v.to_string()).filter(|s| !s.is_empty()),
        ),
    );
    // Engine and collation exist on MySQL only; PostgreSQL has neither, so the
    // controls aren't shown rather than shown and ignored.
    let mysql_only = v_stack((
        form_setting(
            "Engine",
            bound_field_with_menu(
                &ui,
                draft.engine.clone().unwrap_or_default(),
                FIELD_W,
                "InnoDB",
                // A storage-engine name isn't SQL text the way a type is.
                false,
                ddl::MYSQL_ENGINES.iter().map(|e| e.to_string()).collect(),
                |d, v| d.engine = Some(v.trim().to_string()).filter(|s| !s.is_empty()),
            ),
        ),
        form_setting(
            "Collation",
            bound_field(
                &ui,
                draft.collation.clone().unwrap_or_default(),
                FIELD_W,
                "utf8mb4_general_ci",
                |d, v| d.collation = Some(v.trim().to_string()).filter(|s| !s.is_empty()),
            ),
        ),
    ))
    .style(move |s| {
        let s = s.flex_col().gap(FORM_GAP).width_full();
        if mysql { s } else { s.hide() }
    });

    // No section heading and no "In {database}" row: the tab strip above already
    // says which section this is, and the modal title carries where the table
    // lives — neither engine moves one between databases from here, so that row
    // was a caption repeating the title.
    v_stack((name, comment, mysql_only))
        .style(|s| s.flex_col().gap(FORM_GAP).width_full())
        .into_any()
}

// ── the Columns section ──────────────────────────────────────────────────────

fn columns_list(ui: Ui) -> AnyView {
    let d = ui.ddl;
    let draft = d.draft.get_untracked();
    let ui_rows = ui.clone();
    let rows = v_stack_from_iter(draft.columns.iter().enumerate().map(|(i, c)| {
        let is_pk = draft.is_in_primary_key(i);
        list_row(
            ui_rows.clone(),
            i,
            c.info.name.clone(),
            c.info.type_name.clone(),
            is_pk.then_some((icons::KEY_ROUND, theme::key_primary)),
        )
    }))
    .style(|s| s.flex_col().width_full());

    let add_ui = ui.clone();
    let del_ui = ui.clone();
    let up_ui = ui.clone();
    let down_ui = ui.clone();
    let mysql = d
        .designer
        .get_untracked()
        .is_some_and(|t| t.dialect != SqlDialect::Postgres);
    list_pane(
        rows,
        list_actions(
            move || {
                let ui = add_ui.clone();
                ui.ddl.draft.update(|d| {
                    d.columns.push(ColumnDraft::new(ColumnInfo {
                        name: unique_name(&d.column_names(), "column"),
                        type_name: default_type(&ui),
                        nullable: true,
                        ..Default::default()
                    }))
                });
                let n = ui.ddl.draft.with_untracked(|d| d.columns.len());
                ui.ddl.selected.set(n.saturating_sub(1));
                ui.ddl.rev.update(|r| *r += 1);
            },
            move || {
                let ui = del_ui.clone();
                let i = ui.ddl.selected.get_untracked();
                ui.ddl.draft.update(|d| d.remove_column(i));
                clamp_selection(&ui, |d| d.columns.len());
            },
            // Only MySQL can move a column; offering arrows on PostgreSQL would
            // promise an edit no statement can carry out.
            mysql.then(|| Rc::new(move || swap_selected(&up_ui, -1)) as Rc<dyn Fn()>),
            mysql.then(|| Rc::new(move || swap_selected(&down_ui, 1)) as Rc<dyn Fn()>),
        ),
    )
    .into_any()
}

fn column_form(ui: Ui, target: &DesignerTarget) -> AnyView {
    let d = ui.ddl;
    let i = d.selected.get_untracked();
    let draft = d.draft.get_untracked();
    let Some(c) = draft.columns.get(i).map(|c| c.info.clone()) else {
        return empty_hint("No column selected.").into_any();
    };
    let pg = target.dialect == SqlDialect::Postgres;
    let in_pk = draft.is_in_primary_key(i);

    let name = form_setting(
        "Name",
        bound_field(&ui, c.name.clone(), FIELD_W, "column_name", move |d, v| {
            d.rename_column(i, v.trim())
        }),
    );
    let ty = form_setting(
        "Type",
        field_with_hint(
            bound_field_with_menu(
                &ui,
                c.type_name.clone(),
                FIELD_W,
                "varchar(255)",
                true,
                ddl::common_types(target.dialect)
                    .iter()
                    .map(|t| t.to_string())
                    .collect(),
                move |d, v| {
                    if let Some(col) = d.columns.get_mut(i) {
                        col.info.type_name = v.trim().to_string();
                    }
                },
            ),
            "Written straight through — the server decides what it means.",
        ),
    );
    let nullable = bound_toggle(
        &ui,
        "Nullable",
        "Allow NULL in this column.",
        c.nullable,
        move |d, v| {
            if let Some(col) = d.columns.get_mut(i) {
                col.info.nullable = v;
            }
        },
    );
    let primary = bound_toggle(
        &ui,
        "Primary key",
        "Part of the table's primary key, appended in the order you add columns.",
        in_pk,
        move |d, v| d.set_in_primary_key(i, v),
    );
    let auto = bound_toggle(
        &ui,
        if pg { "Identity" } else { "Auto-increment" },
        if pg {
            "The server assigns the value (GENERATED BY DEFAULT AS IDENTITY)."
        } else {
            "The server assigns the value (AUTO_INCREMENT)."
        },
        c.auto_increment,
        move |d, v| {
            if let Some(col) = d.columns.get_mut(i) {
                col.info.auto_increment = v;
            }
        },
    );
    let default = form_setting(
        "Default",
        field_with_hint(
            sql_field(
                &ui,
                c.default.clone().unwrap_or_default(),
                FIELD_W,
                "",
                move |d, v| {
                    if let Some(col) = d.columns.get_mut(i) {
                        col.info.default = ddl::norm_default(Some(v));
                    }
                },
            ),
            "SQL text, as you'd write it: 'draft', 0, CURRENT_TIMESTAMP.",
        ),
    );
    let generated = form_setting(
        "Generated from",
        field_with_hint(
            sql_field(
                &ui,
                c.generated.clone().unwrap_or_default(),
                FIELD_W,
                "",
                move |d, v| {
                    if let Some(col) = d.columns.get_mut(i) {
                        col.info.generated = Some(v.trim().to_string()).filter(|s| !s.is_empty());
                    }
                },
            ),
            "An expression, e.g. qty * price. A generated column takes no default.",
        ),
    );
    let comment = form_setting(
        "Comment",
        bound_field(
            &ui,
            c.comment.clone().unwrap_or_default(),
            FIELD_W,
            "",
            move |d, v| {
                if let Some(col) = d.columns.get_mut(i) {
                    col.info.comment = Some(v.to_string()).filter(|s| !s.is_empty());
                }
            },
        ),
    );
    let collation = form_setting(
        "Collation",
        bound_field(
            &ui,
            c.collation.clone().unwrap_or_default(),
            FIELD_W,
            "",
            move |d, v| {
                if let Some(col) = d.columns.get_mut(i) {
                    col.info.collation = Some(v.trim().to_string()).filter(|s| !s.is_empty());
                }
            },
        ),
    );
    // `ON UPDATE CURRENT_TIMESTAMP` is MySQL's alone.
    let on_update = form_setting(
        "On update",
        bound_field(
            &ui,
            c.on_update.clone().unwrap_or_default(),
            FIELD_W,
            "CURRENT_TIMESTAMP",
            move |d, v| {
                if let Some(col) = d.columns.get_mut(i) {
                    col.info.on_update = Some(v.trim().to_string()).filter(|s| !s.is_empty());
                }
            },
        ),
    )
    .style(move |s| if pg { s.hide() } else { s });

    v_stack((
        name, ty, nullable, primary, auto, default, generated, collation, on_update, comment,
    ))
    .style(|s| s.flex_col().gap(FORM_GAP).width_full().padding_bottom(10.0))
    .into_any()
}

// ── the Indexes section ──────────────────────────────────────────────────────

fn indexes_list(ui: Ui) -> AnyView {
    let d = ui.ddl;
    let draft = d.draft.get_untracked();
    let ui_rows = ui.clone();
    let rows = v_stack_from_iter(draft.indexes.iter().enumerate().map(|(i, ix)| {
        list_row(
            ui_rows.clone(),
            i,
            ix.info.name.clone(),
            if ix.info.unique {
                "unique".to_string()
            } else {
                String::new()
            },
            Some((icons::KEY_SQUARE, theme::key_index)),
        )
    }))
    .style(|s| s.flex_col().width_full());

    let add_ui = ui.clone();
    let del_ui = ui.clone();
    list_pane(
        rows,
        list_actions(
            move || {
                let ui = add_ui.clone();
                ui.ddl.draft.update(|d| {
                    let names: Vec<String> =
                        d.indexes.iter().map(|i| i.info.name.clone()).collect();
                    // Seed on the selected column, which is nearly always the one
                    // the index is wanted for.
                    let first = d.columns.first().map(|c| c.info.name.clone());
                    d.indexes.push(IndexDraft::new(IndexInfo {
                        name: unique_name(&names, &format!("{}_idx", d.name)),
                        columns: first
                            .map(|c| vec![schemaic_core::schema::IndexColumn::plain(c)])
                            .unwrap_or_default(),
                        ..Default::default()
                    }));
                });
                let n = ui.ddl.draft.with_untracked(|d| d.indexes.len());
                ui.ddl.selected.set(n.saturating_sub(1));
                ui.ddl.rev.update(|r| *r += 1);
            },
            move || {
                let ui = del_ui.clone();
                let i = ui.ddl.selected.get_untracked();
                ui.ddl.draft.update(|d| {
                    if i < d.indexes.len() {
                        d.indexes.remove(i);
                    }
                });
                clamp_selection(&ui, |d| d.indexes.len());
            },
            None,
            None,
        ),
    )
    .into_any()
}

fn index_form(ui: Ui, target: &DesignerTarget) -> AnyView {
    let d = ui.ddl;
    let i = d.selected.get_untracked();
    let draft = d.draft.get_untracked();
    let Some(ix) = draft.indexes.get(i).map(|x| x.info.clone()) else {
        return empty_hint("No index selected. The primary key lives on the columns.").into_any();
    };
    let pg = target.dialect == SqlDialect::Postgres;
    let key_hint: &'static str = if pg {
        "Comma-separated, in key order. Add DESC for a descending column."
    } else {
        "Comma-separated, in key order. bio(20) is a prefix length; add DESC to sort down."
    };

    let pg_only = v_stack((
        form_setting(
            "Method",
            field_with_hint(
                bound_field(
                    &ui,
                    ix.method.clone().unwrap_or_default(),
                    180.0,
                    "btree",
                    move |d, v| {
                        if let Some(x) = d.indexes.get_mut(i) {
                            x.info.method = Some(v.trim().to_string()).filter(|s| !s.is_empty());
                        }
                    },
                ),
                "Leave empty for the default (btree).",
            ),
        ),
        form_setting(
            "Only rows where",
            field_with_hint(
                bound_field(
                    &ui,
                    ix.predicate.clone().unwrap_or_default(),
                    FIELD_W * 1.4,
                    "",
                    move |d, v| {
                        if let Some(x) = d.indexes.get_mut(i) {
                            x.info.predicate = Some(v.trim().to_string()).filter(|s| !s.is_empty());
                        }
                    },
                ),
                "A partial index's condition, without the WHERE.",
            ),
        ),
    ))
    .style(move |s| {
        let s = s.flex_col().gap(FORM_GAP).width_full();
        if pg { s } else { s.hide() }
    });

    v_stack((
        form_setting(
            "Name",
            bound_field(&ui, ix.name.clone(), FIELD_W, "index_name", move |d, v| {
                if let Some(x) = d.indexes.get_mut(i) {
                    x.info.name = v.trim().to_string();
                }
            }),
        ),
        form_setting(
            "Columns",
            field_with_hint(
                bound_field(
                    &ui,
                    key_list_text(&ix.columns),
                    FIELD_W * 1.4,
                    "id, name",
                    move |d, v| {
                        if let Some(x) = d.indexes.get_mut(i) {
                            x.info.columns = parse_key_list(v);
                        }
                    },
                ),
                key_hint,
            ),
        ),
        bound_toggle(
            &ui,
            "Unique",
            "Refuse duplicate values across these columns.",
            ix.unique,
            move |d, v| {
                if let Some(x) = d.indexes.get_mut(i) {
                    x.info.unique = v;
                }
            },
        ),
        pg_only,
    ))
    .style(|s| s.flex_col().gap(FORM_GAP).width_full().padding_bottom(10.0))
    .into_any()
}

// ── the Foreign keys section ─────────────────────────────────────────────────

fn fks_list(ui: Ui) -> AnyView {
    let d = ui.ddl;
    let draft = d.draft.get_untracked();
    let ui_rows = ui.clone();
    let rows = v_stack_from_iter(draft.foreign_keys.iter().enumerate().map(|(i, fk)| {
        list_row(
            ui_rows.clone(),
            i,
            fk.info.name.clone(),
            format!("→ {}", fk.info.ref_table),
            Some((icons::KEY_SQUARE, theme::key_foreign)),
        )
    }))
    .style(|s| s.flex_col().width_full());

    let add_ui = ui.clone();
    let del_ui = ui.clone();
    list_pane(
        rows,
        list_actions(
            move || {
                let ui = add_ui.clone();
                ui.ddl.draft.update(|d| {
                    let names: Vec<String> =
                        d.foreign_keys.iter().map(|f| f.info.name.clone()).collect();
                    let first = d.columns.first().map(|c| c.info.name.clone());
                    d.foreign_keys.push(ForeignKeyDraft::new(ForeignKeyInfo {
                        name: unique_name(&names, &format!("fk_{}", d.name)),
                        columns: first.into_iter().collect(),
                        ..Default::default()
                    }));
                });
                let n = ui.ddl.draft.with_untracked(|d| d.foreign_keys.len());
                ui.ddl.selected.set(n.saturating_sub(1));
                ui.ddl.rev.update(|r| *r += 1);
            },
            move || {
                let ui = del_ui.clone();
                let i = ui.ddl.selected.get_untracked();
                ui.ddl.draft.update(|d| {
                    if i < d.foreign_keys.len() {
                        d.foreign_keys.remove(i);
                    }
                });
                clamp_selection(&ui, |d| d.foreign_keys.len());
            },
            None,
            None,
        ),
    )
    .into_any()
}

/// How a referential action reads in its dropdown. `NO ACTION` is the default
/// both engines leave unwritten, so it's shown as itself rather than as blank.
fn action_label(a: Option<&str>) -> String {
    a.unwrap_or("NO ACTION").to_string()
}

fn fk_form(ui: Ui, target: &DesignerTarget) -> AnyView {
    let d = ui.ddl;
    let i = d.selected.get_untracked();
    let draft = d.draft.get_untracked();
    let Some(fk) = draft.foreign_keys.get(i).map(|f| f.info.clone()) else {
        return empty_hint("No foreign key selected.").into_any();
    };
    let draft_sig = d.draft;
    let actions: Vec<String> = ddl::FK_ACTIONS.iter().map(|a| action_label(*a)).collect();

    // The referenced table comes from the database's own table list — a name
    // typed by hand is the single most common way a new key fails.
    let tables = target.tables.clone();
    let ref_table = form_setting(
        "References table",
        owned_dropdown(
            move || {
                draft_sig.with(|d| {
                    d.foreign_keys
                        .get(i)
                        .map(|f| f.info.ref_table.clone())
                        .unwrap_or_default()
                })
            },
            tables,
            FIELD_W,
            move |v| {
                draft_sig.update(|d| {
                    if let Some(f) = d.foreign_keys.get_mut(i) {
                        f.info.ref_table = v.clone();
                    }
                })
            },
        ),
    );

    let on_delete = form_setting(
        "On delete",
        owned_dropdown(
            move || {
                draft_sig.with(|d| {
                    action_label(
                        d.foreign_keys
                            .get(i)
                            .and_then(|f| f.info.on_delete.as_deref()),
                    )
                })
            },
            actions.clone(),
            180.0,
            move |v| {
                draft_sig.update(|d| {
                    if let Some(f) = d.foreign_keys.get_mut(i) {
                        f.info.on_delete = Some(v.clone()).filter(|a| a != "NO ACTION");
                    }
                })
            },
        ),
    );
    let on_update = form_setting(
        "On update",
        owned_dropdown(
            move || {
                draft_sig.with(|d| {
                    action_label(
                        d.foreign_keys
                            .get(i)
                            .and_then(|f| f.info.on_update.as_deref()),
                    )
                })
            },
            actions,
            180.0,
            move |v| {
                draft_sig.update(|d| {
                    if let Some(f) = d.foreign_keys.get_mut(i) {
                        f.info.on_update = Some(v.clone()).filter(|a| a != "NO ACTION");
                    }
                })
            },
        ),
    );

    v_stack((
        form_setting(
            "Name",
            bound_field(&ui, fk.name.clone(), FIELD_W, "fk_name", move |d, v| {
                if let Some(f) = d.foreign_keys.get_mut(i) {
                    f.info.name = v.trim().to_string();
                }
            }),
        ),
        form_setting(
            "Columns",
            field_with_hint(
                bound_field(
                    &ui,
                    fk.columns.join(", "),
                    FIELD_W * 1.2,
                    "customer_id",
                    move |d, v| {
                        if let Some(f) = d.foreign_keys.get_mut(i) {
                            f.info.columns = parse_name_list(v);
                        }
                    },
                ),
                "Comma-separated, paired in order with the referenced columns.",
            ),
        ),
        ref_table,
        form_setting(
            "References columns",
            bound_field(
                &ui,
                fk.ref_columns.join(", "),
                FIELD_W * 1.2,
                "id",
                move |d, v| {
                    if let Some(f) = d.foreign_keys.get_mut(i) {
                        f.info.ref_columns = parse_name_list(v);
                    }
                },
            ),
        ),
        on_delete,
        on_update,
    ))
    .style(|s| s.flex_col().gap(FORM_GAP).width_full().padding_bottom(10.0))
    .into_any()
}

fn checks_list(ui: Ui) -> AnyView {
    let d = ui.ddl;
    let draft = d.draft.get_untracked();
    let ui_rows = ui.clone();
    let rows = v_stack_from_iter(draft.check_constraints.iter().enumerate().map(|(i, ck)| {
        list_row(
            ui_rows.clone(),
            i,
            ck.info.name.clone(),
            // The predicate *is* the constraint — a list of names alone would
            // say nothing about what any of them enforce.
            ck.info.expression.clone(),
            None,
        )
    }))
    .style(|s| s.flex_col().width_full());

    let add_ui = ui.clone();
    let del_ui = ui.clone();
    list_pane(
        rows,
        list_actions(
            move || {
                let ui = add_ui.clone();
                ui.ddl.draft.update(|d| {
                    let names: Vec<String> = d
                        .check_constraints
                        .iter()
                        .map(|c| c.info.name.clone())
                        .collect();
                    d.check_constraints.push(CheckDraft::new(CheckInfo {
                        name: unique_name(&names, &format!("{}_chk", d.name)),
                        ..Default::default()
                    }));
                });
                let n = ui.ddl.draft.with_untracked(|d| d.check_constraints.len());
                ui.ddl.selected.set(n.saturating_sub(1));
                ui.ddl.rev.update(|r| *r += 1);
            },
            move || {
                let ui = del_ui.clone();
                let i = ui.ddl.selected.get_untracked();
                ui.ddl.draft.update(|d| {
                    if i < d.check_constraints.len() {
                        d.check_constraints.remove(i);
                    }
                });
                clamp_selection(&ui, |d| d.check_constraints.len());
            },
            None,
            None,
        ),
    )
    .into_any()
}

fn check_form(ui: Ui, dialect: SqlDialect) -> AnyView {
    let d = ui.ddl;
    let i = d.selected.get_untracked();
    let draft = d.draft.get_untracked();
    let Some(ck) = draft.check_constraints.get(i).map(|c| c.info.clone()) else {
        return empty_hint("No check selected.").into_any();
    };

    let name = form_setting(
        "Name",
        bound_field(
            &ui,
            ck.name.clone(),
            FIELD_W,
            "qty_positive",
            move |d, v| {
                if let Some(c) = d.check_constraints.get_mut(i) {
                    c.info.name = v.trim().to_string();
                }
            },
        ),
    );
    let expression = form_setting(
        "Expression",
        field_with_hint(
            bound_field(
                &ui,
                ck.expression.clone(),
                FIELD_W * 1.6,
                "qty > 0",
                move |d, v| {
                    if let Some(c) = d.check_constraints.get_mut(i) {
                        // Stored bare, as the introspected form is — the emitter
                        // wraps it in `CHECK (…)` exactly once.
                        c.info.expression = v.trim().to_string();
                    }
                },
            ),
            "The condition every row must satisfy. Written without CHECK (…).",
        ),
    );

    // `NOT ENFORCED` is MySQL's alone; offering the switch on PostgreSQL would
    // promise something the emitter deliberately doesn't write.
    let enforced: AnyView = if dialect == SqlDialect::Postgres {
        empty().into_any()
    } else {
        bound_toggle(
            &ui,
            "Enforced",
            "Off records the constraint without applying it — existing and new rows are both accepted.",
            ck.enforced,
            move |d, v| {
                if let Some(c) = d.check_constraints.get_mut(i) {
                    c.info.enforced = v;
                }
            },
        )
    };

    v_stack((name, expression, enforced))
        .style(|s| s.flex_col().gap(FORM_GAP).width_full().padding_bottom(10.0))
        .into_any()
}

// ── list helpers ─────────────────────────────────────────────────────────────

/// What a list-plus-form pane shows while nothing is selected.
///
/// Top-left, **not** centred: it stands exactly where the form's first label
/// would, so selecting an item doesn't make the pane's content jump from the
/// middle of the empty space up to the top. The inset comes from the detail pane
/// itself, the same one every form in this modal gets — nothing here adds its own.
///
/// Shared with the trigger editor, which lays its two panes out the same way.
pub(crate) fn empty_hint(msg: &'static str) -> impl IntoView {
    text(msg).style(|s| {
        s.color(theme::text_faint())
            .font_size(theme::FONT_BODY)
            .width_full()
    })
}

/// A name that isn't taken yet: `column`, then `column_2`, `column_3`…
fn unique_name(taken: &[String], base: &str) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|c| !taken.iter().any(|t| t == c))
        .unwrap_or_else(|| base.to_string())
}

/// The type a new column starts as — a sane, obviously-editable default per
/// engine rather than an empty field that fails validation on sight.
fn default_type(ui: &Ui) -> String {
    let pg = ui
        .ddl
        .designer
        .get_untracked()
        .is_some_and(|t| t.dialect == SqlDialect::Postgres);
    if pg {
        "text".to_string()
    } else {
        "varchar(255)".to_string()
    }
}

/// Keep the selection on a real row after a removal.
fn clamp_selection(ui: &Ui, len: impl Fn(&TableDraft) -> usize) {
    let n = ui.ddl.draft.with_untracked(|d| len(d));
    let i = ui.ddl.selected.get_untracked();
    ui.ddl.selected.set(i.min(n.saturating_sub(1)));
    ui.ddl.rev.update(|r| *r += 1);
}

/// Move the selected item one place, taking the selection with it.
fn swap_selected(ui: &Ui, delta: isize) {
    let i = ui.ddl.selected.get_untracked();
    let j = i as isize + delta;
    let len = ui.ddl.draft.with_untracked(|d| d.columns.len()) as isize;
    if j < 0 || j >= len {
        return;
    }
    ui.ddl.draft.update(|d| d.columns.swap(i, j as usize));
    ui.ddl.selected.set(j as usize);
    ui.ddl.rev.update(|r| *r += 1);
}

// ── the modal ────────────────────────────────────────────────────────────────

/// The change set the draft currently describes — the same call the preview
/// emits from, so the footer's count can never disagree with the SQL.
fn change_set(target: &DesignerTarget, draft: &TableDraft) -> ddl::ChangeSet {
    match &target.current {
        Some(cur) => ddl::diff(cur, draft, target.dialect),
        None => ddl::create(draft, target.dialect),
    }
}

/// The table designer. Absolutely positioned over the workspace when
/// `ui.ddl.designer` is `Some`.
pub(crate) fn table_designer_overlay(ui: Ui) -> impl IntoView {
    let d = ui.ddl;
    let close = move || d.designer.set(None);

    dyn_container(
        // The preview stacks on top of the designer, which stays *open* behind
        // it (Cancel there returns here with the draft intact) but must render
        // nothing — an overlay that isn't absolutely positioned lands in the
        // workspace's flex row and paints itself into the layout. The draft lives
        // in a signal, not in this view, so unmounting costs nothing.
        move || (d.designer.get().is_some(), d.preview.get().is_some()),
        move |(open, previewing)| {
            if !open || previewing {
                return empty().into_any();
            }
            let Some(target) = d.designer.get_untracked() else {
                return empty().into_any();
            };
            let ui = ui.clone();
            let title = match &target.current {
                Some(t) => format!(
                    "Edit {}.{}",
                    object_location(&target.database, t.schema.as_deref()),
                    t.name
                ),
                None => format!(
                    "Create table in {}",
                    object_location(&target.database, target.schema.as_deref())
                ),
            };

            // The list re-renders on every draft change (it shows names and
            // types); the form does NOT, or typing into a field would rebuild
            // the field mid-keystroke. That's why the two are separate
            // containers with different keys.
            let list_ui = ui.clone();
            let list = dyn_container(
                move || (d.tab.get(), d.draft.get()),
                move |(tab, _)| match tab {
                    DesignerTab::Table => empty().into_any(),
                    DesignerTab::Columns => columns_list(list_ui.clone()),
                    DesignerTab::Indexes => indexes_list(list_ui.clone()),
                    DesignerTab::ForeignKeys => fks_list(list_ui.clone()),
                    DesignerTab::Checks => checks_list(list_ui.clone()),
                },
            )
            .style(move |s| {
                if d.tab.get() == DesignerTab::Table {
                    s.hide()
                } else {
                    s.height_full().flex_shrink(0.0_f32)
                }
            });

            let form_ui = ui.clone();
            let form_target = target.clone();
            let form = dyn_container(
                // `rev` is what makes a *structural* edit rebuild the form: after
                // removing item 2, index 2 is a different item but `selected` is
                // unchanged, so nothing else here would notice.
                move || (d.tab.get(), d.selected.get(), d.rev.get()),
                move |(tab, ..)| {
                    let ui = form_ui.clone();
                    match tab {
                        DesignerTab::Table => table_section(ui, &form_target),
                        DesignerTab::Columns => column_form(ui, &form_target),
                        DesignerTab::Indexes => index_form(ui, &form_target),
                        DesignerTab::ForeignKeys => fk_form(ui, &form_target),
                        DesignerTab::Checks => check_form(ui, form_target.dialect),
                    }
                },
            )
            // Without this the whole detail column is content-sized. A
            // `dyn_container` carries no style of its own, so it sizes to its
            // child; the form inside asks for `width_full`, which then resolves
            // against *the widest line of text in the form* rather than the pane
            // — so the switches sat wherever the longest hint happened to end,
            // and moved between sections as the wording changed. It has to be
            // stated on every link of the chain (scroll → container → this →
            // form) or the percentage has nothing definite to resolve against.
            .style(|s| s.width_full());

            let body = h_stack((list, detail_pane(d.tab, form))).style(|s| {
                s.flex_row()
                    .width_full()
                    .flex_grow(1.0_f32)
                    .min_height(0.0)
                    .padding_horiz(MODAL_PAD_H)
                    .padding_vert(18.0)
                    .gap(0.0)
            });

            // Footer status: validation first (it blocks), then the change count.
            let status_target = target.clone();
            let status = dyn_container(
                move || d.draft.get(),
                move |draft| {
                    let errs = draft.validate();
                    if let Some(first) = errs.first() {
                        return text(first.clone())
                            .style(|s| {
                                s.color(theme::error())
                                    .font_size(theme::FONT_LABEL)
                                    .max_width(460.0)
                            })
                            .into_any();
                    }
                    let n = change_set(&status_target, &draft).len();
                    text(match n {
                        0 => "No changes".to_string(),
                        1 => "1 change".to_string(),
                        n => format!("{n} changes"),
                    })
                    // Green once there's something to apply — the same signal the
                    // Preview button gives by lighting up, at the other end of
                    // the footer where the count lives.
                    .style(move |s| {
                        s.font_size(theme::FONT_LABEL).color(if n == 0 {
                            theme::text_faint()
                        } else {
                            theme::change_count()
                        })
                    })
                    .into_any()
                },
            );

            let preview_ui = ui.clone();
            let preview_target = target.clone();
            let actions = dyn_container(
                move || d.draft.get(),
                move |draft| {
                    let ui = preview_ui.clone();
                    let target = preview_target.clone();
                    let cs = change_set(&target, &draft);
                    let ready = draft.validate().is_empty() && !cs.is_empty();
                    h_stack((
                        action_button("Cancel", ActionKind::Neutral, true, close),
                        action_button("Preview SQL", ActionKind::Primary, ready, move || {
                            let cs = change_set(&target, &draft);
                            ddl_preview::open_preview(&ui, preview_from(&target, &draft, &cs));
                        }),
                    ))
                    .style(|s| s.flex_row().items_center().gap(ACTION_GAP))
                    .into_any()
                },
            );

            let close_x: Rc<dyn Fn()> = Rc::new(close);
            let panel = v_stack((
                modal_title_owned(title, close_x),
                tab_strip(ui.clone()),
                body,
                // The count sits at the far left, the actions at the far right —
                // it's what those actions are *about*, not a label on them.
                modal_footer_split(status.style(|s| s.min_width(0.0)), actions),
            ))
            .on_click_stop(|_| {})
            .style(|s| panel_style(s).width(PANEL_W).height(PANEL_H));

            focus_root(container(panel))
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| close())
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
        if d.designer.get().is_some() && d.preview.get().is_none() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// A new table has no server-side name to be titled after, so the preview is
/// headed by the one the draft is giving it.
fn preview_from(target: &DesignerTarget, draft: &TableDraft, cs: &ddl::ChangeSet) -> DdlPreview {
    let subject = match target.current {
        Some(_) => target.display(),
        None => draft.name.clone(),
    };
    ddl_preview::preview_of(
        target.conn_id,
        &target.database,
        subject,
        cs,
        target.read_only,
    )
}
