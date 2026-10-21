# `app/install_cli.rs`

`install_cli.rs` — the side-effect half of `core::cli_install`, which owns every decision: this
gathers the `Probe` (`current_exe`, whether it still `exists`,
`APPIMAGE`, `HOME`, `PATH`, whether `schemaic.com` sits beside the exe, whether it is a
Velopack install — its folder named `current`, compared ASCII-case-insensitively, with
`Update.exe` in the parent, neither of which the portable `.zip` ships — and on Windows
`user_path_now` — the registry value read and expanded) and performs the `Plan` (`install`) or
the `Removal`
(`remove`); `on_uninstall`, Windows-only, is `remove` for the Velopack hook with the outcome
logged rather than shown — a copy that never ran Install finds nothing and changes nothing.
What `install` and `remove` hand back is the interface's wording: core answers with a `Refusal`
or a `Report`, and the private `refusal` and `report` word each variant from
`install-refuse-*` / `install-report-*`, `english_refuses_every_install_as_core_does` and
`english_reports_every_install_as_core_does` holding the English equal to core's `Display`. So
the hook logs that wording, not `Display` — English all the same, since it runs before
`i18n::init` and the loader starts on `en-GB`.
**On Windows it is a read-modify-write of `HKCU\Environment\Path` in the value's own registry
type**, through `RegCreateKeyExW`/`RegQueryValueExW`/`RegSetValueExW`: a `REG_EXPAND_SZ` must
stay one or every `%USERPROFILE%\…` entry in it stops resolving, and a missing value is created
as `REG_EXPAND_SZ`. A value of any other type, or one that is not valid UTF-16, is **refused
rather than written back lossily**, and both refusals are core's: `win::UserPath::open` only
reads the bytes — sizing with the first `RegQueryValueExW`, truncating its buffer to the length
the second returns — and hands the type to `cli_install::user_path_type` and the units to
`decode_user_path`. That decode used to sit inline here, the one decision made outside core and
one no test could reach without a registry. Core's `REG_SZ`/`REG_EXPAND_SZ` are its own numbers,
and `cores_registry_types_are_windows_own`, a `cfg(windows)` test and this module's only one,
pins them to `windows-sys`'s. Both directions share that through a `win` module:
`UserPath { key, ty, raw }`, whose `open` reads the raw value in its own type (or refuses it),
whose `write` puts a value back in the type it was read in, and whose `Drop` closes the key, so
every early return between the open and the write closes it too; and
`broadcast_environment_change`, below. A directory the write finds already in the registry —
added between the planner's read and this one, or missed because the planner could not read the
registry and fell back to the process `PATH` — reports as added without a second write, and
one Remove finds absent reports `report_path_absent` and writes nothing. After a write,
`SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, "Environment", SMTO_ABORTIFHUNG, 5 s)`
tells Explorer, so a terminal it starts next gets the new value; that is best-effort, the value
being written either way, and a terminal already open keeps the old `PATH`, which
`report_added` and `report_removed` say. `env_lookup`, the `%NAME%` resolver both directions
and `installed` hand to core, is `cfg(windows)`: nothing else reads it, and left unconditional
it is dead code on Linux, which CI's clippy `-D warnings` fails. **The whole operation runs on a
`std::thread`, not the UI thread**, because
the broadcast waits on every top-level window and a slow one should stall a thread nobody is
looking at. This is `schemaic-app`'s first *direct* `windows-sys` dependency — 0.61,
`cfg(windows)`, features `Win32_Foundation`, `Win32_Security`, `Win32_System_Registry` and
`Win32_UI_WindowsAndMessaging` — at the version already in the tree transitively, so no new crate
version enters the lock file. **`Win32_Security` looks unused and is not**: `RegCreateKeyExW`
takes a `SECURITY_ATTRIBUTES` pointer and windows-sys compiles it out without that feature. The
workspace builds without it anyway, because another crate enables it for the shared
`windows-sys` — so the omission built green, and only compiling this module on its own showed
it. On macOS and Linux `existing_at` reads what is at the link
(`symlink_metadata`, `read_link`, and `exists` to follow it for liveness) for both directions.
`link_command` asks `link_step`, then `create_dir_all`s `~/.local/bin`, removes a dangling link
it is replacing and `std::os::unix::fs::symlink`s; `unlink_command` asks `unlink_step` and
`remove_file`s the link — which on a symlink removes the link, never what it points at, so an
AppImage outlives its link.
**`installed()` is the row's other question — would Remove do anything — and it decides
nothing itself**: it runs the same `probe` and `removal`, reads what sits where the `Removal`
would edit into a `core::cli_install::Found` — `read_user_path` through the same
`win::UserPath::open` Remove uses but with `Access::Read` (`RegOpenKeyExW`, `KEY_QUERY_VALUE`
only: a check that runs at every launch has no business asking for write access or creating the
key, which `Access::Write`'s `RegCreateKeyExW` would), `read_link_state` through `existing_at`,
nothing at all for a `Package` — and asks `removable`. **Every failure answers `false`**: a probe or removal that
errs, a registry value `open` refuses, a link that can't be read. The worst case is then a
missing button, never a Remove that reports it found nothing. **`row_status()` is the third
question — what the row says, and whether Install has anything to do** — and decides as little:
`probe`, `plan`, and core's `row_status`, with a probe that fails passed on as a plan not yet
read, so it too keeps the plain hint and an enabled Install, whose click reports why. The
closures the Settings row calls are built in `main.rs` next to `update::start`, one factory making both: it owns the
**double-click guard** — a click on either button while `state.busy()` is ignored, so an
Install blocks a Remove and the reverse, the buttons themselves staying enabled — and posts the
result back with `create_ext_action` as a `(Result, (bool, RowStatus))`, the pair being
`installed()` and `row_status()` re-read together on the same worker thread once the operation
is done, so Remove appears or goes, and Install greys or returns, in the same update that shows
the outcome. The block also reads the pair once at startup, on a `std::thread` of its own — a
registry read or a `stat`, but not the UI thread's to wait on — and until that answers
`CliCommand::installed` is `false` and `status` is `row_status(os, None)`: the plain hint and an
enabled Install.
**Verified by hand on both platforms.** Linux, under WSL: Remove with nothing there, Install
then Remove, a dangling link removed, a live link elsewhere and a regular file both refused, an
AppImage's target file surviving the removal, and the package case. Windows: an Install → Remove
round-trip left the real user `PATH` byte-identical and still `REG_EXPAND_SZ`, and a second
Remove reported it absent; and the real debug `schemaic.exe --veloapp-uninstall 0.26.0`, with
`APPDATA` redirected to a scratch profile, removed a seeded entry, exited 0, left `PATH`
byte-identical to the backup and logged the removal. **`row_status` is not yet verified by
hand**: the greyed Install and the "comes with the app" hint need a real `.deb` install to see.
