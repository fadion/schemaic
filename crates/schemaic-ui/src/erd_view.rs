//! The read-only ER-diagram modal (roadmap #9).
//!
//! Opened from the schema tree's "Show diagram" (a database → whole-DB view, a
//! table → its FK neighbourhood), it renders `schemaic_core::erd`'s graph +
//! deterministic layout as a canvas of table cards joined by crow's-foot FK
//! edges. All the *what to draw* logic (graph, layout, density, edge geometry)
//! is pure + unit-tested in the core; this module is the Floem rendering.
//!
//! Edges are drawn by a custom paint view ([`EdgeCanvas`]) that strokes the
//! bezier curves + crow's-foot markers directly and repaints on change via
//! `request_paint`. (A Floem `svg` view — whether rebuilt via `dyn_container` or
//! updated in place via `update_value` — does NOT repaint reliably on reactive
//! change here and blanked the edges on every drag/hover, even though the SVG
//! itself parsed fine; the custom view gives full repaint control and per-edge
//! colour.) Edge hover is a Rust-side proximity hit-test on the canvas's
//! `PointerMove` — see [`erd::nearest_polyline`]. Node positions are reactive:
//! a card can be dragged
//! (edges re-route live) and the arrangement persists to `diagrams.json` per
//! `(connection, database)`; the toolbar's "Reset layout" restores the auto-layout;
//! double-clicking a table opens/reveals it; the "+N more" / "show less" row
//! toggles a card's collapse (resizing it and re-routing edges).
//!
//! Zoom is **semantic**, not a paint-time transform: cards + edges multiply their
//! own positions/sizes/fonts by the zoom factor `z` (positions/sizes stay logical,
//! `×z` only at render), so text re-lays-out crisply at every level.
//!
//! The canvas is an **infinite free-pan surface**, not a scroll view: a screen
//! position is `pan + logical·z`, `pan` unbounded. Drag empty space — or middle-drag
//! anywhere (pointer captured so it continues over cards) — to pan; **Ctrl+wheel**
//! zooms about the cursor (keeping the point under it fixed); **Shift+wheel** /
//! plain wheel pan horizontally / vertically; the toolbar `−`/`+` / **Fit** zoom
//! about the viewport centre. No scrollbars; the layer just clips to the modal body.
//! Cards can sit at any (incl. negative) logical position. The edge hit-test / card
//! drag map the cursor back to logical space (`(p − pan) / z`).

use std::collections::HashMap;
use std::rc::Rc;

use floem::AnyView;
use floem::context::PaintCx;
use floem::event::{Event, EventListener, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::{BezPath, Line, Point, Stroke};
use floem::prelude::*;
use floem::reactive::create_effect;
use floem::views::{container, empty, v_stack_from_iter};
use floem::{View, ViewId};
use floem_renderer::Renderer;

use schemaic_core::erd::{
    self, Cardinality, DiagramColumn, DiagramGraph, DiagramNode, NodeKind, Pt, Rect,
};
use schemaic_core::schema::{DbSchema, SchemaState, classify_column_type};

use crate::schema_tree::column_type_icon;
use crate::widgets::{
    centered_msg, measure_text_px_at, measure_text_px_bold_at, modal_title_borderless, panel_style,
    window_size,
};
use crate::{ConnNode, Ui, icons, theme};

/// The key role tint for a column, matching the schema panel / Find-Anywhere:
/// primary-key columns gold, foreign-key columns purple, others normal text.
fn col_tint(pk: bool, fk: bool) -> floem::peniko::Color {
    if pk {
        theme::key_primary()
    } else if fk {
        theme::key_foreign()
    } else {
        theme::text()
    }
}

/// Card width is sized to its widest row (measured), clamped to this range; names
/// longer than the max truncate with an ellipsis.
const NODE_MIN_W: f64 = 180.0;
const NODE_MAX_W: f64 = 340.0;
const HEADER_H: f64 = 30.0;
const ROW_H: f64 = 24.0;
const EXPANDER_H: f64 = 22.0;
const CANVAS_PAD: f64 = 28.0;
const COLLAPSED_COLS: usize = 5;
/// Cursor-to-edge proximity (px) that counts as hovering an edge.
const EDGE_HOVER_PX: f64 = 7.0;
/// Straight run (px) an edge keeps off each card edge before the curve bends, so
/// the crow's-foot / bar / optionality-circle markers sit on a straight, symmetric
/// segment. Long enough to clear the parent end's bar (10) + optionality circle (~20).
const EDGE_STUB: f64 = 20.0;
const ZOOM_MIN: f64 = 0.25;
const ZOOM_MAX: f64 = 3.0;

/// Resolve the active connection's loaded schema for `database`, if introspected.
fn resolve_schema(db_nodes: RwSignal<Vec<ConnNode>>, database: &str) -> Option<DbSchema> {
    db_nodes.get_untracked().iter().find_map(|n| {
        if n.database == database {
            match n.schema.get_untracked() {
                SchemaState::Loaded(s) => Some(s),
                _ => None,
            }
        } else {
            None
        }
    })
}

/// A node placed on the canvas: its rect. Which columns show is computed
/// reactively from the `collapsed` signal (so the expand/collapse toggle works).
struct Placed {
    node: DiagramNode,
    x: f64,
    y: f64,
    w: f64,
    /// Initial height (default collapse state); the card resizes on toggle.
    h: f64,
}

/// One FK edge's drawn geometry: the crow's-foot / bar marker line segments and
/// the flattened curve polyline (also the hover hit-test path). We stroke the
/// *flattened* curve, not a `CubicBez`, because Floem's vger renderer `todo!()`s
/// on cubic path segments — it only strokes lines and quadratics.
struct EdgeShapes {
    markers: Vec<Line>,
    poly: Vec<Pt>,
}

/// Card width sized to its widest content row over *all* columns (so width stays
/// stable across expand/collapse), clamped to `[NODE_MIN_W, NODE_MAX_W]`.
fn node_width(node: &DiagramNode) -> f64 {
    // Header: table icon (13) + gap (7) + bold name (13px). Measured BOLD, as it's
    // drawn — measuring at regular weight under-reports, which sized every card
    // narrower than its own title and ellipsized names nowhere near `NODE_MAX_W`.
    let mut content = 13.0 + 7.0 + measure_text_px_bold_at(&node.id, 13.0);
    for c in &node.columns {
        // column icon (13) + gap (8) + name + gap (8) + type.
        let row = 13.0
            + 8.0
            + measure_text_px_at(&c.name, 11.5)
            + 8.0
            + measure_text_px_at(&c.type_name, 10.5);
        content = content.max(row);
    }
    // + horizontal padding (20) + a little slack for the measurer's font drift.
    (content + 20.0 + 6.0).clamp(NODE_MIN_W, NODE_MAX_W)
}

/// A table card's rendering metrics for a given collapse state: which columns
/// show, whether the node can collapse at all (has hidden columns when
/// collapsed), and its total height.
fn card_metrics(node: &DiagramNode, collapsed: bool) -> (Vec<usize>, bool, f64) {
    let collapsed_v = erd::collapsed_visible(&node.columns, COLLAPSED_COLS);
    let collapsible = collapsed_v.len() < node.columns.len();
    let visible: Vec<usize> = if collapsed && collapsible {
        collapsed_v
    } else {
        (0..node.columns.len()).collect()
    };
    // A collapsible node always reserves the toggle row ("+N more" / "show less").
    let toggle = if collapsible { EXPANDER_H } else { 0.0 };
    let h = HEADER_H + visible.len() as f64 * ROW_H + toggle;
    (visible, collapsible, h)
}

/// Size each node (default-collapse height + measured width), lay them out
/// deterministically, and offset by the canvas padding. Returns the placed nodes,
/// the content size, and each real node's default collapse state.
fn build_placed(graph: &DiagramGraph) -> (Vec<Placed>, f64, f64, HashMap<String, bool>) {
    let total = graph.nodes.len();
    let opts = erd::DensityOpts::default();
    let mut sizes: HashMap<String, (f64, f64)> = HashMap::new();
    let mut collapsed: HashMap<String, bool> = HashMap::new();
    for n in &graph.nodes {
        let (w, h) = if n.kind == NodeKind::Stub {
            (
                (measure_text_px_at(&n.id, 13.0) + 20.0).clamp(NODE_MIN_W, NODE_MAX_W),
                HEADER_H,
            )
        } else {
            let default_collapsed = erd::should_collapse(n.columns.len(), total, opts);
            collapsed.insert(n.id.clone(), default_collapsed);
            let (_visible, _collapsible, h) = card_metrics(n, default_collapsed);
            (node_width(n), h)
        };
        sizes.insert(n.id.clone(), (w, h));
    }

    let cells = erd::layout(graph);
    let positions = erd::place(&cells, &sizes, erd::LayoutOpts::default());
    let pos: HashMap<&str, &erd::NodePos> = positions.iter().map(|p| (p.id.as_str(), p)).collect();

    let (mut cw, mut ch) = (0.0_f64, 0.0_f64);
    let mut placed = Vec::with_capacity(graph.nodes.len());
    for n in &graph.nodes {
        let p = pos[n.id.as_str()];
        let (w, h) = sizes[&n.id];
        let (x, y) = (p.x + CANVAS_PAD, p.y + CANVAS_PAD);
        cw = cw.max(x + w);
        ch = ch.max(y + h);
        placed.push(Placed {
            node: n.clone(),
            x,
            y,
            w,
            h,
        });
    }
    (placed, cw + CANVAS_PAD, ch + CANVAS_PAD, collapsed)
}

/// Crow's-foot / bar / optionality-circle marker line segments for an edge
/// child(`p0`) → parent(`p1`). `out0`/`out1` are each anchor's outward horizontal
/// direction (from [`erd::edge_dirs`]); the markers are laid along those straight
/// stubs so they stay perpendicular/symmetric regardless of the cards' vertical
/// offset. When `optional` (a nullable FK — the child may have no parent), a small
/// "zero" circle is drawn just outside the parent bar (crow's-foot optionality).
fn marker_lines(
    p0: Pt,
    p1: Pt,
    out0: f64,
    out1: f64,
    card: Cardinality,
    optional: bool,
) -> Vec<Line> {
    let mut v = Vec::new();
    // "One" bar at the parent end, 10px out along its stub.
    let bx = p1.x + out1 * 10.0;
    v.push(Line::new(
        Point::new(bx, p1.y - 5.0),
        Point::new(bx, p1.y + 5.0),
    ));
    // Optionality "zero" circle just outside the bar (farther from the parent),
    // approximated by short line segments (vger strokes lines, not arcs).
    if optional {
        let (cx, cy, r) = (p1.x + out1 * 16.5, p1.y, 3.2);
        let n = 20;
        let mut prev = Point::new(cx + r, cy);
        for k in 1..=n {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            let cur = Point::new(cx + r * a.cos(), cy + r * a.sin());
            v.push(Line::new(prev, cur));
            prev = cur;
        }
    }
    match card {
        Cardinality::OneToMany => {
            // Crow's-foot fanning from a foot 12px out into the child anchor.
            let fx = p0.x + out0 * 12.0;
            let foot = Point::new(fx, p0.y);
            v.push(Line::new(foot, Point::new(p0.x, p0.y - 6.0)));
            v.push(Line::new(foot, Point::new(p0.x, p0.y)));
            v.push(Line::new(foot, Point::new(p0.x, p0.y + 6.0)));
        }
        Cardinality::OneToOne => {
            // A matching bar at the child end, 10px out along its stub.
            let cx = p0.x + out0 * 10.0;
            v.push(Line::new(
                Point::new(cx, p0.y - 5.0),
                Point::new(cx, p0.y + 5.0),
            ));
        }
    }
    v
}

/// Each node's rect from its current (possibly dragged) position + static size.
fn rects(
    positions: &HashMap<String, (f64, f64)>,
    sizes: &HashMap<String, (f64, f64)>,
) -> HashMap<String, Rect> {
    sizes
        .iter()
        .map(|(id, &(w, h))| {
            let (x, y) = positions.get(id).copied().unwrap_or((0.0, 0.0));
            (id.clone(), Rect { x, y, w, h })
        })
        .collect()
}

/// Each real node's currently-visible column indices, given the reactive collapse
/// state — the input to column-precise edge anchoring (which FK/PK row an edge end
/// attaches to). Stubs (no columns) are omitted.
fn visible_map(
    graph: &DiagramGraph,
    collapsed: &HashMap<String, bool>,
) -> HashMap<String, Vec<usize>> {
    graph
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::Stub)
        .map(|n| {
            let is_collapsed = collapsed.get(&n.id).copied().unwrap_or(false);
            let (visible, _collapsible, _h) = card_metrics(n, is_collapsed);
            (n.id.clone(), visible)
        })
        .collect()
}

/// Build every edge's drawable shapes from the current node rects. `visible` maps a
/// node id to its shown column indices, so each edge end anchors on the exact FK
/// (child) / referenced (parent) column row when that row is visible, falling back
/// to the card's vertical centre when the column is collapsed away.
fn edge_shapes(
    graph: &DiagramGraph,
    rect: &HashMap<String, Rect>,
    visible: &HashMap<String, Vec<usize>>,
) -> Vec<EdgeShapes> {
    let node_by_id: HashMap<&str, &DiagramNode> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    // Absolute row-centre y for `cols` on node `id`, or None (→ card-edge fallback).
    let row_y = |id: &str, cols: &[String], r: Rect| -> Option<f64> {
        let node = node_by_id.get(id)?;
        let vis = visible.get(id)?;
        erd::column_row_offset(&node.columns, vis, cols, HEADER_H, ROW_H).map(|off| r.y + off)
    };
    let mut out = Vec::new();
    for e in &graph.edges {
        let (Some(&fr), Some(&tr)) = (rect.get(&e.from), rect.get(&e.to)) else {
            continue;
        };
        let from_y = row_y(&e.from, &e.from_columns, fr);
        let to_y = row_y(&e.to, &e.to_columns, tr);
        // A self-referencing FK (`from == to`) loops on the card's right side; a
        // normal edge runs between the two facing sides.
        let self_ref = e.from == e.to;
        let (p0, p1, o0, o1) = if self_ref {
            let (p0, p1) = erd::self_loop_anchors(fr, from_y, to_y);
            (p0, p1, 1.0, 1.0) // both ends leave rightward
        } else {
            let (p0, p1) = erd::edge_anchors_rows(fr, tr, from_y, to_y);
            let (o0, o1) = erd::edge_dirs(fr, tr);
            (p0, p1, o0, o1)
        };
        // Straight stubs off each card edge, then the curve bends between the stub
        // ends — so the markers sit on a straight run and stay symmetric.
        let p0s = Pt {
            x: p0.x + o0 * EDGE_STUB,
            y: p0.y,
        };
        let p1s = Pt {
            x: p1.x + o1 * EDGE_STUB,
            y: p1.y,
        };
        // A self-loop bulges outward from its same-side stubs; a normal edge flows
        // between opposite-facing stubs.
        let (c1, c2) = if self_ref {
            erd::self_loop_controls(p0s, p1s, o0)
        } else {
            erd::cubic_controls(p0s, p1s, o0, o1)
        };
        // Anchor → stub (straight), stub → stub (curve), stub → anchor (straight).
        let mut poly = Vec::with_capacity(35);
        poly.push(p0);
        poly.extend(erd::sample_cubic(p0s, c1, c2, p1s, 32));
        poly.push(p1);
        out.push(EdgeShapes {
            markers: marker_lines(p0, p1, o0, o1, e.cardinality, e.optional),
            poly,
        });
    }
    out
}

/// A custom paint view that strokes the FK edges directly (bezier + crow's-foot
/// markers). It repaints on position/hover change via a `create_effect` that
/// calls `request_paint` — a `svg` view updated reactively did not repaint here
/// and blanked the edges on drag/hover. The hovered edge is stroked heavier in
/// the accent colour.
struct EdgeCanvas {
    id: ViewId,
    graph: Rc<DiagramGraph>,
    positions: RwSignal<HashMap<String, (f64, f64)>>,
    sizes: RwSignal<HashMap<String, (f64, f64)>>,
    collapsed: RwSignal<HashMap<String, bool>>,
    hovered: RwSignal<Option<usize>>,
    zoom: RwSignal<f64>,
    pan: RwSignal<(f64, f64)>,
}

fn edge_canvas(
    graph: Rc<DiagramGraph>,
    positions: RwSignal<HashMap<String, (f64, f64)>>,
    sizes: RwSignal<HashMap<String, (f64, f64)>>,
    collapsed: RwSignal<HashMap<String, bool>>,
    hovered: RwSignal<Option<usize>>,
    zoom: RwSignal<f64>,
    pan: RwSignal<(f64, f64)>,
) -> EdgeCanvas {
    let id = ViewId::new();
    // Repaint whenever a node moves/resizes, its collapse (→ column-precise anchor
    // row) changes, the hovered edge changes, or the view (zoom / pan) changes.
    create_effect(move |_| {
        positions.track();
        sizes.track();
        collapsed.track();
        hovered.track();
        zoom.track();
        pan.track();
        id.request_paint();
    });
    EdgeCanvas {
        id,
        graph,
        positions,
        sizes,
        collapsed,
        hovered,
        zoom,
        pan,
    }
}

impl View for EdgeCanvas {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let base = theme::erd_edge();
        let accent = theme::erd_edge_hover();
        let z = self.zoom.get_untracked();
        let (panx, pany) = self.pan.get_untracked();
        // Edge geometry is computed in logical space, then mapped to screen space:
        // scale by the zoom factor and offset by the pan (same transform the cards
        // bake into their insets — semantic zoom + free pan, no view transform).
        let sc = |p: Pt| Point::new(panx + p.x * z, pany + p.y * z);
        let r = rects(&self.positions.get_untracked(), &self.sizes.get_untracked());
        let vis = visible_map(&self.graph, &self.collapsed.get_untracked());
        let hov = self.hovered.get_untracked();
        for (i, sh) in edge_shapes(&self.graph, &r, &vis).iter().enumerate() {
            let hot = Some(i) == hov;
            let brush = if hot { accent } else { base };
            // Hover changes colour only — width stays constant (no thickening).
            let stroke = Stroke::new(1.4 * z);
            // Stroke the flattened curve as a polyline path (line segments — vger
            // can't stroke cubic segments).
            if let Some((first, rest)) = sh.poly.split_first() {
                let mut path = BezPath::new();
                path.move_to(sc(*first));
                for p in rest {
                    path.line_to(sc(*p));
                }
                cx.stroke(&path, brush, &stroke);
            }
            for m in &sh.markers {
                let line = Line::new(
                    sc(Pt {
                        x: m.p0.x,
                        y: m.p0.y,
                    }),
                    sc(Pt {
                        x: m.p1.x,
                        y: m.p1.y,
                    }),
                );
                cx.stroke(&line, brush, &stroke);
            }
        }
    }
}

/// One column row — pointer-transparent so a drag started on it still reaches the
/// card, and text isn't selectable. Sizes/fonts multiply by `zoom` (semantic
/// zoom: text re-lays-out crisply at each level rather than being transform-scaled).
/// The row background lights up (`erd_row_highlight`) while its node/column is an
/// endpoint of the hovered edge — both linked rows highlight together.
fn column_row(
    c: &DiagramColumn,
    zoom: RwSignal<f64>,
    hovered: RwSignal<Option<usize>>,
    graph: Rc<DiagramGraph>,
    node_id: Rc<str>,
) -> AnyView {
    let name = c.name.clone();
    let hl_name = c.name.clone();
    let ty = c.type_name.clone();
    let (pk, fk) = (c.pk, c.fk);
    // Type-family glyph + name in the full key tint (gold PK / purple FK / normal),
    // type muted.
    let glyph = column_type_icon(classify_column_type(&ty));
    h_stack((
        icons::icon(glyph, 13.0).style(move |s| {
            let z = zoom.get() as f32;
            s.color(col_tint(pk, fk))
                .flex_shrink(0.0_f32)
                .width(13.0 * z)
                .height(13.0 * z)
        }),
        text(name).style(move |s| {
            s.font_size(11.5 * zoom.get() as f32)
                .color(col_tint(pk, fk))
                .flex_grow(1.0_f32)
                .min_width(0.0)
                .text_ellipsis()
        }),
        text(ty).style(move |s| {
            s.font_size(10.5 * zoom.get() as f32)
                .color(theme::text_muted())
        }),
    ))
    .style(move |s| {
        let z = zoom.get();
        // Light up when this column is an endpoint of the hovered edge.
        let hl = hovered
            .get()
            .is_some_and(|idx| erd::edge_touches_column(&graph, idx, &node_id, &hl_name));
        s.items_center()
            .gap(8.0 * z)
            .height(ROW_H * z)
            .width_full()
            .padding_horiz(10.0 * z)
            .background(if hl {
                theme::erd_row_highlight()
            } else {
                floem::peniko::Color::TRANSPARENT
            })
    })
    .pointer_events(|| false)
    .into_any()
}

/// The visible column rows for `node` at a collapse state, plus a clickable
/// expand/collapse toggle row when the node has hidden columns. Pressing the
/// toggle flips `collapsed[id]` and updates `sizes[id]` so edges re-route.
#[allow(clippy::too_many_arguments)]
fn column_rows(
    node: Rc<DiagramNode>,
    is_collapsed: bool,
    sizes: RwSignal<HashMap<String, (f64, f64)>>,
    collapsed: RwSignal<HashMap<String, bool>>,
    zoom: RwSignal<f64>,
    hovered: RwSignal<Option<usize>>,
    graph: Rc<DiagramGraph>,
) -> impl IntoView {
    let (visible, collapsible, _h) = card_metrics(&node, is_collapsed);
    let node_id: Rc<str> = Rc::from(node.id.as_str());
    let mut rows: Vec<AnyView> = visible
        .iter()
        .map(|&ci| {
            column_row(
                &node.columns[ci],
                zoom,
                hovered,
                graph.clone(),
                node_id.clone(),
            )
        })
        .collect();
    if collapsible {
        let hidden = node.columns.len() - visible.len();
        let label = if is_collapsed {
            format!("+{hidden} more")
        } else {
            "show less".to_string()
        };
        let id = node.id.clone();
        let node2 = node.clone();
        rows.push(
            text(label)
                .style(move |s| {
                    let z = zoom.get();
                    s.font_size(10.5 * z as f32)
                        .color(theme::text_dim())
                        .height(EXPANDER_H * z)
                        .width_full()
                        .padding_horiz(10.0 * z)
                        .items_center()
                        .hover(|s| s.color(theme::text()))
                })
                // Toggle on press, consumed so it doesn't start a card drag.
                .on_event_stop(EventListener::PointerDown, move |_| {
                    let now = !is_collapsed;
                    collapsed.update(|m| {
                        m.insert(id.clone(), now);
                    });
                    let (_v, _c, h) = card_metrics(&node2, now);
                    let w = sizes
                        .get_untracked()
                        .get(&id)
                        .map(|s| s.0)
                        .unwrap_or(NODE_MIN_W);
                    sizes.update(|m| {
                        m.insert(id.clone(), (w, h));
                    });
                })
                .into_any(),
        );
    }
    v_stack_from_iter(rows).style(|s| s.width_full())
}

/// One table/stub card. Positioned from the reactive `positions` signal (so it
/// follows drags) and, for real tables, draggable — a drag updates `positions`
/// and calls `persist` on release. Double-clicking a real table invokes `reveal`
/// (opens/reveals it in the app).
#[allow(clippy::too_many_arguments)]
fn node_card(
    p: &Placed,
    positions: RwSignal<HashMap<String, (f64, f64)>>,
    sizes: RwSignal<HashMap<String, (f64, f64)>>,
    collapsed: RwSignal<HashMap<String, bool>>,
    zoom: RwSignal<f64>,
    pan: RwSignal<(f64, f64)>,
    hovered: RwSignal<Option<usize>>,
    graph: Rc<DiagramGraph>,
    persist: Rc<dyn Fn()>,
    reveal: Rc<dyn Fn(String)>,
) -> AnyView {
    let id = p.node.id.clone();
    let (ix, iy, w) = (p.x, p.y, p.w);
    // This card's live position, **borrowed** from the map rather than cloned out
    // of it. `SignalGet::get` clones the whole value, so `positions.get()` here
    // cloned the entire N-entry map to read one entry — and every card's style
    // closure re-runs on every `positions`/`pan` change, making a single pointer
    // move O(cards²): 1.4 ms at 200 cards, 8.4 ms at 500, before floem lays out
    // anything. `with` borrows, so a move is linear again.
    let at = move |key: &str| {
        positions.with(|m: &HashMap<String, (f64, f64)>| m.get(key).copied().unwrap_or((ix, iy)))
    };

    if p.node.kind == NodeKind::Stub {
        let id_s = id.clone();
        return container(text(p.node.id.clone()).style(move |s| {
            s.font_size(13.0 * zoom.get() as f32)
                .color(theme::text_dim())
                .padding_horiz(10.0 * zoom.get())
        }))
        .style(move |s| {
            let z = zoom.get();
            let (panx, pany) = pan.get();
            let (x, y) = at(&id_s);
            s.absolute()
                .inset_left(panx + x * z)
                .inset_top(pany + y * z)
                .width(w * z)
                .height(HEADER_H * z)
                .items_center()
                .border(1.0)
                .border_color(theme::text_muted())
                .border_radius(6.0 * z)
                .background(theme::erd_node_bg())
        })
        .into_any();
    }

    let name = p.node.id.clone();
    // Does the name ellipsize at this card's width? The header lays out as
    // icon (13) + gap (7) + name, inside 10px horizontal padding each side, so the
    // name gets `w - 40`. Measured BOLD, matching how it's drawn — a node id is
    // `schema.table` outside `public`, which truncates far more often than a bare
    // name did. Compared in unzoomed space: zoom scales both sides alike.
    let name_truncated = measure_text_px_bold_at(&name, 13.0) > w - 40.0;
    let full_name = name.clone();
    // Table icon + colour from the schema panel (base table vs. view).
    let is_view = p.node.kind == NodeKind::View;
    let header = h_stack((
        icons::icon(
            if is_view {
                icons::TABLE_CELLS_MERGE
            } else {
                icons::TABLE
            },
            13.0,
        )
        .style(move |s| {
            let z = zoom.get() as f32;
            s.color(if is_view {
                theme::view_icon()
            } else {
                theme::table_icon()
            })
            .width(13.0 * z)
            .height(13.0 * z)
        }),
        text(name).style(move |s| {
            s.font_size(13.0 * zoom.get() as f32)
                .font_bold()
                .color(theme::text())
                .flex_grow(1.0_f32)
                .min_width(0.0)
                .text_ellipsis()
        }),
    ))
    .style(move |s| {
        let z = zoom.get();
        s.items_center()
            .gap(7.0 * z)
            .height(HEADER_H * z)
            .width_full()
            .padding_horiz(10.0 * z)
            .background(theme::erd_node_header())
            .border_bottom(1.0)
            .border_color(theme::border())
    });

    // A truncated name gets a tooltip with the full text — only on the header, and
    // only when it actually ellipsizes (same rule as the tab strip).
    //
    // The tooltip needs hover, so the header can't stay `pointer_events(false)`
    // like the rest of the card's content. It registers no pointer handlers, so
    // PointerDown/Move/Up and the double-click still reach the card's drag zone by
    // bubbling — a child only stops propagation when it *consumes* an event.
    let header: AnyView = if name_truncated {
        header.tooltip(move || text(full_name.clone())).into_any()
    } else {
        // Nothing to reveal → stay pointer-transparent, exactly as before.
        header.pointer_events(|| false).into_any()
    };

    // Rows are reactive: rebuilt when this node's collapse state changes.
    let node = Rc::new(p.node.clone());
    let rows_view = {
        let node = node.clone();
        let id_k = id.clone();
        dyn_container(
            move || collapsed.get().get(&id_k).copied().unwrap_or(false),
            move |is_collapsed| {
                column_rows(
                    node.clone(),
                    is_collapsed,
                    sizes,
                    collapsed,
                    zoom,
                    hovered,
                    graph.clone(),
                )
                .into_any()
            },
        )
        .style(|s| s.width_full())
    };

    let card = v_stack((header, rows_view));
    let cid = card.id();
    let dragging = RwSignal::new(false);
    let moved = RwSignal::new(false);
    let grab = RwSignal::new((0.0_f64, 0.0_f64));
    let id_style = id.clone();
    let id_move = id.clone();
    card.style(move |s| {
        let z = zoom.get();
        let (panx, pany) = pan.get();
        let (x, y) = at(&id_style);
        s.absolute()
            .inset_left(panx + x * z)
            .inset_top(pany + y * z)
            .width(w * z)
            .border(1.0)
            .border_color(theme::border())
            .border_radius(6.0 * z)
            .background(theme::erd_node_bg())
    })
    .on_event(EventListener::PointerDown, move |e| {
        if let Event::PointerDown(pe) = e
            && pe.button.is_primary()
        {
            grab.set((pe.pos.x, pe.pos.y));
            dragging.set(true);
            moved.set(false);
            cid.request_active(); // capture moves even off the card
            return EventPropagation::Stop;
        }
        EventPropagation::Continue
    })
    .on_event(EventListener::PointerMove, move |e| {
        if dragging.get_untracked()
            && let Event::PointerMove(pe) = e
        {
            // `pe.pos` is relative to the (moving) card, so the delta from the grab
            // offset is how far to shift — same idiom as the editor scrollbar drag.
            // Positions are stored in logical (unzoomed) space, so divide the
            // screen-space delta by the zoom factor.
            let (gx, gy) = grab.get_untracked();
            let z = zoom.get_untracked();
            positions.update(|m| {
                let (cx, cy) = m.get(&id_move).copied().unwrap_or((ix, iy));
                // Free canvas — no clamp; a table can live at any (incl. negative)
                // logical position and you pan to reach it.
                let nx = cx + (pe.pos.x - gx) / z;
                let ny = cy + (pe.pos.y - gy) / z;
                m.insert(id_move.clone(), (nx, ny));
            });
            moved.set(true);
        }
        // Consume moves over the card so the canvas edge-hover test fires only in
        // the gaps between cards.
        EventPropagation::Stop
    })
    .on_event(EventListener::PointerUp, move |_| {
        if dragging.get_untracked() {
            dragging.set(false);
            cid.clear_active();
            // Only persist when a drag actually moved the card — not on a plain
            // click or the clicks that make up a double-click.
            if moved.get_untracked() {
                (persist)();
            }
        }
        EventPropagation::Continue
    })
    .on_double_click_stop(move |_| {
        // A double-click reveals the table; clear any drag/press state first
        // (the second PointerUp is consumed by the double-click).
        dragging.set(false);
        cid.clear_active();
        (reveal)(id.clone());
    })
    .into_any()
}

/// Shared surface for every toolbar control (count chip, icon button, zoom unit).
/// Lives in `widgets` now that the header's Retry wears the same chrome; text and
/// icon colour is `theme::text()` (~#C6C8D6, the app's approximation of the
/// spec's #C2C4D2), and everything in the toolbar is 13px.
use crate::widgets::{TOOLBAR_FONT, control_surface as toolbar_surface};

/// A read-only count pill (e.g. "3 tables"), styled like the buttons.
fn count_chip(label: String) -> AnyView {
    text(label)
        .style(|s| {
            toolbar_surface(s)
                .font_size(TOOLBAR_FONT)
                .color(theme::text())
                .padding_horiz(10.0)
                .padding_vert(5.0)
        })
        .into_any()
}

/// A standalone icon button (Fit, Reset layout), brightening on hover.
fn control_button(glyph: &'static str, action: Rc<dyn Fn()>) -> AnyView {
    container(icons::icon(glyph, 16.0).style(|s| s.color(theme::text())))
        .on_click_stop(move |_| (action)())
        .style(|s| {
            toolbar_surface(s)
                .items_center()
                .justify_center()
                .padding_horiz(10.0)
                .padding_vert(5.0)
                .hover(|s| s.background(theme::erd_node_bg()))
        })
        .into_any()
}

/// The zoom control as a single segmented unit: `−` │ `100%` │ `+`. One outer
/// border (`erd_control_border`); the two internal separators share that colour.
/// The `−`/`+` steps run the supplied `zoom_out`/`zoom_in` actions (which pivot at
/// the viewport centre); the label reads the live `zoom`.
fn zoom_unit(zoom: RwSignal<f64>, zoom_out: Rc<dyn Fn()>, zoom_in: Rc<dyn Fn()>) -> AnyView {
    // An icon step inside the unit — no border of its own (the unit carries it).
    let step = |glyph: &'static str, action: Rc<dyn Fn()>| {
        container(icons::icon(glyph, 16.0).style(|s| s.color(theme::text())))
            .on_click_stop(move |_| (action)())
            .style(|s| {
                s.items_center()
                    .justify_center()
                    .padding_horiz(10.0)
                    .padding_vert(5.0)
                    .hover(|s| s.background(theme::erd_node_bg()))
            })
            .into_any()
    };
    // Full-height divider between the percentage and each icon step.
    let sep = || {
        empty()
            .style(|s| {
                s.width(1.0)
                    .height_full()
                    .background(theme::erd_control_border())
            })
            .into_any()
    };
    let minus = step(icons::MINUS, zoom_out);
    // Fixed width (fits "300%") + centred, so the label doesn't reflow narrower at
    // 2-digit percentages and the flanking separators/icons stay put.
    let percent = dyn_container(
        move || (zoom.get() * 100.0).round() as i32,
        move |pct| {
            text(format!("{pct}%"))
                .style(|s| s.font_size(TOOLBAR_FONT).color(theme::text()))
                .into_any()
        },
    )
    .style(|s| s.width(48.0).items_center().justify_center())
    .into_any();
    let plus = step(icons::PLUS, zoom_in);
    h_stack((minus, sep(), percent, sep(), plus))
        .style(|s| toolbar_surface(s).items_center())
        .into_any()
}

pub(crate) fn erd_overlay(ui: Ui) -> impl IntoView {
    let erd_sig = ui.overlay.erd;
    let db_nodes = ui.schema.db_nodes;
    let open_table = ui.tab_actions.open_table.clone();
    let win = window_size();

    dyn_container(
        move || {
            erd_sig
                .get()
                .map(|t| (t.database.clone(), format!("{:?}", t.seed)))
        },
        move |open| {
            let Some(target) = erd_sig.get_untracked() else {
                return empty().into_any();
            };
            let _ = open;
            let close: Rc<dyn Fn()> = Rc::new(move || erd_sig.set(None));

            // Resolve the schema; if it isn't introspected yet, say so.
            let Some(schema) = resolve_schema(db_nodes, &target.database) else {
                let body = centered_msg(
                    "Schema isn't loaded for this database yet.",
                    theme::text_dim(),
                )
                .into_any();
                return modal_frame(win, close, "—".to_string(), Vec::new(), Vec::new(), body)
                    .into_any();
            };

            let graph = erd::build_graph(&schema, &target.database, &target.seed);
            let scope = match &target.seed {
                schemaic_core::erd::DiagramSeed::Database => target.database.clone(),
                schemaic_core::erd::DiagramSeed::Table(t) => format!("{}.{t}", target.database),
            };

            // Empty graph (e.g. a table with no relationships, or unknown seed).
            if graph.nodes.is_empty() {
                let body = centered_msg(
                    "No foreign-key relationships to diagram.",
                    theme::text_dim(),
                )
                .into_any();
                return modal_frame(win, close, scope, chips(&graph), Vec::new(), body).into_any();
            }

            let (placed, cw, ch, collapsed_defaults) = build_placed(&graph);
            // Left-side count pills (tables / relationships / hidden).
            let counts = chips(&graph);
            let graph = Rc::new(graph);
            // Sizes + collapse state are reactive so the expand/collapse toggle
            // resizes a card and the edges re-route.
            let sizes: RwSignal<HashMap<String, (f64, f64)>> = RwSignal::new(
                placed
                    .iter()
                    .map(|p| (p.node.id.clone(), (p.w, p.h)))
                    .collect(),
            );
            let collapsed = RwSignal::new(collapsed_defaults);
            let zoom = RwSignal::new(1.0_f64);
            // Free-pan offset (screen px): a card/edge at logical (x,y) draws at
            // pan + (x,y)*zoom. Pan is unbounded — the canvas is "infinite".
            let pan = RwSignal::new((0.0_f64, 0.0_f64));
            // Viewport size (measured on resize) for centre-pivot toolbar zoom + Fit.
            let viewport_size = RwSignal::new((800.0_f64, 500.0_f64));

            // Zoom about a viewport point, keeping the logical point under it fixed
            // (so Ctrl+wheel zooms where the cursor is). A Copy closure, reused below.
            let zoom_at = move |cx: f64, cy: f64, factor: f64| {
                let z0 = zoom.get_untracked();
                let z1 = (z0 * factor).clamp(ZOOM_MIN, ZOOM_MAX);
                if (z1 - z0).abs() < f64::EPSILON {
                    return;
                }
                let (px, py) = pan.get_untracked();
                pan.set((cx - (cx - px) * (z1 / z0), cy - (cy - py) * (z1 / z0)));
                zoom.set(z1);
            };

            // Fit the whole diagram in the measured viewport and centre it.
            let fit: Rc<dyn Fn()> = Rc::new(move || {
                let (z, p) = erd::fit_view((cw, ch), viewport_size.get_untracked(), ZOOM_MIN);
                zoom.set(z);
                pan.set(p);
            });

            // Positions: auto-layout, overridden by any saved manual layout for this
            // (connection, database). Unknown/stale saved ids are ignored.
            let mut pos_map: HashMap<String, (f64, f64)> = placed
                .iter()
                .map(|p| (p.node.id.clone(), (p.x, p.y)))
                .collect();
            // The pure auto-layout, kept for the "Reset layout" action.
            let auto_positions = pos_map.clone();
            let saved: schemaic_core::erd::DiagramLayoutsFile =
                schemaic_core::persist::load_json("diagrams.json");
            if let Some(s) =
                schemaic_core::erd::get_layout(&saved, target.conn_id, &target.database)
            {
                for (id, xy) in s {
                    if pos_map.contains_key(id) {
                        pos_map.insert(id.clone(), *xy);
                    }
                }
            }
            let positions = RwSignal::new(pos_map);

            // Persist the manual layout (called when a drag ends).
            let persist: Rc<dyn Fn()> = {
                let db = target.database.clone();
                let cid = target.conn_id;
                Rc::new(move || {
                    let mut f: schemaic_core::erd::DiagramLayoutsFile =
                        schemaic_core::persist::load_json("diagrams.json");
                    schemaic_core::erd::upsert_layout(&mut f, cid, &db, positions.get_untracked());
                    schemaic_core::persist::save_json("diagrams.json", &f);
                })
            };

            // Double-clicking a table closes the modal and opens/reveals it. A
            // diagram node id is a *display* name (`sales.orders` outside
            // `public`), so it's resolved back to a real table rather than used
            // as one. A stub node (a cross-database FK target) resolves to
            // nothing and is left alone.
            let reveal: Rc<dyn Fn(String)> = {
                let ot = open_table.clone();
                let db = target.database.clone();
                let close = close.clone();
                let resolved = schema.clone();
                Rc::new(move |node_id: String| {
                    let Some(t) = resolved.find_by_display(&node_id) else {
                        return;
                    };
                    (close)();
                    (ot)(schemaic_core::schema::TableSource::new(
                        db.clone(),
                        t.schema.clone(),
                        t.name.clone(),
                    ));
                })
            };

            // Reset layout → auto positions, and reset the view so the arrangement
            // is centred/on-screen again.
            let reset: Rc<dyn Fn()> = {
                let auto = auto_positions;
                let persist = persist.clone();
                Rc::new(move || {
                    positions.set(auto.clone());
                    pan.set((0.0, 0.0));
                    zoom.set(1.0);
                    (persist)();
                })
            };

            // Toolbar +/- pivot at the viewport centre.
            let zoom_in: Rc<dyn Fn()> = Rc::new(move || {
                let (vw, vh) = viewport_size.get_untracked();
                zoom_at(vw / 2.0, vh / 2.0, 1.2);
            });
            let zoom_out: Rc<dyn Fn()> = Rc::new(move || {
                let (vw, vh) = viewport_size.get_untracked();
                zoom_at(vw / 2.0, vh / 2.0, 1.0 / 1.2);
            });

            // Right-side controls, left→right: zoom unit, Fit, Reset layout. Keep a
            // clone of `fit` for the one-shot fit-on-open below.
            let fit_on_open = fit.clone();
            let controls: Vec<AnyView> = vec![
                zoom_unit(zoom, zoom_out, zoom_in),
                control_button(icons::SCAN_SQUARE, fit),
                control_button(icons::ROTATE_CCW, reset),
            ];
            // Fit-on-open runs once, the first time the canvas reports its real size.
            let did_autofit = RwSignal::new(false);

            let hovered = RwSignal::new(None::<usize>);

            // Edge layer: fills the viewport and paints edges at pan + logical*zoom
            // (see `EdgeCanvas`); pointer-transparent so drags/pans fall through.
            let edge_layer = edge_canvas(
                graph.clone(),
                positions,
                sizes,
                collapsed,
                hovered,
                zoom,
                pan,
            )
            .pointer_events(|| false)
            .style(|s| {
                s.absolute()
                    .inset_left(0.0)
                    .inset_top(0.0)
                    .width_full()
                    .height_full()
            });

            // Node cards over the edge layer.
            let mut children: Vec<AnyView> = vec![edge_layer.into_any()];
            for p in &placed {
                children.push(node_card(
                    p,
                    positions,
                    sizes,
                    collapsed,
                    zoom,
                    pan,
                    hovered,
                    graph.clone(),
                    persist.clone(),
                    reveal.clone(),
                ));
            }

            // Infinite canvas: an absolutely-laid-out layer filling the modal body.
            // Cards/edges place themselves via pan+zoom (baked into insets / paint),
            // so there's no scroll and no scrollbars. Drag empty space — or
            // middle-drag anywhere — to pan (the pointer is captured so a pan
            // continues over cards); Ctrl+wheel zooms about the cursor; Shift+wheel /
            // plain wheel pan horizontally / vertically.
            //
            // `.clip()` shrinks a view to its *content*, and our children are all
            // absolute (zero in-flow content) — so the clip layer must carry an
            // explicit size, not `size_full`, or it collapses to 0×0 (blank canvas,
            // no hit area). We measure the flex-grow wrapper (`on_resize`) into
            // `viewport_size` and size this inner clip layer to it.
            let g_hit = graph.clone();
            let panning = RwSignal::new(false);
            let last = RwSignal::new((0.0_f64, 0.0_f64));
            let base = v_stack_from_iter(children);
            let vid = base.id();
            let canvas_inner = base
                .on_event(EventListener::PointerDown, move |e| {
                    // Primary on empty space (cards consume their own primary) or
                    // middle anywhere → begin panning, capturing the pointer.
                    if let Event::PointerDown(pe) = e
                        && (pe.button.is_primary() || pe.button.is_auxiliary())
                    {
                        panning.set(true);
                        last.set((pe.pos.x, pe.pos.y));
                        vid.request_active();
                        return EventPropagation::Stop;
                    }
                    EventPropagation::Continue
                })
                .on_event(EventListener::PointerMove, move |e| {
                    let Event::PointerMove(pe) = e else {
                        return EventPropagation::Continue;
                    };
                    if panning.get_untracked() {
                        let (lx, ly) = last.get_untracked();
                        pan.update(|p| {
                            p.0 += pe.pos.x - lx;
                            p.1 += pe.pos.y - ly;
                        });
                        last.set((pe.pos.x, pe.pos.y));
                        return EventPropagation::Stop;
                    }
                    // Edge hover: map the cursor back to logical space (undo pan/zoom).
                    let z = zoom.get_untracked();
                    let (panx, pany) = pan.get_untracked();
                    let r = rects(&positions.get_untracked(), &sizes.get_untracked());
                    let vis = visible_map(&g_hit, &collapsed.get_untracked());
                    let polys: Vec<Vec<Pt>> = edge_shapes(&g_hit, &r, &vis)
                        .into_iter()
                        .map(|e| e.poly)
                        .collect();
                    let near = erd::nearest_polyline(
                        Pt {
                            x: (pe.pos.x - panx) / z,
                            y: (pe.pos.y - pany) / z,
                        },
                        &polys,
                        EDGE_HOVER_PX / z,
                    );
                    if hovered.get_untracked() != near {
                        hovered.set(near);
                    }
                    EventPropagation::Continue
                })
                .on_event(EventListener::PointerUp, move |_| {
                    if panning.get_untracked() {
                        panning.set(false);
                        vid.clear_active();
                        return EventPropagation::Stop;
                    }
                    EventPropagation::Continue
                })
                .on_event(EventListener::PointerLeave, move |_| {
                    if hovered.get_untracked().is_some() {
                        hovered.set(None);
                    }
                    EventPropagation::Continue
                })
                .on_event(EventListener::PointerWheel, move |e| {
                    let Event::PointerWheel(pe) = e else {
                        return EventPropagation::Continue;
                    };
                    let dy = if pe.delta.y != 0.0 {
                        pe.delta.y
                    } else {
                        pe.delta.x
                    };
                    if pe.modifiers.control() {
                        // Ctrl+wheel → zoom about the cursor (up = in, down = out).
                        if dy != 0.0 {
                            let factor = if dy < 0.0 { 1.1 } else { 1.0 / 1.1 };
                            zoom_at(pe.pos.x, pe.pos.y, factor);
                        }
                    } else if pe.modifiers.shift() {
                        // Shift+wheel → horizontal pan.
                        let dx = if pe.delta.x != 0.0 {
                            pe.delta.x
                        } else {
                            pe.delta.y
                        };
                        pan.update(|p| p.0 -= dx);
                    } else {
                        // Plain wheel → vertical pan.
                        pan.update(|p| p.1 -= dy);
                    }
                    EventPropagation::Stop
                })
                // Size must be applied *before* `.clip()`: clip wraps the view, so a
                // style after it lands on the wrapper and leaves this inner layer
                // (which holds the children) zero-sized → nothing renders.
                .style(move |s| {
                    let (vw, vh) = viewport_size.get();
                    s.width(vw).height(vh).background(theme::erd_canvas())
                })
                .clip();

            // Flex-grow wrapper fills the modal body; its measured size feeds
            // `viewport_size`, which sizes the clip layer above. `min_*(0)` lets it
            // shrink within the panel column.
            let canvas = container(canvas_inner)
                .on_resize(move |rect| {
                    let (w, h) = (rect.width(), rect.height());
                    if viewport_size.get_untracked() != (w, h) {
                        viewport_size.set((w, h));
                    }
                    // Fit-on-open: the first time the canvas reports a real size, if
                    // the diagram overflows the viewport, fit + centre it (small
                    // diagrams keep their 100% top-left open). One-shot, so a later
                    // window resize never yanks the user's zoom/pan.
                    if !did_autofit.get_untracked() && w > 1.0 && h > 1.0 {
                        did_autofit.set(true);
                        if erd::view_overflows((cw, ch), (w, h)) {
                            (fit_on_open)();
                        }
                    }
                })
                .style(|s| {
                    s.flex_grow(1.0_f32)
                        .width_full()
                        .min_height(0.0)
                        .min_width(0.0)
                });

            modal_frame(win, close, scope, counts, controls, canvas.into_any()).into_any()
        },
    )
    .style(move |s| {
        if erd_sig.get().is_some() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// The toolbar's stat chips for a graph.
fn chips(graph: &DiagramGraph) -> Vec<AnyView> {
    let tables = graph
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::Stub)
        .count();
    let mut v = vec![
        count_chip(format!("{tables} tables")),
        count_chip(format!("{} relationships", graph.edges.len())),
    ];
    if !graph.hidden_islands.is_empty() {
        v.push(count_chip(format!(
            "{} unrelated hidden",
            graph.hidden_islands.len()
        )));
    }
    v
}

/// Assemble the modal: backdrop + panel (header, toolbar, body). `body` is the
/// canvas (or a message). Sized ~80% of the window.
fn modal_frame(
    win: RwSignal<(f64, f64)>,
    close: Rc<dyn Fn()>,
    scope: String,
    counts: Vec<AnyView>,
    controls: Vec<AnyView>,
    body: AnyView,
) -> impl IntoView {
    let toolbar = h_stack((
        // Left: scope breadcrumb (label + value grouped) then the count pills 10px
        // to its right.
        h_stack((
            h_stack((
                text("Scope").style(|s| s.font_size(TOOLBAR_FONT).color(theme::text_muted())),
                text(scope).style(|s| s.font_size(TOOLBAR_FONT).color(theme::text())),
            ))
            .style(|s| s.items_center().gap(6.0)),
            h_stack_from_iter(counts).style(|s| s.items_center().gap(8.0)),
        ))
        .style(|s| s.items_center().gap(10.0)),
        empty().style(|s| s.flex_grow(1.0_f32)),
        // Right: zoom unit / Fit / Reset, 10px apart; last is 10px from the edge.
        h_stack_from_iter(controls).style(|s| s.items_center().gap(10.0)),
    ))
    .style(|s| {
        // `flex_shrink(0)`: keep the toolbar's fixed height in the panel's column
        // flex. Without it the toolbar competes for negative space with the body as
        // the (zoomed) canvas content grows, and gets progressively compressed.
        // 1px top+bottom border in `erd_toolbar_border`.
        s.items_center()
            .gap(10.0)
            .width_full()
            .height(48.0)
            .flex_shrink(0.0_f32)
            .padding_left(16.0)
            .padding_right(10.0)
            .border_top(1.0)
            .border_bottom(1.0)
            .border_color(theme::erd_toolbar_border())
            .background(theme::erd_node_header())
    });

    // Borderless header so the toolbar's own top border is the single divider —
    // two adjacent 1px lines (header bottom + toolbar top) read as a fuzzy 2px band.
    let panel = v_stack((
        modal_title_borderless("ER Diagram", close.clone()),
        toolbar,
        body,
    ))
    .on_click_stop(|_| {})
    .style(move |s| {
        let (ww, wh) = win.get();
        panel_style(s)
            .width(ww * 0.8)
            .height(wh * 0.8)
            .background(theme::bg_panel())
    });

    let esc = close.clone();
    container(panel)
        .keyboard_navigable()
        .request_focus(|| {})
        .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
        .on_click_stop(move |_| close())
        .style(|s| {
            s.size_full()
                .items_center()
                .justify_center()
                .background(theme::modal_backdrop())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemaic_core::erd::{Cardinality, DiagramEdge, DiagramGraph, DiagramNode, NodeKind};

    fn node(id: &str) -> DiagramNode {
        DiagramNode {
            id: id.to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
        }
    }

    /// Each edge yields a bezier curve, crow's-foot markers, and a non-empty
    /// hit-test polyline whose endpoints match the curve — for the initial and a
    /// dragged layout.
    #[test]
    fn edge_shapes_are_built_for_all_layouts() {
        let graph = DiagramGraph {
            nodes: vec![node("a"), node("b")],
            edges: vec![DiagramEdge {
                from: "a".into(),
                from_columns: vec![],
                to: "b".into(),
                to_columns: vec![],
                cardinality: Cardinality::OneToMany,
                optional: true,
            }],
            hidden_islands: vec![],
            total_tables: 2,
        };
        let sizes: HashMap<String, (f64, f64)> = [
            ("a".to_string(), (200.0, 120.0)),
            ("b".to_string(), (200.0, 90.0)),
        ]
        .into_iter()
        .collect();

        // Initial layout, then a "dragged" layout (b shoved to an odd offset).
        for positions in [
            [("a", (40.0, 40.0)), ("b", (300.0, 40.0))],
            [("a", (40.0, 300.0)), ("b", (517.0, 12.0))],
        ] {
            let positions: HashMap<String, (f64, f64)> = positions
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
            let vis = visible_map(&graph, &HashMap::new());
            let shapes = edge_shapes(&graph, &rects(&positions, &sizes), &vis);
            assert_eq!(shapes.len(), 1);
            let s = &shapes[0];
            assert!(!s.markers.is_empty(), "crow's-foot markers present");
            assert!(
                s.poly.len() >= 2,
                "flattened curve / hit-test polyline sampled"
            );
        }
    }

    fn col(name: &str, pk: bool, fk: bool) -> DiagramColumn {
        DiagramColumn {
            name: name.to_string(),
            type_name: "int".into(),
            nullable: !pk,
            pk,
            fk,
        }
    }

    /// The child edge end anchors on the FK column's row, the parent end on the
    /// referenced PK row — not the cards' vertical centres.
    #[test]
    fn edge_ends_anchor_on_the_key_column_rows() {
        // parent `users`: id(0). child `orders`: id(0), user_id(1, FK→users.id).
        let users = DiagramNode {
            id: "users".into(),
            kind: NodeKind::Table,
            columns: vec![col("id", true, false)],
        };
        let orders = DiagramNode {
            id: "orders".into(),
            kind: NodeKind::Table,
            columns: vec![col("id", true, false), col("user_id", false, true)],
        };
        let graph = DiagramGraph {
            nodes: vec![orders, users],
            edges: vec![DiagramEdge {
                from: "orders".into(),
                from_columns: vec!["user_id".into()],
                to: "users".into(),
                to_columns: vec!["id".into()],
                cardinality: Cardinality::OneToMany,
                optional: false,
            }],
            hidden_islands: vec![],
            total_tables: 2,
        };
        // orders card left of users so the edge runs orders.right → users.left.
        let positions: HashMap<String, (f64, f64)> = [
            ("orders".to_string(), (0.0, 0.0)),
            ("users".to_string(), (400.0, 0.0)),
        ]
        .into_iter()
        .collect();
        let sizes: HashMap<String, (f64, f64)> = [
            ("orders".to_string(), (200.0, HEADER_H + 2.0 * ROW_H)),
            ("users".to_string(), (200.0, HEADER_H + ROW_H)),
        ]
        .into_iter()
        .collect();
        let vis = visible_map(&graph, &HashMap::new());
        let shapes = edge_shapes(&graph, &rects(&positions, &sizes), &vis);
        assert_eq!(shapes.len(), 1);
        let poly = &shapes[0].poly;
        // Child end = first poly point (source anchor): on orders.user_id (row 1).
        let child_y = HEADER_H + 1.5 * ROW_H; // 0.0 + header + (1 + 0.5)*row
        assert_eq!(poly.first().unwrap().y, child_y);
        assert_eq!(poly.first().unwrap().x, 200.0, "orders right edge");
        // Parent end = last poly point: on users.id (row 0).
        let parent_y = HEADER_H + 0.5 * ROW_H;
        assert_eq!(poly.last().unwrap().y, parent_y);
        assert_eq!(poly.last().unwrap().x, 400.0, "users left edge");
    }

    /// A self-referencing FK (child == parent) loops on the card's right side: both
    /// poly ends sit on the right edge at their key rows, and the curve bulges out
    /// past that edge instead of wrapping around the whole card.
    #[test]
    fn self_referencing_fk_loops_on_one_side() {
        // employees.reports_to (row 1, FK) → employees.id (row 0, PK).
        let employees = DiagramNode {
            id: "employees".into(),
            kind: NodeKind::Table,
            columns: vec![col("id", true, false), col("reports_to", false, true)],
        };
        let graph = DiagramGraph {
            nodes: vec![employees],
            edges: vec![DiagramEdge {
                from: "employees".into(),
                from_columns: vec!["reports_to".into()],
                to: "employees".into(),
                to_columns: vec!["id".into()],
                cardinality: Cardinality::OneToMany,
                optional: true,
            }],
            hidden_islands: vec![],
            total_tables: 1,
        };
        let positions: HashMap<String, (f64, f64)> = [("employees".to_string(), (0.0, 0.0))]
            .into_iter()
            .collect();
        let w = 200.0;
        let sizes: HashMap<String, (f64, f64)> =
            [("employees".to_string(), (w, HEADER_H + 2.0 * ROW_H))]
                .into_iter()
                .collect();
        let vis = visible_map(&graph, &HashMap::new());
        let shapes = edge_shapes(&graph, &rects(&positions, &sizes), &vis);
        assert_eq!(shapes.len(), 1);
        let poly = &shapes[0].poly;
        // Both ends anchor on the right edge (x == card right), at the FK and PK rows.
        assert_eq!(poly.first().unwrap().x, w, "child end on the right edge");
        assert_eq!(poly.last().unwrap().x, w, "parent end on the right edge");
        assert_eq!(
            poly.first().unwrap().y,
            HEADER_H + 1.5 * ROW_H,
            "reports_to row"
        );
        assert_eq!(poly.last().unwrap().y, HEADER_H + 0.5 * ROW_H, "id row");
        // The curve bulges out to the right of the card, never crossing to the left.
        let max_x = poly.iter().fold(f64::MIN, |m, p| m.max(p.x));
        let min_x = poly.iter().fold(f64::MAX, |m, p| m.min(p.x));
        assert!(max_x > w + EDGE_STUB, "loop bulges past the right stub");
        assert!(min_x >= w, "loop stays on the right side of the card");
    }
}
