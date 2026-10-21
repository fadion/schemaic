# `core/cli_install.rs`

`cli_install.rs` — **putting the `schemaic` command on `PATH`, and taking it off again**: the
decision half of Settings → General → Command line → Install / Remove and of the Windows
uninstall hook, with the registry write, the symlink and the broadcast left to
`schemaic-app`'s `install_cli.rs`. The CLI itself needs nothing installed — it is the app's own
argv branch, plus `schemaic.com` on Windows — but it shipped reachable only by its full path on
Windows and macOS, and getting it onto `PATH` is a write to the user's own environment, so it
happens only on a click, never at install time, and every outcome names the path it resolved.
`plan(&Probe) -> Result<Plan, Refusal>` is the whole decision: `Plan::Already { dir }`,
`AddUserPath { dir }` (Windows) or `Link { link, target }` (macOS and Linux). On Windows the
directory is the one holding `schemaic.com`, and **a copy with no shim beside it is refused**
rather than planned — a development build has `schemaic-cli.exe` there instead, so putting that
directory on `PATH` would hand a terminal the GUI
(`windows_without_the_shim_is_refused_and_says_why`). So is a folder whose name holds a `;`:
`D:\Downloads;old\Schemaic` would be read back as `D:\Downloads` and a cwd-relative
`old\Schemaic`, neither of them this folder and neither one Remove could ever match
(`a_folder_whose_name_holds_a_semicolon_is_refused`). **A copy at a drive root plans `D:\`,
never `D:`** — `parent_of` keeps a root's separator on Windows, because a bare `D:` as a `PATH`
entry is whatever folder is current on drive D, not its root
(`a_copy_at_a_drive_root_adds_the_root_not_the_drive`). **`Already` is read off
`Probe::user_path`** — the registry's user `PATH` at probe time, `%NAME%`-expanded — and off
the process `PATH` only when that can't be read, because the process `PATH` was fixed at
launch: after Install → restart → Remove it still held the folder, so Install answered
"already on your `PATH`" and wrote nothing
(`windows_decides_from_the_user_path_in_the_registry_when_it_can_read_it`). A copy only on
the machine `PATH` gets a redundant user entry, harmless. macOS and a loose Linux binary get
`~/.local/bin/schemaic` → the running executable, **and are refused when `Probe::exe_exists`
says it is gone**: once a package upgrade has replaced the binary under a running process,
Linux's `current_exe()` reports `<exe> (deleted)`, and Install linked to that and reported
success (`a_replaced_binary_is_not_linked_to`). **Under an AppImage the target is `$APPIMAGE`,
never `current_exe`**: the running executable sits inside the image's mount, which is gone the
moment the app exits, so a link to it — or an "already on `PATH`" answered because of it — would
be false by the next terminal (`an_appimage_links_the_image_not_its_mount`,
`an_appimage_mount_on_path_is_not_already_installed`); the existence check is skipped there,
since the target is the image and not `exe` (`an_appimage_links_to_the_image_whatever_exe_says`).
A `.deb`/`.rpm` install is
`/usr/bin/schemaic` with `/usr/bin` on `PATH`, so the planner answers `Already` and nothing is
written; that needs the target to be *named* `schemaic`, since a `schemaic-nightly` in a `PATH`
directory does not make the command resolve
(`a_binary_on_path_under_another_name_still_gets_a_link`).
`path_has_dir` is string work rather than `Path`, per OS — `;`, case-folding, `/` read as `\`,
quotes and trailing separators dropped on Windows; `:` and exact bytes elsewhere — so the Windows
arms answer the same when the tests run on Linux CI, where a backslash is an ordinary file-name
byte. An empty entry never matches, because on Unix it means the current directory. A drive
root keeps its separator through `normalize_entry`, which used to trim `d:\` to `d:` and so
made the root and the drive-relative entry one (`a_drive_relative_entry_is_not_the_drive_root`).
**`user_path_update` appends to the *raw* user `PATH`, and that is the point of it.** The
classic `setx PATH "%PATH%;…"` writes the *expanded, merged* system-plus-user value back into
the user key; this keeps every entry as written — an unexpanded `%USERPROFILE%\…` stays
unexpanded — and returns `None` when the directory is already there once `expand_env` has been
applied (`%NAME%` the way `ExpandEnvironmentStringsW` does it, an unknown name and a lone `%`
left as written — `user_path_update_sees_an_unexpanded_entry_as_present`).
**`user_path_type` and `decode_user_path` are how the app turns that raw value into text, and
both refuse rather than guess**, because the write goes back in the type it was read in: only
`REG_SZ` and `REG_EXPAND_SZ` pass — a `REG_MULTI_SZ` or a blob is left alone
(`only_a_text_path_value_is_read`) — and the UTF-16 units lose every trailing NUL, since one
left behind would be written back as part of the last entry
(`the_terminators_are_dropped_and_nothing_else`), then must convert losslessly: an unpaired
surrogate cannot survive a `String`, and a replacement character written back would corrupt the
entry holding it (`a_value_that_is_not_lossless_utf16_is_refused`). The two type constants are
core's own numbers, so this crate links no Windows API; the app pins them to `windows-sys`'s.
`link_step` decides what to do about whatever already sits at the link: `Create` where nothing
is, `Keep` a link already resolving to the target (a relative one resolved lexically against its
own directory), `Replace` a *dangling* one — an AppImage that was moved or deleted — and
`Refuse`, naming it, anything live pointing elsewhere or any regular file, because something
that isn't Install's is left alone. The reports (`report_already`, `report_added`,
`report_linked`) are built here so the "names the path it resolved" promise is tested
(`reports_name_the_path_they_resolved`); `report_linked` **always** adds a "put it on your
`PATH`" hint on macOS, since an app started from Finder gets launchd's minimal `PATH` and cannot
tell whether the user's shell has `~/.local/bin` (`report_linked_always_hints_on_macos`), and on
Linux only when the process's own `PATH` lacks it.
**Nothing here returns finished English**: every refusal is a `Refusal` — the `Err` of `plan`,
`removal`, `user_path_type` and `decode_user_path`, and the payload of `LinkStep::Refuse` and
`UnlinkStep::Refuse` — every outcome a `Report`, which the `report_*` functions construct
(`Linked` carrying a `LinkedHint` for the macOS and Linux `PATH` word), and the row's hint a
`RowHint`, which `hint(os)` returns. Each type's `Display` is the English these used to return
as `String`s, byte for byte, and the tests here read it; the app words every variant itself
(`install_cli.rs`, `settings::cli_hint` — under *The string catalogue*).
**`row_status(os, Option<&Result<Plan, Refusal>>) -> RowStatus { hint, can_install }` is `plan`'s
answer asked ahead of the click.** The row's hint used to be `hint(os)` alone — a description of
what Install *would* do — beside an Install that was always enabled, with `plan` run only behind
the button: on a `.deb` install the row read "Links it into ~/.local/bin", and the click
answered that the command was already on `PATH`, in `/usr/bin`. Now
`Some(Ok(Plan::Already { dir }))` gives `can_install: false` and a hint naming where — on macOS
and Linux that the command comes with the app, at `{dir}/schemaic`, joined by hand because
`dir` is a Unix path and `Path::join` would use the separator of whichever platform is asking;
on Windows that its folder is already on the user `PATH`, since `Already` there is a folder
Install or the user put there, Remove's case rather than a package's
(`a_package_install_says_where_the_command_is_and_offers_no_install`,
`a_windows_folder_already_on_path_offers_no_install_and_names_it`). Everything else — `None` for
a plan not read yet, `Link`, `AddUserPath`, and a plan that *failed* — keeps `hint(os)` and the
button (`an_install_with_work_to_do_keeps_the_hint_and_the_button`,
`an_unread_or_failed_plan_leaves_install_on_offer`). **The failed plan is the one not to
tidy**: its reason reaches the user only through the click, so a disabled button there would
refuse with no explanation — and it is why a Windows development build, whose missing
`schemaic.com` is an `Err`, keeps the old row. The tests get their `Plan` from `plan(&probe)`,
never a hand-made one, so they cover the composition rather than `row_status` alone. A link
Install made does not grey the button on macOS or Linux: `plan` goes on answering `Link` while
the target's own directory is not on `PATH`, and a second click is `link_step`'s `Keep`; on
Windows the re-read registry answers `Already`, so it does. `InstallState` (`Idle`, `Running`,
`Removing`, `Done(String)`, `Failed(String)`) is the Settings row's lifecycle — transient, never
persisted — and `busy()` is `Running | Removing`, because **either operation blocks both
buttons**: a Remove racing an Install over the same registry value or link would leave
whichever finished second as the answer while the row showed the other
(`busy_covers_install_and_remove_and_nothing_else`).
**`removal(&Probe) -> Result<Removal, Refusal>` is the inverse, and deliberately narrower: it only
ever undoes what Install could have written.** `Removal::UserPath { dir }` (Windows) is this
copy's folder, and unlike `plan` it neither asks for `schemaic.com` nor reads the process
`PATH` — the uninstall hook must clean up whether or not the shim survived
(`windows_removal_does_not_need_the_shim`), and what this process inherited says nothing about
the registry, which is what Remove edits (`windows_removal_ignores_the_process_path`).
**It is offered only to a Velopack install**: `removal` refuses when `Probe::velopack` is false —
the exe's folder is not `current\` with `Update.exe` in its parent, which the portable `.zip`
never is. A Velopack folder is one nobody else puts programs in, so an entry naming it is
Install's; a portable copy's may be a `C:\tools` the user had on `PATH` for years, where Install
answered `Already` and wrote nothing, and the entry cannot say who wrote it — so Remove would
have taken every other program in that folder off `PATH` with ours
(`a_portable_copy_is_not_offered_remove`). The cost is that a portable copy's Install has no
in-app undo; the refusal points at Environment Variables instead. The uninstall hook is
unaffected, since it only ever runs for a Velopack install.
`Unlink { link, target }` is the pair Install computes, because `plan` and `removal` both go
through `link_target`, `on_path_as_command` and `link_path` — so under an AppImage Remove looks
for a link to `$APPIMAGE` too (`unix_removal_targets_the_same_link_install_writes`).
`Package { dir }` is a deb/rpm's `/usr/bin`: the command came with the package, so removing it
is the package manager's job, and `report_package` says so instead of touching it.
`user_path_remove` takes out every entry whose *whole* `expand_env` expansion, normalised, is
the directory — so an entry written unexpanded goes too
(`remove_takes_a_variable_that_is_exactly_our_folder`) — keeps every other entry verbatim, empty
ones included, and returns `None` when there was none. **Whole, not any `;`-piece of it**: that
was the old test, so a `%TOOLS%` expanding to `C:\tools;<dir>` was dropped and took `C:\tools`
off `PATH` with ours. Such an entry is now left as written
(`remove_leaves_a_variable_that_names_other_folders_too`) — taking one folder out of a variable
is an edit to the variable, not to `PATH`. The asymmetry is deliberate: `plan` and
`user_path_update` still count that variable as holding the folder, since the command does
resolve through it, so Install writes nothing, and `removable` answers `false`, so no Remove is
offered for an entry Remove would not take. **The round-trip is pinned**:
`user_path_remove_undoes_user_path_update_exactly` gives back every entry the user wrote, and
the one difference it allows is a trailing `;`, the separator the entry was appended after.
`unlink_step` mirrors `link_step` over the same `link_points_at` resolution: `Remove` a link to
this copy *or a dangling one* — what a moved or deleted copy leaves behind, pointing at nothing
anyone could rely on — `Absent` where nothing is, and `Refuse`, naming it, a live link elsewhere
or a regular file, for Install's reason. `report_removed`, `report_path_absent`,
`report_unlinked`, `report_link_absent` and `report_package` carry the names-the-path promise
over to this half (`removal_reports_name_the_path`). **On Windows Velopack's uninstall hook runs
`removal` as well; nothing of ours runs when a macOS app or an AppImage is thrown away**, so
there the Remove button is the only undo.
**`removable(&Removal, &Found, lookup) -> bool` is whether the Settings row offers Remove at
all, and it is those two functions asked without acting** — `user_path_remove(..).is_some()`
for `Removal::UserPath`, `unlink_step(..) == UnlinkStep::Remove` for `Unlink` — so the button
and the action cannot disagree about what counts as installed. `Found` is what the app read
where the `Removal` would edit: `UserPath(raw)`, `Link(Existing)`, or `Nothing` for a package
install or a read that failed. Only those two matching pairs can answer `true`; a `Package`, an
unread state and a mismatched pairing — a Windows removal handed a link, or the reverse, which
is a bug at the boundary — are never removable, because the safe answer to any of them is not
to offer Remove (`a_package_or_an_unread_state_is_never_removable`,
`a_mismatched_read_is_never_removable`). A dangling link counts, as it does for `unlink_step`
(`removable_when_the_link_is_ours_or_dangling`).
