//! ER-diagram model: turn an introspected [`DbSchema`] into a diagram graph
//! (nodes = tables, edges = foreign keys) for the read-only ER-diagram modal.
//!
//! Pure + unit-tested. This layer answers *what* to draw — the node/edge set for a
//! seed (a single table's FK neighbourhood, or a whole database), which columns to
//! show when a node is collapsed, and each FK's cardinality. Pixel layout lives in
//! [`auto_layout`](crate::erd) (next), rendering in the UI. Nothing here does IO or
//! touches the UI.
//!
//! The graph is built from one [`DbSchema`] (a single database). A foreign key that
//! points at a table in *another* database (MySQL only — `ref_schema` differs from
//! the current DB) can't be enumerated here, so it becomes a **stub node** carrying
//! just the qualified name, never expanded.

use crate::schema::{DbSchema, ForeignKeyInfo, TableInfo};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// What seeds the diagram.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagramSeed {
    /// The whole database: every table that participates in a relationship
    /// (tables with no FK in or out are hidden as islands).
    Database,
    /// One table plus its one-hop FK neighbours (tables it references and tables
    /// that reference it).
    Table(String),
}

/// A node's kind: a real table in this database (with columns), or a stub standing
/// in for a cross-database FK target we can't enumerate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// A base table in the current database.
    Table,
    /// A view in the current database.
    View,
    /// A cross-database FK target — name only, not expandable.
    Stub,
}

/// One column row inside a table node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagramColumn {
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
    /// Part of the primary key.
    pub pk: bool,
    /// Referenced by a foreign key declared on this table.
    pub fk: bool,
}

impl DiagramColumn {
    /// A key column (PK or FK) — pinned into the collapsed view.
    pub fn is_key(&self) -> bool {
        self.pk || self.fk
    }
}

/// A table (or stub) in the diagram. `id` is the stable key used by edges and by
/// persisted layout positions: the bare table name for a real node, `db.table` for
/// a cross-database stub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagramNode {
    /// Identity **and** card label. There was a separate `name` field, always
    /// set to a clone of this one — the label is deliberately the id so two
    /// same-named tables in different namespaces are told apart on the canvas
    /// rather than both reading "orders". Two fields that must stay equal is a
    /// standing invitation for a comparison to use the wrong one, and the
    /// island sweep already did: it collected `name`s and then tested `id`s
    /// against them.
    pub id: String,
    pub kind: NodeKind,
    /// Columns, in schema order. Empty for a stub.
    pub columns: Vec<DiagramColumn>,
}

/// Relationship multiplicity, from the referencing side's uniqueness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cardinality {
    /// The referencing columns are not unique — many children per parent row.
    OneToMany,
    /// The referencing columns are themselves unique (or the child PK) — 1:1.
    OneToOne,
}

/// A foreign-key relationship, drawn child → parent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagramEdge {
    /// Referencing (child) node id.
    pub from: String,
    /// Referencing columns, in key order.
    pub from_columns: Vec<String>,
    /// Referenced (parent) node id.
    pub to: String,
    /// Referenced columns, aligned to `from_columns`.
    pub to_columns: Vec<String>,
    pub cardinality: Cardinality,
    /// The FK is optional — at least one referencing column is nullable, so a child
    /// row may have no parent ("zero-or-one" at the parent end → optionality circle).
    pub optional: bool,
}

/// The built diagram graph.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagramGraph {
    pub nodes: Vec<DiagramNode>,
    pub edges: Vec<DiagramEdge>,
    /// Tables omitted because they have no relationships (Database seed only),
    /// surfaced as a "N unrelated tables hidden" note. Sorted, for stable display.
    pub hidden_islands: Vec<String>,
    /// Total table count in the source schema (the "N tables" chip).
    pub total_tables: usize,
}

/// Does `fk_cols` (a foreign key's referencing columns) form a unique key on
/// `child` — either exactly the primary key, or a UNIQUE index over the same set?
/// If so the relationship is 1:1, otherwise 1:many.
fn fk_is_unique(child: &TableInfo, fk_cols: &[String]) -> bool {
    let want: HashSet<&str> = fk_cols.iter().map(String::as_str).collect();
    if want.is_empty() {
        return false;
    }
    let pk: HashSet<&str> = child
        .columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name.as_str())
        .collect();
    if !pk.is_empty() && pk == want {
        return true;
    }
    child
        .indexes
        .iter()
        .any(|ix| ix.unique && ix.column_names().collect::<HashSet<_>>() == want)
}

/// Is the foreign key optional — i.e. any of its referencing columns nullable, so
/// a child row may reference no parent? Drives the optionality circle drawn at the
/// parent ("one") end of the edge. An unknown column (not found on `child`) is
/// treated as non-null (conservative — no false "optional").
fn fk_is_optional(child: &TableInfo, fk_cols: &[String]) -> bool {
    let want: HashSet<&str> = fk_cols.iter().map(String::as_str).collect();
    child
        .columns
        .iter()
        .any(|c| c.nullable && want.contains(c.name.as_str()))
}

/// A table's stable diagram id: its display name, so a PostgreSQL table outside
/// `public` is `schema.table` and everything else stays the bare name.
///
/// Keeping MySQL and `public` ids byte-identical matters — `diagrams.json`
/// persists manual layout positions keyed by node id, so a qualified-everything
/// scheme would silently orphan every saved layout.
fn node_id(t: &TableInfo) -> String {
    crate::schema::display_name(t.schema.as_deref(), &t.name)
}

/// The `id` of an FK's target, and whether it's a cross-*database* reference this
/// graph can't enumerate (→ a stub node).
///
/// `ForeignKeyInfo::ref_schema` means different things per engine, so the `child`
/// table decides how to read it:
/// - **PostgreSQL** (the child carries a namespace): `ref_schema` is a *namespace*,
///   and a FK can't cross databases there — so the target always resolves inside
///   this schema, whichever namespace it names.
/// - **MySQL** (no namespace): `ref_schema` is a *database*; a different one is
///   genuinely cross-database and can't be enumerated.
fn target_id(child: &TableInfo, fk: &ForeignKeyInfo, current_db: &str) -> (String, bool) {
    if child.schema.is_some() {
        return (
            crate::schema::display_name(fk.ref_schema.as_deref(), &fk.ref_table),
            false,
        );
    }
    match &fk.ref_schema {
        Some(s) if s != current_db => (format!("{s}.{}", fk.ref_table), true),
        _ => (fk.ref_table.clone(), false),
    }
}

/// Build the diagram graph for `seed` from `schema` (the schema of `current_db`).
///
/// - `Database`: every table that has at least one FK (in or out); tables with no
///   relationship go to `hidden_islands`.
/// - `Table(name)`: the named table plus every table one FK hop away. An unknown
///   table name yields an empty graph.
///
/// Edges are drawn for FKs whose *child* is in the node set; a same-database target
/// outside the set (a second hop, in the neighbourhood case) is skipped, while a
/// cross-database target adds a stub node.
pub fn build_graph(schema: &DbSchema, current_db: &str, seed: &DiagramSeed) -> DiagramGraph {
    // Keyed by *id*, not bare name: two namespaces may hold same-named tables,
    // and collapsing them loses a node and misroutes its FK edges.
    let by_id: HashMap<String, &TableInfo> =
        schema.tables.iter().map(|t| (node_id(t), t)).collect();

    // 1. Decide which real tables are in the node set.
    let included: Vec<&TableInfo> = match seed {
        DiagramSeed::Database => schema.tables.iter().collect(),
        DiagramSeed::Table(seed_id) => {
            let Some(seed_t) = by_id.get(seed_id.as_str()) else {
                return DiagramGraph {
                    total_tables: schema.tables.len(),
                    ..Default::default()
                };
            };
            let mut set: HashSet<String> = HashSet::new();
            set.insert(node_id(seed_t));
            // Tables the seed references (same-database targets only).
            for fk in &seed_t.foreign_keys {
                let (to, cross) = target_id(seed_t, fk, current_db);
                if !cross && by_id.contains_key(&to) {
                    set.insert(to);
                }
            }
            // Tables that reference the seed.
            for t in &schema.tables {
                if t.foreign_keys.iter().any(|fk| {
                    let (to, cross) = target_id(t, fk, current_db);
                    !cross && to == *seed_id
                }) {
                    set.insert(node_id(t));
                }
            }
            schema
                .tables
                .iter()
                .filter(|t| set.contains(&node_id(t)))
                .collect()
        }
    };
    let included_ids: HashSet<String> = included.iter().map(|t| node_id(t)).collect();

    // 2. Real table nodes (columns with pk/fk flags).
    let mut nodes: Vec<DiagramNode> = included.iter().map(|t| table_node(t)).collect();

    // 3. Edges (and any cross-database stub nodes they need).
    let mut edges: Vec<DiagramEdge> = Vec::new();
    let mut stubs: Vec<String> = Vec::new();
    for child in &included {
        let from_id = node_id(child);
        for fk in &child.foreign_keys {
            let (to, cross) = target_id(child, fk, current_db);
            if !included_ids.contains(&to) {
                // The target isn't a node yet. What that means depends on the
                // seed, and conflating the two cost a whole diagram.
                match seed {
                    // The node set is every table in the schema, so an
                    // unresolvable target isn't a second hop — it isn't in the
                    // schema at all. That happens whenever the schema is
                    // partial: MySQL's `information_schema.TABLES` is
                    // privilege-filtered while `KEY_COLUMN_USAGE` still reports
                    // the constraint, so `SELECT` on the child but not the
                    // parent produces exactly this. Skipping the edge also lost
                    // the *child* to the island sweep below, and the modal then
                    // reported a database with relationships as having none.
                    // A stub is what `NodeKind::Stub` already means — named,
                    // not enumerable — and the cross-database path has always
                    // done this for the identical situation.
                    DiagramSeed::Database => {
                        if !stubs.contains(&to) {
                            stubs.push(to.clone());
                        }
                    }
                    // One hop from the seed, and no further. A *neighbour's*
                    // unresolvable target is two hops, so it stays out —
                    // including a cross-database one, which used to be drawn
                    // because the `cross` arm ran before any membership test.
                    // What "one hop" means must not depend on which side of a
                    // database boundary the second hop happens to sit.
                    DiagramSeed::Table(seed_id) => {
                        if !cross || &from_id != seed_id {
                            continue;
                        }
                        if !stubs.contains(&to) {
                            stubs.push(to.clone());
                        }
                    }
                }
            }
            edges.push(DiagramEdge {
                from: node_id(child),
                from_columns: fk.columns.clone(),
                to,
                to_columns: fk.ref_columns.clone(),
                cardinality: if fk_is_unique(child, &fk.columns) {
                    Cardinality::OneToOne
                } else {
                    Cardinality::OneToMany
                },
                optional: fk_is_optional(child, &fk.columns),
            });
        }
    }
    for id in stubs {
        nodes.push(DiagramNode {
            id,
            kind: NodeKind::Stub,
            columns: Vec::new(),
        });
    }

    // 4. Island tables (Database seed only): real nodes with no incident edge.
    let mut hidden_islands: Vec<String> = Vec::new();
    if *seed == DiagramSeed::Database {
        let connected: HashSet<&str> = edges
            .iter()
            .flat_map(|e| [e.from.as_str(), e.to.as_str()])
            .collect();
        hidden_islands = nodes
            .iter()
            .filter(|n| n.kind != NodeKind::Stub && !connected.contains(n.id.as_str()))
            .map(|n| n.id.clone())
            .collect();
        hidden_islands.sort();
        let hidden: HashSet<&str> = hidden_islands.iter().map(String::as_str).collect();
        nodes.retain(|n| n.kind == NodeKind::Stub || !hidden.contains(n.id.as_str()));
    }

    DiagramGraph {
        nodes,
        edges,
        hidden_islands,
        total_tables: schema.tables.len(),
    }
}

/// Build a real table node, flagging each column PK (from schema) and FK (any
/// column named in one of the table's foreign keys).
fn table_node(t: &TableInfo) -> DiagramNode {
    let fk_cols: HashSet<&str> = t
        .foreign_keys
        .iter()
        .flat_map(|fk| fk.columns.iter().map(String::as_str))
        .collect();
    let columns = t
        .columns
        .iter()
        .map(|c| DiagramColumn {
            name: c.name.clone(),
            type_name: c.type_name.clone(),
            nullable: c.nullable,
            pk: c.primary_key,
            fk: fk_cols.contains(c.name.as_str()),
        })
        .collect();
    DiagramNode {
        id: node_id(t),
        kind: if t.is_view {
            NodeKind::View
        } else {
            NodeKind::Table
        },
        columns,
    }
}

/// Density thresholds for node collapse (all tunable starting guesses).
#[derive(Clone, Copy, Debug)]
pub struct DensityOpts {
    /// A table with this many columns or more starts collapsed.
    pub full_cols_max: usize,
    /// When the diagram has this many nodes or more, every node starts collapsed.
    pub crowded_nodes: usize,
    /// How many leading columns a collapsed node shows.
    pub collapsed_cols: usize,
}

impl Default for DensityOpts {
    fn default() -> Self {
        DensityOpts {
            full_cols_max: 15,
            crowded_nodes: 25,
            collapsed_cols: 5,
        }
    }
}

/// Whether a node should render collapsed by *default* (the user can still toggle):
/// collapse a table with too many columns, or every table once the canvas is
/// crowded. A small table on an uncrowded canvas draws in full.
pub fn should_collapse(node_col_count: usize, total_nodes: usize, opts: DensityOpts) -> bool {
    node_col_count >= opts.full_cols_max || total_nodes >= opts.crowded_nodes
}

/// The column indices a collapsed node shows: the first `collapsed_cols`, plus any
/// key (PK/FK) column beyond that cutoff pinned in, preserving schema order. The
/// count of remaining hidden columns is `cols.len() - result.len()` (the "⌄ N more"
/// expander).
pub fn collapsed_visible(cols: &[DiagramColumn], collapsed_cols: usize) -> Vec<usize> {
    cols.iter()
        .enumerate()
        .filter(|(i, c)| *i < collapsed_cols || c.is_key())
        .map(|(i, _)| i)
        .collect()
}

/// The centre y-offset (from a card's top) at which an edge for `col_names` should
/// anchor: `header_h + (row + 0.5) * row_h`, averaged over the named columns that
/// are currently *visible* (`visible` = the shown column indices from
/// [`collapsed_visible`] / the full range), or `None` if none of them is visible
/// (the FK's columns are collapsed away → the caller falls back to the card edge).
/// `header_h`/`row_h` are the UI's layout metrics, kept as parameters so this stays
/// layout-agnostic. FK and PK columns are pinned by [`collapsed_visible`], so an FK
/// edge normally anchors precisely even on a collapsed card.
pub fn column_row_offset(
    cols: &[DiagramColumn],
    visible: &[usize],
    col_names: &[String],
    header_h: f64,
    row_h: f64,
) -> Option<f64> {
    let want: HashSet<&str> = col_names.iter().map(String::as_str).collect();
    let ys: Vec<f64> = visible
        .iter()
        .enumerate()
        .filter(|&(_, &ci)| cols.get(ci).is_some_and(|c| want.contains(c.name.as_str())))
        .map(|(row, _)| header_h + (row as f64 + 0.5) * row_h)
        .collect();
    if ys.is_empty() {
        None
    } else {
        Some(ys.iter().sum::<f64>() / ys.len() as f64)
    }
}

/// Whether the edge at index `edge` (into `graph.edges`) attaches to column `col`
/// of node `node_id`: true when the node is the edge's child and `col` is one of
/// its FK columns, or the node is the parent and `col` a referenced column. Drives
/// the endpoint-row highlight when a relationship is hovered (both ends light up).
pub fn edge_touches_column(graph: &DiagramGraph, edge: usize, node_id: &str, col: &str) -> bool {
    let Some(e) = graph.edges.get(edge) else {
        return false;
    };
    (e.from == node_id && e.from_columns.iter().any(|c| c == col))
        || (e.to == node_id && e.to_columns.iter().any(|c| c == col))
}

// ── Deterministic auto-layout ──────────────────────────────────────────────
//
// A layered ("Sugiyama-lite") layout: nodes are assigned to layers by longest FK
// dependency chain (a referenced parent lands in an earlier layer than the child
// that references it), then ordered within each layer by the barycentre of their
// already-placed neighbours to reduce crossings. Fully deterministic — the same
// graph always lays out identically — so it's a stable *starting* arrangement that
// manual drag + persistence then refine. Self-loops and cycles are handled (a
// back-edge simply doesn't raise the layer), never panicking.

/// A node's place in the layer grid: its layer (horizontal band) and order within
/// that layer (vertical slot). Turned into pixels by [`place`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutCell {
    pub id: String,
    pub layer: usize,
    pub order: usize,
}

/// Longest-path layer of every node: `layer(n) = 1 + max(layer(t))` over `n`'s
/// in-graph FK targets (self-loops and back-edges in a cycle contribute nothing),
/// so a node with no outgoing FK is layer 0. Memoised; cycle-safe via a DFS stack.
fn compute_layer<'a>(
    node: &'a str,
    targets: &HashMap<&'a str, Vec<&'a str>>,
    memo: &mut HashMap<&'a str, usize>,
    on_stack: &mut HashSet<&'a str>,
) -> usize {
    if let Some(&l) = memo.get(node) {
        return l;
    }
    on_stack.insert(node);
    let mut best = 0;
    if let Some(ts) = targets.get(node) {
        for &t in ts {
            if on_stack.contains(t) {
                continue; // back-edge (cycle / self-loop) — don't raise the layer.
            }
            best = best.max(compute_layer(t, targets, memo, on_stack) + 1);
        }
    }
    on_stack.remove(node);
    memo.insert(node, best);
    best
}

/// Assign every node a [`LayoutCell`] (layer + within-layer order). Deterministic.
pub fn layout(graph: &DiagramGraph) -> Vec<LayoutCell> {
    let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    let id_set: HashSet<&str> = ids.iter().copied().collect();

    // FK targets per node: in-graph, excluding self-loops (they can't set a layer).
    let mut targets: HashMap<&str, Vec<&str>> = ids.iter().map(|&id| (id, Vec::new())).collect();
    for e in &graph.edges {
        if e.from != e.to
            && id_set.contains(e.to.as_str())
            && let Some(v) = targets.get_mut(e.from.as_str())
        {
            let t = e.to.as_str();
            if !v.contains(&t) {
                v.push(t);
            }
        }
    }

    let mut memo: HashMap<&str, usize> = HashMap::new();
    let mut layer_of: HashMap<&str, usize> = HashMap::new();
    for &id in &ids {
        let mut on_stack = HashSet::new();
        let l = compute_layer(id, &targets, &mut memo, &mut on_stack);
        layer_of.insert(id, l);
    }

    // Group by layer (BTreeMap → ascending, so lower layers order first).
    let mut by_layer: std::collections::BTreeMap<usize, Vec<&str>> =
        std::collections::BTreeMap::new();
    for &id in &ids {
        by_layer.entry(layer_of[id]).or_default().push(id);
    }

    // Order within each layer by the barycentre of targets' orders (they live in
    // lower, already-ordered layers), tie-broken by id for determinism.
    let mut order_of: HashMap<&str, usize> = HashMap::new();
    let mut cells: Vec<LayoutCell> = Vec::with_capacity(ids.len());
    for (&layer, layer_ids) in &by_layer {
        let mut sorted: Vec<&str> = layer_ids.clone();
        sorted.sort_by(|a, b| {
            let bary = |n: &str| -> f64 {
                let ts = &targets[n];
                let placed: Vec<usize> =
                    ts.iter().filter_map(|t| order_of.get(t).copied()).collect();
                if placed.is_empty() {
                    f64::INFINITY // no placed targets → fall to id order (sorts last, stable)
                } else {
                    placed.iter().sum::<usize>() as f64 / placed.len() as f64
                }
            };
            bary(a)
                .partial_cmp(&bary(b))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.cmp(b))
        });
        for (i, &id) in sorted.iter().enumerate() {
            order_of.insert(id, i);
            cells.push(LayoutCell {
                id: id.to_string(),
                layer,
                order: i,
            });
        }
    }
    cells
}

/// Pixel gaps between layers (horizontal) and stacked nodes (vertical).
#[derive(Clone, Copy, Debug)]
pub struct LayoutOpts {
    pub h_gap: f64,
    pub v_gap: f64,
}

impl Default for LayoutOpts {
    fn default() -> Self {
        LayoutOpts {
            h_gap: 64.0,
            v_gap: 28.0,
        }
    }
}

/// Absolute canvas position of a node's top-left corner.
#[derive(Clone, Debug, PartialEq)]
pub struct NodePos {
    pub id: String,
    pub x: f64,
    pub y: f64,
}

/// Turn [`LayoutCell`]s into pixel positions given each node's `(width, height)`
/// (the UI measures these from column count + collapse state). Each layer is a
/// column whose x clears the widest node of every earlier layer; within a layer,
/// nodes stack top-down by order. A node with no entry in `sizes` is treated as
/// zero-sized. Pure arithmetic.
pub fn place(
    cells: &[LayoutCell],
    sizes: &HashMap<String, (f64, f64)>,
    opts: LayoutOpts,
) -> Vec<NodePos> {
    let size = |id: &str| sizes.get(id).copied().unwrap_or((0.0, 0.0));
    let max_layer = cells.iter().map(|c| c.layer).max().unwrap_or(0);

    // x offset of each layer = sum of prior layers' max widths + gaps.
    let mut layer_x = vec![0.0_f64; max_layer + 1];
    for l in 1..=max_layer {
        let prev_max_w = cells
            .iter()
            .filter(|c| c.layer == l - 1)
            .map(|c| size(&c.id).0)
            .fold(0.0_f64, f64::max);
        layer_x[l] = layer_x[l - 1] + prev_max_w + opts.h_gap;
    }

    cells
        .iter()
        .map(|c| {
            // y = stacked heights of earlier-ordered nodes in the same layer.
            let y = cells
                .iter()
                .filter(|o| o.layer == c.layer && o.order < c.order)
                .map(|o| size(&o.id).1 + opts.v_gap)
                .sum();
            NodePos {
                id: c.id.clone(),
                x: layer_x[c.layer],
                y,
            }
        })
        .collect()
}

/// The bounding box of a laid-out diagram: the union of every sized node's rect,
/// grown by `pad` on all four sides. `None` when there is nothing to frame.
///
/// This is the **live** extent, which is the whole point: the UI's `sizes` and
/// `positions` signals are what the cards actually render from, and both change
/// under a drag and a collapse toggle. Fit used to be handed the extent captured
/// when the modal opened, so after one drag it framed an arrangement that no
/// longer existed — and since the dragged layout is what persists, it was wrong
/// from the first click on the next open too.
///
/// A node the canvas has a size for but no position sits at the origin, matching
/// how the card renders. Positions may be **negative** (dragging up/left is not
/// clamped), which is why this returns an origin as well as a size — a bare
/// `(w, h)` cannot express it.
pub fn content_bounds(
    positions: &HashMap<String, (f64, f64)>,
    sizes: &HashMap<String, (f64, f64)>,
    pad: f64,
) -> Option<Rect> {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for (id, &(w, h)) in sizes {
        let (x, y) = positions.get(id).copied().unwrap_or((0.0, 0.0));
        bounds = Some(match bounds {
            None => (x, y, x + w, y + h),
            Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + w), y1.max(y + h)),
        });
    }
    bounds.map(|(x0, y0, x1, y1)| Rect {
        x: x0 - pad,
        y: y0 - pad,
        w: (x1 - x0) + pad * 2.0,
        h: (y1 - y0) + pad * 2.0,
    })
}

/// Zoom + pan that fits `content` centred in `viewport` `(w, h)`: the largest
/// zoom in `[zoom_min, 1.0]` that shows the whole diagram — never magnifying past
/// 100% — then the pan that centres the scaled content. Drives both the "Fit"
/// control and the fit-on-open behaviour. A zero content dimension yields zoom
/// 1.0 (nothing to scale). Returns `(zoom, (pan_x, pan_y))`.
///
/// The canvas draws a card at `pan + (x, y) * zoom`, so the pan subtracts the
/// content's own scaled origin — a diagram whose top-left has been dragged into
/// negative space still lands centred.
pub fn fit_bounds(content: Rect, viewport: (f64, f64), zoom_min: f64) -> (f64, (f64, f64)) {
    let Rect { x, y, w, h } = content;
    let (vw, vh) = viewport;
    let z = if w > 0.0 && h > 0.0 {
        (vw / w).min(vh / h).clamp(zoom_min, 1.0)
    } else {
        1.0
    };
    (z, ((vw - w * z) / 2.0 - x * z, (vh - h * z) / 2.0 - y * z))
}

/// The pan that puts **one card's** centre at the centre of `viewport`, at the
/// zoom already in force — [`fit_bounds`]' single-card case, and the thing that
/// makes a find hit on an off-screen card useful at all.
///
/// The canvas draws a card at `pan + logical · zoom`, so this is that transform
/// solved for `pan`: `pan + (x + w/2)·z == vw/2`. Deliberately *centre* rather
/// than *make visible* — a card larger than the viewport still lands centred,
/// which is the reading a search wants (the name is at the card's top-left, and
/// clamping would leave the hit off screen while the readout said "1 match").
///
/// Here rather than in the effect that calls it because a sign slip, a `w` where
/// `h` belongs, or `zoom` applied to the wrong term all fail *silently*: the
/// diagram jumps somewhere useless and every readout still agrees a match was
/// found.
pub fn center_pan(viewport: (f64, f64), card: Rect, zoom: f64) -> (f64, f64) {
    let (vw, vh) = viewport;
    (
        vw / 2.0 - (card.x + card.w / 2.0) * zoom,
        vh / 2.0 - (card.y + card.h / 2.0) * zoom,
    )
}

/// Does `content` `(w, h)` overflow `viewport` `(w, h)` in either dimension — i.e.
/// would opening at 100% top-left hide part of the diagram? Gate for fit-on-open.
pub fn view_overflows(content: (f64, f64), viewport: (f64, f64)) -> bool {
    content.0 > viewport.0 || content.1 > viewport.1
}

// ── Edge geometry (SVG drawing + hover hit-test) ────────────────────────────
//
// Pure geometry shared by the UI's edge-drawing (one generated `<svg>` string)
// and the hover hit-test (Floem's `svg` view is one opaque picture with no
// per-path events, so the UI finds the hovered edge by proximity in Rust).

/// A 2-D point in canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

/// A node's rectangle in canvas pixels (top-left origin).
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Anchor points for an edge between two card rects: leave the source on the side
/// facing the target and enter the target on its facing side, both at the card's
/// vertical centre. The side is chosen purely by horizontal *centre* position, so
/// the connection stays right-edge→left-edge (or the mirror) even when the cards
/// overlap horizontally — no "both exit the same side" case, which produced a
/// jarring flip as two cards were dragged close.
///
/// Card-centre anchoring; see [`edge_anchors_rows`] for column-row-precise anchoring
/// (this is `edge_anchors_rows(from, to, None, None)`).
pub fn edge_anchors(from: Rect, to: Rect) -> (Pt, Pt) {
    edge_anchors_rows(from, to, None, None)
}

/// Column-precise anchor points: same facing-side selection as [`edge_anchors`],
/// but each end's *y* is placed at a specific column row (`from_y`/`to_y` are
/// absolute canvas y-coordinates for the FK-child / parent-PK rows) instead of the
/// card centre. A `None` end — the column is collapsed away or hidden — falls back
/// to the card's vertical centre. Each row y is clamped inside the card (with a 6px
/// inset) so it can never sit exactly on a corner.
pub fn edge_anchors_rows(from: Rect, to: Rect, from_y: Option<f64>, to_y: Option<f64>) -> (Pt, Pt) {
    let anchor_y = |r: Rect, y: Option<f64>| match y {
        Some(y) => {
            let lo = r.y + 6.0;
            let hi = (r.y + r.h - 6.0).max(lo);
            y.clamp(lo, hi)
        }
        None => r.y + r.h / 2.0,
    };
    let fy = anchor_y(from, from_y);
    let ty = anchor_y(to, to_y);
    if to.x + to.w / 2.0 >= from.x + from.w / 2.0 {
        // Target centre to the right: source right edge → target left edge.
        (
            Pt {
                x: from.x + from.w,
                y: fy,
            },
            Pt { x: to.x, y: ty },
        )
    } else {
        // Target centre to the left: source left edge → target right edge.
        (
            Pt { x: from.x, y: fy },
            Pt {
                x: to.x + to.w,
                y: ty,
            },
        )
    }
}

/// The outward horizontal direction (`+1` → the edge leaves/enters on the card's
/// right side, `-1` → its left) of each anchor from [`edge_anchors`], in the same
/// `(from, to)` order. Lets the UI draw a short straight stub off each card before
/// the curve bends, so the crow's-foot / bar markers sit on a straight segment and
/// stay symmetric even when the two cards are vertically offset.
pub fn edge_dirs(from: Rect, to: Rect) -> (f64, f64) {
    if to.x + to.w / 2.0 >= from.x + from.w / 2.0 {
        (1.0, -1.0) // source right edge → target left edge
    } else {
        (-1.0, 1.0) // source left edge → target right edge
    }
}

/// Cubic-bezier control points for the horizontal-flow curve between `p0` and `p1`.
/// Each control is pushed along that end's *outward* direction (`out0`/`out1` from
/// [`edge_dirs`]) so the curve continues the straight marker stub — its tangent
/// matches the stub (no kink) and the direction is fixed by the anchor's side, not
/// by the sign of `p1.x - p0.x` (that sign-based version flipped the curve when the
/// stub-ends crossed as two cards drew close).
///
/// The lead length per end is half the horizontal *flow toward the other end in
/// this end's outward direction*, floored at 8px. For a normal gap that's half the
/// separation (unchanged from before); when the ends are "crossed" (cards close, the
/// inward-pointing stubs overshoot) the flow is negative and it floors to 8 — a small
/// bend, not a big overshoot loop.
pub fn cubic_controls(p0: Pt, p1: Pt, out0: f64, out1: f64) -> (Pt, Pt) {
    let dx0 = ((p1.x - p0.x) * out0 * 0.5).max(8.0);
    let dx1 = ((p0.x - p1.x) * out1 * 0.5).max(8.0);
    (
        Pt {
            x: p0.x + out0 * dx0,
            y: p0.y,
        },
        Pt {
            x: p1.x + out1 * dx1,
            y: p1.y,
        },
    )
}

/// Anchor points for a *self-referencing* edge (`from == to` — e.g.
/// `employees.reports_to → employees.id`): both ends leave the **same** side of the
/// card (its right edge) at their respective column rows, instead of wrapping
/// right-edge→left-edge across the whole node (which drew an awkward loop around the
/// card). `from_y`/`to_y` are the FK-child / referenced-PK row ys (absolute canvas),
/// falling back to the card centre when a column is collapsed away. The two ys are
/// spread to a minimum separation so the loop is always a visible arc, then clamped
/// inside the card. Pair with [`self_loop_controls`] for the outward bulge.
pub fn self_loop_anchors(rect: Rect, from_y: Option<f64>, to_y: Option<f64>) -> (Pt, Pt) {
    let centre = rect.y + rect.h / 2.0;
    let mut fy = from_y.unwrap_or(centre);
    let mut ty = to_y.unwrap_or(centre);
    const MIN_SEP: f64 = 24.0;
    if (fy - ty).abs() < MIN_SEP {
        let mid = (fy + ty) / 2.0;
        fy = mid - MIN_SEP / 2.0;
        ty = mid + MIN_SEP / 2.0;
    }
    let lo = rect.y + 6.0;
    let hi = (rect.y + rect.h - 6.0).max(lo);
    let x = rect.x + rect.w;
    (
        Pt {
            x,
            y: fy.clamp(lo, hi),
        },
        Pt {
            x,
            y: ty.clamp(lo, hi),
        },
    )
}

/// Cubic control points for a self-loop between two same-side anchors, bulging
/// outward in direction `dir` (`+1` right / `-1` left). Both controls keep their
/// anchor's y and are pushed out by a `bulge` = a small base plus a gentle fraction
/// of the vertical gap, **capped** — so a tall loop (top row → bottom row of a big
/// card) stays snug against the card instead of ballooning far to the side, while a
/// short gap still reads as a rounded loop.
pub fn self_loop_controls(p0: Pt, p1: Pt, dir: f64) -> (Pt, Pt) {
    let bulge = (16.0 + (p0.y - p1.y).abs() * 0.25).min(72.0);
    (
        Pt {
            x: p0.x + dir * bulge,
            y: p0.y,
        },
        Pt {
            x: p1.x + dir * bulge,
            y: p1.y,
        },
    )
}

/// SVG `d` attribute for the cubic bezier `p0 → p1` with controls `c1`, `c2`.
pub fn cubic_path_d(p0: Pt, c1: Pt, c2: Pt, p1: Pt) -> String {
    format!(
        "M {:.1} {:.1} C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1}",
        p0.x, p0.y, c1.x, c1.y, c2.x, c2.y, p1.x, p1.y
    )
}

/// Sample a cubic bezier into `segments + 1` points (for the hit-test polyline).
pub fn sample_cubic(p0: Pt, c1: Pt, c2: Pt, p1: Pt, segments: usize) -> Vec<Pt> {
    let n = segments.max(1);
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let mt = 1.0 - t;
            let (a, b, c, d) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
            Pt {
                x: a * p0.x + b * c1.x + c * c2.x + d * p1.x,
                y: a * p0.y + b * c1.y + c * c2.y + d * p1.y,
            }
        })
        .collect()
}

/// Distance from point `p` to segment `a→b`.
pub fn dist_point_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let (abx, aby) = (b.x - a.x, b.y - a.y);
    let len2 = abx * abx + aby * aby;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * abx + (p.y - a.y) * aby) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a.x + t * abx, a.y + t * aby);
    ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt()
}

/// Index of the polyline nearest to `p` within `threshold` px (min distance over
/// its segments), or `None` if none is close enough. Used for edge hover.
pub fn nearest_polyline(p: Pt, polylines: &[Vec<Pt>], threshold: f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, poly) in polylines.iter().enumerate() {
        let d = poly
            .windows(2)
            .map(|w| dist_point_segment(p, w[0], w[1]))
            .fold(f64::INFINITY, f64::min);
        if d <= threshold && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

// ── Persisted manual layout ─────────────────────────────────────────────────
//
// After auto-layout, the user can drag nodes; their positions persist per diagram
// so the arrangement is theirs. Stored in `diagrams.json` keyed by
// `conn_id:database`. Node ids not present fall back to auto-layout; stale ids
// (a dropped table) are ignored on load.

/// One diagram's manual node positions: node id → (x, y) top-left in canvas px.
pub type NodePositions = HashMap<String, (f64, f64)>;

/// Persisted manual layouts for every diagram (`diagrams.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DiagramLayoutsFile {
    #[serde(default)]
    pub layouts: HashMap<String, NodePositions>,
}

/// Storage key for a diagram's layout.
pub fn layout_key(conn_id: u64, database: &str) -> String {
    format!("{conn_id}:{database}")
}

/// The saved positions for a diagram, if any.
pub fn get_layout<'a>(
    file: &'a DiagramLayoutsFile,
    conn_id: u64,
    database: &str,
) -> Option<&'a NodePositions> {
    file.layouts.get(&layout_key(conn_id, database))
}

/// Forget every diagram layout belonging to `conn_id` — the connection was
/// deleted, and nothing keyed to it should outlive it. Layouts are keyed by
/// `conn_id:database`, so this matches on that prefix rather than a field.
pub fn clear_conn_layouts(file: &mut DiagramLayoutsFile, conn_id: u64) {
    let prefix = format!("{conn_id}:");
    file.layouts.retain(|k, _| !k.starts_with(&prefix));
}

/// Store (replacing) a diagram's manual positions.
pub fn upsert_layout(
    file: &mut DiagramLayoutsFile,
    conn_id: u64,
    database: &str,
    positions: NodePositions,
) {
    file.layouts
        .insert(layout_key(conn_id, database), positions);
}

// ── Find-in-diagram ─────────────────────────────────────────────────────────

/// What a diagram search found inside one node.
///
/// Kept per-node rather than as a flat hit list because both things the diagram
/// does with a search are per-card: highlight the card's matched parts, and — when
/// every hit landed in one card — pan to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeMatch {
    /// The node id, which is also its card label.
    pub node: String,
    /// The table name itself matched.
    pub name: bool,
    /// The matched column names, in the node's column order. Always empty for a
    /// stub, which has no columns to search.
    pub columns: Vec<String>,
}

impl NodeMatch {
    /// How many separate things matched here: the name, plus each column.
    pub fn hits(&self) -> usize {
        usize::from(self.name) + self.columns.len()
    }
}

/// Every node a find term touches, in the graph's node order.
///
/// The term is trimmed and lower-cased here so the UI stays a thin caller, and
/// each name goes through [`crate::schema::object_name_matches`] — the same
/// predicate the schema tree's filter and Find-Anywhere use, rather than a third
/// hand-rolled `to_lowercase().contains`. An empty or whitespace-only term finds
/// nothing, which is that predicate's rule too: "no filter" is the caller's
/// separate case.
///
/// Note what this counts and the canvas may not show: a card collapsed to its key
/// columns hides the rest, so a matched column can be real but off-screen. The
/// count is the truth about the diagram, and the card outline is what says where
/// to look.
pub fn search(graph: &DiagramGraph, needle: &str) -> Vec<NodeMatch> {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    graph
        .nodes
        .iter()
        .filter_map(|n| {
            let name = crate::schema::object_name_matches(&n.id, &needle);
            let columns: Vec<String> = n
                .columns
                .iter()
                .filter(|c| crate::schema::object_name_matches(&c.name, &needle))
                .map(|c| c.name.clone())
                .collect();
            (name || !columns.is_empty()).then(|| NodeMatch {
                node: n.id.clone(),
                name,
                columns,
            })
        })
        .collect()
}

/// Everything the search found, across every node.
pub fn total_hits(matches: &[NodeMatch]) -> usize {
    matches.iter().map(NodeMatch::hits).sum()
}

/// The one card every hit landed in, or `None` when the hits span several cards
/// (or there are none).
///
/// This is the pan-and-flash trigger, and it asks about **cards, not hits**: three
/// matches inside `orders` still name one place to go, so the diagram goes there.
/// Two cards leave the choice to the user, and moving the canvas would only be
/// guessing which one was meant.
pub fn sole_node(matches: &[NodeMatch]) -> Option<&str> {
    match matches {
        [only] => Some(only.node.as_str()),
        _ => None,
    }
}

/// A search's hits with a per-node index over them — what the *cards* need.
///
/// Every card asks the same question once per keystroke ("did this search touch
/// me?"), and asking it by scanning the hit list is O(cards × matches): a
/// one-character term on a 500-card diagram matches every card, so 250,000
/// comparisons per keystroke, measured at 0.65 ms on top of the search itself and
/// before floem does anything. Built once here instead, so a card's answer is a
/// hash lookup.
///
/// It carries the hit list rather than replacing it, because the readout and the
/// pan both want it whole and in graph order ([`match_label`], [`sole_node`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Matches {
    hits: Vec<NodeMatch>,
    by_node: HashMap<String, usize>,
}

impl Matches {
    pub fn new(hits: Vec<NodeMatch>) -> Self {
        let by_node = hits
            .iter()
            .enumerate()
            .map(|(i, m)| (m.node.clone(), i))
            .collect();
        Self { hits, by_node }
    }

    /// What this search found in `node`, if anything.
    pub fn of(&self, node: &str) -> Option<&NodeMatch> {
        self.by_node.get(node).map(|i| &self.hits[*i])
    }

    /// The hits, in the graph's node order.
    pub fn hits(&self) -> &[NodeMatch] {
        &self.hits
    }
}

/// The find bar's readout: how much was found, and whether it is all in one card.
///
/// The card span is only worth saying when there is more than one — "2 matches"
/// inside a single table is already unambiguous, and the diagram will have panned
/// to it.
pub fn match_label(matches: &[NodeMatch]) -> String {
    let hits = total_hits(matches);
    match (hits, matches.len()) {
        (0, _) => "No matches".to_string(),
        (1, _) => "1 match".to_string(),
        (h, 1) => format!("{h} matches"),
        (h, t) => format!("{h} matches in {t} tables"),
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;

    fn dcol(name: &str) -> DiagramColumn {
        DiagramColumn {
            name: name.to_string(),
            type_name: "int".into(),
            nullable: false,
            pk: false,
            fk: false,
        }
    }

    fn dnode(id: &str, cols: &[&str]) -> DiagramNode {
        DiagramNode {
            id: id.to_string(),
            kind: NodeKind::Table,
            columns: cols.iter().map(|c| dcol(c)).collect(),
        }
    }

    fn graph(nodes: Vec<DiagramNode>) -> DiagramGraph {
        let total = nodes.len();
        DiagramGraph {
            nodes,
            edges: Vec::new(),
            hidden_islands: Vec::new(),
            total_tables: total,
        }
    }

    #[test]
    fn a_table_name_and_a_column_name_both_match() {
        let g = graph(vec![
            dnode("orders", &["id", "customer_id"]),
            dnode("customers", &["id", "email"]),
        ]);
        let m = search(&g, "customer");
        assert_eq!(m.len(), 2);
        // `orders` matched only on a column, `customers` only on its name.
        assert_eq!(m[0].node, "orders");
        assert!(!m[0].name);
        assert_eq!(m[0].columns, ["customer_id"]);
        assert_eq!(m[1].node, "customers");
        assert!(m[1].name);
        assert!(m[1].columns.is_empty());
    }

    /// A node whose name *and* columns match counts every one of them — the
    /// readout says how many things were found, not how many cards.
    #[test]
    fn hits_count_the_name_and_each_column_separately() {
        let g = graph(vec![dnode("orders", &["order_id", "order_date", "total"])]);
        let m = search(&g, "order");
        assert_eq!(m.len(), 1);
        assert!(m[0].name);
        assert_eq!(m[0].columns, ["order_id", "order_date"]);
        assert_eq!(m[0].hits(), 3);
        assert_eq!(total_hits(&m), 3);
    }

    #[test]
    fn matching_ignores_case_and_surrounding_space() {
        let g = graph(vec![dnode("Orders", &["Customer_ID"])]);
        assert_eq!(search(&g, "  ORDERS  ").len(), 1);
        assert_eq!(search(&g, "customer_id")[0].columns, ["Customer_ID"]);
    }

    /// An empty or whitespace-only box is "no search", not "match everything" —
    /// the same rule [`crate::schema::object_name_matches`] states.
    #[test]
    fn an_empty_needle_matches_nothing() {
        let g = graph(vec![dnode("orders", &["id"])]);
        assert!(search(&g, "").is_empty());
        assert!(search(&g, "   ").is_empty());
    }

    #[test]
    fn a_needle_nothing_contains_matches_nothing() {
        let g = graph(vec![dnode("orders", &["id"])]);
        assert!(search(&g, "zzz").is_empty());
        assert_eq!(total_hits(&[]), 0);
    }

    /// A stub is a named card on the canvas, so it answers a name search — it
    /// just has no columns to offer.
    #[test]
    fn a_stub_matches_on_its_name() {
        let mut g = graph(vec![dnode("orders", &["id"])]);
        g.nodes.push(DiagramNode {
            id: "archive.orders".into(),
            kind: NodeKind::Stub,
            columns: Vec::new(),
        });
        let m = search(&g, "archive");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].node, "archive.orders");
        assert!(m[0].name);
    }

    /// The index answers exactly what a scan of the hit list would, for every node
    /// and for one that isn't there — and keeps the list whole, in graph order, for
    /// the readout and the pan.
    #[test]
    fn the_match_index_answers_what_a_scan_would() {
        let g = graph(vec![
            dnode("orders", &["id", "created_at"]),
            dnode("customers", &["id", "created_at"]),
            dnode("items", &["sku"]),
        ]);
        let hits = search(&g, "created");
        let m = Matches::new(hits.clone());
        assert_eq!(m.hits(), hits.as_slice(), "whole, and in graph order");
        for h in &hits {
            assert_eq!(m.of(&h.node), Some(h));
        }
        assert_eq!(m.of("items"), None, "matched nothing");
        assert_eq!(m.of("nosuch"), None);
        // An empty search indexes nothing and answers nothing.
        assert_eq!(Matches::new(search(&g, "")).of("orders"), None);
    }

    /// The pan-and-flash trigger: every hit is in one card, however many hits
    /// that is. Two cards means the user still has to choose, so nothing moves.
    #[test]
    fn sole_node_is_the_one_card_every_hit_landed_in() {
        let g = graph(vec![
            dnode("orders", &["order_id", "order_date"]),
            dnode("customers", &["id"]),
        ]);
        // Three hits, one card → still sole.
        let m = search(&g, "order");
        assert_eq!(total_hits(&m), 3);
        assert_eq!(sole_node(&m), Some("orders"));
        // Two cards → no single place to go.
        let m = search(&g, "id");
        assert!(m.len() > 1);
        assert_eq!(sole_node(&m), None);
        // Nothing found → nowhere to go.
        assert_eq!(sole_node(&[]), None);
    }

    #[test]
    fn the_readout_counts_hits_and_names_the_card_span() {
        let one = vec![NodeMatch {
            node: "orders".into(),
            name: true,
            columns: vec![],
        }];
        let two_in_one = vec![NodeMatch {
            node: "orders".into(),
            name: true,
            columns: vec!["order_id".into()],
        }];
        let across = vec![
            NodeMatch {
                node: "orders".into(),
                name: true,
                columns: vec![],
            },
            NodeMatch {
                node: "customers".into(),
                name: true,
                columns: vec![],
            },
        ];
        assert_eq!(match_label(&[]), "No matches");
        assert_eq!(match_label(&one), "1 match");
        assert_eq!(match_label(&two_in_one), "2 matches");
        assert_eq!(match_label(&across), "2 matches in 2 tables");
    }
}

#[cfg(test)]
mod layout_clear_tests {
    use super::*;

    #[test]
    fn clear_conn_layouts_drops_only_that_connections_diagrams() {
        let mut file = DiagramLayoutsFile::default();
        upsert_layout(&mut file, 1, "shop", NodePositions::new());
        upsert_layout(&mut file, 1, "blog", NodePositions::new());
        upsert_layout(&mut file, 2, "shop", NodePositions::new());
        // Connection 12 must not be swept up by connection 1's prefix.
        upsert_layout(&mut file, 12, "shop", NodePositions::new());
        clear_conn_layouts(&mut file, 1);
        let mut keys: Vec<&String> = file.layouts.keys().collect();
        keys.sort();
        assert_eq!(keys, ["12:shop", "2:shop"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ColumnInfo, ForeignKeyInfo, IndexInfo};

    fn col(name: &str, ty: &str, pk: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            type_name: ty.to_string(),
            nullable: !pk,
            primary_key: pk,
            ..Default::default()
        }
    }

    fn fk(cols: &[&str], ref_table: &str, ref_cols: &[&str]) -> ForeignKeyInfo {
        ForeignKeyInfo {
            columns: cols.iter().map(|s| s.to_string()).collect(),
            ref_schema: None,
            ref_table: ref_table.to_string(),
            ref_columns: ref_cols.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn table(name: &str, cols: Vec<ColumnInfo>, fks: Vec<ForeignKeyInfo>) -> TableInfo {
        TableInfo {
            schema: None,
            name: name.to_string(),
            columns: cols,
            foreign_keys: fks,
            ..Default::default()
        }
    }

    /// customers ← orders ← orderdetails → products ; plus an island `logs`.
    fn shop() -> DbSchema {
        DbSchema {
            tables: vec![
                table("customers", vec![col("id", "int", true)], vec![]),
                table(
                    "orders",
                    vec![col("id", "int", true), col("customer_id", "int", false)],
                    vec![fk(&["customer_id"], "customers", &["id"])],
                ),
                table(
                    "orderdetails",
                    vec![col("order_id", "int", true), col("product_id", "int", true)],
                    vec![
                        fk(&["order_id"], "orders", &["id"]),
                        fk(&["product_id"], "products", &["id"]),
                    ],
                ),
                table("products", vec![col("id", "int", true)], vec![]),
                table("logs", vec![col("id", "int", true)], vec![]), // island
            ],
            ..Default::default()
        }
    }

    #[test]
    fn database_seed_hides_island_and_counts() {
        let g = build_graph(&shop(), "shop", &DiagramSeed::Database);
        assert_eq!(g.total_tables, 5);
        assert_eq!(g.hidden_islands, vec!["logs".to_string()]);
        let names: HashSet<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(!names.contains("logs"));
        assert!(names.contains("customers") && names.contains("orderdetails"));
        // 3 FKs → 3 edges (orders→customers, orderdetails→orders, orderdetails→products).
        assert_eq!(g.edges.len(), 3);
    }

    #[test]
    fn column_flags_mark_pk_and_fk() {
        let g = build_graph(&shop(), "shop", &DiagramSeed::Database);
        let orders = g.nodes.iter().find(|n| n.id == "orders").unwrap();
        let cust = orders
            .columns
            .iter()
            .find(|c| c.name == "customer_id")
            .unwrap();
        assert!(cust.fk && !cust.pk && cust.is_key());
        let id = orders.columns.iter().find(|c| c.name == "id").unwrap();
        assert!(id.pk && !id.fk);
    }

    #[test]
    fn table_seed_is_one_hop_neighbourhood() {
        // orders neighbours: customers (referenced) + orderdetails (references orders).
        let g = build_graph(&shop(), "shop", &DiagramSeed::Table("orders".into()));
        let names: HashSet<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(
            names,
            ["orders", "customers", "orderdetails"]
                .into_iter()
                .collect()
        );
        // `products` (a second hop, via orderdetails) is NOT pulled in...
        assert!(!names.contains("products"));
        // ...and orderdetails' FK to products is dropped (target out of set), so only
        // orders→customers and orderdetails→orders remain.
        assert_eq!(g.edges.len(), 2);
        // Neighbourhood never reports islands.
        assert!(g.hidden_islands.is_empty());
    }

    #[test]
    fn unknown_seed_table_yields_empty_graph_but_keeps_count() {
        let g = build_graph(&shop(), "shop", &DiagramSeed::Table("nope".into()));
        assert!(g.nodes.is_empty() && g.edges.is_empty());
        assert_eq!(g.total_tables, 5);
    }

    /// A schema where `orders.customer_id → customers.id` is declared but
    /// `customers` isn't in `schema.tables` — what MySQL hands back when the
    /// user holds `SELECT` on one table and not the other, since
    /// `information_schema.TABLES` is privilege-filtered while
    /// `KEY_COLUMN_USAGE` still reports the constraint.
    fn partial_grant() -> DbSchema {
        DbSchema {
            tables: vec![
                table(
                    "orders",
                    vec![col("id", "int", true), col("customer_id", "int", false)],
                    vec![fk(&["customer_id"], "customers", &["id"])],
                ),
                table("products", vec![col("id", "int", true)], vec![]),
            ],
            ..Default::default()
        }
    }

    /// The table that *has* a relationship was being deleted as unrelated, and
    /// the modal then said "No foreign-key relationships to diagram" over
    /// "0 tables · 0 relationships · 2 unrelated hidden". Every part wrong, with
    /// no way to tell it from a genuinely FK-free database. Reproduced on the
    /// user's instance with a MariaDB grant on `world.city` only.
    #[test]
    fn database_seed_keeps_a_table_whose_fk_target_is_missing() {
        let g = build_graph(&partial_grant(), "shop", &DiagramSeed::Database);
        assert!(
            g.nodes.iter().any(|n| n.id == "orders"),
            "the table with the relationship must survive: {:?}",
            g.nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
        );
        assert!(
            !g.hidden_islands.contains(&"orders".to_string()),
            "it is not an island — it has an edge"
        );
        assert_eq!(g.edges.len(), 1, "the relationship is drawn");
        assert_eq!(
            g.hidden_islands,
            vec!["products".to_string()],
            "only the genuinely FK-less table is hidden"
        );
    }

    /// The cross-database path already did the right thing for the identical
    /// situation, which is what made this a defect rather than a limitation:
    /// one character of `ref_schema` decided between a stub plus the edge and
    /// deleting the table outright.
    #[test]
    fn an_unresolvable_same_db_target_becomes_a_stub_like_the_cross_db_one() {
        let g = build_graph(&partial_grant(), "shop", &DiagramSeed::Database);
        let stub = g
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Stub)
            .expect("a named-but-not-enumerable target is exactly what Stub means");
        assert_eq!(stub.id, "customers");
        assert!(stub.columns.is_empty());
    }

    /// "One hop" must not depend on which side of a database boundary the
    /// second hop sits: `orderdetails` is a neighbour of `orders`, and neither
    /// its same-database FK (`products`) nor a cross-database one belongs in a
    /// one-hop diagram.
    #[test]
    fn table_seed_does_not_pull_in_a_neighbours_cross_db_stub() {
        let mut s = shop();
        let od = s
            .tables
            .iter_mut()
            .find(|t| t.name == "orderdetails")
            .unwrap();
        od.foreign_keys.push(ForeignKeyInfo {
            columns: vec!["wh".into()],
            ref_schema: Some("warehouse".into()),
            ref_table: "inventory".into(),
            ref_columns: vec!["id".into()],
            ..Default::default()
        });

        let g = build_graph(&s, "shop", &DiagramSeed::Table("orders".into()));
        let mut ids: Vec<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
        ids.sort();
        assert_eq!(
            ids,
            vec!["customers", "orderdetails", "orders"],
            "a neighbour's second hop must not be drawn, cross-database or not"
        );
    }

    /// The seed's *own* cross-database FK is one hop, and must still be drawn.
    #[test]
    fn table_seed_keeps_the_seeds_own_cross_db_stub() {
        let mut s = shop();
        let orders = s.tables.iter_mut().find(|t| t.name == "orders").unwrap();
        orders.foreign_keys.push(ForeignKeyInfo {
            columns: vec!["wh".into()],
            ref_schema: Some("warehouse".into()),
            ref_table: "inventory".into(),
            ref_columns: vec!["id".into()],
            ..Default::default()
        });
        let g = build_graph(&s, "shop", &DiagramSeed::Table("orders".into()));
        assert!(
            g.nodes.iter().any(|n| n.id == "warehouse.inventory"),
            "one hop from the seed, so it belongs"
        );
    }

    #[test]
    fn cross_database_fk_becomes_a_stub_node() {
        let mut s = DbSchema {
            tables: vec![table(
                "orders",
                vec![col("id", "int", true), col("wh", "int", false)],
                vec![ForeignKeyInfo {
                    columns: vec!["wh".into()],
                    ref_schema: Some("warehouse".into()),
                    ref_table: "inventory".into(),
                    ref_columns: vec!["id".into()],
                    ..Default::default()
                }],
            )],
            ..Default::default()
        };
        // Also add the referenced customers-less setup: orders is the only table.
        let g = build_graph(&s, "shop", &DiagramSeed::Database);
        let stub = g.nodes.iter().find(|n| n.kind == NodeKind::Stub).unwrap();
        assert_eq!(stub.id, "warehouse.inventory");
        assert!(stub.columns.is_empty());
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].to, "warehouse.inventory");
        // orders has a cross-DB relationship, so it is NOT an island.
        assert!(g.hidden_islands.is_empty());
        // A ref_schema equal to the current DB is treated as same-database.
        s.tables[0].foreign_keys[0].ref_schema = Some("shop".into());
        s.tables[0].foreign_keys[0].ref_table = "orders".into(); // self-ref, in-set
        let g2 = build_graph(&s, "shop", &DiagramSeed::Database);
        assert!(g2.nodes.iter().all(|n| n.kind != NodeKind::Stub));
    }

    #[test]
    fn cardinality_one_to_one_when_fk_is_unique() {
        // profile.user_id is the PK of profile → 1:1 with users.
        let s = DbSchema {
            tables: vec![
                table("users", vec![col("id", "int", true)], vec![]),
                table(
                    "profile",
                    vec![col("user_id", "int", true)],
                    vec![fk(&["user_id"], "users", &["id"])],
                ),
            ],
            ..Default::default()
        };
        let g = build_graph(&s, "app", &DiagramSeed::Database);
        assert_eq!(g.edges[0].cardinality, Cardinality::OneToOne);

        // Same but with a UNIQUE index instead of PK backing the FK column.
        let mut s2 = s.clone();
        s2.tables[1].columns[0].primary_key = false;
        s2.tables[1].indexes = vec![IndexInfo {
            foreign: true,
            ..IndexInfo::plain("uq", vec!["user_id"], true)
        }];
        let g2 = build_graph(&s2, "app", &DiagramSeed::Database);
        assert_eq!(g2.edges[0].cardinality, Cardinality::OneToOne);
    }

    #[test]
    fn cardinality_one_to_many_by_default() {
        let g = build_graph(&shop(), "shop", &DiagramSeed::Database);
        let e = g.edges.iter().find(|e| e.from == "orders").unwrap();
        assert_eq!(e.cardinality, Cardinality::OneToMany);
    }

    #[test]
    fn edge_optional_when_any_fk_column_nullable() {
        let g = build_graph(&shop(), "shop", &DiagramSeed::Database);
        // orders.customer_id is nullable → the orders→customers FK is optional.
        let opt = g.edges.iter().find(|e| e.from == "orders").unwrap();
        assert!(opt.optional, "nullable FK column → optional");
        // orderdetails' FK columns are its (NOT NULL) composite PK → mandatory.
        let mand = g.edges.iter().find(|e| e.from == "orderdetails").unwrap();
        assert!(!mand.optional, "NOT NULL FK columns → mandatory");
    }

    #[test]
    fn should_collapse_rule() {
        let d = DensityOpts::default(); // 15 / 25 / 5
        assert!(!should_collapse(5, 3, d), "small table, few nodes → full");
        assert!(should_collapse(20, 3, d), "wide table → collapse");
        assert!(should_collapse(5, 30, d), "crowded canvas → collapse");
        assert!(
            should_collapse(15, 1, d),
            "at the column threshold → collapse"
        );
    }

    #[test]
    fn collapsed_visible_pins_keys_past_the_cutoff() {
        let cols = vec![
            DiagramColumn {
                name: "a".into(),
                type_name: "int".into(),
                nullable: false,
                pk: true,
                fk: false,
            },
            DiagramColumn {
                name: "b".into(),
                type_name: "int".into(),
                nullable: true,
                pk: false,
                fk: false,
            },
            DiagramColumn {
                name: "c".into(),
                type_name: "int".into(),
                nullable: true,
                pk: false,
                fk: false,
            },
            DiagramColumn {
                name: "d".into(),
                type_name: "int".into(),
                nullable: true,
                pk: false,
                fk: false,
            },
            // Beyond the cutoff of 3, but an FK → pinned in.
            DiagramColumn {
                name: "e_fk".into(),
                type_name: "int".into(),
                nullable: true,
                pk: false,
                fk: true,
            },
            DiagramColumn {
                name: "f".into(),
                type_name: "int".into(),
                nullable: true,
                pk: false,
                fk: false,
            },
        ];
        let vis = collapsed_visible(&cols, 3);
        assert_eq!(vis, vec![0, 1, 2, 4], "first 3 + the pinned FK at index 4");
        assert_eq!(
            cols.len() - vis.len(),
            2,
            "b-count for the ⌄ N more expander"
        );
    }

    // ── layout ──

    /// A bare graph: node ids + `from → to` edges (child references parent). No
    /// columns needed for layout.
    fn graph_of(ids: &[&str], edges: &[(&str, &str)]) -> DiagramGraph {
        DiagramGraph {
            nodes: ids
                .iter()
                .map(|id| DiagramNode {
                    id: id.to_string(),
                    kind: NodeKind::Table,
                    columns: Vec::new(),
                })
                .collect(),
            edges: edges
                .iter()
                .map(|(f, t)| DiagramEdge {
                    from: f.to_string(),
                    from_columns: vec![],
                    to: t.to_string(),
                    to_columns: vec![],
                    cardinality: Cardinality::OneToMany,
                    optional: false,
                })
                .collect(),
            hidden_islands: vec![],
            total_tables: ids.len(),
        }
    }

    fn layer(cells: &[LayoutCell], id: &str) -> usize {
        cells.iter().find(|c| c.id == id).unwrap().layer
    }

    #[test]
    fn layout_chain_layers_by_dependency_depth() {
        // a → b → c : c is the ultimate parent (layer 0), a the deepest child.
        let cells = layout(&graph_of(&["a", "b", "c"], &[("a", "b"), ("b", "c")]));
        assert_eq!(layer(&cells, "c"), 0);
        assert_eq!(layer(&cells, "b"), 1);
        assert_eq!(layer(&cells, "a"), 2);
    }

    #[test]
    fn layout_diamond_uses_longest_path() {
        // a→b, a→c, b→d, c→d : d=0, b=c=1, a=2 (longest path a→b→d = a→c→d = 2).
        let cells = layout(&graph_of(
            &["a", "b", "c", "d"],
            &[("a", "b"), ("a", "c"), ("b", "d"), ("c", "d")],
        ));
        assert_eq!(layer(&cells, "d"), 0);
        assert_eq!(layer(&cells, "b"), 1);
        assert_eq!(layer(&cells, "c"), 1);
        assert_eq!(layer(&cells, "a"), 2);
    }

    #[test]
    fn layout_handles_cycles_and_self_loops_without_panicking() {
        // 2-cycle a↔b and a self-loop on c: must terminate with finite layers.
        let cells = layout(&graph_of(
            &["a", "b", "c"],
            &[("a", "b"), ("b", "a"), ("c", "c")],
        ));
        assert_eq!(cells.len(), 3);
        assert_eq!(
            layer(&cells, "c"),
            0,
            "self-loop doesn't raise its own layer"
        );
        // The cycle resolves to adjacent layers (one back-edge broken), not infinity.
        assert!(layer(&cells, "a") <= 1 && layer(&cells, "b") <= 1);
    }

    #[test]
    fn layout_disconnected_components_are_independent() {
        // a→b, and a lone c.
        let cells = layout(&graph_of(&["a", "b", "c"], &[("a", "b")]));
        assert_eq!(layer(&cells, "b"), 0);
        assert_eq!(layer(&cells, "a"), 1);
        assert_eq!(layer(&cells, "c"), 0, "island sits in the base layer");
    }

    #[test]
    fn layout_orders_within_a_layer_deterministically() {
        // Two children a, b both referencing parent p: p in layer 0, a & b in
        // layer 1 ordered by id (both share the same single barycentre).
        let cells = layout(&graph_of(&["b", "a", "p"], &[("a", "p"), ("b", "p")]));
        let a = cells.iter().find(|c| c.id == "a").unwrap();
        let b = cells.iter().find(|c| c.id == "b").unwrap();
        assert_eq!(a.layer, 1);
        assert_eq!(b.layer, 1);
        assert!(a.order < b.order, "tie broken by id: a before b");
        // Deterministic: a second run gives the identical result.
        assert_eq!(
            cells,
            layout(&graph_of(&["b", "a", "p"], &[("a", "p"), ("b", "p")]))
        );
    }

    #[test]
    fn place_lays_layers_left_to_right_and_stacks_by_order() {
        let cells = vec![
            LayoutCell {
                id: "p".into(),
                layer: 0,
                order: 0,
            },
            LayoutCell {
                id: "a".into(),
                layer: 1,
                order: 0,
            },
            LayoutCell {
                id: "b".into(),
                layer: 1,
                order: 1,
            },
        ];
        let sizes: HashMap<String, (f64, f64)> = [
            ("p".to_string(), (100.0, 60.0)),
            ("a".to_string(), (100.0, 40.0)),
            ("b".to_string(), (100.0, 40.0)),
        ]
        .into_iter()
        .collect();
        let pos = place(
            &cells,
            &sizes,
            LayoutOpts {
                h_gap: 50.0,
                v_gap: 20.0,
            },
        );
        let at = |id: &str| pos.iter().find(|p| p.id == id).unwrap();
        // Layer 0 at x=0; layer 1 clears p's width (100) + gap (50) = 150.
        assert_eq!(at("p").x, 0.0);
        assert_eq!(at("a").x, 150.0);
        assert_eq!(at("b").x, 150.0);
        // Within layer 1: a at y=0, b below by a's height (40) + gap (20) = 60.
        assert_eq!(at("a").y, 0.0);
        assert_eq!(at("b").y, 60.0);
    }

    // ── fit-to-view ──

    fn origin(w: f64, h: f64) -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            w,
            h,
        }
    }

    #[test]
    fn fit_bounds_never_magnifies_and_centres_small_content() {
        // Content smaller than the viewport → zoom stays 1.0 (no magnify), centred.
        let (z, (px, py)) = fit_bounds(origin(200.0, 100.0), (800.0, 600.0), 0.25);
        assert_eq!(z, 1.0);
        assert_eq!((px, py), (300.0, 250.0));
    }

    #[test]
    fn fit_bounds_scales_down_overflowing_content() {
        // Wide content: limited by the width ratio 800/1600 = 0.5; centred vertically.
        let (z, (px, py)) = fit_bounds(origin(1600.0, 600.0), (800.0, 600.0), 0.25);
        assert_eq!(z, 0.5);
        assert_eq!((px, py), (0.0, 150.0));
        // Huge content clamps at the zoom floor rather than going smaller.
        let (z2, _) = fit_bounds(origin(8000.0, 8000.0), (800.0, 600.0), 0.25);
        assert_eq!(z2, 0.25);
    }

    #[test]
    fn fit_bounds_handles_zero_content() {
        // No content → zoom 1.0, no NaN/inf.
        let (z, (px, py)) = fit_bounds(origin(0.0, 0.0), (800.0, 600.0), 0.25);
        assert_eq!(z, 1.0);
        assert_eq!((px, py), (400.0, 300.0));
    }

    /// The reason `fit_view((w, h), …)` was replaced: a diagram dragged up and to
    /// the left has a negative origin, and centring its *size* leaves it off
    /// screen. The card at the content's top-left must land at the same viewport
    /// point it would have if the diagram started at (0, 0).
    #[test]
    fn fit_bounds_centres_content_whose_origin_is_not_zero() {
        let shifted = Rect {
            x: -500.0,
            y: -200.0,
            w: 200.0,
            h: 100.0,
        };
        let (z, (px, py)) = fit_bounds(shifted, (800.0, 600.0), 0.25);
        assert_eq!(z, 1.0);
        // A card at the content origin draws at pan + origin*z — dead centre.
        assert_eq!((px + shifted.x * z, py + shifted.y * z), (300.0, 250.0));
    }

    // ── centring one card ──
    //
    // Where the user is looking after a search. Every way of getting this wrong —
    // a sign, a `w` for an `h`, `zoom` on the wrong term — leaves the sole match
    // off screen while the readout still says "1 match", so each case below
    // re-derives the transform the canvas actually draws with rather than
    // restating the formula.

    /// The card's centre lands at the viewport's centre, at zoom 1.
    #[test]
    fn center_pan_puts_a_cards_centre_in_the_middle() {
        let card = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 100.0,
        };
        let (px, py) = center_pan((800.0, 600.0), card, 1.0);
        assert_eq!((px, py), (300.0, 250.0));
        // Re-derived: the canvas draws the card at `pan + logical · zoom`.
        assert_eq!(px + (card.x + card.w / 2.0), 400.0);
        assert_eq!(py + (card.y + card.h / 2.0), 300.0);
    }

    /// Zoom scales the card's position *and* its size, and both terms are on the
    /// same side of the solve — asserted at each end of the zoom range.
    #[test]
    fn center_pan_centres_the_scaled_card() {
        let card = Rect {
            x: 640.0,
            y: 480.0,
            w: 240.0,
            h: 120.0,
        };
        for z in [0.4_f64, 1.0, 2.0] {
            let (px, py) = center_pan((800.0, 600.0), card, z);
            assert!(
                (px + (card.x + card.w / 2.0) * z - 400.0).abs() < 1e-9,
                "z = {z}"
            );
            assert!(
                (py + (card.y + card.h / 2.0) * z - 300.0).abs() < 1e-9,
                "z = {z}"
            );
        }
    }

    /// The canvas allows negative logical positions (a diagram dragged up and
    /// left), so a card there has to centre like any other.
    #[test]
    fn center_pan_handles_a_negative_position() {
        let card = Rect {
            x: -900.0,
            y: -300.0,
            w: 200.0,
            h: 100.0,
        };
        let (px, py) = center_pan((800.0, 600.0), card, 1.0);
        assert_eq!(
            (px + card.x + card.w / 2.0, py + card.y + card.h / 2.0),
            (400.0, 300.0)
        );
    }

    /// **Centre, not clamp.** A card taller than the viewport still lands centred:
    /// its name is at the top-left, and this is the case where "centre" and "make
    /// visible" differ — `fit_bounds` answers it by zooming out, which a search
    /// deliberately does not do.
    #[test]
    fn center_pan_centres_a_card_bigger_than_the_viewport() {
        let card = Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 2000.0,
        };
        let (px, py) = center_pan((800.0, 600.0), card, 1.0);
        assert_eq!((px, py), (250.0, -700.0));
        assert!(py < 0.0, "the card's top is above the viewport, centred");
    }

    // ── live content bounds ──

    fn xy(pairs: &[(&str, f64, f64)]) -> HashMap<String, (f64, f64)> {
        pairs
            .iter()
            .map(|(id, a, b)| (id.to_string(), (*a, *b)))
            .collect()
    }

    #[test]
    fn content_bounds_covers_every_card_plus_the_pad() {
        let pos = xy(&[("a", 0.0, 0.0), ("b", 300.0, 100.0)]);
        let sizes = xy(&[("a", 200.0, 80.0), ("b", 200.0, 80.0)]);
        let b = content_bounds(&pos, &sizes, 40.0).expect("two cards");
        assert_eq!((b.x, b.y), (-40.0, -40.0));
        assert_eq!((b.w, b.h), (500.0 + 80.0, 180.0 + 80.0));
    }

    /// The bug this replaced: Fit used the extent captured when the modal opened,
    /// so a card dragged far to the right stayed outside the frame.
    #[test]
    fn content_bounds_follows_a_dragged_node() {
        let sizes = xy(&[("a", 200.0, 80.0), ("b", 200.0, 80.0)]);
        let before = content_bounds(&xy(&[("a", 0.0, 0.0), ("b", 300.0, 0.0)]), &sizes, 0.0);
        let after = content_bounds(&xy(&[("a", 0.0, 0.0), ("b", 1800.0, 0.0)]), &sizes, 0.0);
        assert_eq!(before.unwrap().w, 500.0);
        assert_eq!(after.unwrap().w, 2000.0);
    }

    #[test]
    fn content_bounds_covers_negative_positions() {
        let b = content_bounds(
            &xy(&[("a", -300.0, -150.0), ("b", 100.0, 0.0)]),
            &xy(&[("a", 200.0, 80.0), ("b", 200.0, 80.0)]),
            0.0,
        )
        .expect("two cards");
        assert_eq!((b.x, b.y), (-300.0, -150.0));
        assert_eq!((b.w, b.h), (600.0, 230.0));
    }

    /// A card the canvas has a size for but no position renders at the origin, so
    /// the bounds have to include it there rather than skipping it.
    #[test]
    fn content_bounds_places_an_unpositioned_card_at_the_origin() {
        let b = content_bounds(&HashMap::new(), &xy(&[("a", 200.0, 80.0)]), 0.0).expect("one card");
        assert_eq!((b.x, b.y, b.w, b.h), (0.0, 0.0, 200.0, 80.0));
    }

    #[test]
    fn content_bounds_is_none_when_there_is_nothing_to_frame() {
        assert!(content_bounds(&xy(&[("a", 10.0, 10.0)]), &HashMap::new(), 40.0).is_none());
    }

    #[test]
    fn view_overflows_only_when_content_exceeds_viewport() {
        assert!(!view_overflows((200.0, 100.0), (800.0, 600.0)), "fits");
        assert!(view_overflows((900.0, 100.0), (800.0, 600.0)), "too wide");
        assert!(view_overflows((200.0, 700.0), (800.0, 600.0)), "too tall");
        assert!(
            !view_overflows((800.0, 600.0), (800.0, 600.0)),
            "exact fit is not overflow"
        );
    }

    // ── edge geometry ──

    fn pt(x: f64, y: f64) -> Pt {
        Pt { x, y }
    }

    #[test]
    fn edge_anchors_pick_facing_sides() {
        let from = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 40.0,
        };
        let right = Rect {
            x: 200.0,
            y: 0.0,
            w: 100.0,
            h: 40.0,
        };
        let (a, b) = edge_anchors(from, right);
        assert_eq!(a, pt(100.0, 20.0), "leaves source right edge at its centre");
        assert_eq!(b, pt(200.0, 20.0), "enters target left edge at its centre");
        // Target to the left mirrors the sides.
        let left = Rect {
            x: -200.0,
            y: 0.0,
            w: 100.0,
            h: 40.0,
        };
        let (a2, b2) = edge_anchors(from, left);
        assert_eq!(a2, pt(0.0, 20.0));
        assert_eq!(b2, pt(-100.0, 20.0));
    }

    #[test]
    fn edge_anchors_rows_place_ends_at_specific_rows() {
        let from = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        let to = Rect {
            x: 200.0,
            y: 40.0,
            w: 100.0,
            h: 100.0,
        };
        // Both ends anchored at a given row y (absolute canvas coords).
        let (a, b) = edge_anchors_rows(from, to, Some(18.0), Some(70.0));
        assert_eq!(a, pt(100.0, 18.0), "leaves source right edge at the row y");
        assert_eq!(b, pt(200.0, 70.0), "enters target left edge at the row y");
        // A `None` end falls back to that card's vertical centre.
        let (a2, b2) = edge_anchors_rows(from, to, None, Some(70.0));
        assert_eq!(a2, pt(100.0, 50.0), "None → source card centre");
        assert_eq!(b2, pt(200.0, 70.0));
        // With both None it matches the card-centre `edge_anchors`.
        assert_eq!(
            edge_anchors_rows(from, to, None, None),
            edge_anchors(from, to)
        );
    }

    #[test]
    fn edge_anchors_rows_clamp_y_inside_the_card() {
        let from = Rect {
            x: 0.0,
            y: 10.0,
            w: 80.0,
            h: 40.0,
        };
        let to = Rect {
            x: 200.0,
            y: 10.0,
            w: 80.0,
            h: 40.0,
        };
        // A y above/below the card is clamped to the 6px inset band [16, 44].
        let (a, _) = edge_anchors_rows(from, to, Some(-100.0), None);
        assert_eq!(a.y, 16.0, "clamped to top inset");
        let (a2, _) = edge_anchors_rows(from, to, Some(999.0), None);
        assert_eq!(a2.y, 44.0, "clamped to bottom inset");
    }

    #[test]
    fn column_row_offset_averages_visible_named_rows() {
        let cols = vec![
            DiagramColumn {
                name: "id".into(),
                type_name: "int".into(),
                nullable: false,
                pk: true,
                fk: false,
            },
            DiagramColumn {
                name: "a_id".into(),
                type_name: "int".into(),
                nullable: true,
                pk: false,
                fk: true,
            },
            DiagramColumn {
                name: "b_id".into(),
                type_name: "int".into(),
                nullable: true,
                pk: false,
                fk: true,
            },
        ];
        let visible = vec![0, 1, 2];
        // header 30, row 24 → row centres at 42, 66, 90.
        assert_eq!(
            column_row_offset(&cols, &visible, &["id".into()], 30.0, 24.0),
            Some(42.0)
        );
        // Composite FK over rows 1 & 2 → average of 66 and 90 = 78.
        assert_eq!(
            column_row_offset(&cols, &visible, &["a_id".into(), "b_id".into()], 30.0, 24.0),
            Some(78.0)
        );
        // A column collapsed away (not in `visible`) → None (fall back to card edge).
        assert_eq!(
            column_row_offset(&cols, &[0], &["a_id".into()], 30.0, 24.0),
            None
        );
        // An unknown column name → None.
        assert_eq!(
            column_row_offset(&cols, &visible, &["nope".into()], 30.0, 24.0),
            None
        );
    }

    #[test]
    fn column_row_offset_uses_visible_position_not_schema_index() {
        // Same cols as above, but only id (0) and b_id (2) are visible — b_id is the
        // *second* visible row, so its centre is 30 + 1.5*24 = 66, not row-2's 90.
        let cols = vec![
            DiagramColumn {
                name: "id".into(),
                type_name: "int".into(),
                nullable: false,
                pk: true,
                fk: false,
            },
            DiagramColumn {
                name: "a_id".into(),
                type_name: "int".into(),
                nullable: true,
                pk: false,
                fk: true,
            },
            DiagramColumn {
                name: "b_id".into(),
                type_name: "int".into(),
                nullable: true,
                pk: false,
                fk: true,
            },
        ];
        assert_eq!(
            column_row_offset(&cols, &[0, 2], &["b_id".into()], 30.0, 24.0),
            Some(66.0)
        );
    }

    #[test]
    fn edge_touches_column_marks_both_endpoints() {
        // orders.user_id (FK) → users.id (referenced).
        let graph = graph_of(&["orders", "users"], &[("orders", "users")]);
        let mut graph = graph;
        graph.edges[0].from_columns = vec!["user_id".into()];
        graph.edges[0].to_columns = vec!["id".into()];
        // Child end: the FK column on the child node.
        assert!(edge_touches_column(&graph, 0, "orders", "user_id"));
        // Parent end: the referenced column on the parent node.
        assert!(edge_touches_column(&graph, 0, "users", "id"));
        // Not the child's PK, not the parent's other columns, not swapped roles.
        assert!(!edge_touches_column(&graph, 0, "orders", "id"));
        assert!(!edge_touches_column(&graph, 0, "users", "user_id"));
        // Out-of-range edge index → false, never panics.
        assert!(!edge_touches_column(&graph, 9, "orders", "user_id"));
    }

    #[test]
    fn edge_dirs_match_the_facing_sides() {
        let from = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 40.0,
        };
        let right = Rect {
            x: 200.0,
            y: 0.0,
            w: 100.0,
            h: 40.0,
        };
        // Source leaves rightward (+1), target entered from its left (outward -1).
        assert_eq!(edge_dirs(from, right), (1.0, -1.0));
        let left = Rect { x: -200.0, ..right };
        assert_eq!(edge_dirs(from, left), (-1.0, 1.0));
        // Horizontally overlapping cards decide by centre (no "both exit the same
        // side" flip): this one's centre (90) is right of `from`'s (50) → right→left.
        let overlap = Rect {
            x: 40.0,
            y: 200.0,
            w: 100.0,
            h: 40.0,
        };
        assert_eq!(edge_dirs(from, overlap), (1.0, -1.0));
    }

    #[test]
    fn sample_cubic_hits_both_endpoints() {
        let (p0, p1) = (pt(0.0, 0.0), pt(90.0, 30.0));
        // p0 exits right (+1), p1 entered from its left (-1).
        let (c1, c2) = cubic_controls(p0, p1, 1.0, -1.0);
        let pts = sample_cubic(p0, c1, c2, p1, 16);
        assert_eq!(pts.len(), 17);
        assert_eq!(*pts.first().unwrap(), p0);
        assert_eq!(*pts.last().unwrap(), p1);
    }

    #[test]
    fn self_loop_anchors_share_the_right_side_and_spread() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 200.0,
        };
        // Two distinct rows → anchors sit at those ys on the card's right edge.
        let (p0, p1) = self_loop_anchors(rect, Some(50.0), Some(120.0));
        assert_eq!(p0, pt(100.0, 50.0));
        assert_eq!(p1, pt(100.0, 120.0));
        // Collapsed (both None) → spread symmetrically around the card centre so the
        // loop is a visible arc rather than a zero-height sliver.
        let (a, b) = self_loop_anchors(rect, None, None);
        assert_eq!(a.x, 100.0);
        assert_eq!(b.x, 100.0);
        assert!((a.y - b.y).abs() >= 24.0, "spread to a visible separation");
        assert!(
            ((a.y + b.y) / 2.0 - 100.0).abs() < 1e-9,
            "centred on the card"
        );
    }

    #[test]
    fn self_loop_controls_bulge_outward() {
        let (p0, p1) = (pt(100.0, 40.0), pt(100.0, 100.0));
        let (c1, c2) = self_loop_controls(p0, p1, 1.0);
        // Both controls pushed right of the anchors, each keeping its anchor's y.
        assert!(c1.x > p0.x && c2.x > p1.x);
        assert_eq!(c1.y, 40.0);
        assert_eq!(c2.y, 100.0);
        // Bulge = 16 + 0.25*gap: gap 60 → 16 + 15 = 31.
        assert!((c1.x - (100.0 + 31.0)).abs() < 1e-9);
        // A very tall gap is capped at 72 so the loop stays near the card.
        let (c1big, _) = self_loop_controls(pt(100.0, 0.0), pt(100.0, 400.0), 1.0);
        assert!((c1big.x - (100.0 + 72.0)).abs() < 1e-9);
        // Leftward direction mirrors.
        let (c1l, _) = self_loop_controls(p0, p1, -1.0);
        assert!(c1l.x < p0.x);
    }

    #[test]
    fn cubic_path_d_formats_move_and_curve() {
        let d = cubic_path_d(pt(1.0, 2.0), pt(3.0, 4.0), pt(5.0, 6.0), pt(7.0, 8.0));
        assert_eq!(d, "M 1.0 2.0 C 3.0 4.0, 5.0 6.0, 7.0 8.0");
    }

    #[test]
    fn dist_point_segment_cases() {
        let (a, b) = (pt(0.0, 0.0), pt(10.0, 0.0));
        assert!(
            (dist_point_segment(pt(5.0, 0.0), a, b) - 0.0).abs() < 1e-9,
            "on segment"
        );
        assert!(
            (dist_point_segment(pt(5.0, 3.0), a, b) - 3.0).abs() < 1e-9,
            "perpendicular"
        );
        assert!(
            (dist_point_segment(pt(-4.0, 0.0), a, b) - 4.0).abs() < 1e-9,
            "clamps past the start endpoint"
        );
    }

    #[test]
    fn nearest_polyline_picks_closest_within_threshold() {
        // Two horizontal polylines at y=0 and y=100.
        let near = vec![pt(0.0, 0.0), pt(100.0, 0.0)];
        let far = vec![pt(0.0, 100.0), pt(100.0, 100.0)];
        let polys = vec![near, far];
        assert_eq!(nearest_polyline(pt(50.0, 3.0), &polys, 6.0), Some(0));
        assert_eq!(nearest_polyline(pt(50.0, 97.0), &polys, 6.0), Some(1));
        // Equidistant-ish but only one within threshold.
        assert_eq!(
            nearest_polyline(pt(50.0, 50.0), &polys, 6.0),
            None,
            "too far from both"
        );
    }

    // ── persisted layout ──

    #[test]
    fn layout_key_is_conn_and_db() {
        assert_eq!(layout_key(7, "shop"), "7:shop");
        assert_ne!(layout_key(7, "shop"), layout_key(8, "shop"));
        assert_ne!(layout_key(7, "shop"), layout_key(7, "warehouse"));
    }

    #[test]
    fn upsert_then_get_roundtrips_and_is_isolated() {
        let mut f = DiagramLayoutsFile::default();
        let mut a: NodePositions = HashMap::new();
        a.insert("orders".into(), (10.0, 20.0));
        upsert_layout(&mut f, 1, "shop", a);
        upsert_layout(&mut f, 2, "shop", HashMap::new());

        assert_eq!(
            get_layout(&f, 1, "shop").unwrap().get("orders"),
            Some(&(10.0, 20.0))
        );
        // A different connection with the same db name is a separate entry.
        assert!(get_layout(&f, 2, "shop").unwrap().is_empty());
        assert!(get_layout(&f, 9, "shop").is_none());
    }

    #[test]
    fn upsert_replaces_existing_layout() {
        let mut f = DiagramLayoutsFile::default();
        let mut a: NodePositions = HashMap::new();
        a.insert("t".into(), (1.0, 1.0));
        upsert_layout(&mut f, 1, "d", a);
        let mut b: NodePositions = HashMap::new();
        b.insert("t".into(), (2.0, 2.0));
        upsert_layout(&mut f, 1, "d", b);
        assert_eq!(get_layout(&f, 1, "d").unwrap().get("t"), Some(&(2.0, 2.0)));
    }

    #[test]
    fn layouts_file_json_roundtrips() {
        let mut f = DiagramLayoutsFile::default();
        let mut a: NodePositions = HashMap::new();
        a.insert("orders".into(), (10.5, 20.5));
        upsert_layout(&mut f, 1, "shop", a);
        let json = serde_json::to_string(&f).unwrap();
        let back: DiagramLayoutsFile = serde_json::from_str(&json).unwrap();
        assert_eq!(
            get_layout(&back, 1, "shop").unwrap().get("orders"),
            Some(&(10.5, 20.5))
        );
    }
}

#[cfg(test)]
mod multi_schema_tests {
    use super::*;
    use crate::schema::{ColumnInfo, ForeignKeyInfo};

    fn col(name: &str, pk: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            type_name: "integer".to_string(),
            nullable: !pk,
            primary_key: pk,
            ..Default::default()
        }
    }

    /// A PostgreSQL table in an explicit namespace.
    fn pg(ns: &str, name: &str, cols: &[&str], fks: Vec<ForeignKeyInfo>) -> TableInfo {
        TableInfo {
            name: name.to_string(),
            schema: Some(ns.to_string()),
            columns: cols.iter().map(|c| col(c, *c == "id")).collect(),
            foreign_keys: fks,
            ..Default::default()
        }
    }

    /// An FK whose target names a namespace (the PostgreSQL shape).
    fn pg_fk(cols: &[&str], ref_ns: &str, ref_table: &str, ref_cols: &[&str]) -> ForeignKeyInfo {
        ForeignKeyInfo {
            columns: cols.iter().map(|s| s.to_string()).collect(),
            ref_schema: Some(ref_ns.to_string()),
            ref_table: ref_table.to_string(),
            ref_columns: ref_cols.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    /// The `warehouse` fixture's shape: a same-named `orders` in two namespaces,
    /// plus a cross-schema FK from `analytics` into `sales`.
    fn warehouse() -> DbSchema {
        DbSchema {
            tables: vec![
                pg("public", "orders", &["id", "legacy_ref"], vec![]),
                pg("sales", "customers", &["id", "name"], vec![]),
                pg(
                    "sales",
                    "orders",
                    &["id", "customer_id"],
                    vec![pg_fk(&["customer_id"], "sales", "customers", &["id"])],
                ),
                pg(
                    "analytics",
                    "daily_revenue",
                    &["id", "order_id"],
                    vec![pg_fk(&["order_id"], "sales", "orders", &["id"])],
                ),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn same_named_tables_in_two_schemas_are_separate_nodes() {
        let g = build_graph(&warehouse(), "warehouse", &DiagramSeed::Database);
        let ids: Vec<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
        // `public.orders` is an island (no FKs) so it's hidden, but `sales.orders`
        // must be its own node — keyed by namespace, not collapsed onto the other.
        assert!(ids.contains(&"sales.orders"), "ids: {ids:?}");
        assert!(ids.contains(&"sales.customers"), "ids: {ids:?}");
        assert!(
            ids.iter().all(|i| *i != "orders"),
            "a bare `orders` id means the two namespaces collapsed: {ids:?}"
        );
        // Every id is unique — the collapse would have silently deduped one away.
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate node ids: {ids:?}");
    }

    #[test]
    fn a_cross_schema_fk_links_the_real_node_not_a_stub() {
        // `ref_schema` is a *namespace* on Postgres, and a FK can't cross
        // databases there — so this must resolve to the real `sales.orders`
        // node, not become an unexpandable cross-database stub.
        let g = build_graph(&warehouse(), "warehouse", &DiagramSeed::Database);
        let e = g
            .edges
            .iter()
            .find(|e| e.from == "analytics.daily_revenue")
            .expect("the cross-schema FK is drawn");
        assert_eq!(e.to, "sales.orders");
        assert!(
            g.nodes.iter().all(|n| n.kind != NodeKind::Stub),
            "no stub should be created for a same-database namespace hop"
        );
    }

    #[test]
    fn a_qualified_seed_picks_the_right_same_named_table() {
        // Seeding on `sales.orders` must not pull in `public.orders`.
        let g = build_graph(
            &warehouse(),
            "warehouse",
            &DiagramSeed::Table("sales.orders".to_string()),
        );
        let ids: Vec<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"sales.orders"), "{ids:?}");
        // Its FK neighbours, both directions.
        assert!(ids.contains(&"sales.customers"), "{ids:?}");
        assert!(ids.contains(&"analytics.daily_revenue"), "{ids:?}");
        // Not the unrelated same-named table in `public`.
        assert!(!ids.contains(&"orders"), "{ids:?}");
    }

    #[test]
    fn mysql_node_ids_stay_bare_names() {
        // No namespaces → ids are exactly what they always were, so persisted
        // diagram layouts (keyed by node id) keep applying.
        let s = DbSchema {
            tables: vec![
                TableInfo {
                    name: "orders".into(),
                    columns: vec![col("id", true), col("cust", false)],
                    foreign_keys: vec![ForeignKeyInfo {
                        columns: vec!["cust".into()],
                        ref_schema: None,
                        ref_table: "customers".into(),
                        ref_columns: vec!["id".into()],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                TableInfo {
                    name: "customers".into(),
                    columns: vec![col("id", true)],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let g = build_graph(&s, "shop", &DiagramSeed::Database);
        let mut ids: Vec<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["customers", "orders"]);
        assert_eq!(g.edges[0].from, "orders");
        assert_eq!(g.edges[0].to, "customers");
    }

    #[test]
    fn a_mysql_cross_database_fk_still_stubs() {
        // The MySQL case `target_id` exists for must keep working: `ref_schema`
        // there is a *database*, and a different one can't be enumerated.
        let s = DbSchema {
            tables: vec![TableInfo {
                name: "orders".into(),
                columns: vec![col("id", true), col("cust", false)],
                foreign_keys: vec![ForeignKeyInfo {
                    columns: vec!["cust".into()],
                    ref_schema: Some("other_db".into()),
                    ref_table: "customers".into(),
                    ref_columns: vec!["id".into()],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let g = build_graph(&s, "shop", &DiagramSeed::Database);
        assert!(
            g.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Stub && n.id == "other_db.customers")
        );
        assert_eq!(g.edges[0].to, "other_db.customers");
    }
}
