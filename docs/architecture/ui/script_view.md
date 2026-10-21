# `ui/script_view.rs`

`script_view.rs` — the **script-load** modal, **Import** to the user, over `core::script`; the
inverse of `dump_view.rs` below, and the entry directly *above* it in the two menus that carry
both (a database's and a PostgreSQL namespace's), where the group reads
**`Import → Export ▸ → Create ▸`** (a database's with *Back up* and *Restore from file* before
`Create ▸`). Those menus could write a `.sql` file nothing here could read
back, which is the hole this closes. **Import leads and the order is not alphabetical**: it is
the entry that changes the database, so it takes the slot furthest from where the pointer rests
after a right-click — the reasoning that keeps `Drop` last everywhere else in these menus.
**One word, two scopes.** *Import* on a **table** is `import_view`'s CSV/JSON/Excel loader;
*Import* on a **database** is this. A script has no table to load into because its statements
name their own, so the scope of the node picks the loader — and the namespace a namespace-node open carries is
for the **title only**, since nothing can confine someone else's `CREATE TABLE` to `sales`.
**Its own `ScriptUi` bundle rather than an arm of `ImportUi`**, though the user reaches both
through one word: a CSV import is twenty-odd signals about delimiters, headers, null tokens and a
column mapping, and a script reads none of them. Folding it in would have meant branching every
one of those on a scope that leaves them all unread; sharing the entry point and the frame is the
part the user can see, and is what was actually asked for.
The panel is `dump_view`'s shape because the two are one journey in opposite directions — pick a
file, see what it holds, run it with progress and a stop, read the outcome — and it shares that
modal's rules: it **stays open while the run goes** (its signals are the run's only channel, so
closing would hide work in flight), every exit routes through one `exit_action` so the footer,
Escape and the ✕ cannot disagree, and the dismissive slot wears **Stop** in `Danger` while
running. The second step is `core::script::probe`'s readout — the kind histogram, what the file
destroys, and whether it opens its own transaction — with every count printed through
`count_label`, so a bounded probe says `400+` rather than a total it did not earn.
**Run is `ActionKind::Primary`** — the same button the table import's own *Import* is. It was
`Danger` for one build, on the argument that this runs someone else's DDL with no undo; what
that actually produced was one modal reachable from two menu entries wearing two different
confirming colours, which reads as two features rather than as a warning. What the file will do
is said in words, in the panel, where it can be specific about *this* file. `open_script` clears
`ui.dump.target` and `open_dump` clears `ui.script.target`: the two share one tuple element in
the modal layer and each fills it when open, so both being set would stack two full-screen
overlays — a rule the trigger/routine/event group next to them also states, and one that relying
on reachability alone has already broken once.
The guard is `sql::script_verdict`, asked in the same synchronous step that launches
(`widgets::accept_launch`) — see the write-guard invariant for why it is stricter than
`run_verdict` rather than a second, laxer gate.
**Off `whole_ui_gate`'s list, 6 to zero, on a module that is nearly all driver** — this modal is
a file picker, a probe and a run — so the split `import_view.rs` established lands almost wholly
on one side: `pick_file`, `run_script` and `script_overlay` take a `ScriptCtx`, the overlay **by
value** for `compare_view.rs`'s return-type reason. `ScriptCtx` holds six things and no `Ui` —
`script: ScriptUi`, `conn: ConnUi`, `layout: LayoutUi` and the `probe`/`run`/`cancel` closures —
built once in `modals.rs` by naming the reads. The three that are *not* on it are the part worth
reading. `open_script` takes `(ScriptUi, DumpUi)`, `open_for_server`'s and `open_import`'s case a
third time: it only writes signals, and the `DumpUi` is the export modal it clears on the way in.
`watch_connection` takes the two bundles it watches. And **`policy` takes `(ConnUi, LayoutUi)`,
the write guard's two halves, named** — a `GuardPolicy` assembled out of a context struct would
hide which parts of the app decide whether a `.sql` file may run, and that is the one decision
here the write-guard invariant is about, this being the module whose guard that invariant's own
text cites. Behaviour did not move with the narrowing: `run_script` still asks `policy` and
`ScriptRequest::approved` in the same synchronous step as `accept_launch`, and the exit path's
`stop` is `ctx.cancel`, the closure `ui.schema_actions.script_cancel` was. What came *off*
`ScriptCtx` before it shipped, and why that is the mirror of `DumpCtx`'s near-miss, is under
`lib.rs`'s `whole_ui_gate`.
**Its words are the catalogue's** (`script-*`) — title, file row, hints, the
probe summary and its red warnings, the transaction line, progress, Run, the outcome sentences
and `durability_sentence`'s four, with Stop and Close the shared `dialog-stop`/`dialog-close`.
**A count is two arguments, not one**: `$count` is the figure as shown — `Probe::count_label`'s,
so `412+` where the read-ahead stopped short — and `$n` the number Fluent chooses the plural by,
since a `412+` is no number to choose one with; both warnings carry two plurals over that one
`$n` (*statements … destroy*, *statements … act*). The server's error goes into the failure
sentences unchanged. `script_view` has no tests of its own, so
`i18n::tests::the_script_import_sentences_read_as_they_did` holds
every composed sentence to the `format!` it replaced, for one statement and for three. The
system file dialog's file-type name was a hard-coded English *SQL script*, because
`FileSpec::name` is a `&'static str`; it is now `file-sql-filter`, the message the app's own SQL
dialogs name the type with, made `&'static` by the UI crate's `tr_static!` (*The string
catalogue*).
