# `core/pairs.rs`

`pairs.rs` — caret-driven, boundary-aware editor highlights + auto-close pairs (via
`skip_noncode`): `auto_pair` (auto-close `()`/`''`/`""`/`` `` `` [MySQL] at code positions, wrap a
selection, type-over a closer/quote already at the caret — respects string/comment regions and
word-adjacency guards), `backspace_pair` (delete both halves of an empty pair — one the lexer
agrees is one: a quote pair only where its first half opens a string or quoted name of exactly
those two bytes, a paren pair only in code, so `'it'|'s'` and `'f(|)'` lose one character,
`backspace_takes_only_an_empty_pair_the_lexer_agrees_is_one`), `bracket_jump` (Ctrl+Shift+\,
matched on the physical key: the caret to the same side of `match_paren`'s partner as it was
of the bracket beside it, so a second press returns —
`a_bracket_jump_lands_beside_the_partner_and_comes_back`), `match_paren` (the paren or square
bracket adjacent to the caret + its partner, ignoring both in strings/comments — so the pair
highlight and the jump reach a PostgreSQL array or slice too. **Each kind pairs only with its
own**: a `]` never closes a `(`, and a close at the caret with nothing open or the other kind
innermost has no partner; a stray close anywhere else is passed over rather than popped, so it
cannot take the pair from the close of the right kind — `(a])` still pairs `(` with `)`,
`match_paren_pairs_square_brackets_by_kind`. Where `[name]` is a quoted identifier, SQL Server
and SQLite, `skip_noncode` skips it and it pairs nothing), `match_block` and `block_jump` (the
same two questions asked of words: `BEGIN`/`CASE` and its `END`, as word ranges, the pane
asking them when no bracket answers. A transaction's `BEGIN` — `BEGIN TRAN`, `TRANSACTION`,
`WORK`, `BEGIN;` — opens nothing, `END CASE` is an `END`, and `t.end` is a name. **On MySQL a
compound statement is a block too**: `IF`, `[label:] LOOP`, `WHILE`, `REPEAT` and MariaDB's
`FOR` open one, and the `END` of its `END IF`/`END LOOP`/… closes it, matched on the one stack
innermost first — so the jump, the partner highlight and a typed `END`'s dedent all reach
them. **An opener counts only where a statement starts**: the text's first word, after a `;`
or a label's `:`, or after `BEGIN`/`THEN`/`ELSE`/`DO`/`LOOP`/`REPEAT` — `block_words` records
that per word from the last code token, a comment not counting and a string counting. That is
what keeps out the `IF()` and `REPEAT()` functions, `IF [NOT] EXISTS`, `FOR UPDATE` and a
trigger's `FOR EACH ROW`; and an `IF (` is the statement only when the word after its matching
`)` is `THEN`, since `CASE WHEN a THEN IF(b,1,2) END` starts the `IF` exactly where a statement
would (`a_compound_statement_matches_its_named_end`,
`a_word_that_is_no_compound_statement_opens_nothing`). Whether an engine does this is
`names_its_block_ends`, an exhaustive match that answers `true` for MySQL alone: PostgreSQL's
`END IF` lives in a dollar-quoted body, a literal to the lexer; T-SQL's `IF`/`WHILE` take one
statement or a `BEGIN … END` and have no `END` of their own; SQLite has no compound
statements. Where it is `false` an `END IF`/`LOOP`/`WHILE`/`REPEAT`/`FOR` closes a block this
does not track and is passed over, as it was everywhere before. It runs on every caret move,
so it asks the caret's own word first and scans only when that is one of the eight block
words — `a_block_word_matches_its_end`,
`a_word_that_opens_or_closes_no_block_matches_nothing`,
`a_block_jump_lands_on_the_same_side_and_comes_back`. Inside a PostgreSQL `$$` body the
lexer sees a literal, so nothing there pairs; `enclosing_block`, the innermost block still open
at an offset — an open compound statement included
(`an_open_compound_statement_encloses_what_follows`) — reads the same `block_words`),
`identifier_occurrences` (every whole-word, ASCII-case-insensitive occurrence of the identifier
under the caret — a name being its whole spelling on its engine, so on SQL Server `@id` is not
the column `id` — excludes keywords/numbers/strings, needs ≥2 to fire), `region_at`
(`Code`/`Str`/`Comment` classification), and `bracket_depth_at_lines` (how many code `(`/`[`
are open where each line starts, one entry per line — the depth the editor's rainbow brackets
start a line's lexing at, under `sql_highlight.rs`. A string or comment over several lines is
skipped whole by `skip_noncode`, so each line it starts is at the depth outside it; a stray close
never takes the depth below zero; and a SQL Server/SQLite `[name]` is a quoted name and counts
nothing — `bracket_depth_is_counted_at_each_line_start`. **It keeps one depth for both kinds**,
where `match_paren` pairs each only with its own, so on mismatched input such as `(a])` the
colours and the pair highlight can disagree; the depth only ever chooses a colour. It is now
`line_depths(..).depth`: `line_depths` makes the same one pass and also records whether each
line starts in code (`LineDepths { depth, code }`), which is what lets `line_depths_after` take
the previous revision's answer forward over an edit instead of walking the whole document — a
full pass measured 3.0 / 12.5 / 52 ms at 1 / 4 / 16 MiB (release, a realistic script) per
keystroke on the UI thread. It resyncs as `sql_highlight::carried_lines_after` does, restarting
at the last line before the edit that starts in code, at its depth, and stopping at the first
line start past the edit that is in code in both texts — **and at the same depth in both**. That
third condition is load-bearing: a stray close saturates at zero, so a depth below an edit
is not the old one plus a constant offset, and only an equal depth makes the old tail true. An
edit that leaves a bracket open past itself changes every depth below and rescans to the end.
Both functions run through one scan, `scan_depths`, and
`line_depths_after_an_edit_match_a_full_pass` holds them equal over a corpus of edits on every
engine, bracket-heavy bases included. A line a string or comment starts is never in code, even
one starting right where the span ends: no closed span ends in a `\n`, so that is an
unterminated span at the end of the text — `/* a\n`, the ordinary way a block comment gets typed
— and marking its empty last line code restarted the next keystroke's scan outside the comment,
every depth below it one too deep until a full rebuild
(`line_depths_after_typing_into_an_open_comment_match_a_full_pass`; the corpus only ever
appended to a closed text). `line_depths_after` finds the edit by comparing the old
text (`text::common_affixes`) and hands it to `line_depths_after_edit`, which takes the edit
already known — as the tab editor knows it from its `text::EditLog`, with no old text kept. The
lines an edit moved come from the two line counts (`count_newlines(new) + 1` against `prev`'s),
not from the old edit region, which the `_edit` form never sees; the same test runs every corpus
edit through it again at half the shared front and back, as merged edits hand them over.
Called once per edit from `SqlStyling`'s cache, not on the per-caret path below).
Pure + unit-tested; dialect-aware, and its two "is a
backtick an identifier quote here" sites ask `SqlDialect::backtick_ident` (`pub(crate)` for
them) rather than re-spelling `!= Postgres`, which is the capability rule one module over.
**Three of those walk the document, and all three were on the editor's *undebounced*
per-caret-move path** — `match_paren` and `identifier_occurrences` were two `create_effect`s with
no debounce and no size cap, and `sqlfile::open_verdict` will open a 64 MiB script — so each
stops as early as its answer allows. `identifier_occurrences`' answer is the whole document, so
it cannot stop early (2 / 8 / 32 ms at 1 / 4 / 16 MiB per move); it is the one taken off that
path instead: the effect in `editor_pane.rs` asks it 80 ms after the caret settles (a
generation, `occ_gen`, retiring superseded asks), and clears the boxes at once on an edit — the
text's length moved, so their offsets have — or when the caret leaves them. `region_at` stops **at the caret**: the answer is settled
once the walk reaches `offset`, since a span opening at or after it cannot contain it, and
everything past that was a scan of the rest of the document for nothing (at 16 MiB the full walk
measured 37.9 ms, so one arrow key was ~123 ms of UI-thread work across the two effects).
`match_paren` returns **at the pair** rather than after every pair: it used to build a `Vec` of
every balanced pair in the document and then `find_map` the one touching the caret, 44.6 ms at
16 MiB for a single arrow key. The walk is the same walk — one stack, the shared lexer skipping
strings and comments — and the pair it picks is unchanged, closes still being visited in
increasing order with a position the open or the close of exactly one pair;
`match_paren_returns_the_carets_pair_not_the_first_one_closed` separates "the first pair that
closes" from "the caret's pair" in the three shapes a return placed one line too early would
confuse. `paren_pairs`, the whole-document sweep both of those went through, is **deleted** —
there is no second caller to keep it for, and leaving it is how the quadratic spelling comes
back.
**`regions_at` is `region_at` for many offsets in one walk**, because asking `region_at` in a
loop is quadratic and the loop was real: Ctrl+/ inside a PostgreSQL `$$ … $$` body called it
once per selected line, 223 ms over a 3,200-line selection, quadrupling with every doubling —
around 11 s extrapolated at 1 MiB, on the UI thread, for one keystroke. Every one of those
walks re-scanned the same prefix. Its `offsets` must be non-decreasing, the answers come back in
the same order, and each is exactly what `region_at` would have said — an offset the walk passes
without finding a span that strictly contains it is `Code`, which is `region_at`'s own rule read
forwards.
