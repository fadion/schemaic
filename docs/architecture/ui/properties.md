# `ui/properties.rs`

`properties.rs` — the **table properties** modal (`properties_overlay`), opened by setting
`overlay.properties` — from a Table or View row's context menu, or from the RESULTS title bar's
Properties icon for a result that names one table; an effect in the modal calls
`SchemaActions::table_stats`, whose result lands in `overlay.properties_state`. Both entry
points go through `open_for_table`, which takes the **connection explicitly** rather than
reading the active one: a query tab keeps the connection it was opened on, and the fetch keys on
`db_for(target.conn_id)`. Sizes, row
estimate, the storage-split bar, table options and the index list — deliberately **not**
structure, which the tree, the designer and Generate DDL already show three times over. The
column/key counts and the collation come free from the in-memory `TableInfo`
(`table_designer::loaded_table`), which is what gives a view something to show on an engine
that publishes no statistics for one.
**Off `whole_ui_gate`'s list, 5 to zero, on a `PropertiesCtx` — and the footer is why.** The
panel itself reads only `overlay` and the loaded schema, but its **Edit** routes to the table
designer or the view editor, so it wants what those two doors want — `ConnUi`, `SchemaUi`,
`DdlUi` and the `ViewAlgoFn` the view editor's door ends in — on top of the four arguments
`footer` already carries. Named one by one that is nine parameters, past clippy's seven: the
lint
and the gate's "take the child bundle" agree, and the ctx is what they agree on. `PropertiesCtx`
holds eight things and no `Ui` — `overlay: OverlayUi`, `schema: SchemaUi`, `conn: ConnUi`,
`ddl: DdlUi` and the `stats`/`count_rows`/`count_cancel`/`view_algorithm` closures — built once
by naming the reads at its one call site, `PropertiesCtx::new(ui.overlay, ui.schema, ui.conn,
ui.ddl, &ui.schema_actions)` in `modals.rs`. `open_for_table` stayed off it for
`users_view::open_for_server`'s and `import_view::open_import`'s reason a fourth time — a door
that only writes the modal's own signals has no business cloning four `Rc`s to do it — so it
takes `OverlayUi`, and both its call sites (the table context menu in `overlays.rs`, the results
toolbar's `open_properties` callback in `lib.rs`) pass `ui.overlay`. **The detail worth
recording
is `stats_body`, which reads nothing at all**: it took a `&Ui` purely to carry it down to
`count_row`, whose two actions are the only reads on that path, and it carries a
`&PropertiesCtx` now — the same shape, honestly labelled. A parameter that exists only to be
passed on is still a dependency the signature is claiming, and that one was claiming the whole
bundle.
- **Qualifying the figures is the feature.** An estimate prints `~4.21m` via
  `RowCount::label`, `Freshness::note` prints the staleness caveat in words, and an index is
  called unused only where `IndexStats::is_unused` says so — worded with its window attached,
  because the counter resets when the server does. An *uncounted* index says "usage not
  counted"; blank would read as none.
- **Count rows** (`SchemaActions::count_rows` → `Db::count_rows`) is a button because it is an
  uncapped `COUNT(*)`. Its result is folded into the loaded `TableStats::exact_rows` rather
  than kept beside it, so the headline and the Markdown copy read one figure. Its lifecycle is
  separate from the fetch's (`properties_counting`/`properties_count_err`) for the opposite
  reason: a failed count must not replace statistics that loaded. **It is offered from every one
  of `stats_body`'s arms past `Loading`, and the `Failed` one used to be the exception.** A `COUNT(*)` needs no
  catalogue — which is `count_row`'s whole argument, made twice more in this module and once in
  `stats::count_rows_sql` — so the state with least to show is not the state to withhold the one
  figure still obtainable from; and it was the only arm in which `CountHint::Error` could never
  appear, so a count error raised elsewhere and then followed by a failed re-fetch was
  swallowed. The decision itself already had a core test
  (`the_count_row_disappears_only_when_it_has_nothing_to_say`); the gap was the view's
  composition with it, which no test here can reach. The slow warning or the error beside the
  button is one `widgets::ellipsized_text` line with the whole of it in a tooltip, and the
  button's face is `flex_shrink(0)`: in a narrow modal the hint clipped mid-word before.
- **A MySQL view reached the full `Loaded` arm on a creation timestamp alone**, printing two
  em-dashes under a staleness caveat about figures the server never publishes. The fix is in
  `stats::TableStats::has_any`, so the panel and the Markdown copy move together — see
  `core/stats.rs` for the measurement and for why it could not be fixed in this file. **The
  `!has_any()` arm calls `options_section` too**, which it did not when that predicate stopped
  counting the timestamps: the arm written for the object with no statistics then dropped the
  `Created`/`Updated` rows, which on that view were the only thing the server had said about it.
  `options_section` is reached from two arms, not only the full `Loaded` one, and its own
  emptiness guard is what keeps it silent on an engine that publishes nothing.
- **A *Partitioning* section follows *Options*** (`partition_section`), from the same two arms:
  `TableInfo::partition_facts(PARTITIONS_LISTED)` — the key and up to 24 partitions with their
  bounds, then how many more — or, on a partition, the table it is a partition of and its bound
  (under `schema.rs`), each row worded by `partition_line_text` from `props-partition-of`/
  `-bound`, `props-partitioned-by` and the plural `props-partitions-more`
  (`english_says_every_partition_line_as_core_does`). It reads the in-memory `TableInfo`, not the statistics, and its own
  emptiness check leaves it out for a table that is neither.
- Both requests outlive a close, so each checks `overlay.properties` still holds the target it
  was asked about before writing.
- **The panel's own words are the catalogue's** (`props-*`), so `section`,
  `section_with_gap`, `detail` and `swatch` take a `String`. The structure counts are Fluent
  plurals and the vacuum note a message, its percentage rounded with `{pct:.0}` before it
  reaches Fluent, which would otherwise print the fraction (`the_shared_counts_read_as_they_did`
  holds both to the old English). Core's index facts and freshness caveat are worded here:
  `index_fact_text` over `stats::index_fact_list`'s `IndexFact`s, `freshness_text` over
  `Freshness`, `props-index-unused` for `unused_note` and `row_caption_text` for the row
  tile's `row_caption` (`english_says_every_stats_note_as_core_does`), their figures in the
  interface's marks — sizes through `i18n::bytes` (imported here as `format_bytes`), counts
  as `RowCount::label` prints them through `i18n::figure` — and the index line's three
  (size, cardinality, scans) through `index_fact_in`, which `text::remark`s each in the marks
  it is given. The cardinality arrives already printed (`~1.25k`), and passed through raw it
  was the one figure on the line a German reader took for ~125k. The staleness window's age is
  worded too, through `i18n::staleness_age(stats::span(secs))` (`props-age-*`): it used to be
  `format_age` as core prints it, on the reasoning that whole units have no mark to move — but
  the units are language, not marks, and `d` is not German (`3 Tg.`). The copied
  Markdown stays core's English.
