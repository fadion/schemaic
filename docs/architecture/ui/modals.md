# `ui/modals.rs`

`modals.rs` — **the modal layer** (`modal_layer`) and the four predicates that raise it
(`ddl_modals_up`, `ddl_editors_up`, `workspace_modals_up`, `settings_modals_up`,
`modal_backdrop_up`), plus the `modal_backdrop_gate` tests. It mounts no modal of its own and
paints no scrim: every member view lives in its own module, and this one decides *which box they
resolve their `inset(0)` against* and *in what order they paint*. Both are policy — the layer's
box stops at `header_h()` so the title-bar band has somewhere to go, and each paint-order rule in
the tuple was bought with a bug (a confirm behind Manage Connections, a transaction prompt behind
the DDL preview, a popup menu behind the panel that opened it).
**The shared confirm is the layer's last entry, above every group, and that is a rule rather
than one more ordering.** A confirm is by definition raised *by* something already on screen, so
"whatever can raise a question comes first" — which `manage_modal` and the DDL preview each state
about their own neighbour — has exactly one stable answer once the layer holds more than one
group: put the question above all of them. It used to live inside the DDL group, entry 3 of six,
while the Live Monitor sits in the workspace group at 5, so *Clear the log?* painted **entirely**
behind the monitor with its focus root holding the keyboard over a question nobody could see;
reordering those two groups only moves which modal has the problem. Being its own entry, it needs
its own term in `modal_backdrop_up` exactly as `find`, `manage` and `plan` do — `ddl_modals_up`
counted it while it was painted there, and that predicate is now only about that group's own box.
**The grid export's progress modal is the second entry of that shape, and it is the confirm's
argument rather than an exception to it.** It was the *third* member of the tuple element
`dump_overlay` and `script_overlay` share, down in the DDL group, on the stated claim that a
dump and a grid export cannot be up together — and nothing enforced that claim. Those two
neighbours clear each other's target on every `open`, while **nothing anywhere clears
`ui.export.target`**, because nothing may: clearing it would take a running export's Stop off the
screen. So a modal four entries below the workspace group was painted over by Properties, the ER
diagram or the binary-cell panel opened while an export ran — Stop included, and reachable
because the save dialog is not window-modal, so the schema tree stays live for as long as the
user spends choosing a file. A surface that stands for the whole *length* of an operation and
carries the only control that ends it is something anything can be up over, so it takes the
confirm's answer: its own entry, above every group, directly below the confirm. It needs no
wrapper, unlike the confirm, because it already carries its own `absolute().inset(0)` gated on
`target` the way `manage_modal`, `plan_overlay` and `find_overlay` do. `export_open` therefore
came **out** of `ddl_modals_up` — left there, that group's wrapper would be a full-window box
with nothing in it for the length of every export, eating every click beneath it — and **into
`modal_backdrop_up` as its own term**, exactly as `find`, `manage`, `plan` and the confirm each
have one.
`modal_up` is a *parameter*
rather than a local, because `workspace` needs the identical answer for the band and the two
must not be able to disagree. The group wrappers exist to fit floem's 16-arity `ViewTuple` limit
and are load-bearing anyway: a group's wrapper fills the layer only while one of its members is
open, since an always-full-window box would eat every click beneath it. See the absolute-child
invariant for the full argument and for what the gate holds up.
**"Open" is not always the same question as "on screen", and `workspace_modals_up` is where the
two come apart.** The Users and privileges browser renders nothing while one of its own account
forms or the DDL preview is up — those are raised from it and painted in the *earlier* DDL
group — so the predicate counts `overlay.users` only while `ddl.account`, `ddl.grant` and
`ddl.preview` are all `None`. A wrapper that still filled the layer would be a transparent
full-window box sitting on top of the form, swallowing every click meant for it: the same
always-full-window failure as above, arrived at through a member that is open but invisible
rather than through a group that is closed. `users_view::users_overlay` asks the identical three
signals, and the two must not be able to disagree. **The Server variables viewer, mounted
directly after the browser, is the contrast**: it raises nothing, so `vars_open` is a plain term
and counts whenever the viewer is open. The *Used by* modal, mounted after Properties, is the
same plain term (`used_by_open`), and so are the *Chart* modal mounted after it (`chart_open`)
and *Compare results* after that (`diff_open`).
**Schema compare, the group's seventh member, takes the same `preview_open` clause — and there for
the rule rather than for a case.** It raises the DDL preview too, painted in the earlier DDL
group, so a wrapper that went on filling the layer would sit on top of it exactly as above. What
is different is that `compare_view::open_plan_preview` closes the comparison on its way to the
preview, so the clause guards nothing that arises today; it is written down anyway, because the
alternative is a member of this group whose correctness rests on a caller remembering to close
itself first.
**The *Copy to* modal, mounted directly after compare, takes the same clause — and there it is
load-bearing.** It does *not* close on its way to the preview it raises: it stays
open behind it, hidden by its own `copy_open && preview_open.is_none()` style, so it can count
the rows once the plan commits (`DdlPreview::then`). Without the clause in
`workspace_modals_up` the group's wrapper would fill the layer over the preview's Apply for the
whole time the plan is on screen. The two must ask the same two signals, as the users browser
and its predicate do.
**`whole_ui_gate` 5 → 2, and the two left are the layer and the union.** The three group
predicates take the bundles they read — `ddl_modals_up(OverlayUi, ImportUi, DumpUi, ScriptUi,
DdlUi)`, `workspace_modals_up(OverlayUi, BlobUi, DdlUi)` and `settings_modals_up(LayoutUi)` —
each having already been a list of `Copy` flags read at the top and never touched again, with
only the signature not saying so. The last took `TermUi` and `AiUi` too until the terminal's and
the assistant's settings modals became sections of Settings: its group holds two overlays now,
`settings_overlay` and `help_overlay`, and both open flags are `LayoutUi`'s. Neither of the other two is a candidate:
`modal_layer` reaches **twenty-one** `Ui` fields and hands the bundle to every modal constructor
in the app, which makes it the widest of everything this gate's entries name — `center`'s
eighteen included, where this sentence used to put the two the other way round on a stale
count — and `modal_backdrop_up` calls
all three predicates above and adds seven terms of its own, so what it needs is their union —
eleven bundles, a parameter list longer than the function.
