//! The table designer: edit a table's shape, see what that means, hand it to the
//! preview.
//!
//! One `TableDraft` is the whole state ([`crate::DdlUi::draft`]). Every control writes
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
use schemaic_core::schema::{CheckInfo, ColumnInfo, ForeignKeyInfo, IndexInfo, ServerFlavour};

use crate::settings::{dropdown_box_style, focusable_toggle_row};
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, MenuEntry, action_button, action_gap, autohide,
    focus_root_with_ring, form_gap, form_hint, form_setting, modal_footer_split, modal_h,
    modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use crate::{
    DdlPreview, DesignerTab, DesignerTarget, FieldCfg, PopupAnchor, Ui, ddl_preview, edit_field,
    icons, object_location, theme,
};

fn panel_w() -> f64 {
    modal_w(900.0)
}
/// The modal's height at 100%. The call site passes it through
/// [`crate::widgets::modal_h`], which scales it and then caps it against the
/// window — so this is a base, not a fixed height.
///
/// (It said the opposite for a while: "**not** scaled … doubling it would take
/// the panel off the display". That was true of the first attempt, and stopped
/// being true when the cap made growing them safe. A comment that contradicts its
/// own call site is worse than no comment: the next reader either adds a second
/// scaling or removes the one that is there.)
const PANEL_H: f64 = 550.0;
/// The item list's width, shared by all three sections so switching between them
/// doesn't shift the form. Wide enough that a long name and a long type
/// (`timestamp without time zone`) can both be read — the detail pane has the
/// slack to give.
fn list_w() -> f64 {
    theme::scaled(320.0)
}
/// One row of the item list.
fn row_h() -> f64 {
    theme::scaled(30.0)
}
/// Text-field width in the detail form, matching the connection form's fields.
fn field_w() -> f64 {
    theme::scaled(260.0)
}
/// The item list's place in the Tab order: ahead of the form it feeds, because
/// it sits to the left of it and choosing *what* to edit comes before editing it.
/// Shared by all four sections — only one list is mounted at a time — and by the
/// trigger editor, which wears the same list-plus-form layout.
pub(crate) const LIST_TAB: u32 = 5;

// ── opening ──────────────────────────────────────────────────────────────────

/// Open the designer on `target`, seeding the draft from the table it names (or
/// a blank one for a new table).
pub(crate) fn open_designer(ui: &Ui, target: DesignerTarget) {
    let d = ui.ddl;
    // A new editing session — see `DdlUi::session`.
    d.session.update(|g| *g += 1);
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
    // Each overlay knows only its own flag, so two open would paint two panels.
    // This used to clear the view editor **and nothing else**, which was true
    // when those were the only two; it is one list now
    // (`ddl_preview::close_peers`), because the set had grown to six.
    crate::ddl_preview::close_peers(d, false);
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
/// Which MySQL-family server `database` was introspected from, when its schema
/// has loaded.
///
/// Read off the schema rather than the connection because that is where
/// `SELECT VERSION()` was actually asked — see [`ServerFlavour`]. `Unknown`
/// until the schema loads, which is the honest answer and the one that makes a
/// per-flavour control hide rather than guess.
pub(crate) fn db_flavour(ui: &Ui, database: &str) -> ServerFlavour {
    ui.schema.db_nodes.with_untracked(|nodes| {
        nodes
            .iter()
            .find(|n| n.database == database)
            .and_then(|n| match n.schema.get_untracked() {
                schemaic_core::schema::SchemaState::Loaded(db) => Some(db.flavour),
                _ => None,
            })
            .unwrap_or_default()
    })
}

/// The introspected table a schema editor seeds its draft from — the **one**
/// funnel all four editor entry points go through (`open_for_table`,
/// `preview_draft_edit`, the trigger editor, the view editor).
///
/// `None` while a re-introspection of the database is in flight, and that is not
/// caution for its own sake. `begin_refresh` keeps a `Loaded` database loaded
/// across a refetch so the tree doesn't blank, which means `Loaded` no longer
/// means *current*: applying an `ALTER` starts a refresh and reports before it
/// lands, so within that window this used to hand back the **pre-apply**
/// `TableInfo`, `TableDraft::from_table` seeded from it, and one more edit to
/// the same column emitted a MySQL `MODIFY COLUMN` restating the old
/// definition — silently reverting the change just applied, with `risks()`
/// disclosing nothing, because from the plan's view the type did not change.
///
/// The caller's existing "no table, do nothing" arm is the right answer: this is
/// the behaviour every refresh had before the tree learned to keep its rows, and
/// the window is one round trip. Opening a designer on a model known to be stale
/// is the outcome worth refusing.
pub(crate) fn loaded_table(
    ui: &Ui,
    database: &str,
    schema: Option<&str>,
    table: &str,
) -> Option<schemaic_core::schema::TableInfo> {
    loaded_schema(ui, database).and_then(|db| {
        db.tables
            .iter()
            .find(|t| t.name == table && t.schema.as_deref() == schema)
            .cloned()
    })
}

/// The whole introspected database behind [`loaded_table`], on the same
/// refresh-in-flight rule.
///
/// For the one caller that has to resolve a name it was *given* rather than one
/// the user clicked: a proposal's table, which is resolved by
/// [`schemaic_core::propose::resolve_target`] so the card and the MCP tool land
/// on the same table.
pub(crate) fn loaded_schema(
    ui: &Ui,
    database: &str,
) -> Option<std::sync::Arc<schemaic_core::schema::DbSchema>> {
    ui.schema.db_nodes.with_untracked(|nodes| {
        nodes
            .iter()
            .find(|n| n.database == database)
            .filter(|n| !n.refreshing.get_untracked())
            .and_then(|n| match n.schema.get_untracked() {
                schemaic_core::schema::SchemaState::Loaded(db) => Some(db),
                _ => None,
            })
    })
}

/// The namespace a *new* object in `database` should land in: `public` on
/// PostgreSQL, `None` on MySQL (which has no level between database and table).
///
/// **Derived from the dialect, refined by the loaded schema.** It used to be
/// read off the loaded schema alone, on the grounds that this is the same
/// question the tree answers by *showing* namespace nodes — but that made it
/// answer `None` for a PostgreSQL database whose schema was still `Loading`, or
/// had `Failed`, which is permanent. The statement then went out unqualified:
/// for a role whose `search_path` is `"$user", public` and which owns a schema
/// of its own, `CREATE TABLE "orders" (…)` lands in `alice`, not the `public`
/// the tree row stands for. The sibling gate in the same menu already reads the
/// *dialect* to decide the entries exist at all, so it offered PostgreSQL-only
/// entries in exactly the state where it couldn't say which namespace they
/// targeted.
///
/// The loaded schema still refines it, and that is worth keeping: a PostgreSQL
/// database really does report its namespaces, and `schemas()` being empty is a
/// genuine "this database has no namespace level" rather than "not looked yet".
pub(crate) fn default_schema(ui: &Ui, database: &str) -> Option<String> {
    if edit_ctx(ui).dialect != schemaic_core::intel::SqlDialect::Postgres {
        return None;
    }
    let loaded_without_namespaces = ui.schema.db_nodes.with_untracked(|nodes| {
        nodes
            .iter()
            .find(|n| n.database == database)
            .is_some_and(|n| match n.schema.get_untracked() {
                schemaic_core::schema::SchemaState::Loaded(s) => s.schemas().is_empty(),
                _ => false,
            })
    });
    (!loaded_without_namespaces).then(|| schemaic_core::schema::PG_DEFAULT_SCHEMA.to_string())
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

/// What the designer should land on once it opens — the row the user
/// right-clicked in the schema tree, named rather than positioned.
///
/// A position wouldn't survive the trip: the tree's sequence and the designer's
/// are different sequences (see [`schemaic_core::ddl::TableDraft::find_key`]),
/// and the designer's own is the draft's, which doesn't exist until the modal
/// opens.
#[derive(Clone, Copy)]
pub(crate) enum DesignerFocus<'a> {
    /// Nothing in particular — the `Table` tab, which is where it opens anyway.
    Table,
    Column(&'a str),
    /// One of the table's keys, as the tree's key row carries it: the index the
    /// row stands for, plus the foreign-key constraint it backs when it backs
    /// one. Which section that lands in is the draft's answer, not the caller's.
    Key {
        index: &'a schemaic_core::schema::IndexInfo,
        foreign_key: Option<&'a str>,
    },
}

/// Open the designer on an existing table, landing on whatever `focus` names —
/// the schema tree's column and key right-clicks put you on the row you clicked
/// rather than on the table summary.
///
/// A focus that resolves to nothing (a name the draft doesn't hold) is dropped
/// silently: the designer opens on the `Table` tab, which is what it did before
/// it was asked at all. The modal still opens — the request that failed is the
/// landing, not the edit.
pub(crate) fn open_for_table(
    ui: &Ui,
    database: &str,
    schema: Option<&str>,
    table: &str,
    focus: DesignerFocus<'_>,
) {
    let Some(info) = loaded_table(ui, database, schema, table) else {
        return;
    };
    // **A view is not a table, and this is the designer for a table.** Every
    // menu that reaches here already asks (`overlays::object_entries`,
    // `field_entries`, `key_entries`); this is the second lock, because the
    // failure it prevents is a full Columns / Indexes / Foreign keys form for an
    // object whose every edit the server refuses — and on SQLite one whose
    // rebuild would emit `DROP TABLE` on a view.
    if info.is_view {
        return;
    }
    let ctx = edit_ctx(ui);
    // Resolved against the introspected table, before it moves into the target.
    // The key case is resolved *after* the open instead, against the draft the
    // open seeds — that is the sequence the Indexes and Foreign keys lists show.
    let column_at = match focus {
        DesignerFocus::Column(c) => info.columns.iter().position(|x| x.name == c),
        _ => None,
    };
    open_designer(
        ui,
        DesignerTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            flavour: db_flavour(ui, database),
            schema: info.schema.clone(),
            dialect: ctx.dialect,
            current: Some(info),
            tables: table_names(ui, database, schema),
            read_only: ctx.read_only,
        },
    );
    let landing = match focus {
        DesignerFocus::Table => None,
        DesignerFocus::Column(_) => column_at.map(|i| (DesignerTab::Columns, i)),
        DesignerFocus::Key { index, foreign_key } => ui
            .ddl
            .draft
            .with_untracked(|d| d.find_key(index, foreign_key))
            .map(|k| match k {
                schemaic_core::ddl::DraftKey::Index(i) => (DesignerTab::Indexes, i),
                schemaic_core::ddl::DraftKey::ForeignKey(i) => (DesignerTab::ForeignKeys, i),
                // The primary key is a tick on a column, not a row of its own.
                schemaic_core::ddl::DraftKey::PrimaryKeyColumn(i) => (DesignerTab::Columns, i),
            }),
    };
    if let Some((tab, i)) = landing {
        ui.ddl.tab.set(tab);
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
    let cs = ddl::diff(
        &info,
        &draft,
        ddl::Target::new(ctx.dialect, db_flavour(ui, database)),
    );
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
            flavour: db_flavour(ui, database),
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
///
/// **One Tab stop for the strip, Left/Right between sections** — the same rule
/// the item list beside it follows. Five separate stops would make Tab-ing into
/// the form, which is where you are going, cost five presses; and leaving the
/// strip out entirely (what it did until now) meant a keyboard user could only
/// ever edit the section the designer happened to open on, since `open_designer`
/// always lands on Table.
fn tab_strip(ui: Ui, ring: FocusRing) -> impl IntoView {
    let d = ui.ddl;
    let strip = h_stack_from_iter(DesignerTab::ALL.into_iter().map(move |t| {
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
                    .font_size(theme::font_body())
                    .padding_horiz(theme::scaled(12.0))
                    .padding_vert(theme::scaled(7.0))
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
            .gap(theme::scaled(4.0))
            .width_full()
            .padding_horiz(modal_pad_h())
            .padding_vert(theme::scaled(8.0))
            .border_bottom(1.0)
            .border_color(theme::border())
    });
    // Clamped, not wrapping: this is a selection, and the ring around it is what
    // wraps. Switching section resets the item selection exactly as a click does
    // — the two paths must not disagree about what "a new section" means.
    crate::widgets::nav_group(
        strip,
        ring,
        crate::widgets::NAV_TAB,
        crate::widgets::NavAxis::Horizontal,
        move |delta| {
            let cur = DesignerTab::ALL
                .iter()
                .position(|t| *t == d.tab.get_untracked())
                .unwrap_or(0);
            if let Some(next) = crate::widgets::list_step(DesignerTab::ALL.len(), cur, delta) {
                d.tab.set(DesignerTab::ALL[next]);
                d.selected.set(0);
                d.rev.update(|r| *r += 1);
            }
        },
    )
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
    ring: FocusRing,
    tabindex: u32,
    apply: impl Fn(&mut TableDraft, &str) + 'static,
) -> AnyView {
    field_view(
        bound_signal(ui, initial, apply),
        width,
        placeholder,
        false,
        ring,
        tabindex,
    )
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
    ring: FocusRing,
    tabindex: u32,
    apply: impl Fn(&mut TableDraft, &str) + 'static,
) -> AnyView {
    field_view(
        bound_signal(ui, initial, apply),
        width,
        placeholder,
        true,
        ring,
        tabindex,
    )
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

fn field_view(
    sig: RwSignal<String>,
    width: f64,
    placeholder: &'static str,
    mono: bool,
    ring: FocusRing,
    tabindex: u32,
) -> AnyView {
    edit_field(
        sig,
        FieldCfg {
            placeholder,
            mono,
            focus: Some((ring, tabindex)),
            ..Default::default()
        },
    )
    .style(move |s| s.width(width))
    .into_any()
}

/// The chevron that sits beside a free-text field and offers a menu of
/// suggestions, each writing itself into that field.
///
/// A **shortcut, not a picker** — which is why it isn't a [`focusable_owned_dropdown`]. A
/// type box has to stay free text, because the answer worth typing is often one
/// no fixed list holds (a length, a domain built on another domain, an array, an
/// extension's type), and a `Dropdown` parked beside a field reads as a second
/// control asking a second question. Shared so the two type fields in the app —
/// the designer's column type and the object editor's domain base type — are one
/// control rather than two that merely offer the same list.
/// In the ring one index above its field, so Tab walks the box you type in and
/// then the list of things you could have typed. It was the one clickable
/// control in either modal Tab couldn't reach, which left the curated type list
/// needing a pointer.
// `use<>`: the view captures only the two `Copy` overlay signals, not the `&Ui`
// it read them off, so it can outlive the borrow and be returned from a form
// builder that took `ui` by reference.
pub(crate) fn suggest_chevron(
    ui: &Ui,
    sig: RwSignal<String>,
    options: Vec<String>,
    ring: FocusRing,
    tabindex: u32,
) -> impl IntoView + use<> {
    let popup = ui.overlay.popup_menu;
    let anchor = ui.overlay.popup_anchor;
    // **The chevron's own id, so the menu can open under the chevron.** The
    // shared `popup_menu` channel falls back to `last_mouse` when no anchor is
    // set, which is right for a right-click and wrong for a button: reached by
    // Tab and pressed with Enter, the pointer is wherever it was left, so the
    // menu opened across the modal — or across the window — from the control
    // that raised it. Filled just below, once the view it names exists.
    let anchor_id: RwSignal<Option<floem::ViewId>> = RwSignal::new(None);
    // Stated once, because the value that *places* the menu is the value that says
    // the open menu is this chevron's — see [`crate::widgets::menu_anchored_at`].
    //
    // `ViewId::layout_rect` is already in window coordinates (floem sets it from
    // `window_origin` during layout), which is the frame `PopupAnchor` is stated
    // in. `None` only before the first layout, and then the cursor fallback is as
    // good an answer as any.
    let anchor_now = move || {
        anchor_id
            .get_untracked()
            .map(|id| id.layout_rect())
            .map(|r| PopupAnchor::BelowIcon(r.x0, r.x1, r.y1))
    };
    let open = Rc::new(move || {
        let here = anchor_now();
        // A second press closes the menu the first opened. Recomputed rather than
        // remembered, so it is the *current* rect that has to match: these fields
        // sit in a scrolling modal body, and scrolling with the menu up moves the
        // chevron out from under it. Then this reports "not mine" and the press
        // reopens at the new position — which is the better answer anyway, and the
        // reason the fallback direction matters more than the exact equality.
        if here.is_some_and(|mine| {
            crate::widgets::menu_anchored_at(
                popup.get_untracked().is_some(),
                anchor.get_untracked(),
                mine,
            )
        }) {
            popup.set(None);
            return;
        }
        anchor.set(here);
        popup.set(Some(
            options
                .iter()
                .map(|o| {
                    let o = o.clone();
                    MenuEntry::action(o.clone(), move || sig.set(o.clone()))
                })
                .collect(),
        ));
    });
    let pressed = open.clone();
    let button = crate::widgets::in_ring_button(
        container(icons::icon(icons::CHEVRON_DOWN, 16.0))
            .on_click_stop(move |_| (open)())
            // Without this the workspace root's "close on down" handler fires
            // first and the click then reopens — down closes, up reopens, and the
            // chevron never toggles however it decides. The toggle above is the
            // second half of the same fix, not an alternative to it: the guard
            // alone would leave a second press re-opening what was never closed.
            .on_event_stop(
                floem::event::EventListener::PointerDown,
                crate::widgets::menu_trigger_press,
            )
            .style(|s| {
                s.padding(theme::scaled(6.0))
                    .color(theme::text_dim())
                    .hover(|s| s.color(theme::text()))
            }),
        ring,
        tabindex,
        true,
        0.0, // an icon face, square
        move || (pressed)(),
    );
    // The ring wrapper, not the face: it is the outermost view of this control,
    // so its rect is the whole chevron's.
    anchor_id.set(Some(button.id()));
    button
}

/// [`bound_field`] plus a [`suggest_chevron`] writing into it.
#[allow(clippy::too_many_arguments)] // a UI builder; grouping into a struct adds no clarity
fn bound_field_with_menu(
    ui: &Ui,
    initial: String,
    width: f64,
    placeholder: &'static str,
    mono: bool,
    options: Vec<String>,
    ring: FocusRing,
    tabindex: u32,
    apply: impl Fn(&mut TableDraft, &str) + 'static,
) -> AnyView {
    let sig = bound_signal(ui, initial, apply);
    h_stack((
        field_view(sig, width, placeholder, mono, ring.clone(), tabindex),
        suggest_chevron(ui, sig, options, ring, tabindex + 1),
    ))
    .style(|s| s.flex_row().items_center().gap(theme::scaled(2.0)))
    .into_any()
}

/// A `<select>`-style dropdown over owned values (the settings one needs `Copy`,
/// and a table name isn't), in a modal's Tab order. Same chrome as the settings
/// picker, so it reads as the same control.
///
/// The keyboard handling — and the four floem behaviours it works around — is
/// [`crate::settings::in_ring_dropdown`]'s, shared with that picker so the two
/// can't drift. There is deliberately no un-focusable variant: every one of
/// these is in a modal that has a ring.
pub(crate) fn focusable_owned_dropdown(
    current: impl Fn() -> String + Copy + 'static,
    options: Vec<String>,
    width: f64,
    ring: crate::widgets::FocusRing,
    tabindex: u32,
    on_pick: impl Fn(String) + 'static,
) -> impl IntoView {
    crate::settings::in_ring_dropdown(
        owned_dropdown_box(current, options, width),
        ring,
        tabindex,
        on_pick,
    )
}

/// The dropdown itself, without an accept action — the shared half, since
/// `on_accept` is a single slot and the ring has to own it.
fn owned_dropdown_box(
    current: impl Fn() -> String + Copy + 'static,
    options: Vec<String>,
    width: f64,
) -> floem::views::dropdown::Dropdown<String> {
    use floem::views::dropdown::Dropdown;
    let main = move |cur: String| {
        h_stack((
            text(if cur.is_empty() {
                "—".to_string()
            } else {
                cur
            })
            .style(|s| s.color(theme::text()).font_size(theme::font_body())),
            empty().style(|s| s.flex_grow(1.0_f32)),
            icons::icon(icons::CHEVRON_DOWN, 16.0)
                .style(|s| s.color(theme::text_dim()).flex_shrink(0.0_f32)),
        ))
        .style(|s| s.items_center().width_full().gap(theme::scaled(8.0)))
        .into_any()
    };
    let row = move |item: String| {
        let this = item.clone();
        text(item)
            .style(move |s| {
                let s = s
                    .width_full()
                    .padding_horiz(theme::scaled(12.0))
                    .padding_vert(theme::scaled(6.0))
                    .color(theme::text())
                    .font_size(theme::font_body())
                    .hover(|s| s.background(theme::dropdown_hover()));
                if current() == this {
                    s.background(theme::dropdown_active())
                } else {
                    s
                }
            })
            .into_any()
    };
    Dropdown::custom(current, main, options, row).style(move |s| dropdown_box_style(s).width(width))
}

/// A toggle row bound to the draft, same shape as the settings modals'.
fn bound_toggle(
    ui: &Ui,
    title: &'static str,
    hint: &'static str,
    initial: bool,
    ring: FocusRing,
    tabindex: u32,
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
    focusable_toggle_row(title, hint, sig, ring, tabindex).into_any()
}

/// The +/−/↑/↓ bar under an item list.
///
/// Four stops from `LIST_TAB + 1`, immediately after the list they act on and
/// still ahead of the form at 10. Four rather than one, unlike the list itself:
/// these are four different verbs, not four of the same thing, so there is
/// nothing for an arrow key to mean between them.
pub(crate) fn list_actions(
    add: impl Fn() + 'static,
    remove: impl Fn() + 'static,
    move_up: Option<Rc<dyn Fn()>>,
    move_down: Option<Rc<dyn Fn()>>,
    ring: FocusRing,
) -> impl IntoView {
    let btn = move |glyph: &'static str, tip: &'static str, slot: u32, act: Rc<dyn Fn()>| {
        let pressed = act.clone();
        // Everything the pointer touches — the padding that *is* the hitbox, the
        // click listener, the hover colour — stays on the face, and only the
        // face goes into `in_ring_button`. Styling the *returned* view instead
        // moved the hitbox onto the ring wrapper while the click listener stayed
        // inside it, and registered a view that already had one, which is the
        // double-fire `in_ring_button` documents.
        let face = container(icons::icon(glyph, 15.0))
            .on_click_stop(move |_| (act)())
            // Colour is the whole affordance, as it is for every icon button in
            // the app (`toolbar_icon`, the modal ✕, the grid's toolbar).
            .style(|s| {
                s.padding(theme::scaled(5.0))
                    .color(theme::text_dim())
                    .hover(|s| s.color(theme::text()))
            })
            .tooltip(move || text(tip).style(crate::widgets::tooltip_style));
        // 0.0: an icon face, square.
        crate::widgets::in_ring_button(
            face,
            ring.clone(),
            LIST_TAB + 1 + slot,
            true,
            0.0,
            move || (pressed)(),
        )
    };
    let arrows: Vec<AnyView> = match (move_up, move_down) {
        (Some(u), Some(d)) => vec![
            btn(icons::CHEVRON_UP, "Move up", 2, u).into_any(),
            btn(icons::CHEVRON_DOWN, "Move down", 3, d).into_any(),
        ],
        _ => Vec::new(),
    };
    h_stack((
        btn(icons::PLUS, "Add", 0, Rc::new(add)),
        btn(icons::TRASH_2, "Remove", 1, Rc::new(remove)),
        h_stack_from_iter(arrows).style(|s| s.flex_row().gap(theme::scaled(2.0))),
    ))
    .style(|s| {
        s.flex_row()
            .items_center()
            .gap(theme::scaled(2.0))
            .width_full()
            .padding(theme::scaled(4.0))
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
            .style(|s| s.width(theme::scaled(13.0)).flex_shrink(0.0_f32))
            .into_any(),
        None => empty()
            .style(|s| s.width(0.0).flex_shrink(0.0_f32))
            .into_any(),
    };
    h_stack((
        mark,
        text(label).style(|s| {
            s.font_size(theme::font_body())
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
            s.font_size(theme::font_label())
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
            .gap(theme::scaled(6.0))
            .width_full()
            .height(row_h())
            .padding_horiz(theme::scaled(8.0))
            .flex_shrink(0.0_f32);
        if selected.get() == idx {
            s.background(theme::row_selected())
        } else {
            s.hover(|s| s.background(theme::row_hover()))
        }
    })
}

/// The list + its action bar, boxed like the import preview's table.
///
/// **One Tab stop for the whole list, then Up/Down inside it.** A list is not N
/// controls — it is one control that answers "which item is the form editing?",
/// so giving every row its own stop would make Tab take as many presses as the
/// table has columns to cross a pane the user was only passing through.
///
/// Up/Down **clamp** rather than wrap, unlike the Tab ring above: wrapping there
/// is what stops Tab escaping the modal, while a selection that jumps from the
/// last column to the first is just a surprise. `+`/`−`/`↑`/`↓` stay on the
/// action bar and take four stops of their own after this one — four different
/// verbs, so unlike the list there is nothing for an arrow to mean between them.
///
/// The selected row is kept on screen with `ensure_visible`, which scrolls only
/// when the row isn't already showing: rows are a fixed [`row_h()`] tall, so the
/// rect is arithmetic rather than a measured view.
pub(crate) fn list_pane(
    rows: impl IntoView + 'static,
    actions: impl IntoView + 'static,
    selected: RwSignal<usize>,
    len: impl Fn() -> usize + 'static,
    ring: FocusRing,
    tabindex: u32,
) -> impl IntoView {
    let step = move |delta: isize| {
        if let Some(next) = crate::widgets::list_step(len(), selected.get_untracked(), delta) {
            selected.set(next);
        }
    };

    let pane = v_stack((
        autohide(scroll(rows))
            .ensure_visible(move || {
                let top = selected.get() as f64 * row_h();
                floem::kurbo::Rect::new(0.0, top, 1.0, top + row_h())
            })
            .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0)),
        actions,
    ))
    .style(|s| {
        s.flex_col()
            .width(list_w())
            .flex_shrink(0.0_f32)
            .height_full()
            .border(1.0)
            .border_color(theme::border())
            .border_radius(6.0)
            // The box already has a border, so focus only recolours it — nothing
            // moves. Floem's own focus ring is suppressed for the same reason the
            // dropdowns suppress theirs.
            .focus(|s| s.border_color(theme::field_border_active()))
            .focus_visible(|s| s.outline(0.0))
    });

    crate::widgets::in_focus_ring(pane, ring, tabindex).on_event(
        floem::event::EventListener::KeyDown,
        move |e| {
            let floem::event::Event::KeyDown(ke) = e else {
                return floem::event::EventPropagation::Continue;
            };
            match ke.key.logical_key {
                Key::Named(NamedKey::ArrowDown) => {
                    step(1);
                    floem::event::EventPropagation::Stop
                }
                Key::Named(NamedKey::ArrowUp) => {
                    step(-1);
                    floem::event::EventPropagation::Stop
                }
                _ => floem::event::EventPropagation::Continue,
            }
        },
    )
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
            .padding_right(theme::scaled(10.0))
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
    v_stack((field, hint(h))).style(|s| s.flex_col().gap(theme::scaled(4.0)))
}

// ── the Table section ────────────────────────────────────────────────────────

fn table_section(ui: Ui, target: &DesignerTarget, ring: FocusRing) -> AnyView {
    let d = ui.ddl.draft;
    // **The engine, not "not PostgreSQL".** A storage engine and a table
    // collation are MySQL's, and asking the question the other way put SQLite —
    // which has neither, and no comments either — on the side that gets both.
    let mysql = target.dialect == SqlDialect::MySql;
    let has_comments = target.dialect != SqlDialect::Sqlite;
    let draft = d.get_untracked();
    let name = form_setting(
        "Name",
        bound_field(
            &ui,
            draft.name.clone(),
            field_w(),
            "table_name",
            ring.clone(),
            10,
            |d, v| d.name = v.trim().to_string(),
        ),
    );
    // SQLite has no comment on anything — not a table, not a column — so the
    // control isn't built rather than built and ignored, for the same reason
    // the engine and collation fields aren't.
    let comment: AnyView = if has_comments {
        form_setting(
            "Comment",
            bound_field(
                &ui,
                draft.comment.clone().unwrap_or_default(),
                field_w() * 1.6,
                "What this table is for",
                ring.clone(),
                20,
                |d, v| d.comment = Some(v.to_string()).filter(|s| !s.is_empty()),
            ),
        )
        .into_any()
    } else {
        crate::widgets::nothing()
    };
    // Engine and collation exist on MySQL only; PostgreSQL has neither, so the
    // controls aren't built rather than built and ignored. Built, not hidden: a
    // `hide()`n view is still in the tree, so its fields would still be in the
    // modal's Tab order and Tab would land on something nobody can see.
    let mysql_only: AnyView = if mysql {
        v_stack((
            form_setting(
                "Engine",
                bound_field_with_menu(
                    &ui,
                    draft.engine.clone().unwrap_or_default(),
                    field_w(),
                    "InnoDB",
                    // A storage-engine name isn't SQL text the way a type is.
                    false,
                    ddl::MYSQL_ENGINES.iter().map(|e| e.to_string()).collect(),
                    ring.clone(),
                    30,
                    |d, v| d.engine = Some(v.trim().to_string()).filter(|s| !s.is_empty()),
                ),
            ),
            form_setting(
                "Collation",
                bound_field(
                    &ui,
                    draft.collation.clone().unwrap_or_default(),
                    field_w(),
                    "utf8mb4_general_ci",
                    ring,
                    40,
                    |d, v| d.collation = Some(v.trim().to_string()).filter(|s| !s.is_empty()),
                ),
            ),
        ))
        .style(|s| s.flex_col().gap(form_gap()).width_full())
        .into_any()
    } else {
        crate::widgets::nothing()
    };

    // No section heading and no "In {database}" row: the tab strip above already
    // says which section this is, and the modal title carries where the table
    // lives — neither engine moves one between databases from here, so that row
    // was a caption repeating the title.
    v_stack((name, comment, mysql_only))
        .style(|s| s.flex_col().gap(form_gap()).width_full())
        .into_any()
}

// ── the Columns section ──────────────────────────────────────────────────────

fn columns_list(ui: Ui, ring: FocusRing) -> AnyView {
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
    let can_reorder = d
        .designer
        .get_untracked()
        .is_some_and(|t| schemaic_core::ddl::supports_column_reorder(t.dialect));
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
            // Which engines can place a column, and why each can or can't, is
            // `ddl::supports_column_reorder` — the same predicate `ddl::diff`
            // asks before it raises the move, so the arrows and the plan cannot
            // drift apart. It was a `!= SqlDialect::Postgres` at both sites.
            can_reorder.then(|| Rc::new(move || swap_selected(&up_ui, -1)) as Rc<dyn Fn()>),
            can_reorder.then(|| Rc::new(move || swap_selected(&down_ui, 1)) as Rc<dyn Fn()>),
            ring.clone(),
        ),
        d.selected,
        move || d.draft.with_untracked(|dr| dr.columns.len()),
        ring,
        LIST_TAB,
    )
    .into_any()
}

fn column_form(ui: Ui, target: &DesignerTarget, ring: FocusRing) -> AnyView {
    let d = ui.ddl;
    let i = d.selected.get_untracked();
    let draft = d.draft.get_untracked();
    let Some(c) = draft.columns.get(i).map(|c| c.info.clone()) else {
        return empty_hint("No column selected.").into_any();
    };
    let pg = target.dialect == SqlDialect::Postgres;
    // Asked as capabilities, because SQLite is neither of the other two here: it
    // *has* a column collation (`COLLATE NOCASE`), has no comment on anything,
    // and has no `ON UPDATE` — that one is MySQL's timestamp attribute, and a
    // field for it would take input the emitter then drops on the floor.
    let has_comments = target.dialect != SqlDialect::Sqlite;
    let has_on_update = target.dialect == SqlDialect::MySql;
    let in_pk = draft.is_in_primary_key(i);

    let name = form_setting(
        "Name",
        bound_field(
            &ui,
            c.name.clone(),
            field_w(),
            "column_name",
            ring.clone(),
            10,
            move |d, v| d.rename_column(i, v.trim()),
        ),
    );
    let ty = form_setting(
        "Type",
        field_with_hint(
            bound_field_with_menu(
                &ui,
                c.type_name.clone(),
                field_w(),
                "varchar(255)",
                true,
                ddl::common_types(target.dialect)
                    .iter()
                    .map(|t| t.to_string())
                    .collect(),
                ring.clone(),
                20,
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
        ring.clone(),
        30,
        move |d, v| {
            if let Some(col) = d.columns.get_mut(i) {
                col.info.nullable = v;
            }
        },
    );
    let primary = bound_toggle(
        &ui,
        "Primary key",
        // The order is the **column** order, not the order these were switched
        // on — that is what `2279fcb` fixed, and this hint still promised the
        // old rule. On InnoDB the primary key is the clustered index, so its
        // order decides the physical layout, and nothing else on screen
        // contradicts a wrong sentence before Preview SQL.
        "Part of the table's primary key, in column order.",
        in_pk,
        ring.clone(),
        40,
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
        ring.clone(),
        50,
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
                field_w(),
                "",
                ring.clone(),
                60,
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
                field_w(),
                "",
                ring.clone(),
                70,
                move |d, v| {
                    if let Some(col) = d.columns.get_mut(i) {
                        col.info.generated = Some(v.trim().to_string()).filter(|s| !s.is_empty());
                    }
                },
            ),
            "An expression, e.g. qty * price. A generated column takes no default.",
        ),
    );
    let comment: AnyView = if has_comments {
        form_setting(
            "Comment",
            bound_field(
                &ui,
                c.comment.clone().unwrap_or_default(),
                field_w(),
                "",
                ring.clone(),
                100,
                move |d, v| {
                    if let Some(col) = d.columns.get_mut(i) {
                        col.info.comment = Some(v.to_string()).filter(|s| !s.is_empty());
                    }
                },
            ),
        )
        .into_any()
    } else {
        crate::widgets::nothing()
    };
    let collation = form_setting(
        "Collation",
        bound_field(
            &ui,
            c.collation.clone().unwrap_or_default(),
            field_w(),
            "",
            ring.clone(),
            80,
            move |d, v| {
                if let Some(col) = d.columns.get_mut(i) {
                    col.info.collation = Some(v.trim().to_string()).filter(|s| !s.is_empty());
                }
            },
        ),
    );
    // `ON UPDATE CURRENT_TIMESTAMP` is MySQL's alone. Built only there rather
    // than built and hidden: a `hide()`n field is still in the Tab order.
    let on_update: AnyView = if !has_on_update {
        crate::widgets::nothing()
    } else {
        form_setting(
            "On update",
            bound_field(
                &ui,
                c.on_update.clone().unwrap_or_default(),
                field_w(),
                "CURRENT_TIMESTAMP",
                ring,
                90,
                move |d, v| {
                    if let Some(col) = d.columns.get_mut(i) {
                        col.info.on_update = Some(v.trim().to_string()).filter(|s| !s.is_empty());
                    }
                },
            ),
        )
        .into_any()
    };

    v_stack((
        name, ty, nullable, primary, auto, default, generated, collation, on_update, comment,
    ))
    .style(|s| {
        s.flex_col()
            .gap(form_gap())
            .width_full()
            .padding_bottom(theme::scaled(10.0))
    })
    .into_any()
}

// ── the Indexes section ──────────────────────────────────────────────────────

fn indexes_list(ui: Ui, ring: FocusRing) -> AnyView {
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
            ring.clone(),
        ),
        d.selected,
        move || d.draft.with_untracked(|dr| dr.indexes.len()),
        ring,
        LIST_TAB,
    )
    .into_any()
}

fn index_form(ui: Ui, target: &DesignerTarget, ring: FocusRing) -> AnyView {
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

    // PostgreSQL-only, and built only there: a `hide()`n field is still in the
    // modal's Tab order.
    let pg_only: AnyView = if pg {
        v_stack((
            form_setting(
                "Method",
                field_with_hint(
                    bound_field(
                        &ui,
                        ix.method.clone().unwrap_or_default(),
                        180.0,
                        "btree",
                        ring.clone(),
                        40,
                        move |d, v| {
                            if let Some(x) = d.indexes.get_mut(i) {
                                x.info.method =
                                    Some(v.trim().to_string()).filter(|s| !s.is_empty());
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
                        field_w() * 1.4,
                        "",
                        ring.clone(),
                        50,
                        move |d, v| {
                            if let Some(x) = d.indexes.get_mut(i) {
                                x.info.predicate =
                                    Some(v.trim().to_string()).filter(|s| !s.is_empty());
                            }
                        },
                    ),
                    "A partial index's condition, without the WHERE.",
                ),
            ),
        ))
        .style(|s| s.flex_col().gap(form_gap()).width_full())
        .into_any()
    } else {
        crate::widgets::nothing()
    };

    v_stack((
        form_setting(
            "Name",
            bound_field(
                &ui,
                ix.name.clone(),
                field_w(),
                "index_name",
                ring.clone(),
                10,
                move |d, v| {
                    if let Some(x) = d.indexes.get_mut(i) {
                        x.info.name = v.trim().to_string();
                    }
                },
            ),
        ),
        form_setting(
            "Columns",
            field_with_hint(
                bound_field(
                    &ui,
                    key_list_text(&ix.columns),
                    field_w() * 1.4,
                    "id, name",
                    ring.clone(),
                    20,
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
            ring,
            30,
            move |d, v| {
                if let Some(x) = d.indexes.get_mut(i) {
                    x.info.unique = v;
                }
            },
        ),
        pg_only,
    ))
    .style(|s| {
        s.flex_col()
            .gap(form_gap())
            .width_full()
            .padding_bottom(theme::scaled(10.0))
    })
    .into_any()
}

// ── the Foreign keys section ─────────────────────────────────────────────────

fn fks_list(ui: Ui, ring: FocusRing) -> AnyView {
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
            ring.clone(),
        ),
        d.selected,
        move || d.draft.with_untracked(|dr| dr.foreign_keys.len()),
        ring,
        LIST_TAB,
    )
    .into_any()
}

/// How a referential action reads in its dropdown. `NO ACTION` is the default
/// both engines leave unwritten, so it's shown as itself rather than as blank.
fn action_label(a: Option<&str>) -> String {
    a.unwrap_or("NO ACTION").to_string()
}

fn fk_form(ui: Ui, target: &DesignerTarget, ring: FocusRing) -> AnyView {
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
        focusable_owned_dropdown(
            move || {
                draft_sig.with(|d| {
                    d.foreign_keys
                        .get(i)
                        .map(|f| f.info.ref_table.clone())
                        .unwrap_or_default()
                })
            },
            tables,
            field_w(),
            ring.clone(),
            30,
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
        focusable_owned_dropdown(
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
            ring.clone(),
            50,
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
        focusable_owned_dropdown(
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
            ring.clone(),
            60,
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
            bound_field(
                &ui,
                fk.name.clone(),
                field_w(),
                "fk_name",
                ring.clone(),
                10,
                move |d, v| {
                    if let Some(f) = d.foreign_keys.get_mut(i) {
                        f.info.name = v.trim().to_string();
                    }
                },
            ),
        ),
        form_setting(
            "Columns",
            field_with_hint(
                bound_field(
                    &ui,
                    fk.columns.join(", "),
                    field_w() * 1.2,
                    "customer_id",
                    ring.clone(),
                    20,
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
                field_w() * 1.2,
                "id",
                ring,
                40,
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
    .style(|s| {
        s.flex_col()
            .gap(form_gap())
            .width_full()
            .padding_bottom(theme::scaled(10.0))
    })
    .into_any()
}

fn checks_list(ui: Ui, ring: FocusRing) -> AnyView {
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
            ring.clone(),
        ),
        d.selected,
        move || d.draft.with_untracked(|dr| dr.check_constraints.len()),
        ring,
        LIST_TAB,
    )
    .into_any()
}

fn check_form(
    ui: Ui,
    dialect: SqlDialect,
    target_flavour: ServerFlavour,
    ring: FocusRing,
) -> AnyView {
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
            field_w(),
            "qty_positive",
            ring.clone(),
            10,
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
                field_w() * 1.6,
                "qty > 0",
                ring.clone(),
                20,
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

    // `NOT ENFORCED` is **MySQL's** alone — not the MySQL *dialect's*.
    // PostgreSQL has no such clause, and neither does MariaDB: unticking this
    // there emitted `ERROR 1064 … near 'NOT ENFORCED'`, measured live. The
    // introspection side was already right (the MariaDB query hardcodes `YES`),
    // so only this control could create one, and only to fail.
    //
    // `Unknown` hides it too: the form withholds a feature rather than offering
    // one the server may reject, which is the call the trigger editor's
    // per-engine form already makes.
    let enforced: AnyView = if dialect == SqlDialect::Postgres
        || target_flavour != ServerFlavour::MySql
    {
        crate::widgets::nothing()
    } else {
        bound_toggle(
            &ui,
            "Enforced",
            "Off records the constraint without applying it — existing and new rows are both accepted.",
            ck.enforced,
            ring,
            30,
            move |d, v| {
                if let Some(c) = d.check_constraints.get_mut(i) {
                    c.info.enforced = v;
                }
            },
        )
    };

    v_stack((name, expression, enforced))
        .style(|s| {
            s.flex_col()
                .gap(form_gap())
                .width_full()
                .padding_bottom(theme::scaled(10.0))
        })
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
            .font_size(theme::font_body())
            .width_full()
    })
}

/// A name that isn't taken yet: `column`, then `column_2`, `column_3`…
///
/// **Case-insensitively taken**, which is the same fold
/// [`TableDraft::validate`](schemaic_core::ddl::TableDraft::validate) applies
/// when it reports "Two columns are both called …". The two rules have to agree:
/// an exact match let `+` generate `Name` beside an existing `name`, and the
/// footer then blanked the change count and disabled Preview SQL over a row the
/// generator had just created.
fn unique_name(taken: &[String], base: &str) -> String {
    let free = |c: &str| !taken.iter().any(|t| t.eq_ignore_ascii_case(c));
    if free(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|c| free(c))
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

/// Where "move the item at `i` by `delta`" lands, or `None` when it can't.
///
/// **Both** indices are checked, not only the destination: `i` comes from
/// `ui.ddl.selected`, which every writer today keeps inside the column list, but
/// nothing states that invariant and an out-of-range one would panic inside
/// `Vec::swap` rather than declining to move.
fn swap_target(len: usize, i: usize, delta: isize) -> Option<usize> {
    let j = i.checked_add_signed(delta)?;
    (i < len && j < len).then_some(j)
}

/// Move the selected item one place, taking the selection with it.
fn swap_selected(ui: &Ui, delta: isize) {
    let i = ui.ddl.selected.get_untracked();
    let len = ui.ddl.draft.with_untracked(|d| d.columns.len());
    let Some(j) = swap_target(len, i, delta) else {
        return;
    };
    ui.ddl.draft.update(|d| d.columns.swap(i, j));
    ui.ddl.selected.set(j);
    ui.ddl.rev.update(|r| *r += 1);
}

// ── the modal ────────────────────────────────────────────────────────────────

/// The change set the draft currently describes — the same call the preview
/// emits from, so the footer's count can never disagree with the SQL.
fn change_set(target: &DesignerTarget, draft: &TableDraft) -> ddl::ChangeSet {
    match &target.current {
        // The flavour goes with it: the emitter's MariaDB-specific risk can
        // only be stated by a plan that knows which server it is for.
        Some(cur) => ddl::diff(cur, draft, ddl::Target::new(target.dialect, target.flavour)),
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
            // One ring for the modal, not one per form: the form rebuilds on
            // every tab/selection change, and `in_focus_ring` unregisters on
            // unmount, so the rebuilt controls simply re-register here. A ring
            // created inside the form would be a different one each time and the
            // root — built once, out here — could never reach the current one.
            //
            // The list pane is in it too, as a *single* stop at `LIST_TAB`:
            // Up/Down move the selection once the keyboard is there.
            let ring = FocusRing::new();
            let root_ring = ring.clone();

            let list_ui = ui.clone();
            let list_ring = ring.clone();
            let list = dyn_container(
                move || (d.tab.get(), d.draft.get()),
                move |(tab, _)| {
                    let (ui, ring) = (list_ui.clone(), list_ring.clone());
                    match tab {
                        DesignerTab::Table => empty().into_any(),
                        DesignerTab::Columns => columns_list(ui, ring),
                        DesignerTab::Indexes => indexes_list(ui, ring),
                        DesignerTab::ForeignKeys => fks_list(ui, ring),
                        DesignerTab::Checks => checks_list(ui, ring),
                    }
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
            let form_ring = ring.clone();
            let form = dyn_container(
                // `rev` is what makes a *structural* edit rebuild the form: after
                // removing item 2, index 2 is a different item but `selected` is
                // unchanged, so nothing else here would notice.
                move || (d.tab.get(), d.selected.get(), d.rev.get()),
                move |(tab, ..)| {
                    let ui = form_ui.clone();
                    let ring = form_ring.clone();
                    match tab {
                        DesignerTab::Table => table_section(ui, &form_target, ring),
                        DesignerTab::Columns => column_form(ui, &form_target, ring),
                        DesignerTab::Indexes => index_form(ui, &form_target, ring),
                        DesignerTab::ForeignKeys => fk_form(ui, &form_target, ring),
                        DesignerTab::Checks => {
                            check_form(ui, form_target.dialect, form_target.flavour, ring)
                        }
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
                    .padding_horiz(modal_pad_h())
                    .padding_vert(theme::scaled(18.0))
                    .gap(0.0)
            });

            // Footer status: validation first (it blocks), then the change count.
            let status_target = target.clone();
            let status = dyn_container(
                move || d.draft.get(),
                move |draft| {
                    let errs = draft.validate(status_target.dialect);
                    if let Some(first) = errs.first() {
                        return text(first.clone())
                            .style(|s| {
                                s.color(theme::error())
                                    .font_size(theme::font_label())
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
                        s.font_size(theme::font_label()).color(if n == 0 {
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
            let ring_actions = ring.clone();
            let actions = dyn_container(
                move || d.draft.get(),
                move |draft| {
                    let ui = preview_ui.clone();
                    let target = preview_target.clone();
                    let ring = ring_actions.clone();
                    let cs = change_set(&target, &draft);
                    let ready = draft.validate(target.dialect).is_empty() && !cs.is_empty();
                    h_stack((
                        action_button(
                            "Cancel",
                            ActionKind::Neutral,
                            true,
                            ring.clone(),
                            ACTION_TAB,
                            close,
                        ),
                        action_button(
                            "Preview SQL",
                            ActionKind::Primary,
                            ready,
                            ring,
                            ACTION_TAB + 10,
                            move || {
                                let cs = change_set(&target, &draft);
                                ddl_preview::open_preview(&ui, preview_from(&target, &draft, &cs));
                            },
                        ),
                    ))
                    .style(|s| s.flex_row().items_center().gap(action_gap()))
                    .into_any()
                },
            );

            let close_x: Rc<dyn Fn()> = Rc::new(close);
            let panel = v_stack((
                modal_title_owned(title, close_x, root_ring.clone()),
                tab_strip(ui.clone(), ring.clone()),
                body,
                // The count sits at the far left, the actions at the far right —
                // it's what those actions are *about*, not a label on them.
                modal_footer_split(status.style(|s| s.min_width(0.0)), actions),
            ))
            .on_click_stop(|_| {})
            .style(|s| panel_style(s).width(panel_w()).height(modal_h(PANEL_H)));

            focus_root_with_ring(container(panel), root_ring)
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

#[cfg(test)]
mod tests {
    use super::{swap_target, unique_name};

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn an_untaken_name_is_used_as_is() {
        assert_eq!(unique_name(&names(&["id"]), "column"), "column");
        assert_eq!(unique_name(&[], "column"), "column");
    }

    #[test]
    fn a_taken_name_counts_up_from_two() {
        assert_eq!(unique_name(&names(&["column"]), "column"), "column_2");
        assert_eq!(
            unique_name(&names(&["column", "column_2"]), "column"),
            "column_3"
        );
        // The gap is filled rather than skipped past.
        assert_eq!(
            unique_name(&names(&["column", "column_3"]), "column"),
            "column_2"
        );
    }

    /// The generator and `TableDraft::validate` have to fold case the same way,
    /// or `+` creates a row the validator immediately refuses — blanking the
    /// change count and disabling Preview SQL over its own output.
    #[test]
    fn taken_is_case_insensitive_like_the_validator() {
        assert_eq!(unique_name(&names(&["Column"]), "column"), "column_2");
        assert_eq!(unique_name(&names(&["COLUMN_2"]), "column"), "column");
        assert_eq!(
            unique_name(&names(&["column", "Column_2"]), "column"),
            "column_3"
        );
    }

    #[test]
    fn a_column_moves_one_place_in_either_direction() {
        assert_eq!(swap_target(3, 1, 1), Some(2));
        assert_eq!(swap_target(3, 1, -1), Some(0));
    }

    #[test]
    fn the_ends_of_the_list_have_nowhere_to_move_to() {
        assert_eq!(swap_target(3, 2, 1), None);
        assert_eq!(swap_target(3, 0, -1), None);
        assert_eq!(swap_target(0, 0, 1), None);
    }

    /// The selection index is checked too, not just the destination: a stale one
    /// would panic inside `Vec::swap` instead of declining to move.
    #[test]
    fn a_selection_past_the_end_declines_rather_than_panicking() {
        assert_eq!(swap_target(3, 7, -1), None);
        assert_eq!(swap_target(3, 7, 1), None);
    }
}
