# `app/update.rs`

`update.rs` — the Velopack half of `core::update`: a background check at startup and every three
hours after (`RECHECK_INTERVAL`), and the "Restart to update" action `start` hands back for the
header chip. **Velopack's API is synchronous and does network + file I/O**, so every call into it
runs under `handle.spawn_blocking` and comes back through Floem's async→UI seam —
`create_ext_action` for the start-of-download and settle hops, `create_signal_from_channel` for
progress. The forwarding thread between the two channels is not
ceremony: Velopack reports progress on a `std::sync::mpsc::Sender<i16>` while
`create_signal_from_channel` wants a crossbeam receiver and must be called on the UI thread, and
`download_updates` blocks the worker for the whole download, so nothing on that thread is free to
drain the ticks. **That whole progress path — the channel pair, the forwarding thread,
`create_signal_from_channel` and the effect folding ticks in through `with_progress` — is built
once per process in `start`, not per round.** `mpsc::Sender` is `Clone`, so each round hands
Velopack its own handle; building it per round would leak a signal and an effect every interval
for as long as the app stayed open, which is precisely the sort of thing an app left running for
days would accumulate. The forwarder thread therefore parks on `recv` for the process lifetime,
which is what keeping `velo_tx` alive in `start` buys.
**Each settled round arms the next**, through `exec_after(RECHECK_INTERVAL, …)` and only when
`core::update`'s `should_recheck` says another round could still change the answer — which is why
`spawn_check`'s `finish` payload is `(CheckGate, Result<Option<VelopackAsset>, String>)` rather
than the result alone: the settle handler has to know whether the gate allowed the round. The
re-arm goes through a `Recheck` handle (`Rc<RefCell<Option<Rc<dyn Fn()>>>>`), the same
self-holding-closure shape the terminal cursor-blink tick uses, because the closure has to be able
to schedule *itself*; `start`
clones the closure out of the cell before calling it, since that first call re-arms through the
same cell and would otherwise run under a live `borrow()`. The timer bails when
`state.try_get_untracked()` is `None`, following the *Floem 0.2 gotchas* rule for perpetual
self-rescheduling ticks: the signal is disposed at shutdown and a surviving timer would read
freed memory. The feed is a `GithubSource` on `https://github.com/fadion/schemaic`, read
anonymously — 60 requests/hour per IP, and a round costs two requests (a releases listing and a
~760-byte manifest), so three-hourly polling spends 16 requests a day against the 1,440 the
limit allows: the two numbers, rather than an order-of-magnitude claim a reviewer has to
recompute. (It read *three* orders of magnitude for a while; the ratio is ~90×.) It is not shorter
because nothing would be gained: the thing being waited for is a human tagging a release.
**The pin is looser than the prose**: `the_recheck_interval_stays_clear_of_the_anonymous_rate_limit`
asserts only `per_hour < 6.0` — a tenth of the limit — so an interval shortened to 20 minutes
would keep the test green while the margin named here stopped being true. It is worth knowing
which of the two is the guard.
`UpdateManager::new` *failing* is how "not a Velopack install" is detected, and is what feeds
`check_gate`; a downgrade (`UpdateInfo::IsDowngrade` — a yanked release, or a dev build ahead of
the tag) is skipped rather than walking the user backwards silently. The apply action builds its
`create_ext_action` per click because that returns an `FnOnce` and the chip stays on screen
until the window actually goes.
**The apply action calls `wait_exit_then_apply_updates` and then `floem::close_window`, never
`apply_updates_and_restart`.** The restart-now call exits this process on the spot, which would
skip the `WindowClosed` handler `flush_session` hangs off and lose up to one debounce interval of
unsaved tab text — the same reasoning the caption bar's close button carries in
`ui::window_chrome`. Closing the window runs the normal shutdown, and the updater we just handed
off to is already sitting there waiting for this process to go away. **That shutdown ends in the
`WindowClosed` listener's own `floem::quit_app()`, not in Floem**, which exits once the last
window is gone everywhere but macOS: there the window went, the process stayed in the menu bar,
and *Restart to update* waited for an exit only the menu bar's *Quit* produced (*app/main.rs*,
the `WindowClosed` flush).
**`FORCE_UPDATE_BADGE` is a development switch and shipping it `true` is a real hazard**, which
is why a test now holds it down. It pins the state at `UpdateState::Ready` and skips the real
check — the only way to look at the "Restart to update" chip while working on it, since every
other route there needs two tagged releases and an update genuinely in flight. Left `true` in a
commit it would show every user a permanent restart offer that does nothing (nothing is staged,
so `apply_action` returns early), and nothing else in the tree reads it, so the build would be
perfectly green. `the_forced_update_badge_is_off_in_a_committed_tree` is the part that notices;
a `const _: () = assert!(…)` would catch it a step earlier but refuse to compile exactly when a
developer has flipped it on purpose. The module's own *decisions* live in `core::update`
(`check_gate`, `should_recheck`, `UpdateState::with_progress`) and are tested there — what is
left here is orchestration that needs a real install and a real feed, plus the constants, so
the constants are what the local tests pin: the switch, the published `OPT_OUT_VAR` name, the
feed URL, and `RECHECK_INTERVAL` against the anonymous rate limit.
**Every leg packs on an explicit, non-default Velopack channel — `win-x64`, `linux-x64`,
`osx-arm64` — and those three strings are effectively permanent.** Left to default the channel
would be plain `win` and `linux`, and a *default* channel reaches only the manifest name: both
platforms would then emit a package called `Schemaic-<version>-full.nupkg`, which two jobs
cannot both upload to one GitHub Release. The rejected upload is the mild failure.
`GithubSource::download_release_entry` resolves a package by matching its `FileName` across the
release's assets, so a Linux client following `releases.linux.json` would be handed whichever
asset won that name — plausibly the Windows build. Naming a non-default channel puts it into
every file name instead, which is what keeps the two sets apart: measured on 2026-08-19,
`--channel win-x64` produced `Schemaic-0.16.0-win-x64-full.nupkg`, `Schemaic-win-x64-Setup.exe`
and `releases.win-x64.json`, against `Schemaic-0.16.0-linux-x64-full.nupkg`,
`Schemaic-linux-x64.AppImage` and `releases.linux-x64.json` for `--channel linux-x64`.
**There is no app-side counterpart and none is needed**: the channel is baked into the manifest
of the release an app was packed from, so an installed app already asks for the channel it came
from. `update.rs` passes `None` for `UpdateManager::new`'s whole `UpdateOptions`, which leaves
`ExplicitChannel` unset — that field is for *switching* channels, not for declaring the one you
were built on. That is also what makes the names unchangeable afterwards, which is stated as an
invariant below.
**Releases carry full packages only — `release.yml` passes `--delta None` on both platforms —
and that is a correctness choice, not a bandwidth one.** Deltas broke the *second* consecutive
update. A client that reaches version N by applying a delta ends up holding a **locally
reassembled** package for N rather than the one CI built: v0.16.1's package was 23,606,057 bytes
on the release and 23,605,881 on disk after reassembly, with a different SHA1. The delta for
N+1 is computed against CI's copy, so it is applied to a base the build never saw. v0.16.0 →
v0.16.1 worked only because that base had arrived whole from `Setup.exe`; v0.16.1 → v0.16.2
failed in the field, and failed *silently*, because that is what `UpdateState::Failed` does. A
full package always verifies against the manifest whatever route the client took to get where it
is. The cost is roughly 15 MB per update on Windows, and next to nothing on Linux, where the
delta was saving 16% against an already-compressed AppImage.
**The Linux `.deb` and `.rpm` sit deliberately outside all of the above.** `release.yml` wraps
the same zigbuild binary the tarball carries with `packaging/linux/` — `stage-payload.sh` lays
out the payload once so the two formats cannot drift, `build-deb.sh` and `build-rpm.sh` +
`schemaic.spec` add each format's metadata, and `install.sh` at the repo root picks between
them and the AppImage. Those installs land in `/usr/bin`, which is *not* a Velopack install, so
`UpdateManager::new` fails, `check_gate` answers `NotInstalled` and `should_recheck` ends the
poll loop for good — the correct outcome, because a distribution package is the package
manager's to update, and there is deliberately **no UI at all** telling a `/usr/bin` install
that a version exists: the header chip never appears there, because nothing it could offer
would be the right way to take the update. **That reasoning used to be aspirational**: the
package manager was the right owner but had nothing to fetch from, which is why this passage
once ended by calling the AppImage the recommended Linux artifact for that reason.
`packaging/repo/` is what made it true, and no Rust changed to do it.
**`packaging/repo/` publishes a signed APT repository and a signed DNF/zypper repository to
GitHub Pages at `https://fadion.github.io/schemaic`, so a `.deb`/`.rpm` install is carried
forward by `apt-get upgrade` / `dnf upgrade` with the rest of the system.** `build-site.sh` is
the whole build: it collects the `.deb` and `.rpm` assets of the five most recent releases,
calls `build-apt-repo.sh` (a `pool/`, a `dists/stable/`, a clearsigned `InRelease` and a
detached `Release.gpg`, all through `apt-ftparchive`) and `build-rpm-repo.sh` (`rpmsign
--addsign` over every package, `createrepo_c`, a detached `repodata/repomd.xml.asc`), then
writes the client configuration, both encodings of the public key — a binary keyring for apt's
`Signed-By`, an armoured `.asc` for dnf's `gpgkey=`, because an inline armoured key needs apt
2.4 and Debian 11 and Ubuntu 20.04 do not have it — and the landing page from `index.html.in`.
**The GitHub Release assets are the source of truth and the site is derived**: nothing is
incremental, every run downloads the packages again and rebuilds from nothing. That buys two
things — there is no accumulated state to corrupt, and a repository that has gone wrong is
repaired by re-running the workflow rather than by unpicking what the last run did.
**There is no `gh-pages` branch, on purpose.** A branch carrying ~40 MB of packages per release
would be cloned by everyone who ever clones this repository, forever, and pruning it later
would not shrink a clone that already exists; an Actions deployment has no git history, so the
published size is the only size — five releases is about 205 MB against Pages' 1 GB site limit,
under a 100 GB/month bandwidth allowance.
**Signing is mandatory and `pages.yml` fails closed without a key**, and that does not reverse
the ship-unsigned decision below: this is *repo* signing, which attests that a package came
from this repository unaltered and vouches for no identity, where Authenticode attests to a
publisher. The alternative here is not "an unsigned repository" — the option apt actually
offers is instructing every user to write `[trusted=yes]`, i.e. to turn the check off, which is
a weaker posture than the direct download it replaces. The key is RSA-4096 with **no expiry**:
EdDSA is unverifiable by RHEL-era `rpm`, and an expiry would break `apt update` on a date
rather than on a release — a scheduled outage with nothing to do about it but rotate. For the
same span of clients the rpm metadata is compressed `gz` rather than `createrepo_c`'s zstd
default, which RHEL-8-era dnf cannot read; it costs a few hundred kilobytes.
**The `.rpm` published on the GitHub Release stays unsigned and the repository re-signs its own
copy**, so the two files differ by exactly the signature header — same payload, same version,
and `dnf upgrade` replaces one with the other. The asymmetry is deliberate: a release must not
be able to fail on a missing GPG key, a repository publish can and does.
**`verify-site.sh` runs before every deploy and is the only check there is** — no unit test
could stand in for it, because the failure class is not a wrong answer from a function but
metadata disagreeing with the packages beside it: a `Packages` naming a `.deb` that was pruned,
a `Release` describing the previous run's `Packages.gz`. Every tool involved reports success
for those, and they surface on a user's machine at install time. So it reads the built site
back the way a client would, offline: `gpgv` and `rpmkeys` against the *published* keyring
rather than the developer's own — a machine holding the secret key would answer yes either
way — and `verify-site.py` re-hashes every file the indexes name and pins the client
configuration. It was exercised against two deliberate corruptions, a flipped byte in a pooled
`.deb` and a deleted `Packages.gz`, and failed on both.
**The publish is *called*, not triggered.** `release.yml`'s `pages` job is `needs: release`,
gated on `github.ref_type == 'tag'`, and `uses: ./.github/workflows/pages.yml` with `secrets:
inherit`. A release created with `GITHUB_TOKEN` does not fire the `release` event, so a
workflow listening for one would never start; a workflow keyed on the tag would start *beside*
`release.yml`, before any asset existed, and publish the previous version. `needs: release` is
the only ordering that is true, because the site is built by downloading the release this run
just made. Failing there retracts nothing — the Release is complete and every direct download
still works; it means the repositories describe the previous version until `workflow_dispatch`
on the Pages workflow rebuilds them.
**`install.sh`'s `debian` and `rpm` families now add the repository and install from it**, and
the old single-package bodies survive as `install_deb_direct` / `install_rpm_direct` — reached
by `SCHEMAIC_NO_REPO=1`, and for rpm automatically on a machine with neither `dnf` nor
`zypper`, which has no repository support worth the source-list entry. Those two are the route
with nothing behind it, so the tail messaging names the real update route per family rather
than one line for all of them. The first publish landed 2026-09-06 and was checked from the
outside rather than declared: `InRelease` and `repomd.xml.asc` verify under `gpgv` against the
published keyring, and `install.sh` was run from `main` in a clean `debian:12` and a clean
`fedora:41` container, each resolving the hand-written dependency list and installing 0.24.0 —
then downgraded and carried back up by `apt-get upgrade` / `dnf upgrade`, which is the whole
claim. On the rpm side `dnf` did that with `gpgcheck=1` and `repo_gpgcheck=1` in force, so it
verified the package signature and the index signature and would have refused either way round.
**Both packages hand-write their dependency lists, and no scanner can replace them**: `readelf
-d` on the binary lists glibc and nothing else, because winit reaches X11, Wayland and xkbcommon
through `libloading` and wgpu reaches Vulkan and EGL the same way. An automatically derived list
therefore produces a package that installs cleanly and then cannot open a window. The two spell
the same set differently — the `.deb` names Debian *packages* because dpkg has no soname
provides, the spec names *sonames* because every rpm distribution auto-provides them, which is
what lets one spec serve Fedora, RHEL and openSUSE despite their disagreeing on nearly every
package name.
**A Linux shell finds the running window's icon by name, and on Wayland it had none to find.**
It matches a window to the desktop entry whose file name is the window's application id — the
Wayland `app_id`, or the class of X11's `WM_CLASS` — and draws that entry's `Icon=`. Floem
0.2.0's `WindowConfig` has no way to set one, and floem-winit 0.29.5 sends `app_id` only when its
platform attributes carry a name, which Floem never passes. On X11 winit falls back to argv[0],
`schemaic`, and the entry's `StartupWMClass=schemaic` matched that; under GNOME on Wayland —
Ubuntu 22's default — nothing was sent, and an `install.sh` install showed a generic icon in the
taskbar and dock while launcher search showed the right one. `WindowConfig::app_id` is the
vendored Floem's first patch (`vendor/floem/PATCHES.md`, and *Floem 0.2 gotchas*): on free Unix
it reaches winit as `WindowBuilderExtX11::with_name(id, id)`, and since the X11 and Wayland
`with_name` set the same field, one call names the window under both. `app/main.rs` passes
`core::APP_ID` — `io.github.fadion.Schemaic`, beside `APP_NAME` in `core/lib.rs` — which is at
once the window's id, the `.desktop` file's name, the icon's name and the macOS `--bundleId`,
and so is identity in the invariant's sense (*A Velopack channel name is app identity*).
`the_app_id_names_the_desktop_entry_and_matches_every_packager`, beside the constant, requires
`packaging/linux/<APP_ID>.desktop` to exist and **every packager's copy** to spell the same
value — `install.sh`'s and `stage-payload.sh`'s `APP_ID="…"`, `schemaic.spec`'s `%global
appid`, the metainfo file's `<id>` and `<launchable>`, and `release.yml`'s `--bundleId`; it was
watched fail with the constant lower-cased. On a case-insensitive filesystem the entry's
existence cannot tell the two spellings apart, so the text comparisons are what catch that.
**Verified on Wayland, not yet on X11**: a Linux debug build run under WSLg with
`WAYLAND_DEBUG=1` and a throwaway `XDG_CONFIG_HOME` sent
`xdg_toplevel.set_app_id("io.github.fadion.Schemaic")` before its `set_title`. X11 reads the
same winit field into `WM_CLASS`, but `xprop` has not been run against it; nor has a real GNOME
dock been seen to draw the icon.
**`StartupWMClass=schemaic` stays, and is not dead weight**: it is what matches a build from
before the patch on X11, and `install.sh`'s AppImage route fetches the `.desktop` from `main`
while the binary beside it comes from the latest release, so the two can be any number of
commits apart. It does not stand in the way of the name match — GNOME tries both.
**macOS is a third leg on its own `osx-arm64` channel — permanent for the reason above, and
named `arm64` rather than a bare `osx` so that an Intel leg can be added later without renaming
this one — and Velopack builds its bundle for us.** `release.yml` hands `vpk pack` a staged
`dist/` and it produces `Schemaic.app`, a `Schemaic-osx-arm64-Setup.pkg` and a ditto-zipped
`-Portable.zip` of the bundle — which is why macOS is the one leg that publishes no archive of
its own, and why the `--noPortable` Windows passes is absent there. **That zip is built but not published**: it is the only predictable
handle on the finished bundle, and a `hdiutil` step unpacks it into the `.dmg` that *is*
published, because the zip and the `.dmg` are the same route and only one of them is the one
Mac users recognise. **A `.app` dragged from that `.dmg` self-updates exactly as the `.pkg`'s
does** — Velopack's `OsxVelopackLocator` works from "am I inside a `.app`" and finds `UpdateMac`
in `Contents/MacOS`, caring nothing for how the bundle arrived. The `.icns` `vpk` insists on (it validates the extension) is generated on
the runner from `assets/icon-1024.png` with `sips` and `iconutil`, so the PNG stays the single
icon source. Unlike a `.deb`, a `.pkg` install **is** a Velopack install, so the in-app update
check runs there exactly as it does on Windows. It is unsigned and un-notarized by the same
decision the Windows installer carries.
**That decision is to ship unsigned, and deliberately *not* self-signed** (taken 2026-08-19).
The app is open source with no audience yet, so neither an application to the SignPath
Foundation nor a paid certificate earns its cost today — but the part worth writing down is that
self-signing is not the cheap fallback it looks like. A self-signed Authenticode certificate
chains to no root any user trusts, so SmartScreen and "Windows protected your PC" say *Unknown
publisher* exactly as they do for an unsigned binary, and no reputation accrues: SmartScreen
reputation attaches to a certificate chaining to a *trusted* root, plus the file hash. Some AV
heuristics score an untrusted signature worse than none. On Linux the question does not arise in
that form at all — there is no CA-based code signing, only GPG with your own key, self-signed by
construction, and nothing on the path we ship verifies it: no runtime or desktop checks an
AppImage's embedded signature by default, and Velopack does not sign AppImages at all. GPG
becomes mandatory only if we run our own apt repository, since apt refuses an unsigned
`Release` — which is no longer hypothetical: `packaging/repo/` does, and `pages.yml` fails
closed without a key. That is repo signing rather than code signing, so it leaves this decision
exactly where it stands; the distinction is drawn where those repositories are described above.
**Signing is not a prerequisite for auto-update**, and an earlier version of this reasoning
wrongly made it one, on the grounds that auto-update pushes executables. Velopack's update
integrity comes from HTTPS plus a size/hash check against `releases.<channel>.json`
(`ChecksumFailedException`); nothing in its documented flow verifies an Authenticode signature on
a downloaded package. Authenticode's real value here is the *first-install* SmartScreen
experience and detecting a locally tampered file — polish, not correctness. What we eat for it:
`Setup.exe` shows the unknown-publisher warning on first download, identical to the raw `.zip` it
replaced and so not a regression, and Squirrel-family installer stubs have a history of AV false
positives that being unsigned makes likelier — which lands on `Update.exe`, the one that runs on
every update. Worth watching once there are real users.
**When we do sign, the hook is one `vpk pack` argument, not a redesign** — `--signParams` (short
`-n`) or `--signTemplate`, which signs the app *and* Velopack's `Update.exe`/`Setup.exe` stubs —
which is why this decision blocks nothing. The options on file: signpath.org, the Foundation, is
free for open source (not signpath.io, the paid enterprise product it runs on), and a licence
check on 2026-07-26 found us eligible — MIT, OSI-approved, no proprietary components — at the
cost of an application, staying actively maintained, and a publisher string reading "SignPath
Foundation" rather than ours; the flow would be `signpath/github-action-submit-signing-request`,
packing first, then submitting, then attaching the signed result to the Release, with the key
never touching our runner. Certum's Open Source Code Signing certificate (~€70–100/yr,
cloud-signable in CI, in *our* name) is the paid alternative. Azure Trusted Signing (~$120/yr) is
out regardless: since 2025-04-02 its onboarding admits only US/Canada organisations with three
years of history, no individuals and no EU enrolment. All of these are OV, and **no OV
certificate grants instant SmartScreen trust** — reputation still builds with download volume, so
signing buys a better first-run dialog rather than a silent one.
**`release.yml` also answers to `workflow_dispatch`, with every publish step gated on
`github.ref_type == 'tag'`.** A tag used to be the only way to run the file, which made a
packaging mistake discoverable only once the tag existed and was awkward to retract — the macOS
leg was proven this way before it ever saw a version number. The dry run additionally skips the
feed-history fetch: without a tag the version falls back to the workspace one, and for any
channel that has already shipped that is a version `vpk pack` refuses to pack over.
**`main.rs` runs `velopack::VelopackApp::build().run()` near the top of `main`**, before tracing,
the font registration, the tokio runtime and any Floem signal or `Scope`: the installer and the
updater re-invoke the exe with `--veloapp-*` args, and `run()` services those and then
*terminates the process*, so anything set up ahead of it is either built to be thrown away or
half-initialised when the process dies mid-hook. With no hook args it returns immediately, so a
normal launch pays nothing. It sits deliberately *after* the `--mcp-serve` early exit, which is a
different program — a stdio JSON-RPC server whose stdout is the protocol stream, so nothing may
write to stdout ahead of it. The two flag sets never co-occur: `--veloapp-*` comes from the
installer or the updater, and `--mcp-serve` comes from **whichever agent CLI the user picked** —
Claude spawning this binary out of the `--mcp-config` file Schemaic writes it, and Codex,
Antigravity, OpenCode, Copilot and Cursor out of their own configuration with `--endpoint-file`
beside it. So the ordering between them is free, and this way the protocol stream stays clean
whichever of the six opened it.
**The headless-CLI branch sits between those two, and being *before* Velopack is the load-bearing
part.** `schemaic list`/`databases`/`ping`/`tables`/`describe`/`query`/`explain`/`export`/`dump`/`compare`/`exec`/`mcp`/`version` return from `main` ahead of the hook, the file
logger, the fonts and Floem: none of that belongs in a one-shot command, and `auto_apply_on_startup` is free
to find a staged package and exit-and-relaunch the process — which is safe for a launch that has
read no session state and is not safe in the middle of a command whose output someone is piping.
What routes it is `schemaic_cli::args::wants_cli`, an allowlist of the **first** argument, which
is what keeps `--veloapp-*` and `--mcp-serve` out of clap's parser; see its entry for what
routing on "has arguments" would have broken. On Windows the branch is reachable only from a
debug build or from `schemaic.com`, a GUI-subsystem binary having no console to print to.
**`release.yml`'s Windows leg builds two binaries for that**, `cargo build --release -p
schemaic-app -p schemaic-cli`, and copies `target/release/schemaic-cli.exe` into the staged
directory as `dist/schemaic.com`. Linux and macOS build one and use the argv branch.
**One Velopack hook is ours, and only on Windows**: there the builder carries
`.on_before_uninstall_fast_callback(|_| { logging::init(); install_cli::on_uninstall(); })`
before `.run()`, and every other platform keeps the plain `.run()`. Velopack runs
`schemaic.exe --veloapp-uninstall <ver>` before it deletes the install folder, calls the hook,
then exits — no UI, a 30-second budget — so the folder Install put on the user `PATH` comes back
off instead of surviving as a dead entry. **`logging::init()` is inside the hook on purpose**,
the one place ahead of the normal `init` that needs it: with no UI the log is the only record
of what the hook did, and it lives in the config directory, which the uninstall leaves behind.
Velopack has no uninstaller on macOS or for an AppImage, so nothing of ours runs there at all.
