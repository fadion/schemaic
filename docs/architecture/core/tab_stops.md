# `core/tab_stops.rs`

`tab_stops.rs` — **a snippet's tab stops**, walked with Tab once its body is in the editor.
There is no new syntax: a stop is a `:name` parameter `params::scan` finds in the body, so
the one template mechanism a snippet has gains a way to be filled in from the keyboard rather
than a second mechanism beside it (`snippet.rs`, above). `TabStops::new(body, at, dialect)`
answers `None` for a body with no parameters — nothing to walk, so Tab indents as ever — and
otherwise holds the stops as byte ranges offset by `at`, where the body landed, plus the
snippet's end. A parameter-shaped string, comment or `::` cast is not a stop, `scan` already
refusing them (`a_body_without_parameters_has_no_stops`). **A name used twice is one stop, at
its first use**, matched exactly as `params::names` keys parameters, so `:id` and `:ID` — two
rows in the parameters bar — are two stops (`a_name_used_twice_is_one_stop`). The module decides nothing about
the editor and the editor decides nothing about the stops: `current()` is what to select,
`forward()` answers `Step::Select(a, b)` for the next stop or, past the last,
`Step::Finish(end)` — the caret goes to the snippet's end and the walk is over — and `back()`
the previous, staying on the first. `shift(start, end, inserted)` carries every stop **and the
end** through one edit by `text_ops::shift_range`, the function the find bar's *in selection*
scope follows edits with, so typing over a selected stop makes the stop *what was typed* and
typing at its end grows it (`typing_over_a_stop_makes_the_stop_what_was_typed`); it answers
`false` — and the editor ends the walk — when the edit enclosed a stop or the end
(`shift_range`'s `None`), since after a whole-document Format the next Tab would otherwise
select every line (`an_edit_enclosing_the_stops_ends_the_walk`). `holds(offset)`
is whether the caret is still in the walk — inside a stop or at either edge of one, the edge
being where typing into it goes on. The UI half (`Completion::stops`,
`completion::begin_snippet`, the `StopNext`/`StopPrev`/`StopEnd` key actions) is under
`completion.rs` and `editor_pane.rs`.
