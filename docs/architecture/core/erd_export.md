# `core/erd_export.rs`

`erd_export.rs` — the ER diagram's **export** renderers, one per format, all pure (the UI half
is `ui/erd_view.rs`'s scene builder + `ui/erd_raster.rs`). Two families, answering different
questions. **Text** — `to_mermaid`, `to_dbml`, `to_plantuml`, `to_dot` — emits the *graph*:
every node with all its columns and every FK with its cardinality, ignoring positions and
collapse state, because those are properties of this app's canvas and mean nothing to the tool
on the other end. **Picture** — `to_svg` — renders the *arrangement*, from an `SvgScene` the UI
fills from its live signals rather than re-deriving any geometry, so the file cannot drift from
what is on screen; PNG is that same document rasterised, with no second renderer anywhere.
The formats that need a bare identifier (Mermaid entities, PlantUML aliases, Graphviz ports)
go through `aliases`, which is **collision-broken on purpose**: `sales.orders` and
`sales_orders` both slug to `sales_orders`, and a Mermaid file naming two entities the same
silently merges them into one card with interleaved columns and no error anywhere. DBML and
Graphviz keep the real names, quoted or escaped.
**`column_names` is that same rule one level down, and it had to be**: sanitising is lossy, so
distinct columns collapse onto one name — `slug` maps every non-ASCII character to `_`, so a
Japanese table's `名前`, `年齢` and `住所` all become `__`, and even in ASCII `user id` and
`user_id` both become `user_id`. What that produced is not cosmetic: a Mermaid entity listing
one attribute three times misrepresents the table, and a Graphviz label declaring one `port`
name three times makes `from:port -> to:port` an **ambiguous endpoint**, so the FK attaches to
whichever row `dot` resolves first. Collisions take `_2`, `_3`, … in column order, so the
mapping is deterministic and both ends of an edge agree — which is why the map is returned once
rather than each site sanitising again. It takes the emitter's own `sanitize`, and DBML's is
split in two for it: `dbml_inner` is the text an identifier carries and `dbml_wrap` the quoting
decision, so the uniquifying suffix lands **inside** the quotes. Appended to a rendered `"a'b"`
it would sit after the closing quote — and `a"b` and `a'b` are both legal column names on every
engine while `plain_label` rewrites `"` to `'`, so the two arrived as one name: a table body
declaring the same column twice, and a `Ref` naming a pair the foreign key is not on.
`crow_ends` is the shared cardinality notation
and reads the *parent* end from `DiagramEdge::optional` — a nullable FK means a child may
reference nothing, so "exactly one" drops to "zero or one" — while the child end is "zero or
more" (`o{`) or "zero or one" (`o|`) from the uniqueness `core::erd` already worked out: zero
either way, since nothing obliges a parent row to have children. **Three of the four text
exports say that the same way.** Mermaid and PlantUML take the two strings as they are;
Graphviz cannot, so
`to_dot` spells the child end as the composite `crowodot`/`teeodot` — the `o` *modifier* applies
only to Graphviz's fillable primitives, so `ocrow` parses and then draws exactly like `crow`,
and in a composite the later shape sits farther from the node, which is where crow's-foot puts
the zero. Without it the `.dot`'s tail read "exactly one" and asserted every parent row has a
child, which the `.mmd` and `.puml` of the same diagram explicitly deny. **DBML is the fourth
and says none of it**: its grammar has no optionality notation at all, so
`Ref: orders.user_id > users.id` is the whole vocabulary and `to_dbml` never calls `crow_ends`
or reads `DiagramEdge::optional` — it matches on the cardinality alone, writing `>` for
many-to-one and `-` for one-to-one, and a nullable FK produces the same line as a `NOT NULL`
one. Nothing is lost: the nullability survives as the column's own `[not null]`, which is where
a DBML reader looks for it. The **canvas** is the
one surface that draws no zero at the child end, and deliberately: on screen that marker would
be on every edge of every diagram without exception, so it separates nothing and costs twenty
more stroked segments per edge on the app's heaviest paint — `crow_ends` and `erd_view`'s
`marker_lines` each carry that argument in their rustdoc, so read both before changing either.
`mermaid_type`
exists because a space ends Mermaid's type token: MySQL's `int unsigned` and PostgreSQL's
`timestamp with time zone` would otherwise turn the rest of the line into a parse error.
`to_dot` deliberately emits **no `pos` attributes**: they bind only under `neato -n`, so a file
claiming to carry the user's layout would silently ignore it under plain `dot`.
On the picture side, `n()` writes coordinates at two decimals — not cosmetic, since a bezier
sample formats as `104.30000000000001`, an edge polyline carries 34 of them, and the noise is
both most of the file and what makes two exports of one layout undiffable. Per-card colours
(`header_fill`, `border`, `title_fill`, `icon_fill`) live on `SvgNode` rather than in
`SvgColors`, because a table with a `db_color` identity colour wears it on the canvas and an
export that flattened every card to one grey would lose the only thing telling them apart at a
glance. `icon_svg` inlines the UI's Lucide glyphs as nested `<svg>` elements and resolves their
`currentColor` to a real value, which nothing in a standalone document would otherwise set.
`ellipsize` is here but takes the measurer as an argument: the core has no fonts, and the
truncation has to fall on the same character the card's own `text_ellipsis` falls on.
