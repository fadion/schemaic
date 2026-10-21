# `app/notify.rs`

`notify.rs` — the opt-in **desktop notification** for a run that ended behind another window:
`show(title, body)`, called from `main.rs`'s `call_back_after` only after
`history::wants_attention` has said yes and only with *Notify when a run finishes in the
background* on. The words are `finished_note`'s: `history::finished_note`'s arguments worded
from `notify-{query,script,export,dump}-{finished,failed}` and `notify-took`, whose `$duration`
is `i18n::took(history::took(ms))`, in the interface's units and marks (German `12,3 s`,
where remarking core's `format_duration` had given `12,3s`), and
`english_says_every_finished_run_as_core_does` holds the English equal to core's for every kind,
outcome and width of duration. Through `notify-rust` 4.18 — a
WinRT toast on Windows (`tauri-winrt-notification`, over the `windows` 0.62 already in the
tree), the notification centre on macOS, `org.freedesktop.Notifications` on Linux. **It runs on
a `std::thread` of its own** because on Linux it is a blocking D-Bus call, and a notification
daemon that never answers must not freeze the window; a failure is a `tracing::warn!` and
nothing else, since the taskbar has already been asked either way. The crate's default `z`
feature puts it on zbus 5 over `async-io`, beside the zbus 4 the keyring and rfd's `ashpd`
share — which `conn::secrets`'s `zbus_is_not_on_its_tokio_backend` checks along with the
other, since it reads every `zbus` the lock resolves (see *Floem 0.2 gotchas*).
**Every platform needs the app's identity to put a name on it, and takes it differently.**
Windows shows a toast only for an AppUserModelID it can resolve; without one it drops the toast
or shows it as PowerShell's. An unpackaged app's is registered under
`HKCU\Software\Classes\AppUserModelId\<core::APP_ID>` — `DisplayName` *Schemaic* and an
`IconUri` naming `notification-icon.png` in `persist::config_dir()`, written from the bundled
`assets/icon.png` when it is missing or differs from it — and the toast is sent under that id. The key is
**rewritten on every notification** rather than once: two values, and a key someone deleted
comes back without anyone having to notice it went. The icon file is compared with the bundled
bytes each time, so a new icon reaches the toast with the release that ships it. Nothing — the
`--veloapp-uninstall` hook included — removes the key or the file on uninstall. **The
*process's* AppUserModelID is deliberately left alone**: setting it would move the window to a
different taskbar group from the installed shortcut's. macOS attributes the notification to a
bundle id through `notify_rust::set_application(APP_ID)`; Linux names the icon `APP_ID` and
sends `Hint::DesktopEntry(APP_ID)`, the desktop entry's file name. That makes the toast's
registry key one more thing `APP_ID` names (*Architecture invariants*).
