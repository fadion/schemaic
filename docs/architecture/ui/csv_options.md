# `ui/csv_options.rs`

`csv_options.rs` — the schema tree's *CSV read options* dialog (`csv_options_overlay`, raised by
`OverlayUi::csv_options`): a title naming the view, a **Delimiter** choice (*Detect*, *Comma*,
*Semicolon*, *Tab*, *Pipe*) and a **Header row** choice (*Detect*, *Yes*, *No*), each a
`widgets::segmented_tabs`, a hint, Cancel and Continue. It is the answer to DuckDB's CSV sniffer
guessing a file wrong in an **Open data file** session, which used to leave the user to rewrite
the view's `read_csv` call by hand.

**It writes no DDL.** `CsvOptionsRequest::apply` — built by the menu entry in `overlays.rs` —
opens the view editor on the view with its body already replaced
(`view_editor::open_for_view_with_body`, the body `data_file::csv_view_body`'s), and the editor's
Preview SQL shows the replace as for any other edit, so the generated-DDL path (draft →
`ddl::diff_view` → preview → `Db::run_ddl`) is the one it always is. The dialog closes before it
calls `apply`, since the editor is a modal of its own.

**The choices are closed lists, not text**: `data_file::CsvDelimiter` holds the four delimiters a
file is nearly always split by, so no escaping question (a tab typed as `\t`) reaches the user,
and *Detect* on either leaves the option out of the call for the sniffer to decide. The dialog
opens on the options the view reads with now, read back by `data_file::csv_view_of`.

**Where it is offered** (`overlays.rs`, under *Edit view*): on a view, in a data session
(`Connection::is_data_session`, DuckDB's alone, so no engine is asked), that `is_editable_view`
admits and whose body `csv_view_of` reads — `SELECT * FROM read_csv('path', …)` with no option it
does not model. A view the user rewrote by hand with `columns = …`, a filter or a second file is
not offered it: the dialog writes the whole call again, and would drop what they wrote. Dimmed on a
read-only connection, as every schema-editing entry is.

It is the fourth member of `snippet_edit.rs`'s tuple element in the modal layer's DDL group,
beside `alias_rename.rs` and `usages_view.rs` — that stack is at floem's 16-arity limit, and none
of the four is up beside another, each being modal — so the element fills the layer while it is
open, `ddl_modals_up` has a `csv_options` arm, and the file is on `PAINTS_A_BACKDROP`. Escape and
✕ close it, and so does a backdrop click (`dismiss_layer`): it holds two segmented choices and
nothing a stray click could lose, so it is outside `floem-gotchas.md`'s no-click-away set. Its words are the catalogue's (`tree-csv-read-options`,
`csv-options-*`); Cancel is the shared `dialog-cancel`.
