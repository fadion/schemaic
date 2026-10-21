# `cli/compare.rs`

`cli/compare.rs` — `schemaic compare`'s decisions, apart from the reading so they are tested
without a server: which objects the script covers, and what is said about it. **No differ of its
own** — the comparison is `core::compare`'s, the one the app's *Compare with* builds,
`SchemaComparison::of` over two freshly introspected schemas (`run.rs` reads them), and the
script is `SchemaComparison::plan`'s. `report(cmp, tables, target, source)` is pure and returns
a `Report { script, notes }`. With no `--table` it plans everything (`plan(|_| true)`); with
some, it plans the union of each named object's `selectable_keys_of(schema, name)` — the rule a
comparison opened from one table node ticks by, so a table brings the triggers on it and
`orders` does not bring `orders_archive` (under `core/compare.rs`). **A name resolves three ways, in
order**: as the comparison labels it (`orders`, `sales.orders`), then as `schema.name` — so
`public.orders` finds the entry whose label leaves `public` off — then by bare name if only one
namespace has a table or view of it, all three compared case-insensitively where the target's
names are (`names_ignore_case`). **A name that resolves to nothing is an `Err`**, which `run.rs`
makes exit 2, rather than a plan of nothing: a typo would otherwise read as "they agree"
(`an_unknown_table_is_refused`). A bare name two namespaces share is refused with both labels,
asking to be qualified rather than guessed (`a_namespaced_table_resolves_by_label_or_qualified_name`).
**`script` is `plan.export_script()`, and `None` when the plan is empty** — an empty plan's
script would be a header alone and read as a migration to run
(`agreeing_databases_print_no_script`). **So an empty stdout means "nothing to script", not
"they agree"** — the first stderr line says which. `notes` are the stderr lines, and the first
has three arms: "the script makes X match Y: N objects — it has not been run"
(`plan.subject()`) when the plan holds anything; "no differences[ in t1, t2]: X already matches
Y" only when the plan is empty **and nothing in scope is left out**; and "nothing to script[ in
…]: X differs from Y only in what is left out below" when the plan is empty but something in
scope is — the CLI's counterpart of the app's `EmptyRows::AgreesExceptNoted`. The middle arm
used to be the whole of "empty", and a MySQL comparison whose only difference was a trigger body
`needs_source` keeps out printed "no differences: … already matches …" over a `left out:` line
naming that trigger (`differences_only_left_out_are_not_called_no_differences`). Then
`destructive: …` for each of `plan.destructive()` and `not in the script: …` for each of
`plan.unsupported()`. **The `left out: …` lines are not `plan.omitted()`**, which they were:
that list is built over the whole comparison, so `-t a` listed a trigger left out on another
table under "no differences in a" (`named_tables_narrow_the_left_out_lines_too`). `report`
builds them from `cmp.differences()` through `plan`'s own two exclusions, `needs_source() ||
unplannable()`, and an `in_scope` predicate — everything with no `--table`, else the named
objects matched as `selectable_keys_of` matches them (the same namespace, and the object
itself or a trigger whose `table` is it) — each line being `CompareEntry::omission_note`, made
`pub` for this (under `core/compare.rs`). The comparison's own `notes()`, which are about the reading
rather than any one object, follow as `note: …` lines whatever the scope. Both tests were
watched fail against the old `report`. Nothing here runs anything; the script is printed for a
person to read.
