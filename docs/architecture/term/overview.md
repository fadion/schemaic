# `schemaic-term`

`schemaic-term` — terminal panel + shell (`shell.rs`).
**`shell::shell_to_persist(loaded, detected, selected)` is which shell profile a settings save
writes, and it exists because a saved choice has to survive a launch that could not detect it.**
The picker's index is an index into `detect_shells()`, and that list is short whenever the shell
is unreachable at startup: on Windows `wsl.exe -l -q` exiting non-zero yields no WSL rows at all,
and on Unix a program that has left `/etc/shells` (or an unreadable `/etc/shells`) leaves a single
`/bin/bash`. `init_selected` matched the saved profile by `(program, args)`, found nothing and
`.unwrap_or(0)` gave **0** — and because the appearance-saving `create_effect` runs its body
immediately on creation, `terminal.json` was rewritten with `detected[0]` **before the window was
drawn, with no user action**. The choice was then gone for good: WSL coming back could not restore
it, because nothing remembered it any more. There was no visible symptom until the next launch,
the session itself spawning from `TerminalSettings::shell` directly — except on **Restart**, which
read the same index and replaced the user's running shell with `detected[0]` unexplained. The rule
is the loaded profile when the detected list does not contain it, the picker's row otherwise, and
three tests fail against the bare `detected.get(selected).cloned()` it replaces — including two
WSL distros told apart by their `-d` args, since matching on the program alone would let one
distro's presence vouch for another's. The app holds `current_shell` beside the index (*what is
running*, which is not the same thing); `term_apply_shell` is its one writer, so the moment the
user picks something the saved preference gives way — which is the half a saved-profile fix can
get wrong, and is a hand check in `review/user-verify-fix.md` rather than a test.
**The reader notifies once per *unread* grid state, not once per read** (`Pending`, an
`AtomicBool` with `mark`/`taken`). Its buffer is 8 KB and it used to notify on every successful
read, so `cat` of a 10 MB log is ~1,280 notifies with no coalescing point anywhere downstream:
`term_notify` sends on an *unbounded* channel, floem's `create_signal_from_channel` drains its
deque with one `set` per queued message, `Signal::run_effects` runs subscribers synchronously
with no equality check, and `ApplicationHandle::idle` paints only once every queued trigger has
run. So each 8 KB chunk cost one full `snapshot()` — a fresh `cols × lines` grid plus a `String`
per run — and one rebuild of the whole terminal `dyn_container`, every row and every run, on the
UI thread, before a single frame was painted. The work was O(chunks) rather than O(frames), and
the window neither repainted nor accepted input until the backlog cleared — with the panel
**closed**, too, since the effect lives in `app_view` and the shell is spawned at launch, so a
`.bashrc` running `neofetch` spends the same thread. `mark` returns true only on the `false →
true` edge and `Terminal::snapshot` clears the flag, so a reader running ahead of the UI notifies
once and then stays quiet until its last output has actually been read. Nothing is dropped: the
state is in the grid and the pending notify only says there is a new one. **Exit is not
coalesced** — `read_loop`'s final notify after `exited.store` must arrive whatever the flag says,
or a shell that exits with its last output already read leaves the panel showing a live prompt
for a process that is gone.
