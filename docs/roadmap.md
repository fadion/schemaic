# Schemaic — roadmap

The larger pieces of work that are planned but not started, each with the decisions already taken,
what is deliberately out of scope, and the order it should land in — several sessions, a design
choice or two, traps worth writing down before anyone walks into them.

Keep it disjoint from its neighbours: a fact about the system as it *is* belongs in
the architecture document (`architecture.md` and `architecture/`), an instruction about how to
work belongs in `AGENTS.md`. When a phase here lands, its facts move into the architecture files in
the same change and the phase is deleted from this file rather than ticked off.

Both entries below came out of the first hands-on run on a Mac. They are meant to ship together,
with whatever the next macOS round turns up, as one release.

## macOS: closing the last window quits the app

On macOS, closing the window leaves the process running: the menu bar still reads *Schemaic*
and only its *Quit* ends it. *Restart to update* is where this bites. It hands the staged update to
Velopack (`wait_exit_then_apply_updates`), which waits for this process to exit before it applies
the update and relaunches, then calls `floem::close_window` (`app::update`, `apply_action`). The
window goes, the process stays, and the updater waits until the user finds the menu bar's *Quit*.
Observed once, in a fast run, so worth reproducing before the fix as well as after.

The cause is the vendored Floem: `AppHandle::close_window` (`vendor/floem/src/app_handle.rs`)
calls `event_loop.exit()` once the last window is gone on every platform *except* macOS, where the
parameter is `_event_loop` and nothing exits. Nothing in the app or in Floem calls
`floem::quit_app`, so on macOS nothing ever ends the loop, `main` never returns, and the tokio
runtime is never dropped.

### Decided

- **Quit from the app, not by patching Floem.** Call `floem::quit_app()` at the end of the
  `WindowClosed` listener in `app/main.rs`, after `flush_session`, `flush_filter` and
  `flush_closed`. The flushes run synchronously before it, so `quit_app` cutting the loop short
  loses nothing, which is the reason `window_chrome`'s close button gives for not calling it
  directly. On the other platforms the loop has already been asked to exit and the call is
  harmless. No new `vendor/floem/PATCHES.md` entry.
- **The app is single-window, so the macOS convention of staying alive with no window does not
  apply.** There is no window to reopen it into from the Dock.

### Traps

- **Two passages assert the opposite and must be corrected in the same change.** The comment
  above the close button in `ui/window_chrome.rs` ("Floem exits on its own once the last window
  is gone") and `docs/architecture/ui/window_chrome.md`; `docs/architecture/app/update.md`'s
  "closing the window runs the normal shutdown" rests on the same assumption.
- **Check that *Quit* from the menu bar and Cmd+Q still save the session.** They go through
  winit's own `NSApplication` terminate, which is not vendored and was not read. If that path does
  not fire `WindowClosed`, tabs typed in the last debounce interval are lost there, a separate bug
  this fix does not touch.
- **The fix can only be built on Windows.** The hand check needs a Mac: close the window with the
  red button and the caption close button, and the process must be gone (no menu-bar entry, absent
  from Activity Monitor); then a real *Restart to update* between two tagged releases must relaunch
  as the new version with no manual *Quit*.

## Read connection secrets when connecting, not at startup

After an update, macOS asked for the login password several times in a row, apparently once per
stored secret, until *Always allow* was clicked. Two causes compound:

- **The keychain ties *Always allow* to the app's code signature**, and the macOS build ships
  unsigned (`docs/architecture/app/update.md`), so on Apple Silicon it carries only the linker's
  ad-hoc signature, which is a hash of the binary. Every update is a new app to the keychain, and
  every item asks again. This entry does not fix that. Only a stable signing identity would.
- **The GUI reads every secret eagerly at startup**, one keychain item per connection per
  `SecretKind` (`core::secrets::hydrate_file`, through `conn::secrets::KeyringStore`), so every
  prompt arrives at launch, for connections the user may not open that day.

### Decided

- **Keep the storage layout: one keyring entry per connection per secret, on every platform.**
  The layout stays identical on macOS, Windows and Linux. A single macOS item holding every secret
  was considered and rejected because macOS would then store secrets differently from the other
  two. The cost accepted with it: after an update macOS still asks once per secret, but only when
  that connection is used.
- **The GUI hydrates a connection's secrets the first time something needs them**: connecting
  (the tree, a tab's run, a pinned `Session`, the Live Monitor, an import, a compare), opening its
  connection form, or handing the AI panel's `--mcp-serve` child its password. It reads them
  once per process and keeps them in memory, so one *Always allow* per item lasts until the next
  update. It reads only the kinds the connection uses (`unused_kinds`, as the CLI's
  `hydrate_connection` does). The form is the exception: it reads all three, because it can
  switch the auth method.
- **Lazy on every platform, not macOS only.** One code path. A locked Linux keyring benefits the
  same way: the unlock prompt waits until a connection needs it.
- **Startup reads nothing.** A connection the restored session reconnects to at launch (the active
  connection's tree, restored tabs) still asks then. That is the user's own connection being
  used, so it is acceptable.

### Traps

- **"Never read" is a third state, and the save path only knows two.** `sanitize_file` deletes the
  entry for every empty field that is not `unreadable`, and before this change an empty field
  meant "read and absent" or "the user cleared it". With lazy hydration every connection not yet
  used has empty fields, and the first ordinary save, such as the read-only toggle, would delete
  their credentials. `Hydration` has to record what was *read*, and the save has to leave alone
  any field it never read. This is the regression to test first, red against today's
  `sanitize_file`.
- **Don't rewrite every secret on every save either.** `sanitize_file` currently `set`s every
  non-empty field. A write to an item another signature created can prompt on macOS too, so a save
  should write only the secrets that changed.
- **Every `Connection` a `Db` is built from must have been hydrated first.** The eager load made
  that true for free, and lazy loading makes it a step a caller can forget. A connection that skips
  it shows up as an authentication failure with an empty password, not as a missing secret. Put
  it in the one place connection handles are minted, not in each caller.
- **The prompt runs on whatever thread asks.** Today it is the UI thread at startup, before the
  window is up. Lazily it would come mid-session, and a blocking keychain call on the UI thread
  freezes the window while the dialog is open. Read off the UI thread, as the connect it precedes
  already runs.
- **The legacy-plaintext migration (`needs_resave`) runs at load today.** It reads the file, not
  the keyring, so it can stay eager.
- **Still open, from `TODO.md`:** whether the keychain prompts when the CLI runs through its PATH
  symlink. That is a signature question, untouched by this entry. Check it in the same macOS round.
