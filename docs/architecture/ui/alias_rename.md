# `ui/alias_rename.rs`

`alias_rename.rs` — the SQL editor's *Rename alias* and *Rename CTE* dialog
(`alias_rename_overlay`, raised by `OverlayUi::alias_rename`): a title, one *New name* field
opened on the name as its definition writes it, Cancel and Rename. The two share it whole;
`AliasRenameRequest::title` and `placeholder` are all that differ — *Rename alias*/*Alias*,
*Rename CTE*/*CTE name* — and the second's decision is `intel::cte_rename`'s. **It exists because a name has to be finished before it can be
checked.** The entry used to hand over a multi-caret selection and let typing do the rename, so
nothing ever compared the name with those the query already binds, and renaming the outer `o` to
an inner query's `i` rebound a correlated `o.id` — in a `DELETE … WHERE EXISTS`, every row or
none (R3-L5-03). The decision is `intel::alias_rename`'s — `output_alias_rename`'s for an output
column's alias, which *Rename alias* also offers (under `intel.rs`); this file asks and
shows the answer, and `AliasRenameRequest::apply` is the editor's own rename, so it knows no
buffer or offset. A refusal — `AliasRenameRefusal`, worded by `refusal_message` — is written in the error colour
under the field and **the dialog stays open on it**, the name still there to correct; editing
the name retracts it, since it answered the name it was given. Enter submits, Escape and ✕
close, and there is **no click-away**, as the snippet editor has none: the name is applied only
by Rename. **However it closes — Cancel, Escape, ✕ or a successful Rename, all through the one
`close` — it calls `AliasRenameRequest::back`**, which the editor fills with
`menu_return(move || vid.request_focus())` on its own content view: the dialog's `focus_root`
hands back to the next root out, and over the workspace there is none, so the keyboard went
nowhere and Ctrl+Z after a rename did nothing until the editor was clicked (`None` only when
`editor_view_id` is unset). The outer `dyn_container` is keyed on *whether* a request is open
rather than on the request, so nothing rebuilds the field and takes its caret; the footer is keyed on whether the
trimmed name is empty, so the buttons rebuild when Rename turns on and not on every keystroke.
It shares `snippet_edit.rs`'s tuple element in the modal layer's DDL group, with `usages_view.rs`
as the element's third member — that stack is at
floem's 16-arity limit, and no two of the three are up together: that is a stated claim rather
than a check, resting on each being modal, so none can be raised from behind another — so the
element fills the layer while
any of them is open, `ddl_modals_up` has an `alias_rename` arm, and the file is on
`PAINTS_A_BACKDROP`. Left out of the predicate it would paint nothing, for the reason the snippet
editor's entry gives.
**Its words are the catalogue's** (`alias-rename-*`) — *New name* and Rename
here; Cancel is the shared `dialog-cancel`. The two titles and placeholders belong to the
openers: `AliasRenameRequest::title` and `placeholder` are `String`s (they were
`&'static str`), and the editor's two entries in `editor_pane.rs`, its only constructors, pass
`alias-rename-title-alias`/`-cte` and `alias-rename-placeholder-alias`/`-cte` ahead of that
file's own conversion. The placeholder reaches the field through `FieldCfg::placeholder_of`, and
the title goes to `widgets::modal_title`, which takes `impl Into<String>` — this was its last
caller with a `&'static str`. A refusal's text is the private `refusal_message`'s, a `tr!` arm
per `AliasRenameRefusal` variant (`Conflict` with the trimmed `$name`), held to core's
`message` in English by `english_refuses_every_alias_rename_as_core_does`, in this file's own
tests.
