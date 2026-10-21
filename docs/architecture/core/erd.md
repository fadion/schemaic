# `core/erd.rs`

`erd.rs` — the **ER-diagram** model (the UI half is `ui/erd_view.rs`). `build_graph` turns an
introspected `DbSchema` into a `DiagramGraph` — nodes = tables, edges = FKs — seeded either by
`DiagramSeed::Database` (whole database, hiding FK-less "island" tables) or `::Table` (one
table's one-hop neighbourhood). A cross-database FK target can't be enumerated from one
`DbSchema`, so it becomes an unexpandable `NodeKind::Stub` rather than a missing edge.
**A sequence is not a node at all**, and the filter is the `drawable` closure —
`shape() != TableShape::Sequence` — applied at every point the node set is built rather than at
one of them. MariaDB lists a `CREATE SEQUENCE` in `information_schema.TABLES`, so it arrives in
`schema.tables`, and this asked `is_view`: the diagram drew a card carrying the sequence's eight
internal counter columns (`next_not_cached_value`, `minimum_value`, …) and offered relationships
on it, and — having no foreign key in either direction — it was *also* counted as an FK-less
island, so the "N unrelated tables hidden" note was partly about an object the diagram has
nothing to say about. Leaving it out costs the diagram nothing, since it can be neither end of
an edge. `a_sequence_is_left_out_of_the_diagram_entirely` pins the node set, the island list and
the table-seeded case, where naming a sequence as the seed now yields an empty graph.
A PostgreSQL **foreign table** is drawable and is drawn as the table it reads as
(`NodeKind::Table`), its columns being real ones. The `match` on
`shape()` names all four shapes rather than taking a `_`, so a fifth is a compile error there.
**Nor is a partition whose parent the schema holds** (`DbSchema::lists_under_parent`, in the
same `drawable` closure): it is the parent's rows under the parent's columns, and its key is the
parent's cloned, so drawn, every partition repeated the parent's card and drew one more edge into
the same table. A `DiagramSeed::Table` naming a partition seeds its topmost ancestor's diagram
instead (`DbSchema::listed_as`) (`a_partition_is_drawn_as_its_parent`).
**Nor is a key the server cloned an edge** (`ForeignKeyInfo::inherited`, skipped in the seed's
referenced sweep, its referencing sweep and the edge loop alike). PostgreSQL clones a key onto
a partitioned table once per partition on the *referencing* table, each clone pointing at a
partition — and a partition is no card, so every clone became a `NodeKind::Stub` for a table
the schema holds, one per partition per referencing table. The original key is the edge drawn;
on a partition the clone is its parent's, whose card already carries it
(`a_key_cloned_onto_each_partition_draws_no_stub`).
`fit_toolbar` is the diagram **toolbar's responsive rule**, and it is here rather than in the
view because it is arithmetic over widths: given the room available and what each optional
group costs, it drops them in a fixed priority order — the count pills, then the zoom stepper,
then the export button — and returns a `ToolbarFit`. **The pills go one at a time, rightmost
first**: `chips` is each pill's cost left to right, `ToolbarFit::counts` is how many are shown
from the left, and the most that fit beside zoom and export are kept before zoom is touched.
It was a `bool` once, and the three vanishing together on one pixel of narrowing read worse to
the user than losing them one by one (`groups_drop_in_priority_order` walks each step; a
diagram with no pills still drops zoom then export —
`a_toolbar_without_chips_still_drops_zoom_then_export`). The order is least-useful-first: the pills
restate what the diagram already shows, the zoom stepper duplicates Ctrl+wheel and its
percentage is only a readout, and export is the one of the three with no other way to reach it.
The scope breadcrumb and the Fit/Reset pair never drop — those two are the *recovery* controls,
and a diagram panned off screen is unusable without them. Each width the caller passes is what
the toolbar **loses by hiding that group**, the flex gap included, since taffy drops a
`Display::None` child before the line the gaps are distributed over is built. An unmeasured
toolbar (0) shows everything, so the first frame is one of overflow rather than a stripped bar
that fills in. Four properties are pinned: the drop order, that the result is always a *prefix*
of it (never a hole), that widening never hides a group (a non-monotone fit would flicker one
in and out under a window drag), and the unmeasured arm.
`should_collapse`/`collapsed_visible` decide per-card density (a wide or crowded table collapses
to a pinned PK/FK subset) and `column_row_offset` is what still anchors an FK edge to the right
row on a collapsed card. `layout`/`place` are a deterministic layered auto-layout (nodes layered
by longest FK-dependency chain, ordered by neighbour barycentre), so the same schema always
arranges the same way; `edge_anchors`/`cubic_controls`/`sample_cubic`/`nearest_polyline` are the
pure bezier geometry + hover hit-test the custom paint canvas uses. `DiagramLayoutsFile`
persists manual drags to `diagrams.json`, falling back to auto-layout
for an unknown or stale id, and **the seed is part of the key**: `layout_key(conn_id, database,
seed)` gives a table diagram a record of its own, because positions are not comparable across
seeds — a table diagram's come from `place()` over *its* four nodes on *its* canvas, a database
diagram's from `place()` over sixteen. With one key per database the two shared a slot, so
pressing **Reset layout** inside a table diagram wrote four origin-adjacent coordinates into the
database diagram's record, and reopening that put those cards on top of whatever the user had
arranged there. `DiagramSeed::Database` keeps the old spelling byte for byte, so every layout
already in `diagrams.json` is still the database diagram's and no migration is needed; only the
table diagrams gain a record, and they had none worth keeping. `clear_conn_layouts`' `{conn_id}:`
prefix still bounds the file either way. That file is read **lazily**, from inside `erd_view`'s drag handlers
rather than at startup, which is why all three of its readers — the diagram's layout read, the
save-side re-read inside the drag's persist closure, and `delete_conn_now`'s layout prune — go
through `schemaic_ui::load_diagram_layouts`, which reports what the read recovered in the same
call; see `core/persist.rs` for the contract they were breaking and for the gate that now holds
them to that one door.
**Find-in-diagram** is the pure half of the modal's Ctrl+F bar. `search(graph, needle)` returns
one `NodeMatch { node, name, columns }` per node the term touches, kept per-card because both
things the diagram does with a search are per-card: highlight the matched parts of a card, and
pan to it when every hit landed in one. The term is trimmed and lower-cased here so the UI stays
a thin caller, and every name goes through `schema::object_name_matches` rather than a third
hand-rolled `to_lowercase().contains` (see the one-predicate invariant); an empty or
whitespace-only term finds nothing, which is that predicate's rule. A `NodeKind::Stub` answers
on its name alone, having no columns to search. `hits()` counts the name as one plus one per
matched column, `total_hits` and `match_count` — a `MatchCount { hits, tables }`, `match_label`
its English — are the bar's readout, worded by `erd_view::match_count_text` from `erd-find-*`
(`english_says_the_find_readout_as_core_does`), and `sole_node` — the
pan-and-flash trigger — asks about **cards, not hits**: three matches inside `orders` still name
one place to go, while two cards leave the choice to the user and moving the canvas would only
be guessing which was meant. The stated limitation is that a collapsed card shows only its key
columns, so a matched column can be real and off-screen — the count is the truth about the
diagram and the card outline is what says where to look. Auto-expanding the card to reveal it is
deliberately not done: that resizes the card and re-routes every edge touching it, as a side
effect of typing. `search_tests` covers case and surrounding space, the empty needle, a stub,
hits versus cards, and each form of the readout.
