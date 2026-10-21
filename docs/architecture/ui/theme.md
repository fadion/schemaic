# `ui/theme.rs`, `themes.rs`, `icons.rs`, `fonts.rs`, `sql_highlight.rs`

`theme.rs`/`themes.rs`/`icons.rs`/`fonts.rs`/`sql_highlight.rs`. `theme.rs` is the call-site
surface (named colour fns, the type scale, `header_h`/`footer_h`, `scaled`/`scaled_font`);
`themes.rs` holds the data and the three runtime axes, including `UiScale` and the pure
`scale_at`/`scale_font_at` rounding; `icons.rs` sizes every glyph, scaling the **base** size its
callers pass.
**Its words are the catalogue's** (`theme-dark`/`theme-light`) —
`UiThemeKind::label`, the interface-theme dropdown's, answers a `String`; `key` is what is
persisted, so nothing saved changes. `EditorThemeKind::label` stays a literal: product names.
**The editor's font size and tab width are healed where they are *loaded*, not only where they
are stored**, which is what `clamped_editor_font`/`clamped_tab_width` and
`EDITOR_FONT_MIN`/`EDITOR_FONT_MAX`/`DEFAULT_EDITOR_FONT` are for (defined in `themes.rs`,
re-exported from `theme.rs` like the rest of the axis). `ui_state.json` carries both as raw
numbers under `#[serde(default)]`, which fills in an *absent* field and validates nothing, so a
hand-edited or newer-build config reached the editor unchecked: a `0` font size laid every SQL
editor out at zero, `-5` did the same (`as usize` saturates), and `1e40` deserialises to
`f32::INFINITY` and saturates the other way to `usize::MAX`. None of it is reachable through the
UI — Ctrl+scroll clamps — but a hand-written config is a real input class here, the reason
`parse_hex` carries a non-ASCII guard. The clamp used to live only inside `set_editor_tab_width`,
so the *stored* value was corrected and the source of it never was: the editor indented by 8
while the signal, the Settings picker and the file all went on saying 99, and `save_ui` writes
those signals back, so a clamp that does not reach them never sticks. `clamped_editor_font`
answers **NaN with the default** rather than passing it through, because `f32::clamp` returns
NaN for a NaN input (and panics on a NaN bound) and `serde_json` will hand over a NaN for a bare
`nan` token.
`sql_highlight.rs` decides nothing about *where* a token is — both halves of that are
`core::sql`'s, and both used to be re-derived here. `lex_line_with` asks `skip_noncode` for the span
and `noncode_kind` for what the span **is**, instead of classifying it by its opening byte with
no dialect (which greyed out every PostgreSQL dollar-quoted body as a comment — see `core/sql.rs`
for the whole of that), and its word scan asks `sql::is_word_start`/`is_word_byte` rather than an
ASCII rule of its own: this was invariant 11's first site outside `core/sql.rs`, and it painted
`select` as a keyword *inside* the legal column name `éselect`, spans that reach the Ctrl+K diff
through `highlight_spans` too. `is_keyword` folds ASCII only, so a whole non-ASCII word now
correctly matches nothing. **What a word is, it asks the engine first**:
`intel::word_class` reads core's lists for the dialect — a builtin of its catalogue *called*
(a `(` follows on the line) or one it calls bare is a `Tok::Function`, a type where only a type
can stand (after a PostgreSQL `::`, or given a size, `VARCHAR(40)`) a `Tok::Type`, `TRUE`,
`FALSE`, `NULL` and `UNKNOWN` a `Tok::Constant`, each in the theme's own colour for it, and
a word the engine reserves a keyword. Its answer wins over the fixed `is_keyword` list where it
names a function, type or constant, so `COUNT(` and `VARCHAR(40)` stop being keyword-coloured
while `LEFT` before `JOIN`, not called, stays one; a column named `date` or `text` is a type
nowhere it is not one (`a_word_is_classed_by_what_it_is_on_the_engine`,
`functions_types_and_constants_take_their_own_colours`). A table named like a builtin is no
call before its column list — `INSERT INTO log (`, `CREATE TABLE log (`, `REFERENCES log (` —
which `word_class` asks of `names_an_object`, signature help's answer to the same question
(`a_table_named_like_a_builtin_is_not_a_call_before_its_columns`). A word runs on through `sql::continues_name` (`a$from` is one name),
a word straight after a `.` is a member, and a variable (`@x`, `@@version`) or a T-SQL temporary
table (`sql::t_sql_name_prefix`: `#t`, `##t`) is a name — none is a keyword however it is spelled
(`a_keyword_spelling_after_a_dot_an_at_or_in_a_dollar_name_is_not_coloured`). The variable
then wears the editor theme's `variable` colour — it used only to be kept from being a keyword —
while the temporary table keeps the default, as a bare table name does. **An `@` is a variable
only where `intel::has_at_variables` says the engine names them so** — PostgreSQL's `@>`,
`<@`, `@@` and `@` are operators — and the name after the sigil runs through
`sql::continues_name`, not over any `@`/`#`: on MySQL a `#` glued to `@id` opens a comment, as
the document pass already read it, and the line lexer took it for part of the name and then
opened a string at the comment's `'` (`an_at_is_a_variable_only_where_the_engine_has_them`).
So does a `:name`
placeholder, which is found by `core::params::scan` — the parameters bar's own rule, not a
second scanner — run over the line from where code starts (past the close of a region carried
in from the line above, or a directive). That rule is what leaves a PostgreSQL `::` cast, MySQL's `:=`, an array
slice and a `:x` in a string or comment uncoloured, and a placeholder spelled like a keyword
(`:limit`) is a placeholder, not a keyword (`variables_and_placeholders_are_coloured`). A number is
coloured whole by `number_end` — `0x1F`, `1.5E-3`, an `e` counting only with a digit after it —
and a one-letter literal prefix glued to its `'` (`N`, `E`, `X`, `B`) takes the string's colour
(`whole_numbers_and_literal_prefixes_are_coloured`). A client directive line — `GO 5`,
`DELIMITER $$` — takes the keyword colour, argument and all, from `sql::line_directive_span`,
which asks `go_directive`/`delimiter_directive` of the line alone: whether a statement begins
there is the splitter's question, and here only a colour rides on it, so a mid-statement line
opening `delimiter x` is coloured as one. A line that starts inside a region carried from above
is never asked (`a_directive_line_is_coloured_as_a_keyword`).
**Every non-code region carries across lines, not only `/* */`.** The styling hook lexes one
line at a time, so what a line starts inside has to be handed to it: `carried_lines` makes one
forward pass over the document's `skip_noncode` regions — string, dollar-quoted body, quoted
name, comment — and gives each line that begins strictly inside one a `LineCarry`, the region's
`noncode_kind` and the byte column just past its close on that line (`None` when it runs on).
`lex_line_with` colours that lead as the kind — a literal the string colour, a comment the
comment colour, a quoted name the default, as one opened on its own line is — returns there if
the region runs past the line, and otherwise lexes the rest as code. This pass used to carry
block comments alone (`block_comment_lines`, now a `#[cfg(test)]` narrowing of it kept for the
tests that pin that case), so a multi-line string's second line was lexed as code and a
PostgreSQL function's `$$` body as keywords. Where a comment closes is the dialect's, since
`skip_noncode` decides it: `/* a /* b */⏎c */` carries into its second line on PostgreSQL,
where comments nest, and not on MySQL (`every_noncode_region_carries_across_lines`).
**After an edit the pass is taken forward, not redone.** Once per revision was still a whole
document per keystroke on the UI thread — measured at 3.5 / 14 / 59 ms at 1 / 4 / 16 MiB
(release, a realistic script, 200-table catalogue). The `CarryCache` keeps the text its answer
was computed for, and `carry_at_cached` hands the previous revision's answer to
`carried_lines_after` while the dialect is unchanged; a new dialect, or the first ask, is the
whole `carried_lines`. It finds the edit by comparing the two texts from each end — a memory
compare — restarts the scan at the last line at or before the edit that starts in code
(`prev[line] == None`, so its start lies before the edit and is identical in both texts), and
stops at the first line start past the edit (its newline in the common suffix) that is in code
in the new text *and* whose old counterpart, shifted by the edit's net newlines, started in
code too; the old answer stands for every line after. Both halves of that test are needed — the
old answers below a line that began inside a region describe that region, not the new text. An
edit that opens a comment or string rescans as far as the change runs, which for an unclosed
`/*` is the rest of the document. `carried_lines_after_an_edit_match_a_full_pass` holds it
equal to the full pass over every edit of a corpus — each base document edited at every byte by
inserting `/*`, `*/`, `'`, a newline, `$$`, `--` or a bracket, or deleting one or three bytes —
on all four engines, and again through `carried_lines_after_edit` at half the shared front and
back. **The tab editor keeps no copy of the text for it.** `carried_lines_after` above is
`FieldSql`'s path, and it now only finds the edit and calls `carried_lines_after_edit`, which
takes the edit already known and counts the lines it moved from the two line counts
(`count_newlines(new) + 1` against `prev`'s length) rather than from the old edit region.
`SqlStyling::following_edits(log, query)` hands the tab editor's styling the pane's
`text::EditLog` and `query`, and `carry_at` then keys its cache on the log's revision instead of
`cache_rev`, takes it forward with `log.since(rev)`, and borrows the text from `query`
(`with_text`, a copy only if `query`'s length is not the document's). Keying on `cache_rev` cost
twice over: the cache kept its own copy of the document per edit, and `inline_diff::set_ghost`
and `set_preview` bump `cache_rev` without changing a byte, so every ghost the completion
recompute published re-ran the pass. **The cache key also holds the document's line count**,
and that is not redundant: an edit the log somehow missed cannot leave a stale answer standing.
A merged front and back longer than the text, or an answer whose line count is not the text's,
falls back to the whole `carried_lines` — the second being what a log that missed an edit would
produce.
`highlight_spans` and its interface-palette sibling `ui_spans` still lex one line with no carry —
neither has a document — so a string or comment spanning a preview's lines is coloured line by
line there. The Ctrl+K diff's added
rows go through `highlight_lines_in`, which takes each row's carry from the document the rows
land in (`inline_diff::HunkCarries`, under `inline_diff.rs`), so a hunk that starts inside a
string or comment the lines above opened is coloured as one. A context that is not one carry
per row falls back to lexing the rows as one text, carrying only what one row opens into the
next — which is all `highlight_lines`, now a wrapper over it with no context, does
(`a_run_of_lines_carries_its_regions`).
**An inlay hint moves every span after it.** `SqlStyling::with_hints(hints)` hands the tab
editor's styling the pane's `inline_diff::HintSignal`, and `apply_attr_styles` shifts each lexer
span by `inline_diff::phantom_shift` — the summed byte lengths of the line's `drawn_hints` at or
before the span's start. Floem hands the hook the line's *pre-phantom* columns and lays the spans
over the combined text (`new_text_layout`, in vendored floem's `views/editor/mod.rs`), which an
end-of-line Ctrl+K block never disturbs and a mid-line label does: unshifted, every colour after
a hint would sit the label's length early. `drawn_hints` is the very call the document draws
from, so the two cannot disagree about which hints a line has (under `inline_diff.rs`); and since
none is drawn while a Ctrl+K view is up, the hints' shift and a `before` block's `prefix_len`
never both apply. `FieldSql` takes no hints.
**Rainbow brackets** (Settings → Editor, off by default — `UiState::editor_rainbow_brackets`,
`theme::editor_rainbow_brackets`) are the one colour that depends on lines other than its own.
`lex_line_with` takes the depth its line starts at, or `None` while the setting is off, which
leaves brackets uncoloured exactly as before; with a depth, each code `(`/`[`/`)`/`]` is a
`Tok::Bracket(depth)` — an opener the depth it opens at, a closer its opener's — painted in turn
from the editor theme's own `type_`, `keyword` and `function` (`[..][depth % 3]`) rather than a
palette of its own (`rainbow_brackets_are_coloured_by_depth`). The starting depths are
`pairs::line_depths`, cached in `SqlStyling::bracket_lines` per edit-log revision, dialect and
line count (`depth_at`) beside `block_lines`, the carried regions above, and taken forward over
an edit by `pairs::line_depths_after_edit` (under `pairs.rs`) as `block_lines` is by
`carried_lines_after_edit`, borrowing `query` the same way and falling back to the whole pass on
the same two conditions. Cached for the same reason: the styling hook
runs per visible line on every relayout, and re-walking the document before each line is the Θ(visible × document) cost `block_lines` was added to remove.
It is computed only while the setting is on. `set_editor_rainbow_brackets` bumps `editor_gen`,
which `SqlStyling::id()` folds into the layout cache key, so a flip re-colours lines already laid
out. `highlight_spans` has no document, so each standalone line of the Ctrl+K diff starts at
depth 0 and a bracket opened on an earlier line is unknown there. The previews outside the editor
took the same path until they moved to `ui_spans`, which colours no bracket at all (under
`FieldSql` below). In the editor a bracket inside a string continued from an earlier line is
part of the carried lead and goes uncoloured; it was coloured as code while only block comments
carried, though the depth each later line starts at never moved, the cached count having
skipped the string whole.
**Visible whitespace and indent guides** (Settings → Editor, both off by default —
`UiState::editor_show_whitespace`/`editor_indent_guides`, pinned by
`whitespace_and_indent_guides_default_off_and_keep_an_on`; `theme::editor_show_whitespace`/
`editor_indent_guides`, whose setters bump `editor_gen` as `set_editor_rainbow_brackets` does)
are floem's own, not ours: `editor_pane`'s `editor_style` closure passes
`render_white_space(RenderWhitespace::All | None)` and `indent_guide(bool)`, and they draw in the
`visible_whitespace`/`indent_guide_color` the closure already set — the editor theme's
`selection`. There is no rendering code of ours behind either — only vendored Floem's third
patch, which centres the whitespace marks in their line (*Floem 0.2 gotchas*). The palette's
*Toggle Whitespace* and *Toggle Indent Guides* sit beside *Toggle Word Wrap*.
**Keyword case** (Settings → *Code formatting*, its own section between *Editor* and *Query*,
the first of its six `focusable_dropdown`s; *As typed* by default — `UiState::format_keyword_case`,
pinned by
`format_keyword_case_defaults_to_as_typed_and_keeps_a_choice`; `LayoutUi::keyword_case`) is the
one editor setting here that paints nothing. **Function case** and **Type case** follow it
(focus order 163 and 166, both *As typed* by default — `UiState::format_function_case`/
`format_type_case`, pinned by `format_function_and_type_case_default_as_typed_and_keep_a_choice`;
`LayoutUi::function_case`/`type_case`), reusing `KeywordCase::ALL` and `keyword_case_label` for
their choices rather than a list of their own. **Commas** sits after them (*End of line* by
default — `UiState::format_comma_style`, pinned by
`format_comma_style_defaults_to_end_of_line_and_keeps_a_choice`; `LayoutUi::comma_style`), and
**Between statements** after that (*One blank line* by default, focus order 180 —
`UiState::format_statement_gap`, pinned by
`format_statement_gap_defaults_to_one_and_keeps_a_choice`; `LayoutUi::statement_gap`), then
**Wrap width** (*Off* by default, focus order 183 — `UiState::format_wrap_width`, pinned by
`format_wrap_width_defaults_off_and_keeps_a_width`; `LayoutUi::wrap_width`), whose choices are
`settings.rs`'s `WRAP_WIDTHS = [0, 80, 100, 120]` labelled by `wrap_width_label`, `0` leading
as *Off* (`the_wrap_widths_lead_with_off`), then
the section's three switches, each a `focusable_toggle_row` off by default: **Right-align clause
keywords** (focus order 185 — `UiState::format_river`, pinned by
`format_river_defaults_off_and_keeps_an_on`; `LayoutUi::river`), **Align columns** (focus
order 188 — `UiState::format_align`, pinned by `format_align_defaults_off_and_keeps_an_on`;
`LayoutUi::align`), and **Join condition on its
own line** (focus order 190 — `UiState::format_join_on_own_line`, pinned by
`format_join_on_own_line_defaults_off_and_keeps_an_on`; `LayoutUi::join_on_own_line`). What
each does to the text is the `core/sqlfmt.rs` entry's. **They reach the formatter as one
value**: the theme state holds a single `format_style: RwSignal<FormatStyle>`, and the app
composes it in one `create_effect` beside the other editor-setting effects, from the nine
signals Settings edits one at a time, into `theme::set_format_style` — which replaced a setter
per setting. Its `tab_width` is not one of them: the effect writes `0`, and
`theme::format_style()` fills in `editor_tab_width` each time it is read, so the formatter
measures a tab as wide as the editor draws one.
`theme::format_style()` reads both **untracked** and the setter bumps no
`editor_gen`, because nothing is drawn from it: Format Code (`format_editor`'s `format_range`)
and both *Format statement* sites — the key/palette request and the right-click menu — ask for
it at the moment they run.
**`FieldSql` carries the same lexing into an `edit_field`** — the connection form's Init SQL, the
DDL preview's SQL box and the view, trigger, routine and snippet bodies, each of which sets
`FieldCfg::sql` (under `lib.rs`). It
is the same `lex_line_with` with the same cross-line state: the per-revision `CarryCache` is
filled by `carry_at_cached` — keyed on `cache_rev` and keeping a copy of the text, since a field
has no edit log, which is the path `SqlStyling::carry_at` takes only without one — and both
end in `carried_lines_after_edit`, so the forward pass that removed the Θ(visible × document)
re-lex is one piece of code rather than two copies of it. It leaves out what only the tab editor has — the zoom, the inline
diff and rainbow brackets (`lex_line_with` is handed `None` for the depth) — and its dialect is a
plain `SqlDialect` fixed when the field is built, where `SqlStyling`'s is a memo.
**Its colours are not the editor's.** `FieldSql::apply` paints through `Tok::ui_color`, the
interface theme's short SQL palette (`UiTheme::sql_keyword`/`sql_string`/`sql_number`/
`sql_function`/`sql_comment`), and `ui_spans` does the same for the read-only SQL a panel shows
through `widgets::highlight_sql_mono` — the History, snippet-library and AI-chat previews and
the account browser's `GRANT` block — one line at a time, with no cross-line state and no
rainbow brackets. It is a beautifier, not full highlighting: a type folds onto the keyword
colour, a constant onto the number, a variable onto the function, and a bracket answers `None`
and keeps the base colour. The keyword is a lavender rather than the accent blue because a
search match is drawn bold in `match_highlight`, which *is* the accent. All of this used to be
the editor theme's palette on the editor's surface (`theme::preview_bg`/`preview_fg`, the editor
background and foreground, both now deleted), on the reasoning that the tokens reproduce
palettes tuned against their own background — and it put editor-coloured blocks inside
UI-themed forms and panels: a Light UI under a dark editor theme showed dark blocks in light
forms. Now each sits on its placement's own surface, which is under `lib.rs` (`FieldCfg::sql`)
and under each panel's entry. Since the palette is the UI theme's, `FieldStyling::id` keys on
`theme::ui_generation()` alone; `FieldSql::generation` (the editor generation) went with the
move, and an editor-theme switch no longer touches a field. **The editor theme now colours the
editor and nothing else** — the Ctrl+K inline diff included, which is drawn on the editor's own
surface and so keeps `highlight_spans`/`highlight_lines`/`highlight_lines_in` on the editor
palette, and the completion popup's `theme::suggest_*`.
`sql_outside_the_editor_takes_the_interface_palette` pins the palette across every UI × editor
theme combination — each token takes its `UiTheme::sql_*` role whatever the editor theme is, a
bracket stays uncoloured. It asks `ui_spans`; **no test covers `FieldSql`** itself — it needs a
floem `Document`, and there is no fixture for one — so what reaches its `apply` is only that
the two share `Tok::ui_color`.
