# `ui/settings.rs`

`settings.rs` — the Settings modal (its sections are General / Editor / Code formatting /
Query / **Terminal** / **AI assistant** / **Appearance**, the last holding the two theme pickers,
the **interface scale** and the **Language** dropdown — `language_section`, tabindex 330, with
*The language change takes effect the next time Schemaic starts.* under it only while
`LanguageChoice::needs_restart`, since unlike everything else on that page it applies at the
next launch; its signal is `LayoutUi::ui_language`, persisted and mirrored by nothing live —
and, after it, *Show CPU and RAM in the status bar*, a `focusable_toggle_row` at tabindex 340 over
`LayoutUi::show_resources`, on by default; off hides `footer`'s two segments *and* stops the
sampling behind them, which is `start_resource_monitor`'s half, under `core/ui-state-models.md`'s
`resource.rs`) and the Shortcuts reference, **and the
four shared controls every modal's form is
built from**: `focusable_toggle`/`focusable_toggle_row` (the switch — Space is ours, Enter is
floem's), `focusable_dropdown` and the picker-agnostic **`in_ring_picker`** under it — the box
that drops the app's *own* popup menu, which is what every `<select>` in the app is now built
on (see **No floem `Dropdown`** below). `themed_toggle` is the un-ringed builder beneath, and
is **private** on purpose: a control nobody can Tab to is one left out of the modal's keyboard
order by accident.
**Its one caller outside the ring is a switch nobody can flip either.**
`focusable_toggle_row_when(title, hint, sig, enabled, ring, tabindex)` is the row for a setting
that refines another and does nothing without it — today only Editor's *Predictions from past
statements*, under *Inline predictions* (`completion::prediction_at` returns before `recall` is
asked when predictions are off). While `enabled` is false the row is **greyed out**: its title
dims to `theme::text_dim`, and its switch is `themed_toggle(sig, true)`, *inert* —
floem-`disabled`, which drops its pointer events and, through floem's `AppState::can_focus`,
keeps floem's own Tab walk off it as well; an `on_toggle` that ignores the flip besides; no hover
change; and its own colours at `INERT_TOGGLE_ALPHA` (0.4, through `multiply_alpha`, floem 0.2
having no `opacity` — see *Floem 0.2 gotchas*), so its value still reads, faintly. It is **out
of the ring** while greyed, the way a disabled `widgets::in_ring_button` is, and back in when
`enabled` returns. A ring control registers when it is built, so that takes a rebuild: the
switch sits in a `dyn_container` on `enabled`, as Install and Remove do below. **The value is
kept either way** — greying says whether the switch can do anything, not what it holds. So the
privacy rule stands, read as a rule about *live* switches: there is still none you can flip and
cannot Tab to. `focusable_toggle_row` and the greyed row share `toggle_row_layout`. Which branch
is built is one `enabled.get()` in the view, and nothing tests it.
**Off `whole_ui_gate`'s list, 4 to zero, and it is `connection_import.rs`'s sweep a second
time** — the module was narrow already and only had to say so, each of the four modals it then
had (three settings ones and Shortcuts) owning one domain and reading nothing else:
`term_settings_overlay(TermUi, Rc<TermActions>)`,
`ai_settings_overlay(AiUi, ConnUi, Rc<AiActions>)` — the `ConnUi` is the active connection's
data-access level, which the AI pane reports — `theme_settings_overlay(LayoutUi,
open_config_dir, CliCommand)`, the last two General's two trips outside the app —
and `help_overlay(LayoutUi)`, which reads a single signal. The
module names no `Ui` anywhere now, prose included, bar `SettingsCtx`'s doc saying it holds none.
**Four became two when the terminal's and the assistant's modals joined Settings as sections**,
and their signatures moved down a level unchanged but for the ring they now share:
`terminal_section(TermUi, Rc<TermActions>, FocusRing)` and `ai_section(AiUi, ConnUi,
Rc<AiActions>, FocusRing)`, each an `AnyView` like its neighbours. The modal holding them is
`settings_overlay(SettingsCtx)` — `theme_settings_overlay` renamed, since it is no longer the
theme's — and the ctx is there for clippy rather than for the gate: `SettingsCtx { layout, term,
term_actions, ai, conn, ai_actions, open_config_dir, cli_command, export_settings,
import_settings }` is ten parameters named one by one, past `too_many_arguments`' seven (it
was eight until the settings file's two trips joined it). It holds no `Ui`, for `CompareCtx`'s reason, and is
built by naming the reads at its one call site in `modals.rs`, so what the modal touches stays
written down there.
**The main modal is Manage connections' two panes, not one long column.** It had been five
headed groups in a single scroll; it is now a section list on the left (`section_list`, a
`SECTION_LIST_W` 170 column with a `border_right` and `padding_vert` 6) and the chosen
section's page on the right, `SECTION_PAGE_W` 500 wide — the single-column modal's 420, which
every hint in it was written to wrap within, and 80 more since a row with buttons beside its
text (*Settings file*'s Export/Import, *Log file*'s Open folder) squeezed that text into a tall
sliver under doubled text; *Settings file*'s two buttons also stack, one above
the other at one width (`bundle_row`). The panel is `modal_w(SECTION_LIST_W +
SECTION_PAGE_W)` at a **fixed** `height(modal_h(560))`, so switching sections never resizes it;
the old `max_height(modal_body_h(560))` cap went with the column. **Both panes scroll**: the
page in its own `autohide(scroll)`, and the list's rows in theirs (`flex_grow`, `min_height(0)`)
with `settings_version_line` pinned beneath — `modal_h` caps the panel to the window, and an
unscrolled list of seven rows drew its last rows and the version below the panel's bottom edge
on a short window at a large interface scale. The sections are
`SettingsSection` (re-exported as `schemaic_ui::SettingsSection`): `General` by default, `ALL`
in list order, and `label()` naming both the row and its page's heading — *Code formatting* for
`Formatting`. Which one is showing is `LayoutUi::settings_section`, created by the app and not
persisted, so the modal reopens on the section it was left on and a fresh start opens on General.
`the_settings_sections_are_listed_once_and_named_apart` pins the default first and no two rows
sharing a name; its exhaustive `match` makes a new variant fail to compile there, which is a
prompt to add it to `ALL` rather than proof that it was — `ALL` is a fixed-length array, and the
test's length check would still read 7. The rows wear Manage connections' list colours
(`theme::conn_list_text`/`conn_list_sel_text`/`conn_list_sel_bg`, a hover brightening the text
only) at its 5px row gap, padded 12×11, and the whole list is **one** ring stop at
`widgets::NAV_TAB`, ahead of every control: a `nav_group` on `NavAxis::Vertical` — the second
after Manage connections' list — whose Up/Down clamps through `list_step` while the ring around
it wraps.
**Only the showing page is built, and only its controls are in the ring.** The page is a
`dyn_container` keyed on the section whose child builds that section alone —
`general_section`, `editor_section`, `formatting_section`, `query_section`,
`terminal_section`, `ai_section`, `appearance_section`, each an `AnyView` from `section_page`
(the heading over its settings at a group's 16) with `ctrl` stacking a caption over its control
— inside its own `autohide(scroll(..))` at padding 14, so a page scrolls on its own and opens at
the top each time it is shown, while the list beside it stays put. A ring control registers
when it is built and unregisters in its cleanup, so a page a switch tears down takes its
controls out of the ring with it, and Tab never walks into a page nobody can see — the AI
page's reason for tearing down what is off rather than hiding it. The five that were the single
form keep the block of tab numbers each had there, and those never collide; Terminal and AI
assistant brought their own modals' numbering (10–50 and 5–60), which does overlap General's,
and is harmless only because one page at a time is ever in the ring.
**`section_page` sets `width_full`, and the AI page is why.** A page is usually the very child
the scroll pads, but the AI page sits one container in — its teardown hook needs a view of its
own (below) — and without the width it took its longest line's width and ran off the panel's
right edge; seen in a sandboxed window, fixed, re-checked. The terminal and AI pages also
took the page's 16 between groups with them, where their modals had spaced groups 25 apart.
**Two gears open Settings on their own section**, through `open_settings_at(LayoutUi,
SettingsSection)`, which sets the section and then the open flag: the terminal pane's
(`terminal_panel` takes `LayoutUi` for it) lands on Terminal, the AI panel's on AI assistant.
Each had opened its own modal until the two joined; their tooltips, *Terminal settings* and
*AI settings*, lost their ellipsis on the way.
**The section list ends in a version caption, and the string it shows is composed in
`schemaic-core`.**
`settings_version_line` renders `core::app_version_label()` — `Schemaic v0.24.0`, built from
`APP_NAME` and `APP_VERSION` (`env!("CARGO_PKG_VERSION")`, which inherits
`[workspace.package].version`). It is the only place the version is on screen anywhere in the
app — there is no About window — and until it landed `APP_VERSION`'s one other reader was the
startup `tracing::info!` line in `app/main.rs`. Composing the string in core rather than in the
view is what stops it going stale: a `chore: release vX.Y.Z` edits one number in one
`Cargo.toml`, and that number is already what the caption renders, so there is no second edit to
remember. Two tests sit beside the function, and the equality one is honest in its own doc
comment about how far it reaches — same-crate `env!` on both sides means a literal substituted
for the derivation still passes until the *next* bump parts the two. Reading `Cargo.toml` would
not close that gap — it holds the same number `env!` does — and catching the literal the moment
it was written would take a source gate over `app_version_label`'s body: the kind of test that
reading the repository's own files is exempt for (the `ui::source_gate` family, `APP_ID`'s
packager test), which nobody has judged worth writing for a one-line function.
It is a **caption, not another row**: `theme::text_dim()` at `theme::font_hint()`, unbold, so it
identifies the panel instead of competing with the sections. It sits at the list's **foot**,
pushed there by a growing spacer and inset by the rows' own 12 so its text starts where theirs
do — which is what puts it in front of every section. It used to open the single column above
`General`, at `font_body()`, carrying a section header's `margin_bottom(scaled(2))` inside an
`intro` stack at a group's 16 so the air under it matched the air under the heading; that
pairing went with the column, and nothing about the caption is derived from the header now.
**The gap between sections was `form_section_gap()` (`scaled(36)`), and it was a literal 28**
until the sections ran together once Settings had grown, and the user asked for a little more
(GitHub #16). The heading moved out of this module with it — it was a private
`settings_section_header` — so the connection form could draw its own sections with the same
two calls; the 16 inside a group did not change. Settings no longer spends the gap, each page
being one section, so the connection form is its one caller.
`log_row` is the General section's one non-toggle: it names the log's **full path** (`log_hint`,
which is the pure half and is tested) and reveals the folder holding it through
`Ui::open_config_dir`. The log had been written and rotated since `logging.rs` landed, and
locating it was still the user's problem — the one artefact a crash report needs sat at a path
the app never said out loud, which the panic hook made worse by putting more in it. The
*directory*, not the file: `schemaic.log` has no natural handler on Windows, `schemaic.log.1`
is a second file worth reaching, and the same folder holds `tabs.json` and `connections.json`.
Spawning a file manager is a process launch, so it is the app boundary's `open_config_dir` and
not a `Command` in a view; a machine with no config directory gets the path-less hint and a
disabled button rather than a control that silently does nothing.
`cli_row` sits under it and is General's other trip outside the app: **Command line**, a
hint read from `CliCommand::status` (`core::cli_install::row_status`, its `RowHint` worded by
`cli_hint` from `install-hint-*`), **Install** and
**Remove** side by side at `action_gap()` (tabindex 30 and 35), reading **Installing** / **Removing** while one runs, and
the outcome on a line of its own under the whole row — `theme::text_dim` on success,
`theme::error` on failure — which stays there for the rest of the session, naming the path the
operation resolved. Remove is on the row because it is the only undo on macOS and under an
AppImage, where nothing of ours runs when the app is thrown away; on Windows a portable copy
never shows it, because `core::cli_install::removal` refuses a non-Velopack install and
`install_cli::installed` answers `false` for it. **It is offered only while it
would do something**: it first shipped unconditionally, so a copy that had never installed
showed a Remove that could only report it found nothing. It is now built in a `dyn_container`
over `CliCommand::installed` — `core::cli_install::removable`'s answer, read by
`install_cli::installed` — so it also leaves the Tab ring when it goes, a ring control
registering when it is built and unregistering in its cleanup; one that goes while it holds the
keyboard hands it back through `in_focus_ring_with`'s cleanup like any other. The container
and the outcome's are both `Display::None` when they have nothing to show, not merely empty,
since an empty flex child still takes its gap — the AI modal's harness-notice fix. **Install,
by contrast, is disabled rather than hidden when it has nothing to do**: a greyed Install beside
a hint saying where the command already is reads as "done", where an empty row would read as
"missing". It first shipped always enabled under a static hint, so a `.deb` install read "Links
it into ~/.local/bin" and the click answered that the command was already in `/usr/bin`. The
hint is now a reactive `label` over `status`, and Install is built in a `dyn_container` keyed on
`can_install`, because `action_button_dyn`'s `enabled` is fixed when the button is built — the
same reason Remove sits in one. That container is never empty, so it needs no `Display::None`.
**The outcome sits under the row, not in the label column, and a tidy-up that moves it back
reintroduces the bug**: as a third line there it made the column taller, and the buttons,
centred on the column, slid down to sit beside the message instead of the label. So the row is
a `v_stack` of an `h_stack` (label column and buttons, `items_center`) and the outcome, at a
gap of 6. **Both outcomes are a local `outcome_line`, not `widgets::footer_error`**: that one's
`max_width(scaled(460))` was then a wrap hint which relied on `modal_footer_split` to shrink it,
and in the Settings modal's `modal_w(420)` nothing did, so a long error was laid out past the
modal's edge and clipped. (`footer_error` is now one ellipsized line with no bound, which is
still not this row's shape: the outcome wraps.)
`outcome_line` is `width_full().min_width(0)`, so it wraps at the
page's width (`SECTION_PAGE_W`, the 420 the modal then was). All of it arrives as one
`Ui::cli_command: CliCommand { install, remove, state, installed, status }` — two actions, the
one `RwSignal<InstallState>` they share, the `RwSignal<bool>` behind Remove and the
`RwSignal<RowStatus>` behind the hint and Install, which the app owns: a registry write or a
symlink is the app boundary's job for `open_config_dir`'s reason.
`bundle_row` is the last row, **Settings file** — a hint saying what goes in the file and what
stays (connections, window layout, the terminal's shell and connection-specific snippets), and
Quiet **Export** and
**Import** buttons at tab stops 40 and 41. Both are always enabled and the row holds no state:
each button calls one of `Ui::export_settings`/`import_settings`, carried in by `SettingsCtx`,
and the dialogs, the file I/O and the confirmation are all the app's (`app/main.rs`), over
`core::settings_bundle`. The same two closures are the palette's *Export Settings* and *Import
Settings*, `instant` commands in `palette_commands`, so the feature is reachable without
opening Settings.
**Export reads `current_ui_state`, the very gathering `save_ui` writes** — the `UiState`
literal that was inside `save_ui` is now that closure, and `save_ui` calls it — so the file and
`ui_state.json` cannot disagree about what a setting's value is. The terminal's half is
`current_term`, a `TerminalSettings` built from the four terminal appearance signals with
`shell: None` — the bundle's allowlist leaves the shell out regardless, so nothing is gathered
for it. **Those signals and `term_prefs` (the `terminal.json` read) are created just above the
bundle section, not beside the terminal** they configure, so that both closures can read and
set them; the terminal code further down uses the same `term_prefs` for its shell. The save
dialog defaults to `BUNDLE_FILE_SUFFIX`, and the write is `persist::write_file_atomic` on
`spawn_blocking`, a failure reaching the error modal through `file_error`, as the connection export's does; unlike
that one, a typed name gets no suffix appended, nothing needing one to recognise the file.
**Import reads and parses off the UI thread** (`decode_text_file`, then `parse`), and decides
on it: `apply_with` runs against `current_ui_state()`, its `normalise` the same conversions and
clamps `apply_settings` uses below, and the snippet count is the merge itself run
over a *copy* of the library, so the number the confirmation names is the number that will be
added. The terminal half runs beside it as `apply_keys(TERMINAL_KEYS, …)` against
`current_term()`, its `normalise` the terminal picker's own reading — `ui::term_font_offered`
(the size if `TERM_FONT_SIZES` holds it, else 13, so a size no row of the picker selects is
refused) and `TermCursor::from_key(..).key()` — and its change count is added to the settings
count, so the confirmation's *N settings* covers both halves. Nothing to change at all is said through `file_error` (*Nothing to import: …*) rather
than asked about; otherwise the shared `Confirm` — *Import settings*, "{path} changes
N settings and adds M snippets. The settings it changes replace your current values; the
snippets are added beside yours.", one catalogue message with both counts as plurals
(`settings-import-question`, through `settings_import_question`) — and only on yes
`apply_settings` sets each portable signal, the four terminal signals are set from the
`apply_keys` answer, `save_ui(Replacing)` runs, and `SnippetStore::import` merges the snippets.
Nothing writes `terminal.json` here: the terminal's own save effect, subscribed to those four
signals, does — through `shell_to_persist`, so the shell it writes is this machine's. **`apply_settings` goes through startup's own
conversions and clamps** (`UiThemeKind`/`EditorThemeKind`/`UiScale::from_key`,
`theme::clamped_editor_font`, `clamped_tab_width`, `AiEffort::from_cli`,
`SchemaScope::from_key`) — the same reading `apply_with` was given, so nothing it accepted is
rewritten here: an unknown theme key or an out-of-range font was refused before the
confirmation counted it. It sets every portable signal, not only the changed ones, from the
`UiState` `apply_with` answered when the file landed.
**Both buttons stay enabled while either runs**, and the guard against a click on either is
`main.rs`'s, beside the launch — the labels only say so.
**The AI assistant page is where the harness is chosen, and half its controls follow that
choice.** It was a modal of its own until it joined Settings, which is the history several
paragraphs below still speak in. The
*Agent CLI* dropdown is `focusable_dropdown(agent, Agent::ALL, agent_label, …)` over the
workspace's **one** `Agent` — `schemaic-ui` depends on `schemaic-ai` for it, deliberately, and
the Cargo.toml comment carries the reason: a UI-side mirror converted at the app boundary is two
definitions free to drift, which is precisely what `AiModel::from_cli` was. Only the words are
the interface's: `agent_label` answers `settings-ai-disabled` for *Disabled* and a harness's own
`label()` — a product name, never translated — for a CLI, and
`english_names_every_agent_as_the_ai_crate_does` holds it equal to `Agent::label` for every
value in `Agent::ALL`. `agent` is a modal-local `RwSignal<Agent>` seeded from `Agent::of(enabled, harness)`
and written back on a pick through `Agent::apply`, whose `AgentPick` the effect only performs:
*Disabled* sets `AiUi::enabled` false and keeps the harness, so turning the assistant back on by
picking the same CLI costs nothing; a CLI sets the harness **only if it differs** — the app's
switch effect files the old harness's path and model on every set it sees
(`picking_the_running_cli_again_changes_nothing`) — and then sets `enabled`; and a pick that
changes either clears `AiUi::unknown_key`.
**Off builds nothing below the choice.** A muted hint under the dropdown says what off means, and
every control after it is made by a `build_rest` closure inside a `dyn_container` keyed on
`enabled`, whose off arm is `widgets::nothing()` — torn down rather than hidden, because a hidden
field stays in the ring (the `.hide()` entry under *Floem 0.2 gotchas*), and Tab would walk an off
page through a CLI path, a model and the rest of a form nobody can see. Each control binds
straight to an `AiUi` signal, so the rebuild on turning it on loses nothing. The grade notice's
memo answers `None` while off: there is no CLI to grade, and the app does not probe one.
**The page commits as it goes away.** Its controls write `AiUi` live, but a changed model, path
or harness reaches the agent only through `AiActions::apply` (the app's `ai_apply`), which the
modal's close used to call. A section's page is torn down at exactly the two moments that close
stood for — a switch to another section, or Settings closing — so the page is wrapped in a
`container` whose `on_cleanup` calls `apply`, **deferred one tick** (`exec_after(ZERO, …)`)
because the page's scope is mid-dispose when its cleanup runs: the scope rule under *Architecture
invariants*. Committing on every leave is safe because `ai_apply` respawns only where
`ai::needs_respawn` says a session-affecting setting changed, and otherwise only persists. That
wrapper is the extra container `section_page`'s `width_full` exists for, so the two go
together. Checked in a sandboxed window: picking Claude Code and then switching to Terminal
rewrote the sandbox's `ui_state.json` with `ai_harness` `"claude"`, which nothing had saved
before the switch.
**The grade notice under it is hidden, not merely empty, when there is nothing to say.**
`Constraint::notice` is `None` on `Sealed`, and a `dyn_container` rendering `empty()` is still a
flex child, so it collected the group's 6px gap on *both* sides — which meant Claude Code, the
one harness silent here, showed the widest gap between the dropdown and the path field below it.
The container carries `Display::None` on that arm instead, and taffy filters such a child out
before the gaps are distributed (the mechanism `erd_view::toolbar_metrics` leans on), so hiding
costs nothing. The notice is held in a `create_memo` keyed on `(harness, cli_path)` rather than
computed inside the container, because the *style* has to ask the same question the child does.
**A path that is not yet a file is not probed, and the predicate deciding that is pure.**
`constraint_notice` resolves the binary and runs `--help` on it *synchronously on the Floem UI
thread*, and this memo tracks the live field — so typing a path asked a different question on
every keystroke, one per prefix, each an unresolvable path and each a blocking spawn. **No count
here on purpose**: the figure the review carried was the length of one path somebody happened to
type, and pinning it to the literal in a test would make a property of the predicate out of a
property of that string. The probe cache is keyed by `(harness,
resolved path)`, so the very thing that was supposed to make this cheap is what made every
keystroke a miss. Only two states are stable enough to ask about: empty, which means auto-detect
and is the key the app already warms, and a complete path that resolves. There is nothing to say
about a binary that is not there anyway — the red *File doesn't exist.* hint above the notice is
the whole answer. The rule is `settings::should_ask_for_a_notice(path, resolves)`, lifted out
because three lines inside a `create_memo` closure are three lines no test can reach and these
three had a failure history; `only_a_settled_cli_path_is_worth_a_probe` walks every prefix of a
typed path and asserts none of them asks.
**Model is a *field* with suggestion chips under it, not a dropdown**, because any id the CLI
accepts is valid — an alias, or a dated snapshot pinned across upgrades — and the closed
three-variant `AiModel` this replaced could not name a model released after the build. The chips
are `Harness::suggested_models` and fill the field on click; an empty list (Antigravity) leaves
the field itself untouched and takes both the chips and the sentence about them with it. The
chip row carries `Display::None` when the list is empty, for the harness notice's reason — an
empty row is still a flex child, so it kept the group's 6px gap and pushed the hint away from
the field, which is the extra space Antigravity showed under the Model box. The hint is the
other half, and is a `label` keyed on the harness rather than a constant: *"Any id this CLI
accepts — an alias, or a dated snapshot to pin one across upgrades"* is about the field and
always shows, while *"The buttons are shortcuts, not the whole list"* is about the chips and is
appended only when there are some. A hint describing a control that is not on screen reads as a
bug in the modal rather than as a note about the field.
`AiEffort` **stays** a closed enum, and that is not an inconsistency: its
vocabulary is defined by the flag, not by a vendor's model catalogue. **What it is closed
*over* is the union of every harness's flag, and that is not optional.** It held Claude's four
(`low|medium|high|xhigh`) until OpenCode arrived with `minimal|high|max`; filtering four variants
by that list leaves exactly `High`, so the dropdown became one item and two of that harness's
three levels were unreachable from the UI — the `AiModel`-coerced-to-Haiku narrowing again, in the
control next to it. The enum is now the six-way union
(`Minimal, Low, Medium, High, Extra, Max`), still ordered low→high because `clamped_to` reads a
level's rank off that order and measures distance along it. `from_cli` no longer folds `max` into
`xhigh`: they were one level while no
harness took both, and OpenCode takes `max` and not `xhigh`, so the fold made a saved `max` come
back as `Extra` and clamp to `high` on the very harness that had written it.
**The Effort row is hidden, not disabled, where the harness has no such flag** (Codex and
Cursor — and Copilot, whose flag exists but whose level list is empty; see `harness.rs`). A
greyed control still claims *this exists for you and is off*, which is a different statement and
a false one. The row builds its dropdown **inside** a `dyn_container` because a floem view is not
`Clone` and each keyed rebuild has to construct its own — and the key is **`effort_levels()`,
not `supports_effort()`**. The child builds its dropdown out of the value it is *handed*, so the
key has to carry everything the child reads: a `bool` that answers `true` for both Claude and
Antigravity cannot tell the dropdown that the list behind it changed, and the box went on
offering `xhigh` under a harness whose flag does not take it.
**That is not because the key dedups — it does not**, and the comment here said it did ("the
child was never rebuilt") until a review caught it. `dyn_container` runs `create_updater` and
there is no equality check on that path (`floem-0.2.0/src/views/dyn_container.rs`,
`floem_reactive-0.2.0/src/effect.rs`), which is the *Floem 0.2 gotchas* entry below and the
belief that cost the DDL editors their caret. A key is a value handed to the child, never a
change detector; stated as one here — where it reads as the tidiest explanation in the file — it
teaches the wrong contract to whoever copies it next.
**The filter the child applies is `AiEffort::offered_by(levels)`, not an expression in the
closure.** It was inline, and the test that claimed to guard it re-performed the same filter in
its own body, so changing the view to `.take(3)` — or to an unfiltered `AiEffort::ALL` — left the
workspace green while the dropdown offered a level the harness never advertised. A decision
inside a view closure is a decision no test can reach: the rule is in `lib.rs` beside
`clamped_to`, the test calls it, and the view calls nothing else.
That container also carries `Display::None` in the unsupported case, for the reason the
harness notice does: an empty container sitting between two 25px section gaps reads as one 50px
hole, which is what Codex showed between Model and Custom instructions. Three containers in this
modal are hidden that way now rather than rendered empty — the harness notice, the model
suggestions and this row — and they are one rule with three instances, not three local fixes.
**The modal is the only place either model or effort is set.** The status bar carried a model
menu and an effort chip until both were deleted, and the *duplication* was the cost rather than
the space they took: each was a second view of one setting that had to re-derive the same
per-harness capability, and the model chip could only ever offer `suggested_models` — never the
free id the field takes — so it presented an open field as a closed menu, while the effort chip
had to hide itself on a harness with no `--effort` flag. Two answers to one capability with two
chances to disagree. `lib.rs`'s `effort_seg_for` went with them, and the 40px break that used to
open the AI group in the bar is carried by `cpu_seg`'s own `margin_left(40)`, so the bar's
spacing is unchanged. With *Show CPU and RAM* off that margin goes with the segment — both
`.hide()` in their style, the way the Tx pill and Commit/Rollback leave, so the collapse chain
reads a segment of no width rather than a 40px gap holding nothing.
**The CLI-path caption follows the harness, and switching it clears both the path and the model
and clamps the effort.** The label is `format!("{} path", harness.label())`, and the clearing is
not tidiness:
`ai_cli_path` is one field shared by every harness, so a Claude path left behind across a switch
is resolved by `harness_bin(Codex, …)` and spawns *Claude* with Codex's argv, dying on the first
unknown flag and reporting it as an installation problem — the one thing that is not wrong with
it. Empty already means auto-detect for the new harness, so clearing is also the right value.
`ai_model` goes the same way for the same reason, with a quieter failure: it too is shared by
every harness and no CLI's `--model` accepts another CLI's ids, so picking Codex while the field
said `haiku` configured the session with a value last chosen for a different program and it died
on an unknown model. Empty already means "the harness's default" and omits the flag entirely —
but on **two** argv builders, and only one of them was covered. `turn_args` skips an empty id for
the per-turn harnesses — and a whitespace-only one, which is the same clear a keystroke short and
the divergence between the builders described under `harness.rs`
(`an_empty_model_lets_the_harness_keep_its_own_default`, which loops
`Harness::ALL` — and whose Claude and Antigravity arms used to assert nothing at all, because
`turn_args` returns `Vec::new()` for both persistent harnesses; it goes through the `spawn_argv`
helper described under `harness.rs` now, which picks each harness's own builder and refuses an
empty argv, and it asserts the positive direction as well;
`a_model_with_surrounding_space_is_sent_trimmed` is the other one that reaches every builder,
picking between `turn_args` and `session_args` off `is_persistent` so neither can drift out of
the loop). The session builder passed
`Some(&model)` straight through, so the clear this paragraph describes spawned
`claude … --model ""`, killed on a rejected id and reported as an installation problem. Both
`--model` and `--effort` are now filtered there through
`.map(str::trim).filter(|m| !m.is_empty())`, and
`an_empty_model_or_effort_is_omitted_rather_than_passed_as_an_empty_flag` is the pin for that
path — so clearing lands on the one value
every harness is guaranteed to take.
**A clear with no way back was the other half of the bug, and `HarnessFields` is the memory
that answers it.** The modal's ✕, its backdrop and Escape all committed — as leaving the page
or closing Settings does now — so there is no cancel, and the clearing fired on any `prev != now` in *both* directions — a user who opened the
dropdown to see what was there and picked their own harness again came back to three empty
fields, permanently, with the modal's own header still promising "Changes commit when the modal
closes". `harness_switch(prev, now, …, remembered)` restores what a harness held the last time
it was selected, and `HarnessFields` is **all three** fields: it was a pair, and the one left
out is the one that got lost. Effort was re-derived from the clamp on every call, and
`clamped_to` is not injective, so the round trip loses — Claude at the shipped default
**Medium**, a glance at OpenCode (`minimal|high|max`, so Medium → High), back to Claude, and
the effort is now High on the harness the user started on, chosen by nobody and written to
`ui_state.json`. Claude at `xhigh` is the same failure downward and needs only a glance at
Antigravity, whose flag stops at `high`. The memory is **session-scoped on purpose**: it is a
"you were just here" affordance, not a fourth thing to persist.
**Effort is clamped to the nearest level the harness offers rather than cleared**, because unlike
the other two it has no "the harness's default" value: `AiEffort` is a closed enum and every level
in it means something. `AiEffort::clamped_to` ranks the requested level against `ALL` and takes the
offered level at the smallest distance, breaking a tie **downward** (`min_by_key` on
`(distance, rank > want)`), and answers `None` when the harness has no effort flag at all and the
row is hidden. The argv is already clamped by `Harness::effort_arg`, so this is about what the
modal *shows*: the closed dropdown renders the selected level unconditionally, so a box reading
"Extra" under a harness that neither offers it in the list below nor sends it is a caption for a
level that exists nowhere else on screen.
**It took the *highest* offered level until a review caught it, and that is a bill rather than a
stale caption.** Taking the last surviving entry of a low→high list is the harness's most expensive
level; it only looks like clamping *down* when the level asked for sits above everything on
offer — Claude's `xhigh` against Antigravity's `low|medium|high`, which is the one case the original
test covered. OpenCode put a hole in the middle: its levels are `minimal|high|max`, so the shipped
default `Medium` fell through to `Max`, the switch persisted `"max"`, and switching back to Claude
carried that on to `xhigh`. Two turns of the harness dropdown left a user who had never opened the
Effort control on the priciest setting of both CLIs.
`a_level_out_of_range_lands_on_the_nearest_the_harness_takes` is the renamed original, and
`clamping_lands_on_the_nearest_offered_level_and_never_the_priciest_by_default` is the property
that would have caught it: every harness × every level, asserting nothing offered is strictly
closer and that a tie goes to the cheaper side. Three narrower pins sit beside it for the
directions prose argues about — `the_default_level_survives_a_round_trip_through_opencode` walks
the exact path above, `a_level_below_everything_offered_moves_up_to_the_nearest` holds that
proximity rather than "clamp down" is the rule (Claude has no `minimal`, so `Minimal` moves *up*
to `low`), and `a_tie_between_two_neighbours_picks_the_cheaper_one` holds the tie break. And
`a_clamped_level_is_always_one_the_harness_advertises` still holds the property the display depends
on.
**The clamp runs on every run of the effect; the path and the model clear only on an actual
switch.** They are different questions — "did the user change harness" against "is the level in the
box one this harness takes" — and gating the clamp on the change meant the restore from
`ui_state.json` never clamped at all: a file pairing `opencode` with `medium` came back reading
"Medium" in a closed dropdown whose list offers only Minimal/High/Max, while `effort_arg` sent no
flag. That is the stale caption the clamp exists to prevent, arrived at from the other direction.
A *remembered* level is clamped too, on the same rule: it is one this harness advertised when
it was left and its list has not changed, so the clamp is a no-op there — but the first call is
the `ui_state.json` restore, and that one can pair a harness with a level it does not take.
The effect returns the **previous** harness rather than reading the
signal twice: on the first run there is none, and clearing then would throw away the override
just restored from `ui_state.json`.
**The hint lines under the path field carry `width_full`, and that is what makes them wrap.** A
floem label sets its own taffy node width to the measured text, so the green *"Auto-detected:
`<path>`"* line and both red ones overflowed the 460px panel rather than stretching to it — and
an auto-detected path is long and has no spaces (Codex resolves to
`C:\Users\…\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe`). Bounded to the panel — the
page's `SECTION_PAGE_W` now, through `section_page`'s own `width_full` — cosmic-text's default `Wrap::WordOrGlyph` falls back to glyph-level breaks, which is what a path
wants anyway.
**The Shortcuts modal's body scrolls under a cap, and it is the last of the three that did.** It
is `autohide(scroll(body))` under `max_height(widgets::modal_body_h(560.0))`, against a modal
taller than the screen losing its *bottom* rows with no affordance saying they exist. The cap is
a `modal_body_h` rather than a constant precisely so it shrinks with a small window instead of
being right at one size only. The AI modal joined that shape last — its eight groups at 25px
apart had outgrown the window — and the main Settings modal had it first; both left it when the
one became two panes and the other a section of it. Settings' panel is a fixed `modal_h(560)`
and each page, the AI assistant's included, scrolls inside it on its own (above).
**A Shortcuts row's description wraps and its keycap pill never does.** `shortcut_row` gives the
description `flex_grow(1)` + `min_width(0)`, so a German row longer than the panel breaks onto
a second line instead of pushing the pill past the panel's edge, and the pill `flex_shrink(0)`,
so a chord like `Strg+Umschalt+C / Strg+Umschalt+V` stays whole; a 12px gap replaced the
spacer between them. The panel widened from `modal_w(420)` to `modal_w(460)` for German's
longer rows and chords as well, and later by a fifth again to `modal_w(552)`, at the user's
request — the wrap is for the row that still does not fit.
**Every word the modal writes itself comes from the catalogue** — the first file converted to
it: labels, hints, placeholders, section names and the Settings and Shortcuts titles
are `tr!` over `settings-*`/`shortcuts-*` messages (`ui/i18n.rs`), with German beside them, and
English output was compared message by message against the literals they replaced. The
Shortcuts modal's headings and rows are `shortcuts.rs`'s, converted after it (below), which is
why `shortcut_group` takes a `shortcuts::Group` and renders `group.label()` and `shortcut_row`
takes an owned description. `TermCursor::label` is `lib.rs`'s and came with that file; the
Agent CLI dropdown's *Disabled* is `agent_label`'s (above); the *Statement timeout* dropdown's
`statement_timeout_label` is `i18n::statement_timeout`'s, the renderer the app's timed-out
message quotes too (*The string catalogue*); and the three option lists core's `sqlfmt` types
name — the keyword, function and type case dropdowns', *Commas*' and *Between statements*' —
are drawn UI-side as properties of a typed value (*The string catalogue*):
`keyword_case_label` from `settings-format-case-*`, `comma_style_label` from
`settings-format-commas-trailing`/`-leading`, and `statement_gap_label` from
`settings-format-gap-one`/`-none`, its *As typed* reusing `settings-format-case-keep`.
`english_names_every_formatting_choice_as_core_does` holds all three equal to core's `label` in
English. No dropdown here shows another crate's text any more. Nor does the AI page's
data-access line: `ai_data_line(name, lvl)` is `settings-ai-data-line`, or
`settings-ai-data-line-named` led by the connection's name, over `i18n::ai_data_label` and
`ai_data_hint` — where a `format!` of core's `AiData::label` and `hint` stood —
and `the_data_access_line_reads_as_it_did` holds it to that `format!`'s English.
That is why its builders take owned text: `focusable_toggle_row`, `focusable_toggle_row_when`,
`toggle_row_layout` and `settings_group_label` take `impl Into<String>`, so a literal caller
still compiles and a `tr!` result passes as well; `SettingsSection::label()` answers a
`String`; both titles go through `modal_title_owned`; and `wrap_width_label` is the Fluent
plural `settings-format-wrap-width`, with `settings-off` for `0`. A translated placeholder is
`FieldCfg::placeholder_of(|| tr!(…))` (under `lib.rs`), `placeholder` itself being `&'static str`.
