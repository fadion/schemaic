# `core/text_ops.rs`

`text_ops.rs` — Ctrl+/ `toggle_line_comment` + `find_matches`/`replace_all`/
`contains_ignore_ascii_case` (find bars). Pure and byte-offset-preserving.
**`find_matches`, `matches_at` and `replace_all` fold non-ASCII letters too** — `É`/`é`,
`Ö`/`ö`, Cyrillic, Greek — where they used to fold ASCII only; `contains_ignore_ascii_case`
(called from `grid`, `history`, `snippet`, `users`, `activity` and `server_vars`) still folds
ASCII only, as
its name says. The fold is per character, a char's lowercase when that is one char
(`fold_char`) — and the word-final sigma `ς` as `σ`, since both have the capital `Σ` and a
per-character lowercase cannot know which one `Σ` stands for, so every Greek word ending in
sigma was found only in its typed case — and **two chars are equal only if they are as wide in UTF-8 as well**. That is
load-bearing: every hit spans exactly `needle.len()` bytes, because that is what the editor's
find bar selects, highlights and replaces by, and what the match highlights in `overlays.rs`
and `widgets.rs` span with `t.len()`. So the few pairs whose cases differ in width — the
Kelvin sign `K` and `k`, `ſ` and `s` — stay case-sensitive rather than report a span that cuts
a character, and the search walks the haystack itself rather than a `to_lowercase` copy, whose
offsets would not be the document's. An ASCII needle keeps the byte-wise path, which matches
exactly what the char walk would, no other character being as narrow as an ASCII one
(`find_folds_the_case_of_non_ascii_letters`, `a_folded_match_never_changes_the_hits_byte_length`).
`fold_for_search` is that fold applied to a whole string, each character replaced only where
its fold is as wide — so the copy keeps every offset, and a `str::find` of one folded string in
another finds what `matches_at` finds, where it finds it. Find Anywhere's body tier searches
bodies folded this way once per schema (`body_search`'s entry); the find bar still walks the
document, which changes under it.
**The find bar's Match case and Whole word are `FindOptions`**, taken by `find_matches_in`,
`matches_at_in` and `replace_all_in`; the three names without the suffix are those with the
default (both off), unchanged in behaviour, so the highlight callers above never see the
options. Every entry point asks one private `hit_at`, so the count, the step and Replace All
cannot disagree about what a hit is. Match case compares the span byte for byte instead of
folding; whole word then refuses a hit with a name byte beside it, by `sql::continues_name` on
`FindOptions::dialect` (the tab's) — the one definition, so `_`, `$` and a non-ASCII letter
continue a word as they do an identifier, and `@`/`#` do on SQL Server; asking
`is_word_byte` alone found `b` inside the column `a$b` and Replace All rewrote it
(`whole_word_ends_a_word_where_the_dialect_ends_a_name`) — asked
only on a side where the needle's own edge is such a byte, so `f(` wants a boundary on its left
alone. Both options keep every hit exactly `needle.len()` bytes, one by comparing the same span
more strictly and the other by only refusing, and the plain functions keep that invariant
still: **`FindOptions::regex` is read only by the hit-based entry points** — `find_hits`,
`hit_holds`, `replacement_for`, `replace_hits` — and `find_matches_in`, `matches_at_in` and
`replace_all_in` ignore it, because a pattern's match has its own length and every caller of
those selects, highlights or replaces by the needle's (`match_case_finds_only_the_exact_spelling`,
`whole_word_finds_the_word_and_not_the_identifier_containing_it`). A **`Hit`** is
`(start, len)` instead. `find_hits` takes an optional `scope` — the find bar's *in selection* —
and in plain mode is `find_matches_between`, the ranged scan `find_matches_in` now delegates to,
which starts at the scope's start while whole word still reads its boundaries from all of
`hay`. In regex mode it compiles through `find_regex` — case-insensitive unless Match case,
`multi_line` so `^`/`$` are line ends, `crlf` so a `\r\n` is one too — a paste from the
Windows clipboard keeps its `\r`, where `$` alone would never match and `.` would swallow it
(`a_crlf_line_end_is_a_line_end_to_a_regex`) — and a compile error turned into one line,
`Not a valid pattern: …`, from the last line of the crate's message, which otherwise draws a
caret under the pattern over several (`a_pattern_that_does_not_compile_says_so_in_one_line`) —
and walks `find_at` over `hay[..hi]`, so `\b` and `^` at the scope's start see the text before
it (`a_scope_confines_both_modes_and_keeps_offsets_absolute`). **That line is a value, not the
sentence**: `find_hits` and `replace_hits` err with `PatternError(Option<String>)`, the crate's
own one-line reason or `None` when it gave none, whose `Display` is the sentence above. The
sentence itself used to reach the bar, so it read *Not a valid pattern* in English in every
language; the bar now words the frame and passes the crate's English through (`pattern_error_text`, under
`editor_pane.rs`). An empty match is skipped, there
being nothing to select or replace (`an_empty_regex_match_is_not_a_hit`), and whole word asks
`word_bounded`, the same test `hit_at` now calls, so the two modes cannot end a word in
different places (`whole_word_bounds_a_regex_hit_by_the_dialects_names`). Both modes stop at
`FIND_HIT_CAP`, 100,000: `.` over a large script matches every character, and a list of
millions is a counter nobody reads and a Replace All nobody meant (`the_hit_list_is_capped`).
`hit_holds` is `matches_at`'s question for a stored hit, and under regex it asks for **the same
span**, not merely a match starting there — the match `find_at` makes at the hit's start must
end where the hit ends, or a number that grew under a `\d+` hit would be replaced only as far
as its old length (`a_stored_hit_holds_only_while_the_text_still_matches_there`).
`replacement_for` is the text a hit is replaced with: literal off regex, where `$1` is two
characters of text; under regex the hit's `captures_at` run through `Captures::expand`, so
`$1`, `${name}` and `$$` work, falling back to the literal text when the match there is no
longer the hit's span (`a_regex_replacement_fills_its_groups`). `replace_hits` is Replace All
over `find_hits`' list, left to right over the original text, so a replacement that itself
matches is not replaced again. `shift_range` carries the scope through one edit: a bound before
the edit stays, one after it moves by the edit's change in length, and one inside the replaced
text lands at the start of what replaced it (the scope's start) or its end (the scope's end),
so an edit inside the scope grows or shrinks it rather than losing it — and **an insertion
exactly at the scope's start counts as inside**, so typing at the head of the block grows it
instead of pushing it along (`a_scope_follows_the_edits_around_and_inside_it`). **An edit that
encloses the scope loses it** — `None`, when the edit replaces all of it and more, as a
whole-document Format does in one delta: where the scope sat inside the replacement is gone,
and mapping its bounds to the replacement's edges would widen it to the whole edit, so that
*Sel*'s Replace All rewrote the document. An edit over exactly the scope is still what
replaced it, and a point at the edit's edge is where it began or ended
(`an_edit_enclosing_the_scope_loses_it_rather_than_widening_to_it`).
`find_seed` and `first_hit_from` are how the editor's find bar opens: Ctrl+F/Ctrl+H seed the
field from the selection only when it is non-empty and on one line — a multi-line selection is
a block being worked on, not a needle, and the field is one line, so it seeds nothing and the
field keeps what it held — and a new query starts at the first hit at or after the selection's
start or the caret, wrapping to the first, rather than at match 0
(`find_seeds_from_a_one_line_selection_only`, `a_new_find_query_starts_at_the_selection_and_wraps`).
`find_scope_seed` is the other half of the first rule: the selection that spans more than one
line — the block `find_seed` declines to make a needle of — becomes the scope to search *in*
(`a_selection_over_several_lines_is_searched_in_not_for`).
`duplicate_lines` is Ctrl+D's: the spanned lines copied as `\n` + lines at the end of the last
one, so the newline-less last line gets a line of its own, with the selection or caret moved
onto the copy (`duplicating_the_last_line_makes_a_new_line`).
`toggle_case` is Ctrl+Shift+U, DataGrip's key: the selection, or on a bare caret the word it
touches — `sql::is_word_byte`'s words, so `user_id` is one and so is a non-ASCII name —
upper-cased unless it holds no lower-case letter, and then lower-cased. It toggles between the
two cases rather than undoing: `Name` goes to `NAME` and then `name`, `ß` to `SS` and then `ss`
— the length change being why a selection is re-selected over its *new* extent, while a bare
caret stays after the same characters: `İ` lower-cases to `i̇`, a byte longer, and `ı`
upper-cases to `I`, a byte shorter, so keeping the caret's old offset left it inside a
character and the next keystroke panicked the rope; the prefix before it is re-cased to find
where it now ends (`toggle_case_keeps_a_bare_caret_on_a_character_boundary`). Nothing to change (no word at the caret, no letters) is `None`, so the pane
makes no edit and no undo step (`toggle_case_upper_cases_then_lower_cases_a_selection`,
`toggle_case_with_nothing_to_change_is_none`). The pane applies it at every caret — a lone one
through `edit_untyped`, several through `per_caret` and `edit_untyped_multi` (*Several carets*,
below), where a caret's `None` only keeps that caret in place rather than refusing the others.
`join_lines` is Ctrl+Shift+J, DataGrip's key again: the caret's line joined with the next, or
every line a selection spans — one ending at a line's start leaving that line out, as Tab's
does. Each break and the blanks around it (spaces, tabs and a CRLF's `\r` before; spaces and
tabs after) become one space, or none after `(`/`[`, before `)`/`]`/`,`/`;`, or where a side
is empty; the left side is read from the output built so far, so a blank line between two
joined lines still leaves one space rather than none. **Only a break in code joins**, which is
why it walks `sql::skip_noncode` — the one boundary lexer, in the tab's dialect: a break inside
a span is a string's or a block comment's own text, and one exactly at the end of a span
opening `-`/`#` is what ends a line comment, whose span stops at the `\n` — joined, the next
line's code would be commented out. Those stay verbatim while the breaks around them still
join; with none left to join it is `None`, so no edit and no undo step
(`join_lines_leaves_a_break_in_a_string_or_ending_a_line_comment`). The caret lands at the
first join, and a selection is re-mapped over the joined text
(`join_lines_joins_every_spanned_line_and_keeps_the_selection`). The pane takes it in
`toggle_case`'s arm, a lone caret through `edit_untyped` and several through `join_lines_all`
and `edit_untyped_multi` (*Several carets*, below).
`toggle_block_comment` is Ctrl+Shift+/, DataGrip's key as well. It **unwraps first**: a walk over
`sql::skip_noncode` looks for the block comment that strictly contains a bare caret, or is
exactly a bare caret's line text — so a caret at either end of a line that is only a comment
still unwraps it — or contains the whole selection, outer blanks aside; the `/*` and `*/` go,
with one space inside each if there is one. Only a span of at least four bytes that opens `/*`
and ends `*/` counts, so an unclosed comment is never unwrapped, nor a MySQL executable
`/*!…*/`, which the lexer returns as code. Otherwise it **wraps** the selection, or a bare
caret's line, as `/* … */` with the outer blanks left outside — refused when that range is
empty, when either end is not in code (`pairs::region_at`: a range starting or ending in a
string would put the comment in the data), or when the wrapped text is not one comment closed
at its own `*/` (`sql::is_one_comment`). That last check is how an inner `*/` is refused on MySQL
and SQLite, where it ends the comment early and leaves the rest as code, and allowed where
comments nest (PostgreSQL, SQL Server) — without asking the dialect a second time. It used to be
`skip_noncode` reaching the wrap's last byte, which an **unclosed** comment does as well: on
PostgreSQL, `a /* b` selected at the end of the document — whose end `region_at` reads as code —
wrapped to `/* a /* b */`, a comment still open. A selection stays over
the comment or the unwrapped text, so a second press undoes the first; a bare caret keeps its
place in the text (`block_comment_wraps_a_selection_and_unwraps_it_again`,
`block_comment_on_a_bare_caret_takes_the_line_or_its_comment`,
`block_comment_is_refused_where_the_wrap_would_not_be_one_comment`). The pane takes it in the
same arm, matched on the physical `KeyCode::Slash` because Shift makes the character `?` on a
US layout.
**`contains_collapsed_ignore_ascii_case` is the same search against a haystack read *as if* its
whitespace runs were collapsed, without building the collapsed string** — exactly
`split_whitespace().collect::<Vec<_>>().join(" ")`, which is what it replaces. The two search
boxes that read SQL want it for the same reason: a statement is stored as the user typed it,
over several lines, and drawn as one collapsed line, so a phrase visibly on screen has to find
the row it is on. Both got there by allocating that collapsed form per candidate per keystroke
(`history`'s since-deleted `full_preview`: a `Vec<&str>` and a `String`, each the size of the
statement). The activity panel is where that bites — see `activity::matches_query` above for the
four things that make it the bad case — and `history::matches_query` shares the helper. The
needle is used as given, since it comes from a single-line search box.
`selected_text` resolves a mirrored byte range against the buffer for the AI panel: the range
comes from the mounted editor while the text comes from the tab's own signal, so the two can
disagree by a keystroke — an empty, reversed, out-of-range or mid-character range yields `None`
("no selection"), never a panic.
**Tab and Shift+Tab are computed here, for hard tabs as well as soft.** `soft_tab_indent` and
`hard_tab_indent` share `indent_lines` — each spanned line indented at its first non-blank
column, blank lines left alone, a selection ending at a line start leaving that line — and
differ only in the pad and in what a bare caret gets (spaces to the next stop, or one `\t`).
Hard tabs used to fall through to floem's `InsertTab`, which **replaces a selection** with one
`\t`: Tab over three selected lines left a single tab character where they had been
(`hard_tab_indents_a_selection_rather_than_replacing_it`). `soft_tab_outdent` already removed
one level of either kind — a leading tab, or up to the tab width in spaces — so `editor_pane`
now asks it whatever the setting, and a plain Shift+Tab no longer reaches floem's
`OutdentLine` and its fixed `Spaces(4)` unit — with the suggestion list open too, which it
closes (it did reach it there, Tab accepting a row and Shift+Tab falling through).
**Enter is computed here too, by `newline_indent`, for the same reason** — floem's
`InsertNewLine` never adds a level, and its `auto_indent` would add four spaces whatever the tab
settings (the two indent units are under *Floem 0.2 gotchas*). The selection becomes `\n` plus
the caret line's indent **cut at the caret**, because floem copied the whole line's indent and
so made Enter inside an indent add to it; one `unit` deeper — `editor_tab_width` spaces, or
`\t` — after an open `(` or `[` in code, trailing blanks allowed; and with the same kind of
closer next after the caret (`(|)`) the closer goes to a line of its own at the old indent, the
caret on the deeper line between — floem split the pair too, but left both lines at the old
indent. At a line's end a deeper next line still lends its indent, floem's rule kept, and a CRLF
line's `\r` counts as its end. **The bracket is asked about at the byte *before* it**: one byte
later, `pairs::region_at` reads a `(` ending a `--` comment as code, at the comment's end
(`enter_after_a_bracket_in_a_string_or_comment_adds_nothing`). **A block-opening word is a
bracket too** (`opens_block`): a line ending, before the caret, in `BEGIN` (T-SQL's `BEGIN
TRY`/`BEGIN CATCH` too), `CASE`, `THEN`, `ELSE`, `LOOP`, `DO` or `REPEAT` gets the deeper
line, and an `END` straight after the caret the closer's own line. The word must be bare and in
code — not `t.begin`, `@case`, a quoted `` `case` `` or one in a comment — and `BEGIN TRAN`/
`BEGIN;` end in another word or a `;`, so a transaction opens nothing
(`enter_after_a_block_opening_word_goes_one_unit_deeper`,
`enter_after_a_word_that_opens_no_block_adds_nothing`). **And the level closes as `END` is
typed**: `dedent_typed_word` answers the letter that makes a line's only word `END` — nothing
after it on the line — with the edit that moves the line to the indent of the line whose `BEGIN`
or `CASE` it closes (on MySQL also the `IF`/`LOOP`/`WHILE`/`REPEAT`/`FOR` its `END IF` and kin
close, rather than the `BEGIN` around them), asked of the text as it will be through
`pairs::match_block`; and the one
that makes it `WHEN` or `ELSE` inside a `CASE` with the edit that puts it one unit under the
`CASE`'s line (`pairs::enclosing_block`, which shares `match_block`'s `block_words`) — the
layout Format gives a `CASE`; an `IF`'s `ELSE` in a `BEGIN` is left alone. `None` leaves the
character to floem (`typing_an_end_dedents_it_to_its_opener`,
`typing_a_case_branch_indents_it_under_its_case`). The pane applies it as the
auto-pair does — floem's own insert suppressed through `read_only`, a Ctrl+K freeze left alone
— and as `InsertChars`, so it undoes with the typing it finished, never alone. With several
carets it bows out and the letter types, undented, at each: its edit is one caret's, and the
`read_only` it sets suppresses floem's insert at all of them, so going on dropped the letter at
every other caret and collapsed them into one
(`the_typed_word_dedent_leaves_several_carets_to_floem`). It plans on the borrowed text
(`completion::with_text`), and `dedent_typed_word` declines before it copies anything unless
the line spells the rest of one of its words — a `d`, `n` or `e` is a large share of all
typing. The pane takes
Enter with no
primary modifier or Alt, after the completion and Ctrl+K arms — so a word typed in full now
reaches it — and leaves several carets to floem's Enter, which handles each one.
**`cut_lines` is Ctrl+X on
bare carets**: each caret's line, line end included, onto the clipboard once and in document
order, a last line with no line end of its own taking the one before it and going onto the
clipboard with a `\n` — and where the cut before it already took that line end (carets on the
last two lines), the run of cuts takes the one before its first line, so no blank line is left
either way. It replaces floem's `DeleteLine`, which never copies — Ctrl+X then
Ctrl+V pasted whatever the clipboard held before — and which, with carets on lines 1 and 50,
deleted every line from 1 to 50 (`cutting_lines_takes_each_carets_line_onto_the_clipboard`).
The pane applies it as one whole-buffer edit through `edit_untyped` and writes the clipboard
only when that edit went through, so a cut the Ctrl+K freeze refuses leaves the clipboard as it
was.
**Several carets are answered here too.** Floem's `Selection` holds one `SelRegion` per caret —
its Alt+click adds one — and the pane's own key actions read the
primary caret alone and set one selection after, collapsing the rest into it. `RegionEdit` is
one caret's share: a replacement in the original text's offsets, and the selection *that edit
alone* would leave. `compose` puts them together as a `MultiEdit` — sorted, each selection
shifted by what the edits before it added or removed — and answers `None` on an overlap, which
no single document edit can make
(`overlapping_edits_are_refused_and_selections_move_with_earlier_edits`). `auto_pair_all` asks
`pairs::auto_pair` at each caret of the original text — pair, wrap a selection, or step over a
closer — a caret with nothing to pair typing the character plainly; it is `None` when no caret
pairs, floem's own insert then serving them all (`each_caret_pairs_where_it_would_alone`).
`backspace_pair_all` is the same for Backspace at bare carets: an empty pair whole, one
character elsewhere, `None` when no caret sits in a pair
(`backspace_deletes_each_carets_empty_pair_and_one_char_elsewhere`). **Both ask the lexer once
for every caret**, not once each: `auto_pair_all` takes every caret's region from one
`pairs::regions_at` walk into `pairs::auto_pair_in`, and `backspace_pair_all` every span it
needs from one `pairs::noncode_spans_at` walk into `backspace_pair_in`, asked only for a caret
whose two bytes look like a pair (`may_be_empty_pair`). Each caret walking from byte 0 was
the quadratic shape `regions_at` exists to remove: a quote typed at a zero-width box down a
3,200-line script measured 600 ms a keystroke, and Backspace there 262 ms, against 1.8 ms and
0.9 ms for the one walk (release). `many_carets_pair_exactly_as_each_would_alone` and
`noncode_spans_at_answers_position_for_position_what_noncode_span_at_does` hold the batch to
the lone answers. **Ctrl+Shift+U and Ctrl+Shift+/ go through `per_caret`**, each caret's
`toggle_case`/`toggle_block_comment` asked of the original text. Composed plainly, one caret
with nothing to change — a word with no letters, a blank line — answered `None` and refused the
keystroke at every caret, and two carets in one word or on one line asked for the same edit
twice, an overlap `compose` refuses. So a caret whose op answers `None` now only keeps its
place, carried through the others' edits by `moved_by` (clamped into an edit it falls inside);
identical edits are made once, each caret keeping its own selection; and the answer is `None`
only when no caret changes anything or two *different* edits overlap
(`a_caret_with_nothing_to_change_does_not_stop_the_others`). **Tab goes through `indent_all`**,
which cannot refuse: each caret's `soft_tab_indent`/`hard_tab_indent`, with the edits that
would overlap — two selections on one line, a bare caret inside another's selected lines —
grouped and their lines indented **once** by `indent_lines`, every caret in the group moved by
the indent added before it (`tab_with_overlapping_carets_indents_once_and_keeps_them_all`).
Before, that overlap was `compose`'s refusal, and Tab fell back to the primary caret and dropped
the rest. An `IndentEdit` converts to a `RegionEdit` (`From`), and both helpers share
`compose_members` with `per_line_block_each`, below. The
line operations — Ctrl+/, Ctrl+D, Alt+↑/↓, Shift+Tab's outdent, Ctrl+Shift+J's join — go through
`per_line_block`, which runs the existing single-range function once per `line_blocks` block:
carets whose lines overlap **or touch** merge into one, so two carets on one line comment it once
rather than twice, and neighbouring lines move as one block
(`carets_on_touching_lines_make_one_block`, `a_line_operation_acts_once_on_each_carets_block`).
Each block is asked of the original text, and one refusing refuses all — Alt+↑ with a caret on
the first line moves no block. **A block keeps its members**, each caret's own region:
`per_line_block` hands the op the members' extent and leaves one selection a block, which is
Ctrl+/'s answer alone (it selects the commented lines, as a lone caret's does), while
`outdent_all`, `move_lines_all`, `duplicate_lines_all` and `join_lines_all` — Shift+Tab,
Alt+↑/↓, Ctrl+D, Ctrl+Shift+J — map every member through the block's edit, so three carets on
touching lines are still three carets after it. Before that they became one selection over the
block, and the next key replaced everything between them. Move and duplicate slide each member
by the distance the block's first line went; outdent takes off what each line lost before the
caret, never past its line's new start; join joins the union of the breaks each caret would
join alone — carets on two touching lines join three lines — with a bare caret at its own
line's join and a selection mapped break by break (`every_caret_in_a_line_block_survives_its_edit`).
A block of several members is given its whole lines (`LineBlock::lines`, through the next line's
start), because the members' extent can end on a bare caret at a line's start, which every line
operation reads as a selection leaving that line out. A lone member is asked exactly as a lone
caret is. `toggle_line_comment` and `move_line` answer with the whole text,
so `RegionEdit::between` narrows each answer to the span that changed, backing the common prefix
and suffix off to char boundaries so the narrowed edit never splits a character
(`a_whole_text_rewrite_narrows_to_what_changed`). The pane applies a `MultiEdit` through
`edit_untyped_multi` (under *Floem 0.2 gotchas*); where `compose` refuses, Ctrl+/ and Ctrl+D
fall back to the primary caret as before, while Alt+↑/↓, Ctrl+Shift+U and Ctrl+Shift+/ do
nothing — and Tab, through `indent_all`, is never refused.
**`box_regions` is Alt+Shift+drag's box selection** — one region a line from the upper of the
anchor's and the head's lines to the lower, each across the columns between the two, with the
caret on the head's side (dragged leftwards a region is `(right, left)`). Columns are counted in
characters, the editor font being monospaced; a tab counts as one, a known approximation, and a
CRLF line's `\r` is not a column. A line with nothing inside the box — no longer than its left
column — is left out, while a box of no width puts a caret on every line, at the line's end
where it is shorter; never empty, the head alone at worst
(`a_box_selection_takes_the_same_columns_on_every_line`). There is **no virtual space**: the
head is the clamped offset under the pointer, so dragging past a short line's end narrows the
box to that line's length. It runs on every pointer move of the drag, so it finds only the box's
own line starts — back from the upper end to its line's start, forward to the lower end — where
a table of the whole document's lines was a full pass of the text per move, 6.8 ms at 16 MiB
against microseconds now. The pane's drag is under `editor_pane.rs`.
