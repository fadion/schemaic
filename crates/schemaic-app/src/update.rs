//! Auto-update plumbing — the Velopack half of [`schemaic_core::update`].
//!
//! One background check per launch: ask the GitHub Releases feed, download the
//! update (delta when Velopack can compute one), stage it, and let the footer
//! offer a restart. The decisions worth testing — whether to check, what the
//! footer says, which progress ticks may move the state — live in the core module
//! and are unit-tested there; what's here is the I/O and the thread hops.
//!
//! **Velopack's API is synchronous and does network + file I/O**, so every call
//! into it runs on `spawn_blocking` and comes back through Floem's async→UI seam
//! (`create_ext_action` / `create_signal_from_channel`). Nothing in this module
//! touches a signal off the UI thread.

use std::cell::RefCell;
use std::rc::Rc;

use floem::ext_event::{create_ext_action, create_signal_from_channel};
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate, create_effect};
use floem::window::WindowId;
use schemaic_core::update::{CheckGate, UpdateState, check_gate};
use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateManager, VelopackAsset};

/// The release feed. Velopack reads the repository's GitHub Releases through the
/// public API — anonymous, hence rate-limited to 60 requests/hour per IP, which a
/// once-per-launch check is nowhere near.
const RELEASE_REPO: &str = "https://github.com/fadion/schemaic";

/// Set this to `1`/`true`/`yes`/`on` to stop the app contacting GitHub at all.
const OPT_OUT_VAR: &str = "SCHEMAIC_NO_UPDATE_CHECK";

/// Build an `UpdateManager` for the release feed.
///
/// Fails when this isn't a Velopack-managed install — a portable-zip extraction,
/// a `cargo run` dev build, a distro package — which is the ordinary case during
/// development and the signal [`CheckGate::NotInstalled`] is derived from, not an
/// error worth showing anyone.
fn manager() -> Result<UpdateManager, velopack::Error> {
    UpdateManager::new(GithubSource::new(RELEASE_REPO, None, false), None, None)
}

/// Start the one-shot background update check and return the action that applies
/// what it staged.
///
/// The returned closure is what the footer's "Restart to update" segment calls.
/// It is a no-op until something is staged, so it's safe to wire unconditionally.
pub fn start(
    cx: Scope,
    handle: &tokio::runtime::Handle,
    window: WindowId,
    state: RwSignal<UpdateState>,
) -> Rc<dyn Fn()> {
    // The asset the check staged, held on the UI thread and cloned into the
    // worker when the user restarts. Both writers (the check's completion
    // callback and the apply action) run on the UI thread, so a `RefCell` is the
    // whole of the synchronisation needed.
    let staged: Rc<RefCell<Option<VelopackAsset>>> = Rc::new(RefCell::new(None));

    spawn_check(cx, handle, state, staged.clone());
    apply_action(cx, handle.clone(), window, state, staged)
}

/// Run the check + download on a worker thread, reporting into `state`.
fn spawn_check(
    cx: Scope,
    handle: &tokio::runtime::Handle,
    state: RwSignal<UpdateState>,
    staged: Rc<RefCell<Option<VelopackAsset>>>,
) {
    // Velopack reports download progress over a `std::sync::mpsc::Sender<i16>`,
    // and Floem's `create_signal_from_channel` wants a crossbeam receiver — and
    // has to be called here, on the UI thread. Hence the pair plus a forwarding
    // thread: `download_updates` blocks the worker for the whole download, so
    // nothing on that thread is free to drain the ticks. The forwarder ends when
    // the worker drops its sender, which is to say when the download finishes,
    // fails, or is never started.
    let (velo_tx, velo_rx) = std::sync::mpsc::channel::<i16>();
    let (ui_tx, ui_rx) = crossbeam_channel::unbounded::<i16>();
    std::thread::spawn(move || {
        while let Ok(pct) = velo_rx.recv() {
            if ui_tx.send(pct).is_err() {
                break;
            }
        }
    });
    let progress = create_signal_from_channel(ui_rx);
    create_effect(move |_| {
        if let Some(pct) = progress.get() {
            // `with_progress` is what keeps a tick queued behind the end of the
            // download from replacing the restart offer with "Updating… 100%".
            state.update(|s| *s = s.with_progress(pct));
        }
    });

    // Two hops back to the UI thread: one when the download starts (so the footer
    // can show progress at all — `with_progress` only moves an in-flight
    // download), one when the whole thing settles.
    let begin = create_ext_action(cx, move |version: String| {
        state.set(UpdateState::Downloading { version, pct: 0 });
    });
    let finish = create_ext_action(cx, move |res: Result<Option<VelopackAsset>, String>| {
        match res {
            Ok(Some(asset)) => {
                let version = asset.Version.clone();
                tracing::info!("update {version} staged; waiting for a restart");
                *staged.borrow_mut() = Some(asset);
                state.set(UpdateState::Ready { version });
            }
            Ok(None) => state.set(UpdateState::Idle),
            // Deliberately not surfaced: a background poll that couldn't reach
            // GitHub is not something to interrupt the user over, and the next
            // launch retries. The log is where this is answerable from.
            Err(e) => {
                tracing::warn!("update check failed: {e}");
                state.set(UpdateState::Failed { message: e });
            }
        }
    });

    let opt_out = std::env::var(OPT_OUT_VAR).ok();
    // Invisible (`Checking` has no label), but it keeps the state machine honest:
    // every path out of here settles it back through `finish`, so the state always
    // describes what the updater is actually doing rather than what it last did.
    state.set(UpdateState::Checking);
    handle.spawn_blocking(move || {
        let um = manager();
        match check_gate(opt_out.as_deref(), um.is_ok()) {
            CheckGate::OptedOut => {
                tracing::info!("update check disabled by {OPT_OUT_VAR}");
                finish(Ok(None));
                return;
            }
            CheckGate::NotInstalled => {
                tracing::debug!("not a Velopack install — no update check");
                finish(Ok(None));
                return;
            }
            CheckGate::Allowed => {}
        }
        // `Allowed` is only reachable through `um.is_ok()`.
        let um = um.expect("manager checked by the gate");

        let info = match um.check_for_updates() {
            Ok(UpdateCheck::UpdateAvailable(info)) => info,
            Ok(UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty) => {
                finish(Ok(None));
                return;
            }
            Err(e) => {
                finish(Err(e.to_string()));
                return;
            }
        };
        // A downgrade means the feed's newest release is *older* than what's
        // installed — a yanked release, or a dev build ahead of the tag. Applying
        // it would silently walk the user backwards, so leave it alone.
        if info.IsDowngrade {
            tracing::info!("remote release is older than the running build — skipping");
            finish(Ok(None));
            return;
        }

        let asset = info.TargetFullRelease.clone();
        begin(asset.Version.clone());
        match um.download_updates(&info, Some(velo_tx)) {
            Ok(()) => finish(Ok(Some(asset))),
            Err(e) => finish(Err(e.to_string())),
        }
    });
}

/// The "Restart to update" action.
fn apply_action(
    cx: Scope,
    handle: tokio::runtime::Handle,
    window: WindowId,
    state: RwSignal<UpdateState>,
    staged: Rc<RefCell<Option<VelopackAsset>>>,
) -> Rc<dyn Fn()> {
    Rc::new(move || {
        let Some(asset) = staged.borrow().clone() else {
            return;
        };
        // Built per click rather than once up front: `create_ext_action` hands
        // back an `FnOnce`, and this closure has to survive being called again —
        // the segment stays on screen until the window actually goes. We're on the
        // UI thread here (it's a click handler), which is where `create_ext_action`
        // must be called from anyway.
        let handover = create_ext_action(cx, move |launched: Result<(), String>| match launched {
            Ok(()) => {
                // **`close_window`, not `apply_updates_and_restart`.** Velopack's
                // restart-now call exits this process on the spot, which would
                // skip the `WindowClosed` handler `flush_session` hangs off and
                // lose up to one debounce interval of unsaved tab text — the exact
                // failure the caption-bar close button documents in
                // `ui::window_chrome`. Closing the window runs the normal
                // shutdown, and the updater we just launched is already sitting
                // there waiting for this process to go away.
                floem::close_window(window);
            }
            Err(e) => {
                tracing::error!("could not launch the updater: {e}");
                state.set(UpdateState::Failed { message: e });
            }
        });
        handle.spawn_blocking(move || {
            let launched = manager().map_err(|e| e.to_string()).and_then(|um| {
                // Hands off to the updater and returns — it sits waiting for this
                // process to exit, then swaps the files in and relaunches us.
                // `silent = false` so a failure is visible; `restart = true` so
                // the user lands back where they were, with no extra argv.
                um.wait_exit_then_apply_updates(&asset, false, true, Vec::<String>::new())
                    .map_err(|e| e.to_string())
            });
            handover(launched);
        });
    })
}
