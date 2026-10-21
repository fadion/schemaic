# `core/body_search.rs`

`body_search.rs` — Find Anywhere's **fourth tier**: the objects whose stored *body* contains the
term, for the palette (`ui/overlays.rs`) to list below its three name tiers. `body_hits_where`
walks one `DbSchema` — a view's `TableInfo::view_definition`, a trigger's `TriggerAction::Body`, then
`RoutineInfo::body` and `EventInfo::body`, in that order and catalogue order within each — and
stops at the caller's `limit`, **counting only the hits its `keep` predicate accepts**, so the
rows a caller would drop cannot spend the cap (`hits_a_caller_drops_do_not_spend_the_limit`);
`body_hits` is it keeping everything. Each `BodyHit` is its `BodyOwner` (`View`, `Routine`, `Event`, or
`Trigger` with the table it fires on) and a snippet. **Everything it reads is already in the
loaded `DbSchema`**, so the tier costs no round trip — and so a MySQL routine is searched in
`information_schema`'s escape-resolved `body`, not the `RoutineSource` its editor reads lazily
(the one field `DbSchema`'s own doc warns is not trustworthy for an edit). A PostgreSQL trigger is
`TriggerAction::Function` and has no body of its own; its function's body is found as a routine
(`a_postgres_trigger_has_no_body_of_its_own`).
**It is a different question from the name tiers, and so it does not go through
`schema::object_name_matches`**: "does this definition contain the term", not "does this name".
The invariant that every schema-search surface matches *names* through the one predicate is
untouched, because nothing here answers for a name. The predicate is asked only to **skip an
object a name tier already lists** — a view, routine or event whose own name matches — so the
object appears once, in the more precise tier. A trigger is never skipped: no name tier lists
triggers, so its body is the only way the palette reaches one
(`an_object_a_name_tier_lists_is_skipped`, `a_trigger_body_is_a_hit_even_when_its_name_matches`).
The match is a plain case-insensitive substring over the body **as stored**, comments and string
literals included — deliberately: a body search is how you find the routine that mentions a
table in a comment, and a lexer-aware search would hide exactly that. It folds case by
`text_ops::matches_at`'s rule, the editor find bar's, non-ASCII letters included
(`the_match_ignores_case_on_both_sides`), **on the term as typed** — the caller must not
lowercase it first: `İ` lowercases to two characters and `ẞ` to a narrower one, and the rule
pairs characters of equal width, so a lowered `İstanbul` found no body spelling it as typed
(`a_term_is_matched_as_typed`; the skip below lowercases a copy for `object_name_matches`).
**The cost is a scan of every body on every
keystroke**, the palette being undebounced, and **that scan is over a copy folded once per
schema** — `BodyIndex`, built on a schema's first body search and kept in
`DbSchema::body_index` (a `BodyIndexCache` whose clone starts empty, since a schema is cloned
to be changed: `a_cloned_schema_does_not_search_its_sources_bodies`). Each body is folded by
`text_ops::fold_for_search`, which keeps every byte offset, and searched with `str::find`.
Asking `matches_at` at every character instead cost, in release over 10,000 routines of 4 KB,
17–30 ms a keystroke for an ASCII term and 166–204 ms for one starting with a non-ASCII letter;
the index takes about 20 ms to build once and then 4–8 ms a keystroke for either. That the two
searches agree is `the_folded_search_finds_what_the_find_bar_rule_finds`, over Turkish, German,
Cyrillic, Greek, titlecase digraphs, the Kelvin and Ohm signs, ligatures and CJK. Even so,
`searches_bodies` waits for
`MIN_TERM_CHARS` (3) — one or two letters match nearly every body in a database — counted in
characters rather than bytes, so three Cyrillic letters qualify as three ASCII ones do
(`a_term_shorter_than_three_characters_searches_no_body`). `BODY_HIT_CAP` (50) is the palette's
cap on the tier. `body_snippet` is the row's text: the matching line, trimmed, and when longer
than `SNIPPET_CHARS` (60) cut to a window starting `SNIPPET_LEAD` (20) characters ahead of the
match — slid back when it would run past the line's end, so a match near the end keeps more than
20 ahead of it and the window stays 60 characters (`a_match_near_either_end_is_cut_on_one_side_only`) —
with an `…` on each side that was cut, on character boundaries, so a multibyte line is never
split (`a_multibyte_line_is_cut_on_character_boundaries`). **The window is measured on the line
as trimmed, before its whitespace runs are collapsed to one space**: the collapse happens as the
window's characters are copied out, so both the 60-character test and the window count raw
characters, and a line padded with runs of spaces or tabs can come out under 60. The window
is counted out of the line rather than collected: a MySQL view's definition is one line, and
gathering it into a `Vec<char>` cost four bytes a character of the whole definition per hit.
**It also answers the palette's sixth tier, the open tabs**, over text no schema holds:
`text_hits(texts, needle, cap, budget)` returns one `TextHit { index, at, snippet }` per text
that contains the term, at its *first* match, in the order given, at most `cap` of them
(`TAB_HIT_CAP`, 20). The rule is the body tier's — the term as typed, folded by the find bar's
rule through the private `first_match` (which `body_snippet` calls too), comments and strings
included, nothing under `MIN_TERM_CHARS` — and the snippet is `body_snippet`'s, cut by the same
`snippet_at` (`a_tab_holding_the_term_is_one_hit_at_its_first_match`,
`the_tab_tier_folds_case_as_typed_and_waits_for_three_characters`). **What differs is that
nothing here is indexed**: a tab's text changes with every keystroke in its editor, so there is
no `BodyIndex` to fold it into once, and `first_match` walks it with `matches_at` as it stands.
`budget` is what bounds that — `TAB_SCAN_BUDGET`, 4 MiB of text per palette keystroke across
every tab — and a text longer than what remains of it is **skipped, not truncated**, while a
smaller one after it is still searched (`the_tab_tier_stops_at_its_cap_and_its_byte_budget`).
The trade is stated plainly: a script larger than the budget is never found by this tier,
because the alternative was a full case-folding walk of it per character typed, on the UI
thread. A skipped text spends none of the budget; a searched one spends all of its length,
match or not. `TextScan` (`new`/`done`/`offer`/`finish`) is the same scan one text at a time,
and `text_hits` is a loop over it: the palette's texts each sit behind a signal of their own,
which a `&[&str]` could only gather by cloning every tab.
