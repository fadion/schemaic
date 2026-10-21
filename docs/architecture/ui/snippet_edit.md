# `ui/snippet_edit.rs`

`snippet_edit.rs` — the snippet editor modal: **the one place a saved query's body can be
changed**, with its name and abbrev alongside. The panel's inline fields cover the two
one-word edits because those are the same act as renaming a tab; a body is SQL, multi-line, and
a 300px panel row is the wrong shape for it, so it gets the modal chrome the view/trigger
editors wear (`modal_title_owned` / `modal_footer` / `panel_style`) and the same multi-line
mono field with `tab_indents`, SQL-coloured as theirs are (`FieldCfg::sql`). A snippet carries
a scope rather than a dialect, so `snippet::body_dialect` answers it: the scope's engine where it
names one, else the active connection's (`ConnUi::dialect`, passed in by `modals.rs`) — a global
snippet is written for whatever is in front of you, and a connection's own is offered only on
that connection. **Save writes only the fields that changed, through the same
per-field actions the inline edits use** — there is deliberately no fourth "save everything"
action, because two paths writing one field is how the two drift. *Which* fields changed is
`core::snippet::edits`, not three comparisons written here: the body's comparison and its
payload had drifted apart, and the whitespace-only edit that got lost to it is written up under
`core/snippet.rs`. It is painted inside the
`ddl_modals_up` group and is therefore *in* that predicate: an overlay in that group whose flag
the predicate doesn't know about resolves its `inset(0)` against a zero-by-zero box and paints
nothing, which is exactly how the event editor once shipped invisible. It shares its tuple
element with *Rename alias* and *Find usages* (`alias_rename.rs` and `usages_view.rs`, next).
**Its words are the catalogue's** (`snippet-edit-*`) — the title (`$name`),
the *Name*, *Expansion shortcut* and *Query* labels, the name field's and the shortcut field's
placeholders (both through `FieldCfg::placeholder_of`; the shortcut's is the word *none*), and
Save; Cancel is the shared `dialog-cancel`. `the_title_reads_as_it_did_in_english` pins the
title. The Query field's `SELECT …` placeholder stays a literal, being a SQL sample.
