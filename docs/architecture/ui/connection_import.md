# `ui/connection_import.rs`

`connection_import.rs` — the **Import Connections** modal, over `core::conn_import` and the
app's `conn_sources`. Raised from the Manage Connections list ("Import connections" — it was
"Import from another client", which misled once the modal read Schemaic's own exports and
Docker's containers too — below New connection and quieter than it: it is the first-run action, and the one nobody
presses twice), and painted *directly above* that modal in `modals.rs` — the question is about
the list behind it, so closing returns to it. Its own term in `modal_backdrop_up`, like
`manage_open`'s, because it is a loose child of the layer and can outlive the modal that raised
it. **"Export connections" sits under that row and is not part of this modal**: the same dim
`in_ring_button` (`icons::UPLOAD` beside Import's `DOWNLOAD`, ring index `NAV_TAB + 3`; all
three rows' faces are `widgets::menu_row_face`, so a label ellipsizes to the `scaled(210)` column), calling
`ConnActions::export_connections`, which goes straight to a save dialog — the app's closure,
titled *Export connections*, defaulting to `schemaic-connections.json`. A typed name without
`conn_import::EXPORT_FILE_SUFFIX` gets it appended (`conn_import::ExportPath::of`), after any
`.json` of its own and in the user's casing, because the suffix is the only thing *Choose a
file…* recognises an export by — the `.json` is compared as bytes, since a name like `日本` puts
the cut inside a character, where a `str` slice panics in the dialog's callback (found reviewing
it, before it shipped). **A renamed path that already exists is asked about first**
(`ExportPath::asks_before_writing`, through the app's `confirm`): the OS dialog checked the name
as typed, so the write landing on `team.schemaic-connections.json` for a typed `team` replaced a
teammate's export with nothing asked; a name the dialog itself showed is its own consent. A *New
connection* nobody has filled in (`Connection::is_untouched_new`) is left out: a 127.0.0.1 placeholder is nothing
to share. The file is written with `persist::write_file_atomic` on `handle.spawn_blocking`; a failure
reaches the error modal through `file_error`, and a success says nothing. What goes in the file
is `core::conn_import`'s decision, under that entry.
**Four ways in, and the modal opens empty.** A pasted *Connection URL* at the top, then
*Choose a file…*, *Scan installed clients* and *Docker containers* beneath it, then — only once one of them has
produced something — the review list. The URL field holds its *Add* inside it
(`FieldCfg::button`, under `lib.rs`) — beside it, as a `control_button`, it stood shorter than
the field — and sits on the default field surface, `bg_deepest`: it wore `bg_editor`, with no
recorded reason since the dialog arrived in cc1af200, and was the one lighter form field in the
app (GitHub #19). **All three source buttons wear the `Quiet` fill.** *Scan installed clients*
wore `Primary` while it stood beside *Choose a file…* alone, since it is what most people come
for (GitHub #18); once *Docker containers* made a third source, one lit button among three read
as the odd one out rather than the obvious one, so it went `Quiet` with the others. Their row
(`row_of_buttons`) **wraps**: three side by side fit English and little else, and under
doubled text the third was cut in half at the modal's edge. The footer's
Import is now the dialog's one `Primary`. Opening does **not** scan: the walk reads the user's home
directory, and a dialog that goes through it because it was opened is doing something nobody
asked for. The four sources all append through the app's one `add_import_result`, so a scan
cannot discard a URL pasted before it and the four cannot disagree about ticking or about
duplicates. **Three of the four run off the UI thread** — the client scan always did, Docker's
did from the start (two process spawns and a daemon round trip), and the
picked file now does: its read, its parse, its `~/.pgpass` completion and its merge all used to
happen inside the file-picker callback, with a frozen window and no cancel, over the one input
whose size and shape the app controls least. It is a `handle.spawn_blocking` reporting through
`create_ext_action`, the shape `scan_installed_clients` beside it already had.
`empty_message` (pure, tested) is why the scan button doesn't look dead on a
machine with none of those clients: an empty list means three different things — an invitation,
progress, an answer — and `ConnImportUi::scanned` is the bool that tells the first from the
third. **The scan's *finding* path has no hand check behind it, and that is a limit of the
harness rather than of the scan**: a sandboxed launch redirects `APPDATA`, which is exactly what
`conn_sources::config_roots`/`data_roots` walk on Windows, so a scan run that way can only ever
find nothing. A session can confirm that the button fires and that the empty answer is the third
state; anything about a row the walk actually produced has to be read off the pure half
(`core::conn_import`) or checked by hand on an unredirected profile.
*Docker containers* (`ConnActions::scan_docker`, over `conn_sources::docker_source`) is `Quiet`
like the other two — one source among several, and the only one that starts a program
— at tab stop 45 after the scan's 40, and reads *Asking Docker…*, disabled, while
`ConnImportUi::docker_busy` is set, for the reason the scan button is disabled while it runs.
That is its own flag rather than `scanning` because the two buttons are independent and either
may be pressed while the other is out. What Docker says that is not a row — not installed, the
daemon down, or *No database containers are running.*, which the app writes when the answer held
neither a row nor a skip, containers that are not databases included — goes in
`ConnImportUi::docker_note`, its own slot under the buttons beside `file_error`, as a
`form_hint` in the label colour rather than an error: "nothing is running" is an answer, and
most of the rest are about the machine rather than a mistake. `open_import` resets both — the
second clearing site, as it is for `scanning`, because a panic on the blocking side never
reaches the callback that would clear the flag.
A review list, not a form: a row is a *proposal* with a tick box, its name, where it points
(`row_target`, mono — `user@host:port/db`, or the file's name on SQLite; pure and tested, and
deliberately **not** a `scheme://` URL, since this project has no URL builder on purpose and
one written to fill a label is what the next reader copies for something that matters), and
capsules for the engine and the source. A tunnel is part of where a row points, so its SSH server
follows the target as `via user@host:port` (`a_tunnelled_target_names_its_ssh_server`); how it
signs in and init SQL reach the screen as notes (`SignsInAs`, `ThroughSsh`, `RunsInitSql`, under
`core::conn_import`'s entry), not as more of the target. Every `ImportNote`
it carries is joined into a third line shown only when there is one — a row can be both
already-saved and password-less, and showing only the first hides the one that decides whether
to tick it. That line is in `status_warn` when it holds `AlreadySaved` or a `SignsInAs`, and
`text_muted` otherwise. Rows that repeat a saved connection arrive unticked, and so do rows that
sign in with the user's own identity. The tick box is `widgets::check_box` — the app's one checkbox, which
was written here first and moved to `widgets` once the dump picker adopted it — and it changes
**by style, never a rebuild**; the
selection is a `HashSet` of indices so a row's read is O(1) — the two rules `dump_view`'s table
picker paid for. Indices are safe because `rows` only ever grows at the end or is reset
wholesale by `open_import`.
The footer is rebuilt on **whether anything is selected**, not on how many — an Import button
left enabled over an empty selection imports nothing while looking like it worked, but keying
the container on the *count* rebuilt both buttons (and their focus-ring registrations) on every
tick of a selection still being made. The count is needed for the label alone, so it goes
through `widgets::action_button_dyn`, whose label is reactive while the button around it is
not. The "Added N connections." line shows only while nothing is selected, which is exactly the
window between an import finishing and the user asking for another — so it can never sit beside
an enabled Import describing a *previous* press. **It has its own full-width wrapping row above
the button row**, where it used to share that row as a `text_ellipsis()` slot about 350 px wide:
the sentence it carries runs to ~190 characters, so the user read `Added 3 connections.
Read-only and the en…` and the warning the line exists to deliver — that read-only and the
environment badge are *not* carried over — never reached the screen at all. The slot had been
sized for the sentence it was first given. That row is `widgets::modal_footer_noted`'s. Its
first spelling, a `v_stack` of note and buttons handed to `modal_footer`, sat inside the actions
slot, which is sized to its content — so the note's `width_full` had nothing to resolve against,
the sentence never wrapped, and it pushed Close and Import off the modal's right edge instead.
The note is `Display::None` while it has nothing to say, so the empty row does not take the gap
— and **what it says and whether it is there are one memo over `outcome_note(idle, done)`**
(pure, tested), which keys the content and decides the `Display::None` alike. They were two
closures re-deriving one predicate, which had to agree for the row to be neither an empty gap
nor hidden over the warning it carries.
`skipped_sentence(skipped, hidden)` (pure, tested)
names up to three left-out entries and counts the rest, returning `None` for an empty list *and*
a zero `hidden` so a stray "0 entries were not imported" can't reach the screen. `hidden` is what
`conn_import::SKIPPED_CAP` kept out of the list; it counts toward the total and nowhere else,
since the whole point of the cap is that those entries were never worth keeping and the whole
point of the count is that a large one is the signal the user needs.
**The row list is bounded at `conn_import::ROW_VIEW_CAP` (500), and says so when it bites.** Not
a cap on the import — `rows` holds every connection and Import creates all of them — but on what
is *constructed*: the list is a plain `v_stack_from_iter` of one `h_stack` with a check box,
three texts and two capsules per row, inside a `scroll` showing about five, rebuilt whole on
every change to `rows`. A `.env` that really is a list of 140,000 URLs would build close to a
million views to show five. `conn_import::row_overflow_sentence` is the disclosure — the view
says it through `row_overflow_note` (`conn-import-row-overflow`), held equal to it in English —
and is separate from the cap so the *disclosure* is the tested half: a list that silently stops at 500
of 140,000 is the failure this would otherwise introduce. A cap rather than a `virtual_stack`,
because an import row is two lines or three depending on whether it carries notes, so
virtualizing means a `VirtualItemSize::Fn` restating the layout — and there is no screenshot
harness here to catch it being wrong. The Live Monitor's change log answered the same question
the other way, because a change row is always one line and `Fixed` is therefore honest for it;
the rule that splits the two is a Floem gotcha.
**Off `whole_ui_gate`'s list, 6 to zero, and it is the cleanest sweep of that campaign because
the module has exactly one piece of state.** All six of its `&Ui` functions read
`ui.conn.import` and nothing else out of the signal half, so all six take `ConnImportUi`:
`paste_row`, `source_buttons`, `row_list`, `skipped_note` and `footer`, each beside the actions
it calls (`add_pasted_url`, `choose_import_file`/`scan_installed_clients`/`scan_docker`,
`import_chosen`), and
`conn_import_overlay(ConnImportUi, Rc<ConnActions>)`, which takes the actions bundle whole for
`schema_settings_overlay`'s reason — it is the one that hands all five down. The tell is worth
carrying to the next module: **a body that opens `let imp = ui.conn.import;` and never says `ui`
again was already narrow and only had to say so**, and six of six were, which is why a file's
number says nothing about how hard it is to move — this 6 took minutes where `overlays.rs`'s
remaining 4 are immovable.
**Its words are the catalogue's** (`conn-import-*`) — the title (through
`modal_title_owned`), the URL field's label, hint and *Add*, the three source buttons and their
busy words, `empty_message`'s three sentences (it returns `String` now), the plural found count
(`conn-import-found`, pinned by `the_found_count_reads_as_it_did_in_english`), Select all/None,
and Import through `import_label` — `conn-import-import` at zero, `conn-import-import-count`
with `$count` otherwise; Close is the shared `dialog-close`. `skipped_sentence` is two whole
messages, `conn-import-skipped` and `conn-import-skipped-more` (plurals on `$count`, with
`$names` and `$rest`), where an opening clause used to be spliced into a `format!`. A row's
notes are core's word drawn UI-side: `import_note_label(ImportNote)` replaces
`ImportNote::label` (`conn-import-note-*`, one `conn-import-note-signs-in-*` key per
`AuthMode`), and
`english_words_every_import_note_as_core_does` holds its English equal to core's. The
`import_label`, `empty_message` and `skipped_sentence` tests pass unchanged. Why an entry was
left out is worded in `i18n.rs`, phase 4's shape, since the app's paste field says it too:
`i18n::url_error_text(&UrlError)` from `conn-import-url-*`, and
`i18n::skip_reason_shown(&SkipReason)` from `conn-import-reason-*` (a `BadUrl` puts
`url_error_text` inside `conn-import-reason-unreadable`) — the *name (why)* of the skipped
sentence here, and the app's paste-field error in `add_pasted_url`, which is
`conn-import-url-empty` or the first skip's `skip_reason_shown`.
**`skip_reason_shown` ends in `conn_import::redact_shown`, over the whole sentence**, for the
reason `SkipReason::message` is redacted (`conn_import.rs`'s entry). `url_error_text` is not
redacted, so it is private and reached only through it.
`i18n::tests::english_says_every_import_refusal_as_core_does` holds every `UrlError` and every
`SkipReason` — a `BadUrl` of each error among them, and the `hunter2` case — equal to core's
English, and `english_says_the_row_overflow_as_core_does` the overflow line. English stays: the URL placeholder, a
sample; the library text an `Unreadable` or `NotAnExport` carries; and the "Added N connections." line,
which the app's import action writes, not this file.
