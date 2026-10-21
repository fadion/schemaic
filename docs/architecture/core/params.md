# `core/params.rs`

`params.rs` — `:name` query parameters: `scan`/`names` (every placeholder and its byte range,
built on `skip_noncode` so a `:id` inside a string, comment, dollar-quoted body or quoted
identifier is not one), `bindings_for` (the parameters bar's rows, re-derived from the SQL on
every edit and carrying across the values already typed), `substitute` (the statement that
actually runs) and `neutralize` (the same text with each `:` rewritten to `_`, byte offsets
preserved, so `intel` can parse a statement mid-parameterisation and only the placeholder ranges
lose their diagnostics). **Named placeholders only** — `?` is a live PostgreSQL JSON operator and
`$1` collides with dollar-quoting. **Substitution is textual, not driver binding**, because
`ORDER BY :col`/`LIMIT :n` are what the feature is for and no driver binds those; values are
quoted through `export::sql_literal`, and `ParamValue::Raw` is emitted verbatim by design.
That last point is why **the run guard must be given the *substituted* statement** — a `Raw`
value otherwise carries a write past a `run_verdict` that read a statement the engine never
receives. `ParamValue` is deliberately not `Serialize`: a value is session-only and never
reaches `tabs.json`. Three lookalikes have named regression tests, since each is one missing
line from a false positive: PostgreSQL's `::` cast (consumed whole — skipping one byte leaves
the shape of a placeholder), MySQL's `:=`, and a PostgreSQL array slice `arr[lo:hi]`.
**The slice rule is the one that keeps growing, and every gap in it is the same failure: an
unrunnable statement, not a cosmetic one.** A phantom row means `prepare_run` answers
`Err(Missing)`, which stops the run *before* the guard and so offers no "Run anyway" — there is
no way through the app at all. Three shapes say "subscript": a word byte immediately before
(`arr[1:2]`), a `]` immediately before (`m[1][2:3]`), and being **the first `:` inside an open
`[`** — `Brackets::slices`, for PostgreSQL's open-ended lower bound `arr[:hi]`, which all
three existing slice tests had missed by writing a lower bound, and for a bound with a space
before the colon, `arr[lo :hi]`, which the first rule cannot see past. The first two look at the
byte *immediately* before rather than the last non-space one, since `SELECT :a` has its `T` a
space away and skipping the space would swallow every placeholder in the language. The `[` rule
asks the caller's `Brackets` — the `(`/`[` the code so far leaves open, fed every **code** byte by
the two callers' own forward walks through `skip_noncode` (`scan`, `sqlfmt`'s tokenizer), and
closed by a `\n` or `;` since a slice is not written across lines. It used to walk *back* over
raw bytes to the line's start, so the `[` in `'see item[' AND id = :id` read as an open
subscript and `:id` vanished from the bar
(`a_bracket_in_a_string_or_comment_hides_no_placeholder`). Only the
*first* colon in it is the slice's, so `arr[1 : :n]` keeps its `:n`
(`a_spaced_slice_colon_is_not_a_placeholder`). It then has to ask **what the bracket hangs
off**, which is the whole of `hangs_off_subscriptable`: `ARRAY[…]` is a *constructor*, not a
subscript, and a rule reading every `[` as a
slice reported `SELECT ARRAY[:a, :b]` as carrying only `:b` — the bar offered one row and
`substitute` left `:a` in the statement verbatim. `arr[` and `ARRAY[` are the same two bytes;
only what precedes them tells them apart, which `word_before` answers. Subscriptable hosts now
include a **quoted identifier** (`SELECT "my arr"[:hi]`, whose quotes `skip_noncode` has already
consumed, so only the closing byte is left to look at — and on SQLite a `[quoted identifier]`'s
`]` is the same byte and the same answer) and a **parenthesised expression or call**
(`(arr)[:hi]`, `(t.arr)[:hi]`, `f(x)[:hi]`, all legal and verified on PG 16.15), with
`ARRAY(SELECT …)` excluded for the constructor reason above. The paren walk refuses an
unbalanced input rather than guessing, which is the direction that keeps a real parameter
visible.
**All of that is one predicate, `opens_placeholder`, and it is `pub(crate)` because a second
module was re-spelling half of it.** `sqlfmt`'s `:name` token arm asked only "is the next byte an
identifier byte", so `a[lo:hi]` lexed as one token and `need_space` wrote it back as `lo :hi` —
which this module's rule then read as a placeholder (it no longer does, the spaced slice being
one of the shapes above), so Format Code created a query parameter by moving a space and the
run then stopped at `Err(Missing)`. One definition, two callers: whatever the slice rule grows
next, the formatter grows it too.
**A sqlcmd command line is skipped whole** on an engine whose scripts carry them
(`has_sqlcmd_commands`, an exhaustive match: SQL Server alone): a `:` first on its line naming
one of sqlcmd's commands — `:setvar Region West`, `:r .\seed.sql`, `:on error exit` — is the
script talking to sqlcmd, and the bar asked for parameters `setvar`, `r` and `on`
(`a_sqlcmd_command_line_is_not_a_parameter`). Not first on its line, or not a command, a `:name`
is a placeholder as anywhere else.
`prepare_run` is the pair the run action calls — substitute, then `sql::run_verdict` on the
result — so the ordering is structural rather than a rule a caller has to remember, and
`strip_param_diagnostics` drops the reports `neutralize`'s own rewrite caused. Pure +
unit-tested.
The UI half: `Tab::params` is the bar's store (session-only — no `SavedTab` field, and
`ParamValue` has no `Serialize` derive), `editor_pane::params_bar` renders one row per distinct
name under the editor, and `guarded_run`/`guarded_run_all` in `schemaic-app` substitute with it.
Rows are keyed by **name**, not value, so typing into a field never rebuilds the field being
typed into. **The names are read once typing settles** (150 ms, a generation retiring the
asks it overtook), not per keystroke: `params::names` scans the whole document, measured 9.7 /
20 ms at 4 / 8 MiB (release) — more than every other per-keystroke pass together — and is set
only when the names change. **Tier-2 live validation is skipped for a statement holding a placeholder**, and
`intel::parses` does not cover that: `sqlparser` accepts `:id` as a placeholder in all three
dialects, so the statement parses, the round-trip happens, and the squiggle is the *server*
complaining about text the user is still filling in. A placeholder in an identifier position
(`FROM :tbl`) is the case that does not parse — which is what `neutralize` exists to rescue.
