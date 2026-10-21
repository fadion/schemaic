# `ui/routine_editor.rs`

`routine_editor.rs` — the **stored routine** modal: one form for a function or a procedure,
over `core::ddl`'s `RoutineDraft`, on the three engines that have them. Reached from the schema
tree's Functions/Procedures folders (row **Edit**, folder **Create**), from the database and
namespace **Create** submenus, from Find-Anywhere, and from the trigger editor's
**New function** / **Edit** buttons — which is the path it was born on and the reason
`open_for_new_trigger_function` is a separate entry point from `open_for_new`: a trigger
function starts from a different draft (`plpgsql`, `RETURNS trigger`, and the `RETURN` that a
first one most often fails at runtime for want of). Same chrome, same
seed-local-signals-then-write-back rule and same `ddl_preview` ending as the editors above —
**with one exception, and it is the Body field.** Its text lives on `DdlUi::routine_body`
(`ui.ddl.routine_body`), owned
outside the form and written by `bound_field_on` (`bound_field` over a caller's signal) rather
than by a view-local one, because a field only the user writes is not what this is: MySQL's
`SHOW CREATE` reply lands after the form is up and has to correct it. Routing that correction
through the overlay's own `dyn_container` key meant the modal was torn down and rebuilt to
deliver it — the seeding *was* the delivery, so the write order inside `done` was load-bearing
with nothing saying so and nothing testing it — and a reply arriving mid-word took the caret
with the old scope. `edit_field` now reconciles its doc from the signal in place. The
write-back contract is unchanged: `prev` is `None` on the first run, so seeding is never
mistaken for an edit, and an external correction reaches the draft exactly as a keystroke does.
The field is SQL-coloured in the target's dialect (`FieldCfg::sql`, under `lib.rs`).
Three things are load-bearing: **the form is per-engine *and* per-kind because the objects
are** — volatility, strictness, language and per-routine `SET` clauses on PostgreSQL;
determinism, data access, definer and comment on MySQL, which has no `LANGUAGE` clause at all;
and neither volatility nor strictness on a PostgreSQL *procedure*, whose `CREATE` rejects both
outright — so it hides what an engine can't express rather than offering it and failing at
apply, which is a rule the kind axis needs as much as the engine one. The Language field is
offered where `ddl::routine_takes_language` says so — PostgreSQL's alone, where the clause is
required — and `RoutineDraft::validate`'s "needs a language" asks the same predicate. The dropdown
carries the routine's own language alongside the two it proposes, for the same reason: a list
that didn't would silently retype a `plpython3u` function the moment the control was touched.
**SQL Server's options are an arm of their own**, and that is the fix as much as the feature:
the section was an `if pg … else`, and the `else` would have offered a SQL Server routine
MySQL's determinism, data access and definer, none of which T-SQL has. It is a `match
target.dialect` now, and the SQL Server arm offers **Recompile** (procedures only — the walk
reads `RECOMPILE` on a procedure alone), **Schema binding** (not on an ordinary procedure,
which refuses it), **Execute as** (one field over
`ExecuteAs::parse_field`, shared with the trigger form), and on a scalar function **Returns NULL
on NULL input** and **Inlining** — a dropdown rather than a toggle, *Automatic* / *INLINE = ON* /
*INLINE = OFF*, because unstated leaves inlining to the server — then **Comment** (the
`MS_Description` property). Which of the middle four show is `TsqlShape::allows`, read **once at
build** like the rest of the form, so a `RETURNS` edited into another shape is the validator's to
name rather than a form that rebuilds under the user. The two toggles write through
`TsqlRoutine::set_option` and the rest through the slot setters, each holding one value in place,
so an option the form does not touch keeps its place and is restated untouched; the null-input
toggle captures the routine as opened, because `set_null_input`'s "off" is the spelling the
original had, and a toggle flipped and flipped back is no change. The
Parameters and Returns placeholders are a `match` per engine too, SQL Server's naming the `@`
its parameters take and the `TABLE` a table function returns.
**MySQL's body is fetched a
second time and it is not an optimisation** (`Db::routine_source` via `RoutineSrcFn`, gated on
`ddl::catalogue_body_is_unescaped` as the trigger editor's fetch is, applied
to *both* sides of the diff so a routine doesn't open already-changed, and guarded by
`DdlUi::session` so a slow reply for a closed modal can't land); and **a new routine's
namespace is inherited, not chosen** — there is no Schema field, for the reason the view
editor has none, and the title is where the inheritance is disclosed. It clears every sibling
overlay's target on open **except the trigger editor's**, which is the handoff described
above.
**Its words are the catalogue's** (`routine-editor-*`) — sections, fields,
every option toggle and its hint, Inlining's *Automatic*, *Settings* and *Add setting*, the
titles and notes, and the comment placeholder, which is the new shared
`editor-comment-placeholder` — with Cancel, Preview SQL and the change count the shared
`dialog-cancel`, `editor-preview-sql` and `editor-change-count`. **The title is one message per
kind**, edit or create × function or procedure
(`routine-editor-title-{edit,create}-{function,procedure}`), where it had been
`format!("Edit {} …", RoutineKind::label())` — the sentence
assembled from an English noun that `database_editor`'s titles stopped being for the same
reason. Of the placeholders only SQL Server's Returns (*int, or TABLE*) and Execute as have
words in them, so those two go through `FieldCfg::placeholder_of`, Returns' still a `match` on
the dialect; the rest are SQL samples and stay, as do the keyword choices (`VOLATILE`,
`CONTAINS SQL`, `INLINE = ON`, …). The local `bound_toggle` takes its label and hint as
`String`. The footer's refusals and Preview's readiness read `RoutineDraft::problems` and
`clash`, not their English wrappers `validate` and `name_clash` (the editors' refusals, under
`i18n.rs`): `routine_problem_text` words each problem from `routine-problem-*`, a message per
routine kind where the sentence names the kind and per `TsqlShape` × natively compiled for a
refused `WITH` option, whose name stays SQL; `routine_clash_text` words the clash from
`routine-clash-{function,procedure}-{refused,destroys,destroys-new}` — *destroys-new* the
sentence for a clash with no `original` to name. Both are held by
`english_says_every_routine_problem_as_core_does`, the clash against `RoutineClash::message`.
