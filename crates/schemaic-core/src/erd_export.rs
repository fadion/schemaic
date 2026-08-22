//! ER-diagram export: turn a built [`DiagramGraph`] into a file the diagram can
//! leave the app in.
//!
//! Two families, and they answer different questions:
//!
//! - **Picture** — [`to_svg`] renders the canvas the user arranged: the same node
//!   rects, the same flattened edge curves, the same crow's-foot markers. It takes
//!   a [`SvgScene`] the UI fills from its live signals rather than re-deriving
//!   geometry, so the export cannot drift from what is on screen. PNG is that SVG
//!   rasterised (in the UI crate, where the fonts live) — there is deliberately no
//!   second renderer.
//! - **Text** — [`to_mermaid`], [`to_dbml`], [`to_plantuml`] and [`to_dot`] emit
//!   the *graph*, not the arrangement: every node with all its columns, every FK
//!   with its cardinality. Positions and collapse state are a property of this
//!   app's canvas and mean nothing to the tool on the other end.
//!
//! Everything here is pure and unit-tested; the caller does the file IO.

use crate::erd::{
    Cardinality, DiagramColumn, DiagramEdge, DiagramGraph, DiagramNode, NodeKind, Pt, Rect,
};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

/// A format the ER diagram can be exported as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErdExportFormat {
    /// Rasterised from [`Svg`](Self::Svg) — the only binary format.
    Png,
    /// Vector, and the source the PNG is rendered from.
    Svg,
    /// Mermaid `erDiagram` — renders inline on GitHub/GitLab/Notion/Obsidian.
    Mermaid,
    /// DBML — dbdiagram.io / dbdocs.io.
    Dbml,
    /// PlantUML entity diagram.
    PlantUml,
    /// Graphviz `digraph` with HTML-like table labels and per-column ports.
    Dot,
}

impl ErdExportFormat {
    /// Every format, in the order the export menu lists them.
    pub const ALL: [ErdExportFormat; 6] = [
        ErdExportFormat::Png,
        ErdExportFormat::Svg,
        ErdExportFormat::Mermaid,
        ErdExportFormat::Dbml,
        ErdExportFormat::PlantUml,
        ErdExportFormat::Dot,
    ];

    /// The menu label.
    pub fn label(self) -> &'static str {
        match self {
            ErdExportFormat::Png => "PNG image",
            ErdExportFormat::Svg => "SVG image",
            ErdExportFormat::Mermaid => "Mermaid",
            ErdExportFormat::Dbml => "DBML",
            ErdExportFormat::PlantUml => "PlantUML",
            ErdExportFormat::Dot => "Graphviz",
        }
    }

    /// The file extension, without the leading dot.
    pub fn extension(self) -> &'static str {
        match self {
            ErdExportFormat::Png => "png",
            ErdExportFormat::Svg => "svg",
            ErdExportFormat::Mermaid => "mmd",
            ErdExportFormat::Dbml => "dbml",
            ErdExportFormat::PlantUml => "puml",
            ErdExportFormat::Dot => "dot",
        }
    }

    /// Extensions for the save dialog's type filter — the shape the dialog wants,
    /// which is a static slice rather than the single [`extension`](Self::extension)
    /// it currently always holds. The two are asserted equal by a test, so a format
    /// that grows a second extension can't leave the filter naming the first only.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            ErdExportFormat::Png => &["png"],
            ErdExportFormat::Svg => &["svg"],
            ErdExportFormat::Mermaid => &["mmd"],
            ErdExportFormat::Dbml => &["dbml"],
            ErdExportFormat::PlantUml => &["puml"],
            ErdExportFormat::Dot => &["dot"],
        }
    }

    /// Whether the export is text the clipboard can hold — everything but PNG,
    /// which is why "Copy as…" lists five of the six.
    pub fn is_text(self) -> bool {
        self != ErdExportFormat::Png
    }

    /// Whether the export renders the *arrangement* (positions, collapse state)
    /// rather than the graph. Asked rather than spelled `== Png || == Svg` at each
    /// site, so a seventh format lands on the right side of the menu by declaring
    /// itself here.
    pub fn is_picture(self) -> bool {
        matches!(self, ErdExportFormat::Png | ErdExportFormat::Svg)
    }
}

/// The save dialog's suggested filename for a diagram of `scope` (`shop` or
/// `shop.orders`), in `format`.
///
/// The scope is a database and optional table name, and neither is constrained to
/// what a filesystem accepts — MySQL allows a `/` in an identifier, and Windows
/// takes none of `\/:*?"<>|`. Anything outside a conservative safe set becomes
/// `_`; a scope that reduces to nothing falls back to `diagram`, so the dialog
/// never opens on a name that is only an extension.
pub fn file_stem(scope: &str, format: ErdExportFormat) -> String {
    let stem: String = scope
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stem = stem.trim_matches(|c| c == '.' || c == '_');
    let stem = if stem.is_empty() { "diagram" } else { stem };
    format!("{stem}.{}", format.extension())
}

// ── Identifier handling ─────────────────────────────────────────────────────

/// A node or column id reduced to `[A-Za-z0-9_]`, for the formats whose grammar
/// won't take a dot or a space in a name (Mermaid entities, PlantUML aliases,
/// Graphviz ports). A leading digit gets an underscore prefix; an empty result
/// becomes `_`.
fn slug(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if out.is_empty() {
        out.push('_');
    } else if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// Every node id mapped to a **unique** slug.
///
/// Uniqueness is the whole point: `sales.orders` and `sales_orders` both slug to
/// `sales_orders`, and a Mermaid file naming two entities the same silently merges
/// them — two tables become one card and their columns interleave, with no error
/// anywhere. Collisions take a `_2`, `_3`, … suffix, assigned in `graph.nodes`
/// order so the mapping is deterministic.
pub fn aliases(graph: &DiagramGraph) -> HashMap<String, String> {
    let mut taken: HashSet<String> = HashSet::new();
    let mut out = HashMap::new();
    for n in &graph.nodes {
        let base = slug(&n.id);
        let mut candidate = base.clone();
        let mut nth = 2;
        while taken.contains(&candidate) {
            candidate = format!("{base}_{nth}");
            nth += 1;
        }
        taken.insert(candidate.clone());
        out.insert(n.id.clone(), candidate);
    }
    out
}

/// A column type reduced to what Mermaid's attribute grammar accepts: alphanumerics
/// plus `()[]-_,`, everything else collapsed to `_`. MySQL's `int unsigned` and
/// PostgreSQL's `timestamp with time zone` are the cases that matter — a space
/// there ends the type token and the rest of the line is a parse error.
fn mermaid_type(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '(' | ')' | '[' | ']' | '-' | '_' | ',') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

/// The key marker for a column, in the notation Mermaid and Graphviz share.
fn key_tags(c: &DiagramColumn) -> Option<String> {
    match (c.pk, c.fk) {
        (true, true) => Some("PK,FK".to_string()),
        (true, false) => Some("PK".to_string()),
        (false, true) => Some("FK".to_string()),
        (false, false) => None,
    }
}

/// A relationship's two crow's-foot ends, **parent first**, in the notation Mermaid
/// and PlantUML share.
///
/// The parent end is "exactly one" unless the FK is nullable, in which case a child
/// may reference nothing and it drops to "zero or one". The child end is "zero or
/// more" for a 1:many FK and "zero or one" for a unique one — zero either way,
/// since nothing obliges a parent row to have children.
///
/// **Every text export says this the same way.** Mermaid and PlantUML take the
/// two strings straight; Graphviz can't use them but spells the same pair
/// `crowodot`/`teeodot` (see [`to_dot`]). The *canvas* is the one surface that
/// draws no zero at the child end, and deliberately: on screen the marker would
/// be on every edge of every diagram without exception, so it distinguishes
/// nothing and only adds twenty stroked segments per edge to a view that is
/// already the app's heaviest paint. A file is read by another tool, where the
/// notation has to be complete rather than informative.
fn crow_ends(e: &DiagramEdge) -> (&'static str, &'static str) {
    let parent = if e.optional { "|o" } else { "||" };
    let child = match e.cardinality {
        Cardinality::OneToMany => "o{",
        Cardinality::OneToOne => "o|",
    };
    (parent, child)
}

/// The columns a text export lists for a node: all of them. Collapse state is a
/// property of this app's canvas, not of the schema — a `.mmd` that dropped the
/// columns the user happened to have folded away would be wrong the moment it was
/// read anywhere else.
fn columns(n: &DiagramNode) -> &[DiagramColumn] {
    &n.columns
}

// ── Mermaid ─────────────────────────────────────────────────────────────────

/// The graph as a Mermaid `erDiagram`.
///
/// Entity names are slugged ([`aliases`]) because Mermaid's ER grammar takes
/// neither a dot nor a space in one — a PostgreSQL `sales.orders` exports as
/// `sales_orders`. Relationship labels carry the referencing columns, so a
/// composite FK stays readable after the rename.
pub fn to_mermaid(graph: &DiagramGraph) -> String {
    let alias = aliases(graph);
    let mut out = String::from("erDiagram\n");
    for n in &graph.nodes {
        let a = &alias[&n.id];
        if n.kind == NodeKind::Stub {
            // A cross-database target with no columns to enumerate. Declared all
            // the same, so its edges have two ends that exist.
            out.push_str(&format!("    {a} {{\n    }}\n"));
            continue;
        }
        out.push_str(&format!("    {a} {{\n"));
        for c in columns(n) {
            let ty = mermaid_type(&c.type_name);
            let name = slug(&c.name);
            match key_tags(c) {
                Some(k) => out.push_str(&format!("        {ty} {name} {k}\n")),
                None => out.push_str(&format!("        {ty} {name}\n")),
            }
        }
        out.push_str("    }\n");
    }
    for e in &graph.edges {
        let (Some(from), Some(to)) = (alias.get(&e.from), alias.get(&e.to)) else {
            continue;
        };
        let (p, c) = crow_ends(e);
        let label = label_text(&e.from_columns.join(", "));
        out.push_str(&format!("    {to} {p}--{c} {from} : \"{label}\"\n"));
    }
    out
}

// ── DBML ────────────────────────────────────────────────────────────────────

/// A server-authored string on its way **inside a double-quoted label** in one
/// of the plain-text formats.
///
/// The backslash first, then the quote, or the escape swallows itself. A column
/// named `a"b` is legal on all three engines (`` `a"b` ``, `"a""b"`) and closed
/// DOT's `label="…"` early, so `dot` rejected the whole file at that line; a
/// column named `a\` swallowed the closing quote and the rest of the line. The
/// module escapes correctly everywhere else — `dot_text` through
/// `export::html_escape`, `dbml_name` by doubling — and these three sites are one
/// missing habit rather than three oversights.
///
/// **Not** `html_escape`: that escapes `&<>` for a text node and leaves `"`
/// alone, which is right for the SVG and exactly wrong here.
///
/// A control character becomes a space as well, because all three formats end a
/// label at the end of its line — PlantUML's runs unquoted to one.
fn label_text(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .flat_map(|c| match c {
            '\\' => vec!['\\', '\\'],
            '"' => vec!['\\', '"'],
            c => vec![c],
        })
        .collect()
}

/// A DBML name, quoted when it isn't a bare identifier. DBML keeps the real name —
/// `Table "sales.orders"` is legal — so nothing is renamed here.
fn dbml_name(s: &str) -> String {
    if !s.is_empty()
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !s.starts_with(|c: char| c.is_ascii_digit())
    {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('"', "\\\""))
    }
}

/// One side of a DBML `Ref`: `table.column`, or `table.(a, b)` for a composite key.
fn dbml_ref_side(table: &str, cols: &[String]) -> String {
    let t = dbml_name(table);
    match cols {
        [one] => format!("{t}.{}", dbml_name(one)),
        many => format!(
            "{t}.({})",
            many.iter()
                .map(|c| dbml_name(c))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Every column name an edge points at on `node`, in first-seen order and
/// deduplicated — the placeholder columns a stub table has to declare for the
/// `Ref`s naming them to resolve.
fn stub_columns(graph: &DiagramGraph, node: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in &graph.edges {
        let cols = if e.from == node {
            &e.from_columns
        } else if e.to == node {
            &e.to_columns
        } else {
            continue;
        };
        for c in cols {
            if !out.iter().any(|s| s == c) {
                out.push(c.clone());
            }
        }
    }
    // A stub nothing points at still needs a body, so the grammar is satisfied.
    if out.is_empty() {
        out.push("id".to_string());
    }
    out
}

/// The graph as DBML (dbdiagram.io / dbdocs.io).
///
/// Unlike Mermaid this keeps qualified names verbatim, quoting where the grammar
/// needs it, and nullability survives as DBML's own `[not null]`.
pub fn to_dbml(graph: &DiagramGraph) -> String {
    let mut out = String::new();
    for n in &graph.nodes {
        if n.kind == NodeKind::Stub {
            // **A DBML table body needs at least one column**, and every `Ref`
            // endpoint has to name a column its table declares. A stub has no
            // introspected columns — it stands for a table outside the schema —
            // so it is given exactly the ones the edges point at. Declared empty
            // instead, with the `Ref`s left dangling on it, the whole file was
            // rejected by the parser (`Expected " " but ":" found`), so the
            // export was unusable on any diagram carrying a cross-database FK
            // *or* a privilege-filtered one.
            out.push_str(&format!("Table {} {{\n", dbml_name(&n.id)));
            for name in stub_columns(graph, &n.id) {
                out.push_str(&format!("  {} unknown\n", dbml_name(&name)));
            }
            out.push_str("  Note: 'cross-database reference'\n}\n\n");
            continue;
        }
        out.push_str(&format!("Table {} {{\n", dbml_name(&n.id)));
        for c in columns(n) {
            let mut settings: Vec<&str> = Vec::new();
            if c.pk {
                settings.push("pk");
            }
            if !c.nullable {
                settings.push("not null");
            }
            let tail = if settings.is_empty() {
                String::new()
            } else {
                format!(" [{}]", settings.join(", "))
            };
            out.push_str(&format!(
                "  {} {}{tail}\n",
                dbml_name(&c.name),
                dbml_name(&c.type_name)
            ));
        }
        if n.kind == NodeKind::View {
            out.push_str("  Note: 'view'\n");
        }
        out.push_str("}\n\n");
    }
    for e in &graph.edges {
        // `>` is many-to-one (child → parent), `-` one-to-one.
        let op = match e.cardinality {
            Cardinality::OneToMany => '>',
            Cardinality::OneToOne => '-',
        };
        out.push_str(&format!(
            "Ref: {} {op} {}\n",
            dbml_ref_side(&e.from, &e.from_columns),
            dbml_ref_side(&e.to, &e.to_columns)
        ));
    }
    out
}

// ── PlantUML ────────────────────────────────────────────────────────────────

/// The graph as a PlantUML entity diagram.
///
/// `entity "real.name" as alias` keeps the qualified name on the card while the
/// relationship lines use the slug the grammar requires. Primary keys go above the
/// `--` divider, the way PlantUML's own ER examples separate them, and `*` marks a
/// required attribute.
pub fn to_plantuml(graph: &DiagramGraph) -> String {
    let alias = aliases(graph);
    let mut out = String::from("@startuml\nhide circle\nskinparam linetype ortho\n\n");
    for n in &graph.nodes {
        let a = &alias[&n.id];
        let stereotype = match n.kind {
            NodeKind::View => " <<view>>",
            NodeKind::Stub => " <<external>>",
            NodeKind::Table => "",
        };
        out.push_str(&format!(
            "entity \"{}\" as {a}{stereotype} {{\n",
            n.id.replace('"', "'")
        ));
        let cols = columns(n);
        let (keys, rest): (Vec<_>, Vec<_>) = cols.iter().partition(|c| c.pk);
        for c in &keys {
            out.push_str(&format!(
                "  * {} : {}\n",
                label_text(&c.name),
                label_text(&c.type_name)
            ));
        }
        if !keys.is_empty() && !rest.is_empty() {
            out.push_str("  --\n");
        }
        for c in &rest {
            let req = if c.nullable { "  " } else { "* " };
            let fk = if c.fk { " <<FK>>" } else { "" };
            out.push_str(&format!(
                "  {req}{} : {}{fk}\n",
                label_text(&c.name),
                label_text(&c.type_name)
            ));
        }
        out.push_str("}\n\n");
    }
    for e in &graph.edges {
        let (Some(from), Some(to)) = (alias.get(&e.from), alias.get(&e.to)) else {
            continue;
        };
        let (p, c) = crow_ends(e);
        out.push_str(&format!(
            "{to} {p}--{c} {from} : {}\n",
            label_text(&e.from_columns.join(", "))
        ));
    }
    out.push_str("\n@enduml\n");
    out
}

// ── Graphviz ────────────────────────────────────────────────────────────────

/// Escape for a Graphviz HTML-like label's text content.
fn dot_text(s: &str) -> String {
    crate::export::html_escape(s)
}

/// The graph as a Graphviz `digraph`, one HTML-like table per node.
///
/// Each column row carries a `port`, so an edge attaches to the exact referencing
/// and referenced rows rather than to the box — which is what makes a multi-FK
/// diagram readable once `dot` has laid it out. Node ids are the [`aliases`] slugs
/// rather than the raw names: a port suffix is written `"node":port`, and a raw id
/// containing a `:` would split in the wrong place.
///
/// Deliberately **no `pos` attributes**. They only bind under `neato -n`, so a file
/// carrying the user's arrangement would silently ignore it under plain `dot` —
/// worse than one that never claimed to have a layout.
pub fn to_dot(graph: &DiagramGraph) -> String {
    let ids = aliases(graph);
    let mut out = String::from("digraph erd {\n");
    out.push_str("  graph [rankdir=LR, splines=spline, nodesep=0.5, ranksep=1.0];\n");
    out.push_str("  node [shape=plaintext, fontname=\"Helvetica\", fontsize=10];\n");
    out.push_str("  edge [dir=both];\n\n");
    for n in &graph.nodes {
        let header_bg = match n.kind {
            NodeKind::Stub => "#e8e8e8",
            NodeKind::View => "#e6eef7",
            NodeKind::Table => "#eef0f2",
        };
        let mut label = String::from(
            "<<table border=\"1\" cellborder=\"0\" cellspacing=\"0\" cellpadding=\"4\">",
        );
        label.push_str(&format!(
            "<tr><td bgcolor=\"{header_bg}\"><b>{}</b></td></tr>",
            dot_text(&n.id)
        ));
        for c in columns(n) {
            let key = match key_tags(c) {
                Some(k) => format!("  <i>{k}</i>"),
                None => String::new(),
            };
            label.push_str(&format!(
                "<tr><td port=\"{}\" align=\"left\">{} : {}{key}</td></tr>",
                slug(&c.name),
                dot_text(&c.name),
                dot_text(&c.type_name)
            ));
        }
        label.push_str("</table>>");
        out.push_str(&format!("  {} [label={label}];\n", ids[&n.id]));
    }
    out.push('\n');
    for e in &graph.edges {
        let (Some(from), Some(to)) = (ids.get(&e.from), ids.get(&e.to)) else {
            continue;
        };
        // Ported onto the first column of each side; a composite FK's remaining
        // columns are named in the label rather than drawn as N parallel edges.
        let fp = e
            .from_columns
            .first()
            .map(|c| format!(":{}", slug(c)))
            .unwrap_or_default();
        let tp = e
            .to_columns
            .first()
            .map(|c| format!(":{}", slug(c)))
            .unwrap_or_default();
        // The edge runs child → parent, so the *tail* carries the many-end crow —
        // and the zero-circle behind it, because the child end is optional either
        // way ([`crow_ends`] says why). Without it the tail read "exactly one" and
        // the `.dot` asserted every parent row has a child, which the `.mmd` and
        // the `.puml` of the same diagram explicitly deny.
        //
        // Spelled as a *composite*, not with the `o` modifier: Graphviz applies
        // `o` only to the fillable primitives, so `ocrow`/`otee` parse and then
        // render exactly like `crow`/`tee` (checked against graphviz itself — an
        // `ocrow` edge draws one fewer circle than a `crowodot` one). In a
        // composite the later shape sits farther from the node, which is where
        // crow's-foot puts the zero.
        let tail = match e.cardinality {
            Cardinality::OneToMany => "crowodot",
            Cardinality::OneToOne => "teeodot",
        };
        let head = if e.optional { "odot" } else { "tee" };
        out.push_str(&format!(
            "  {from}{fp} -> {to}{tp} [arrowtail={tail}, arrowhead={head}, label=\"{}\"];\n",
            label_text(&e.from_columns.join(", "))
        ));
    }
    out.push_str("}\n");
    out
}

// ── SVG ─────────────────────────────────────────────────────────────────────

/// A number for an SVG attribute: two decimals, trailing zeros trimmed.
///
/// Not cosmetic. `f64`'s default formatting writes `104.30000000000001` for a
/// coordinate that came out of a bezier sample, and an edge polyline carries 34 of
/// them — the noise is most of the file's bytes and makes a rendered diagram
/// impossible to diff between two exports of the same layout.
fn n(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    // `-0` is legal SVG but reads as a bug in a diff; normalise it away.
    let r = if r == 0.0 { 0.0 } else { r };
    let mut s = format!("{r:.2}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// Which key a column row is, driving the name's colour — the same three-way the
/// card itself paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgKey {
    None,
    Pk,
    Fk,
}

/// One column row on an exported card. The strings are **already truncated** by
/// the caller: the UI measured these widths with its own font, and it is the only
/// layer that can ellipsize the same way the card does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SvgRow {
    pub name: String,
    pub type_name: String,
    pub key: SvgKey,
    /// The type-family glyph's markup, drawn in the name's colour — the card puts
    /// the same glyph in the same place.
    pub icon: Option<String>,
}

/// One card, in canvas coordinates — the rect the UI is actually drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgNode {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Header text (the node id, ellipsized to fit).
    pub title: String,
    /// A cross-database stub: header only, no rows and no divider.
    pub stub: bool,
    /// The rows currently shown — collapsed cards export collapsed, because this
    /// family of formats exports the *arrangement*.
    pub rows: Vec<SvgRow>,
    /// The expander row's text (`+3 more` / `show less`), when the card has one.
    pub more: Option<String>,
    /// The table/view glyph's markup, and its colour.
    pub icon: Option<String>,
    pub icon_fill: String,
    /// Header text colour. Per node because a stub's name is dimmed on the canvas,
    /// the way it says "this one isn't a table I can show you".
    pub title_fill: String,
    /// Header strip fill and card border — **per node**, not per scene: a table
    /// the user has given an identity colour wears it here exactly as it does on
    /// the canvas, and an export that flattened them all to one grey would lose
    /// the only thing distinguishing the cards at a glance.
    pub header_fill: String,
    pub border: String,
    /// The line under the header strip. **Not `border`**: the painter draws it
    /// with the theme's plain border while the card's outline takes the tint, so
    /// reusing `border` here coincided for an untinted table and diverged for
    /// exactly the coloured ones the export goes out of its way to preserve.
    pub divider: String,
}

/// One relationship's drawn geometry, straight from the canvas: the flattened
/// curve and the crow's-foot / bar / optionality marker segments.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgEdge {
    pub poly: Vec<Pt>,
    pub markers: Vec<(Pt, Pt)>,
}

/// The scene-wide theme colours, each `#rrggbb` (or `#rrggbbaa`). Snapshotted
/// from the live theme, so the export matches what is on screen — a dark-theme
/// diagram exports dark. Per-card colours (header tint, border) live on
/// [`SvgNode`], since a table may carry its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SvgColors {
    pub canvas: String,
    pub card: String,
    pub text: String,
    pub type_text: String,
    pub key_pk: String,
    pub key_fk: String,
    pub edge: String,
    pub muted: String,
}

/// The card metrics the UI lays out with. Parameters rather than constants for the
/// same reason [`crate::erd::column_row_offset`] takes them: pixel metrics belong
/// to the view, and a second copy here would drift the moment a row got taller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SvgMetrics {
    pub header_h: f64,
    pub row_h: f64,
    pub expander_h: f64,
    /// Horizontal padding inside a card.
    pub pad_x: f64,
    pub radius: f64,
    pub title_size: f64,
    pub name_size: f64,
    pub type_size: f64,
    pub edge_width: f64,
    /// The leading glyph's box, and the gap between it and the text — one gap for
    /// the header, another for a column row, because the card uses two.
    pub icon_size: f64,
    pub title_gap: f64,
    pub row_gap: f64,
}

/// A whole diagram ready to be written out: the content bounds, the cards, the
/// edges, and the palette and metrics they are drawn with.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgScene {
    /// Content bounds in canvas coordinates, padding included. Becomes the
    /// `viewBox`, so a diagram dragged into negative space still exports whole.
    pub bounds: Rect,
    pub nodes: Vec<SvgNode>,
    pub edges: Vec<SvgEdge>,
    pub colors: SvgColors,
    pub metrics: SvgMetrics,
}

/// `s` shortened with a trailing `…` until `measure` says it fits `max_px`.
///
/// The card ellipsizes the same way (Floem's `text_ellipsis`), and the caller
/// passes the same measurer it sized the card with — so a name that truncates on
/// the canvas truncates in the export at the same character. Returns `s` unchanged
/// when it already fits, and gives up to the bare `…` rather than looping forever
/// on a width that fits nothing.
///
/// Truncation walks *characters*, not bytes: a `…` spliced into the middle of a
/// UTF-8 sequence is a panic on the next slice, and schema identifiers are not
/// reliably ASCII.
///
/// **Binary search, not a walk down.** `measure` is a full cosmic-text layout —
/// ~10 µs each, measured — and this runs once per column row plus once per card
/// title on the UI thread, before the save dialog opens. Stepping down one
/// character at a time cost 40 calls for a 48-character name against a narrow
/// box; the search costs 6. Width is monotone in the character count for every
/// shaper this is handed, so the answer is the same.
pub fn ellipsize(s: &str, max_px: f64, measure: impl Fn(&str) -> f64) -> String {
    if max_px <= 0.0 {
        return String::new();
    }
    if measure(s) <= max_px {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    // Largest `keep` in `0..chars.len()` whose candidate still fits. `lo` is
    // known-fitting only in the sense that 0 is the fallback (`"…"` alone).
    let (mut lo, mut hi) = (0usize, chars.len());
    let mut best: Option<String> = None;
    while lo < hi {
        let keep = lo + (hi - lo).div_ceil(2);
        let candidate: String = chars[..keep].iter().collect::<String>() + "…";
        if measure(&candidate) <= max_px {
            lo = keep;
            best = Some(candidate);
        } else {
            hi = keep - 1;
        }
    }
    best.unwrap_or_else(|| "…".to_string())
}

/// A Lucide glyph inlined as a nested `<svg>` at `(x, y)`, `size` px square.
///
/// The icons (the UI crate's Lucide set) are whole documents on a 24×24 viewBox
/// stroked in `currentColor`; this lifts out their inner markup, re-frames it at
/// the requested box, and resolves `currentColor` to a real value — SVG's
/// `currentColor` follows the CSS `color` property, which nothing in a standalone
/// export sets. Markup that isn't a well-formed `<svg>…</svg>` yields nothing
/// rather than half an element.
fn icon_svg(markup: &str, x: f64, y: f64, size: f64, color: &str) -> String {
    let (Some(open), Some(close)) = (markup.find('>'), markup.rfind("</svg>")) else {
        return String::new();
    };
    if open + 1 > close {
        return String::new();
    }
    let inner = markup[open + 1..close].replace("currentColor", color);
    format!(
        "<svg x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" viewBox=\"0 0 24 24\" fill=\"none\" \
         stroke=\"{color}\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{inner}</svg>",
        n(x),
        n(y),
        n(size),
        n(size)
    )
}

/// A card's outline: a rounded rect, as a path so the header can reuse the shape.
fn rounded_rect_path(x: f64, y: f64, w: f64, h: f64, r: f64) -> String {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    format!(
        "M{} {}h{}a{r0} {r0} 0 0 1 {r0} {r0}v{}a{r0} {r0} 0 0 1 -{r0} {r0}h-{}a{r0} {r0} 0 0 1 -{r0} -{r0}v-{}a{r0} {r0} 0 0 1 {r0} -{r0}z",
        n(x + r),
        n(y),
        n(w - 2.0 * r),
        n(h - 2.0 * r),
        n(w - 2.0 * r),
        n(h - 2.0 * r),
        r0 = n(r),
    )
}

/// The header strip: rounded on top, square where it meets the first row.
fn header_path(x: f64, y: f64, w: f64, hh: f64, r: f64) -> String {
    let r = r.min(w / 2.0).min(hh).max(0.0);
    format!(
        "M{} {}h{}a{r0} {r0} 0 0 1 {r0} {r0}v{}h-{}v-{}a{r0} {r0} 0 0 1 {r0} -{r0}z",
        n(x + r),
        n(y),
        n(w - 2.0 * r),
        n(hh - r),
        n(w),
        n(hh - r),
        r0 = n(r),
    )
}

/// The scene as a standalone SVG document.
///
/// Draw order is the canvas's: background, edges, then cards on top — an edge that
/// runs behind a card must be occluded by it, which is exactly what the app does
/// (the edge layer sits under the card layer, pointer-transparent).
///
/// The font is named on the root and inherited, rather than repeated on a few
/// thousand text elements. A viewer without IBM Plex Sans falls back and its glyph
/// advances differ slightly from the widths the cards were measured at — harmless,
/// since the caller has already truncated every string to fit. The PNG path has no
/// such drift: it rasterises this document with the app's own embedded font bytes.
pub fn to_svg(scene: &SvgScene) -> String {
    let Rect { x, y, w, h } = scene.bounds;
    let c = &scene.colors;
    let m = &scene.metrics;
    let mut out = String::with_capacity(4096);
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"{} {} {} {}\" \
         font-family=\"'IBM Plex Sans','Segoe UI',system-ui,sans-serif\">",
        n(w),
        n(h),
        n(x),
        n(y),
        n(w),
        n(h)
    );
    let _ = writeln!(
        out,
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
        n(x),
        n(y),
        n(w),
        n(h),
        c.canvas
    );

    // Edges under the cards.
    let _ = writeln!(
        out,
        "<g fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
        c.edge,
        n(m.edge_width)
    );
    for e in &scene.edges {
        if e.poly.len() >= 2 {
            let pts: Vec<String> = e
                .poly
                .iter()
                .map(|p| format!("{},{}", n(p.x), n(p.y)))
                .collect();
            let _ = writeln!(out, "<polyline points=\"{}\"/>", pts.join(" "));
        }
        for (a, b) in &e.markers {
            let _ = writeln!(
                out,
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
                n(a.x),
                n(a.y),
                n(b.x),
                n(b.y)
            );
        }
    }
    out.push_str("</g>\n");

    for node in &scene.nodes {
        let _ = write!(
            out,
            "<g><path d=\"{}\" fill=\"{}\" stroke=\"{}\"/>",
            rounded_rect_path(node.x, node.y, node.w, node.h, m.radius),
            c.card,
            node.border
        );
        let _ = write!(
            out,
            "<path d=\"{}\" fill=\"{}\"/>",
            header_path(node.x, node.y, node.w, m.header_h, m.radius),
            node.header_fill
        );
        if !node.stub {
            let hy = node.y + m.header_h;
            let _ = write!(
                out,
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\"/>",
                n(node.x),
                n(hy),
                n(node.x + node.w),
                n(hy),
                node.divider
            );
        }
        // Header: glyph, gap, bold name — the card's own order, so the two line up
        // when someone puts the export beside the app.
        let mut tx = node.x + m.pad_x;
        if let Some(icon) = &node.icon {
            out.push_str(&icon_svg(
                icon,
                tx,
                node.y + (m.header_h - m.icon_size) / 2.0,
                m.icon_size,
                &node.icon_fill,
            ));
            tx += m.icon_size + m.title_gap;
        }
        // A stub's title is drawn **regular** on canvas, and its card is sized to
        // exactly that regular measurement — so emitting it bold made every
        // name past the minimum card width truncate in the file and nowhere
        // else. The weight follows `stub`, as `export_scene`'s measurer does.
        let weight = if node.stub {
            ""
        } else {
            " font-weight=\"600\""
        };
        let _ = write!(
            out,
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\"{weight} fill=\"{}\" dominant-baseline=\"central\">{}</text>",
            n(tx),
            n(node.y + m.header_h / 2.0),
            n(m.title_size),
            node.title_fill,
            crate::export::html_escape(&node.title)
        );
        for (i, row) in node.rows.iter().enumerate() {
            let cy = node.y + m.header_h + (i as f64 + 0.5) * m.row_h;
            let name_fill = match row.key {
                SvgKey::Pk => &c.key_pk,
                SvgKey::Fk => &c.key_fk,
                SvgKey::None => &c.text,
            };
            let mut rx = node.x + m.pad_x;
            if let Some(icon) = &row.icon {
                out.push_str(&icon_svg(
                    icon,
                    rx,
                    cy - m.icon_size / 2.0,
                    m.icon_size,
                    name_fill,
                ));
                rx += m.icon_size + m.row_gap;
            }
            let _ = write!(
                out,
                "<text x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{}\" dominant-baseline=\"central\">{}</text>",
                n(rx),
                n(cy),
                n(m.name_size),
                name_fill,
                crate::export::html_escape(&row.name)
            );
            if !row.type_name.is_empty() {
                let _ = write!(
                    out,
                    "<text x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{}\" text-anchor=\"end\" dominant-baseline=\"central\">{}</text>",
                    n(node.x + node.w - m.pad_x),
                    n(cy),
                    n(m.type_size),
                    c.type_text,
                    crate::export::html_escape(&row.type_name)
                );
            }
        }
        if let Some(more) = &node.more {
            let cy = node.y + m.header_h + node.rows.len() as f64 * m.row_h + m.expander_h / 2.0;
            // Left, at the same inset every column name uses — the painter
            // renders this as a plain `text` in a `width_full()` box with
            // `padding_horiz`, so a centred one was a difference repeated once
            // per collapsed card on a whole-database diagram.
            let _ = write!(
                out,
                "<text x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{}\" dominant-baseline=\"central\">{}</text>",
                n(node.x + m.pad_x),
                n(cy),
                n(m.type_size),
                c.muted,
                crate::export::html_escape(more)
            );
        }
        out.push_str("</g>\n");
    }

    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str, pk: bool, fk: bool, nullable: bool) -> DiagramColumn {
        DiagramColumn {
            name: name.to_string(),
            type_name: ty.to_string(),
            nullable,
            pk,
            fk,
        }
    }

    fn node(id: &str, kind: NodeKind, columns: Vec<DiagramColumn>) -> DiagramNode {
        DiagramNode {
            id: id.to_string(),
            kind,
            columns,
        }
    }

    /// `orders.user_id -> users.id`, 1:many, required.
    fn sample() -> DiagramGraph {
        DiagramGraph {
            nodes: vec![
                node(
                    "users",
                    NodeKind::Table,
                    vec![
                        col("id", "int", true, false, false),
                        col("email", "varchar(255)", false, false, true),
                    ],
                ),
                node(
                    "orders",
                    NodeKind::Table,
                    vec![
                        col("id", "int", true, false, false),
                        col("user_id", "int unsigned", false, true, false),
                    ],
                ),
            ],
            edges: vec![DiagramEdge {
                from: "orders".to_string(),
                from_columns: vec!["user_id".to_string()],
                to: "users".to_string(),
                to_columns: vec!["id".to_string()],
                cardinality: Cardinality::OneToMany,
                optional: false,
            }],
            hidden_islands: vec![],
            total_tables: 2,
        }
    }

    // ── Formats ─────────────────────────────────────────────────────────────

    #[test]
    fn every_format_has_a_distinct_extension() {
        let mut seen = HashSet::new();
        for f in ErdExportFormat::ALL {
            assert!(seen.insert(f.extension()), "duplicate extension {f:?}");
            assert!(!f.label().is_empty());
            assert_eq!(f.extensions(), [f.extension()]);
        }
    }

    #[test]
    fn png_is_the_only_binary_format() {
        for f in ErdExportFormat::ALL {
            assert_eq!(f.is_text(), f != ErdExportFormat::Png, "{f:?}");
        }
        assert!(ErdExportFormat::Png.is_picture());
        assert!(ErdExportFormat::Svg.is_picture());
        assert!(!ErdExportFormat::Mermaid.is_picture());
    }

    #[test]
    fn file_stem_keeps_the_scope_and_strips_what_a_filesystem_rejects() {
        use ErdExportFormat::*;
        assert_eq!(file_stem("shop", Png), "shop.png");
        assert_eq!(file_stem("shop.orders", Mermaid), "shop.orders.mmd");
        // MySQL allows these in an identifier; Windows allows none of them.
        assert_eq!(file_stem("a/b:c*d?", Svg), "a_b_c_d.svg");
        // A scope that reduces to nothing must not open the dialog on ".dot".
        assert_eq!(file_stem("...", Dot), "diagram.dot");
        assert_eq!(file_stem("", Dbml), "diagram.dbml");
    }

    // ── Identifiers ─────────────────────────────────────────────────────────

    #[test]
    fn slug_keeps_word_chars_and_guards_a_leading_digit() {
        assert_eq!(slug("users"), "users");
        assert_eq!(slug("sales.orders"), "sales_orders");
        assert_eq!(slug("order items"), "order_items");
        assert_eq!(slug("2fa_tokens"), "_2fa_tokens");
        assert_eq!(slug(""), "_");
        assert_eq!(slug("héllo"), "h_llo");
    }

    #[test]
    fn aliases_break_a_slug_collision() {
        // Both slug to `sales_orders`; merging them would silently fuse two tables
        // into one entity in every alias-based format.
        let g = DiagramGraph {
            nodes: vec![
                node("sales.orders", NodeKind::Table, vec![]),
                node("sales_orders", NodeKind::Table, vec![]),
            ],
            ..DiagramGraph::default()
        };
        let a = aliases(&g);
        assert_eq!(a["sales.orders"], "sales_orders");
        assert_eq!(a["sales_orders"], "sales_orders_2");
    }

    #[test]
    fn aliases_are_empty_for_an_empty_graph() {
        assert!(aliases(&DiagramGraph::default()).is_empty());
    }

    #[test]
    fn mermaid_type_collapses_what_the_grammar_rejects() {
        assert_eq!(mermaid_type("int unsigned"), "int_unsigned");
        assert_eq!(mermaid_type("varchar(255)"), "varchar(255)");
        assert_eq!(
            mermaid_type("timestamp with time zone"),
            "timestamp_with_time_zone"
        );
        assert_eq!(mermaid_type("int[]"), "int[]");
        assert_eq!(mermaid_type(""), "unknown");
    }

    #[test]
    fn crow_ends_read_the_parent_end_from_optionality() {
        let mut e = sample().edges.remove(0);
        assert_eq!(crow_ends(&e), ("||", "o{"));
        e.optional = true;
        assert_eq!(crow_ends(&e), ("|o", "o{"));
        e.cardinality = Cardinality::OneToOne;
        assert_eq!(crow_ends(&e), ("|o", "o|"));
    }

    // ── Mermaid ─────────────────────────────────────────────────────────────

    #[test]
    fn mermaid_emits_entities_keys_and_a_parent_first_relationship() {
        let out = to_mermaid(&sample());
        assert!(out.starts_with("erDiagram\n"));
        assert!(out.contains("    users {\n"), "{out}");
        assert!(out.contains("        int id PK\n"), "{out}");
        assert!(out.contains("        varchar(255) email\n"), "{out}");
        assert!(out.contains("        int_unsigned user_id FK\n"), "{out}");
        // Parent first: one user has zero-or-more orders.
        assert!(
            out.contains("    users ||--o{ orders : \"user_id\"\n"),
            "{out}"
        );
    }

    #[test]
    fn mermaid_names_a_composite_fk_in_the_label() {
        let mut g = sample();
        g.edges[0].from_columns = vec!["a".to_string(), "b".to_string()];
        assert!(to_mermaid(&g).contains(": \"a, b\""));
    }

    #[test]
    fn mermaid_declares_a_stub_so_its_edge_has_two_ends() {
        let mut g = sample();
        g.nodes.push(node("other.audit", NodeKind::Stub, vec![]));
        let out = to_mermaid(&g);
        assert!(out.contains("    other_audit {\n    }\n"), "{out}");
    }

    #[test]
    fn mermaid_marks_a_column_that_is_both_pk_and_fk() {
        let mut g = sample();
        g.nodes[1].columns[1].pk = true;
        assert!(to_mermaid(&g).contains("user_id PK,FK"));
    }

    #[test]
    fn mermaid_of_an_empty_graph_is_a_valid_empty_diagram() {
        assert_eq!(to_mermaid(&DiagramGraph::default()), "erDiagram\n");
    }

    // ── DBML ────────────────────────────────────────────────────────────────

    #[test]
    fn dbml_emits_tables_settings_and_a_child_to_parent_ref() {
        let out = to_dbml(&sample());
        assert!(out.contains("Table users {\n"), "{out}");
        assert!(out.contains("  id int [pk, not null]\n"), "{out}");
        assert!(out.contains("  email \"varchar(255)\"\n"), "{out}");
        assert!(
            out.contains("  user_id \"int unsigned\" [not null]\n"),
            "{out}"
        );
        assert!(out.contains("Ref: orders.user_id > users.id\n"), "{out}");
    }

    #[test]
    fn dbml_quotes_a_qualified_name_rather_than_renaming_it() {
        let mut g = sample();
        g.nodes[0].id = "sales.users".to_string();
        g.edges[0].to = "sales.users".to_string();
        let out = to_dbml(&g);
        assert!(out.contains("Table \"sales.users\" {"), "{out}");
        assert!(
            out.contains("Ref: orders.user_id > \"sales.users\".id"),
            "{out}"
        );
    }

    #[test]
    fn dbml_groups_a_composite_ref() {
        let mut g = sample();
        g.edges[0].from_columns = vec!["a".to_string(), "b".to_string()];
        g.edges[0].to_columns = vec!["x".to_string(), "y".to_string()];
        assert!(to_dbml(&g).contains("Ref: orders.(a, b) > users.(x, y)"));
    }

    #[test]
    fn dbml_uses_the_one_to_one_operator_for_a_unique_fk() {
        let mut g = sample();
        g.edges[0].cardinality = Cardinality::OneToOne;
        assert!(to_dbml(&g).contains("Ref: orders.user_id - users.id"));
    }

    /// **A DBML table body needs a column, and every `Ref` endpoint has to name
    /// one the table declares.** The stub arm used to emit a `Note:` and nothing
    /// else, and this test asserted that form was correct — so the suite
    /// defended a file `@dbml/core` rejects outright
    /// (`Expected " " but ":" found`), on every diagram carrying a
    /// cross-database FK or a privilege-filtered one.
    #[test]
    fn dbml_notes_a_view_and_gives_a_stub_the_columns_its_refs_name() {
        let mut g = stubbed();
        g.nodes[0].kind = NodeKind::View;
        let out = to_dbml(&g);
        assert!(out.contains("  Note: 'view'\n"), "{out}");
        assert!(
            out.contains(
                "Table \"other.audit\" {\n  order_id unknown\n  \
                 Note: 'cross-database reference'\n}"
            ),
            "{out}"
        );
        assert!(
            out.contains("Ref: orders.id > \"other.audit\".order_id"),
            "{out}"
        );
        dbml_bodies_and_refs_resolve(&out);
    }

    /// **A column name is server text going inside a quoted label.** `a"b` is
    /// legal on all three engines and closed DOT's `label="…"` early, so `dot`
    /// rejected the whole file at that line; `a\` swallowed the closing quote
    /// and the rest of it. The SVG was already clean — every string there goes
    /// through `html_escape` into a text node — and these three plain-text
    /// formats were the gap.
    #[test]
    fn a_column_name_cannot_break_a_label() {
        let mut g = sample();
        g.edges[0].from_columns = vec!["a\"b\\".to_string()];
        g.nodes[1].columns[1].name = "a\"b\\".to_string();
        for out in [to_dot(&g), to_mermaid(&g), to_plantuml(&g)] {
            for line in out.lines().filter(|l| l.contains("a\\\"b")) {
                // Every quote in the line is either a delimiter or escaped, so
                // the delimiters still pair up. Walked rather than counted with
                // a lookbehind, because `\\"` ends in a backslash that is itself
                // escaped and the quote after it is real.
                let mut escaped = false;
                let mut delimiters = 0usize;
                for c in line.chars() {
                    match c {
                        _ if escaped => escaped = false,
                        '\\' => escaped = true,
                        '"' => delimiters += 1,
                        _ => {}
                    }
                }
                assert_eq!(delimiters % 2, 0, "odd quote count: {line:?}");
                assert!(line.contains("a\\\"b\\\\"), "{line:?}");
            }
        }
    }

    /// The sample graph plus a stub something actually points at — the shape
    /// `erd::build_graph` produces for a cross-database FK, and for a MySQL
    /// schema whose `TABLES` is privilege-filtered while `KEY_COLUMN_USAGE`
    /// still reports the constraint.
    fn stubbed() -> DiagramGraph {
        let mut g = sample();
        g.nodes.push(node("other.audit", NodeKind::Stub, vec![]));
        g.edges.push(DiagramEdge {
            from: "orders".to_string(),
            from_columns: vec!["id".to_string()],
            to: "other.audit".to_string(),
            to_columns: vec!["order_id".to_string()],
            cardinality: Cardinality::OneToMany,
            optional: false,
        });
        g
    }

    /// The two grammar rules the reference parser enforces and this suite can:
    /// **no table body is empty**, and **every `Ref` endpoint names a column its
    /// table declares.** Both fail on a stub emitted the old way.
    ///
    /// The external parsers (`@dbml/core`, `mermaid`, `@viz-js/viz`, `plantuml`)
    /// are JS and Java and cannot be `cargo test` dependencies, so what is
    /// enforceable in-tree is the invariants *they* check — which is what the 41
    /// `assert!(out.contains(…))` tests around this could never do: they answer
    /// "did we write what we meant to", never "is what we meant to write legal".
    fn dbml_bodies_and_refs_resolve(out: &str) {
        let mut columns: HashMap<String, Vec<String>> = HashMap::new();
        let mut table: Option<String> = None;
        for line in out.lines() {
            if let Some(rest) = line.strip_prefix("Table ") {
                let name = rest.trim_end_matches(" {").trim_matches('"').to_string();
                table = Some(name.clone());
                columns.entry(name).or_default();
            } else if line == "}" {
                table = None;
            } else if let (Some(t), Some(word)) = (
                table.as_ref(),
                line.split_whitespace()
                    .next()
                    .filter(|w| !w.starts_with("Note")),
            ) {
                columns
                    .get_mut(t)
                    .expect("the table was declared")
                    .push(word.trim_matches('"').to_string());
            }
        }
        for (t, cols) in &columns {
            assert!(!cols.is_empty(), "table {t} has an empty body:\n{out}");
        }
        for line in out.lines().filter(|l| l.starts_with("Ref: ")) {
            for side in line.trim_start_matches("Ref: ").split([' ', '>', '-']) {
                let side = side.trim();
                if side.is_empty() {
                    continue;
                }
                let (t, c) = side.rsplit_once('.').expect("table.column");
                let t = t.trim_matches('"');
                let declared = columns
                    .get(t)
                    .unwrap_or_else(|| panic!("Ref names undeclared table {t}:\n{out}"));
                for c in c.trim_matches(['(', ')']).split(", ") {
                    let c = c.trim_matches('"');
                    assert!(
                        declared.iter().any(|d| d == c),
                        "Ref names {t}.{c}, which {t} does not declare:\n{out}"
                    );
                }
            }
        }
    }

    // ── PlantUML ────────────────────────────────────────────────────────────

    #[test]
    fn plantuml_wraps_the_diagram_and_aliases_the_real_name() {
        let mut g = sample();
        g.nodes[0].id = "sales.users".to_string();
        g.edges[0].to = "sales.users".to_string();
        let out = to_plantuml(&g);
        assert!(out.starts_with("@startuml\n"));
        assert!(out.trim_end().ends_with("@enduml"));
        assert!(
            out.contains("entity \"sales.users\" as sales_users {\n"),
            "{out}"
        );
        assert!(
            out.contains("sales_users ||--o{ orders : user_id\n"),
            "{out}"
        );
    }

    #[test]
    fn plantuml_puts_primary_keys_above_the_divider() {
        let out = to_plantuml(&sample());
        let users = out.split("entity \"users\"").nth(1).unwrap();
        let pk = users.find("* id : int").expect("pk row");
        let div = users.find("  --\n").expect("divider");
        let other = users.find("email : varchar(255)").expect("plain row");
        assert!(pk < div && div < other, "{users}");
    }

    #[test]
    fn plantuml_marks_required_and_foreign_key_columns() {
        let out = to_plantuml(&sample());
        assert!(out.contains("* user_id : int unsigned <<FK>>"), "{out}");
        // A nullable column keeps the two-space gutter instead of the `*`.
        assert!(out.contains("    email : varchar(255)"), "{out}");
    }

    #[test]
    fn plantuml_stereotypes_views_and_stubs() {
        let mut g = sample();
        g.nodes[0].kind = NodeKind::View;
        g.nodes.push(node("other.audit", NodeKind::Stub, vec![]));
        let out = to_plantuml(&g);
        assert!(out.contains("as users <<view>>"), "{out}");
        assert!(out.contains("as other_audit <<external>>"), "{out}");
    }

    #[test]
    fn plantuml_omits_the_divider_when_a_side_is_empty() {
        let g = DiagramGraph {
            nodes: vec![node(
                "t",
                NodeKind::Table,
                vec![col("id", "int", true, false, false)],
            )],
            ..DiagramGraph::default()
        };
        assert!(!to_plantuml(&g).contains("  --\n"));
    }

    // ── Graphviz ────────────────────────────────────────────────────────────

    #[test]
    fn dot_emits_a_digraph_with_ported_rows() {
        let out = to_dot(&sample());
        assert!(out.starts_with("digraph erd {\n"));
        assert!(out.trim_end().ends_with('}'));
        assert!(out.contains("<b>users</b>"), "{out}");
        assert!(
            out.contains(
                "<td port=\"user_id\" align=\"left\">user_id : int unsigned  <i>FK</i></td>"
            ),
            "{out}"
        );
        assert!(
            out.contains(
                "orders:user_id -> users:id [arrowtail=crowodot, arrowhead=tee, label=\"user_id\"];"
            ),
            "{out}"
        );
    }

    #[test]
    fn dot_marks_an_optional_fk_with_an_open_circle() {
        let mut g = sample();
        g.edges[0].optional = true;
        assert!(to_dot(&g).contains("arrowhead=odot"));
    }

    /// **The four exports must not disagree about the same edge.** `to_dot` used
    /// to end its tail at a bare `crow`/`tee`, which in crow's-foot reads
    /// "one or more" / "exactly one" — asserting that every parent row has a
    /// child, where the `.mmd` and `.puml` of the same graph say `o{`/`o|`
    /// ("zero or …"). The circle belongs on the child end in all of them.
    ///
    /// Spelled as a composite because Graphviz's `o` modifier does not apply to
    /// `crow` or `tee`: `ocrow` parses and draws a plain crow, so the obvious
    /// spelling would have been a silent no-op.
    #[test]
    fn dot_gives_the_child_end_the_same_zero_the_other_formats_do() {
        for (card, tail, mermaid_child) in [
            (Cardinality::OneToMany, "crowodot", "o{"),
            (Cardinality::OneToOne, "teeodot", "o|"),
        ] {
            let mut g = sample();
            g.edges[0].cardinality = card;
            let dot = to_dot(&g);
            assert!(
                dot.contains(&format!("arrowtail={tail},")),
                "{card:?} tail should be {tail}:\n{dot}"
            );
            assert!(
                !dot.contains("arrowtail=crow,") && !dot.contains("arrowtail=tee,"),
                "a bare tail is the mandatory-child reading:\n{dot}"
            );
            // The same edge, in the notation the other two formats share.
            assert_eq!(crow_ends(&g.edges[0]).1, mermaid_child);
            assert!(to_mermaid(&g).contains(mermaid_child), "{card:?}");
            assert!(to_plantuml(&g).contains(mermaid_child), "{card:?}");
        }
    }

    #[test]
    fn dot_escapes_html_label_text() {
        let mut g = sample();
        g.nodes[0].id = "a<b>&c".to_string();
        g.edges[0].to = "a<b>&c".to_string();
        let out = to_dot(&g);
        assert!(out.contains("<b>a&lt;b&gt;&amp;c</b>"), "{out}");
        // ...and the node id itself is the slug, so no raw `<` reaches the graph.
        assert!(out.contains("a_b__c [label="), "{out}");
    }

    #[test]
    fn dot_of_an_empty_graph_still_parses() {
        let out = to_dot(&DiagramGraph::default());
        assert!(out.starts_with("digraph erd {"));
        assert!(out.trim_end().ends_with('}'));
    }

    // ── SVG ─────────────────────────────────────────────────────────────────

    const ICON: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" \
                        fill=\"none\" stroke=\"currentColor\"><path d=\"M12 3v18\"/></svg>";

    fn colors() -> SvgColors {
        SvgColors {
            canvas: "#101010".to_string(),
            card: "#202020".to_string(),
            text: "#e0e0e0".to_string(),
            type_text: "#909090".to_string(),
            key_pk: "#e0b040".to_string(),
            key_fk: "#40a0e0".to_string(),
            edge: "#606060".to_string(),
            muted: "#808080".to_string(),
        }
    }

    fn metrics() -> SvgMetrics {
        SvgMetrics {
            header_h: 30.0,
            row_h: 24.0,
            expander_h: 22.0,
            pad_x: 10.0,
            radius: 6.0,
            title_size: 13.0,
            name_size: 11.5,
            type_size: 10.5,
            edge_width: 1.4,
            icon_size: 13.0,
            title_gap: 7.0,
            row_gap: 8.0,
        }
    }

    fn scene() -> SvgScene {
        SvgScene {
            bounds: Rect {
                x: -28.0,
                y: -28.0,
                w: 456.0,
                h: 212.0,
            },
            nodes: vec![SvgNode {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 78.0,
                title: "users".to_string(),
                stub: false,
                rows: vec![
                    SvgRow {
                        name: "id".to_string(),
                        type_name: "int".to_string(),
                        key: SvgKey::Pk,
                        icon: Some(ICON.to_string()),
                    },
                    SvgRow {
                        name: "email".to_string(),
                        type_name: "varchar(255)".to_string(),
                        key: SvgKey::None,
                        icon: None,
                    },
                ],
                more: Some("+3 more".to_string()),
                icon: Some(ICON.to_string()),
                icon_fill: "#7080a0".to_string(),
                title_fill: "#f0f0f0".to_string(),
                header_fill: "#303030".to_string(),
                border: "#c04040".to_string(),
                divider: "#404040".to_string(),
            }],
            edges: vec![SvgEdge {
                poly: vec![
                    Pt { x: 0.0, y: 0.0 },
                    Pt {
                        x: 10.333_333_3,
                        y: 20.0,
                    },
                ],
                markers: vec![(Pt { x: 1.0, y: 2.0 }, Pt { x: 3.0, y: 4.0 })],
            }],
            colors: colors(),
            metrics: metrics(),
        }
    }

    #[test]
    fn svg_numbers_round_and_trim() {
        assert_eq!(n(10.333_333_3), "10.33");
        assert_eq!(n(1.0), "1");
        assert_eq!(n(1.50), "1.5");
        assert_eq!(n(-0.001), "0");
        assert_eq!(n(0.0), "0");
        assert_eq!(n(-12.5), "-12.5");
    }

    #[test]
    fn svg_root_carries_the_content_bounds_as_the_viewbox() {
        let out = to_svg(&scene());
        assert!(out.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        // Negative origin: a diagram dragged up/left still exports whole.
        assert!(
            out.contains("viewBox=\"-28 -28 456 212\"") && out.contains("width=\"456\""),
            "{out}"
        );
        assert!(out.trim_end().ends_with("</svg>"));
    }

    #[test]
    fn svg_draws_edges_before_cards() {
        let out = to_svg(&scene());
        let edge = out.find("<polyline").expect("edge");
        let card = out.find("<path").expect("card");
        // Background rect aside, an edge running behind a card must be occluded.
        assert!(edge < card, "{out}");
    }

    #[test]
    fn svg_paints_the_background_and_the_edge_geometry() {
        let out = to_svg(&scene());
        assert!(out.contains("fill=\"#101010\""), "{out}");
        assert!(out.contains("<polyline points=\"0,0 10.33,20\"/>"), "{out}");
        assert!(
            out.contains("<line x1=\"1\" y1=\"2\" x2=\"3\" y2=\"4\"/>"),
            "{out}"
        );
        assert!(out.contains("stroke-width=\"1.4\""), "{out}");
    }

    #[test]
    fn svg_writes_a_card_with_a_title_rows_and_the_expander() {
        let out = to_svg(&scene());
        assert!(out.contains(">users</text>"), "{out}");
        // The title clears its glyph: pad (10) + icon (13) + gap (7) = 30.
        assert!(out.contains("<text x=\"30\""), "{out}");
        // PK name takes the key colour; the type is right-aligned and muted.
        assert!(
            out.contains("fill=\"#e0b040\" dominant-baseline=\"central\">id</text>"),
            "{out}"
        );
        assert!(
            out.contains("text-anchor=\"end\" dominant-baseline=\"central\">varchar(255)</text>"),
            "{out}"
        );
        // Row centres: header (30) + (i + 0.5) * row_h (24) → 42, 66.
        assert!(out.contains("y=\"42\""), "{out}");
        assert!(out.contains("y=\"66\""), "{out}");
        // Expander sits below the last row: 30 + 2*24 + 22/2 = 89, and **left**,
        // at the same 10 px inset every column name uses — the painter renders
        // it in a `width_full()` box with `padding_horiz`, so a centred one
        // differed from the canvas once per collapsed card.
        assert!(
            out.contains("<text x=\"10\" y=\"89\"") && out.contains(">+3 more</text>"),
            "{out}"
        );
        assert!(!out.contains("text-anchor=\"middle\""), "{out}");
        // The header divider takes the *theme's* border, not the card's tint:
        // the painter draws them from different colours, and they coincide only
        // for an untinted table.
        assert!(
            out.contains("y1=\"30\" x2=\"200\" y2=\"30\" stroke=\"#404040\""),
            "{out}"
        );
        assert!(
            out.contains("stroke=\"#c04040\""),
            "the card outline is tinted: {out}"
        );
    }

    #[test]
    fn svg_gives_a_stub_no_header_divider() {
        let mut s = scene();
        s.nodes[0].stub = true;
        s.nodes[0].rows.clear();
        s.nodes[0].more = None;
        let out = to_svg(&s);
        assert!(!out.contains("<line x1=\"0\" y1=\"30\""), "{out}");
        assert!(out.contains(">users</text>"), "{out}");
    }

    #[test]
    fn svg_escapes_text_content() {
        let mut s = scene();
        s.nodes[0].title = "a<b>&c".to_string();
        s.nodes[0].rows[0].name = "x<y".to_string();
        let out = to_svg(&s);
        assert!(out.contains(">a&lt;b&gt;&amp;c</text>"), "{out}");
        assert!(out.contains(">x&lt;y</text>"), "{out}");
    }

    #[test]
    fn svg_of_an_empty_scene_is_a_valid_document() {
        let s = SvgScene {
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
            nodes: vec![],
            edges: vec![],
            colors: colors(),
            metrics: metrics(),
        };
        let out = to_svg(&s);
        assert!(out.starts_with("<svg "));
        assert!(out.trim_end().ends_with("</svg>"));
    }

    #[test]
    fn svg_carries_each_card_s_own_header_tint_and_border() {
        let mut s = scene();
        s.nodes[0].header_fill = "#3a2a10".to_string();
        s.nodes[0].border = "#c08020".to_string();
        let out = to_svg(&s);
        assert!(out.contains("fill=\"#3a2a10\""), "{out}");
        assert!(out.contains("stroke=\"#c08020\""), "{out}");
    }

    #[test]
    fn svg_inlines_the_card_glyphs_in_their_row_colour() {
        let out = to_svg(&scene());
        // Header glyph: pad_x in, vertically centred in the 30px header.
        assert!(
            out.contains(
                "<svg x=\"10\" y=\"8.5\" width=\"13\" height=\"13\" viewBox=\"0 0 24 24\""
            ),
            "{out}"
        );
        assert!(out.contains("stroke=\"#7080a0\""), "{out}");
        // The PK row's glyph takes the PK colour, and `currentColor` is resolved.
        assert!(out.contains("stroke=\"#e0b040\""), "{out}");
        assert!(!out.contains("currentColor"), "{out}");
        // The row with no glyph starts its name at the padding, not past a gap.
        assert!(out.contains("<text x=\"10\" y=\"66\""), "{out}");
    }

    #[test]
    fn icon_svg_ignores_markup_that_is_not_an_svg() {
        assert_eq!(icon_svg("", 0.0, 0.0, 13.0, "#fff"), "");
        assert_eq!(icon_svg("<path/>", 0.0, 0.0, 13.0, "#fff"), "");
    }

    #[test]
    fn ellipsize_truncates_only_what_does_not_fit() {
        // One px per char, so the numbers below read as character counts.
        let m = |s: &str| s.chars().count() as f64;
        assert_eq!(ellipsize("users", 10.0, m), "users");
        assert_eq!(ellipsize("users", 5.0, m), "users");
        assert_eq!(ellipsize("customers", 5.0, m), "cust…");
        assert_eq!(ellipsize("customers", 1.0, m), "…");
        assert_eq!(ellipsize("customers", 0.0, m), "");
    }

    /// **The search costs `O(log n)` measurements, not `O(n)`.** `measure` is a
    /// full cosmic-text layout (~10 µs), and this runs once per column row plus
    /// once per card title on the UI thread before the save dialog opens — so a
    /// 500-table diagram was paying for ~20,000 of them, with every truncating
    /// row adding 5–15 more from the walk down.
    #[test]
    fn ellipsize_measures_a_logarithmic_number_of_times() {
        let calls = std::cell::Cell::new(0usize);
        let m = |s: &str| {
            calls.set(calls.get() + 1);
            s.chars().count() as f64
        };
        let long = "a".repeat(512);
        assert_eq!(ellipsize(&long, 4.0, m), "aaa…");
        // 1 for the whole string + ~log2(512); the walk was 509.
        assert!(calls.get() <= 12, "{} measurements", calls.get());
    }

    #[test]
    fn ellipsize_cuts_on_a_character_boundary() {
        // Bytes would split the two-byte `é` and panic on the slice.
        let m = |s: &str| s.chars().count() as f64;
        assert_eq!(ellipsize("héllo", 3.0, m), "hé…");
    }

    #[test]
    fn rounded_rect_path_closes_and_never_inverts_on_a_tiny_card() {
        // A radius larger than the box would otherwise emit negative-length arcs.
        let d = rounded_rect_path(0.0, 0.0, 8.0, 4.0, 6.0);
        assert!(d.starts_with('M') && d.ends_with('z'), "{d}");
        assert!(!d.contains("h-4 "), "{d}");
        let h = header_path(0.0, 0.0, 8.0, 2.0, 6.0);
        assert!(h.starts_with('M') && h.ends_with('z'), "{h}");
    }
}
