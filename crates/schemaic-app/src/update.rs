//! Auto-update plumbing — the Velopack half of [`schemaic_core::update`].
//!
//! A background check at startup, and again every [`RECHECK_INTERVAL`] while the
//! app stays open: ask the GitHub Releases feed, download the update (delta when
//! Velopack can compute one), stage it, and let the header's update chip offer a
//! restart. The decisions worth testing — whether to check, whether to check
//! *again*, what the chip says, which progress ticks may move the state — live in
//! the core module and are unit-tested there; what's here is the I/O and the
//! thread hops.
//!
//! **Velopack's API is synchronous and does network + file I/O**, so every call
//! into it runs on `spawn_blocking` and comes back through Floem's async→UI seam
//! (`create_ext_action` / `create_signal_from_channel`). Nothing in this module
//! touches a signal off the UI thread.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use floem::action::exec_after;
use floem::ext_event::{create_ext_action, create_signal_from_channel};
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate, create_effect};
use floem::window::WindowId;
use schemaic_core::update::{CheckGate, UpdateState, check_gate, should_recheck};
use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateManager, VelopackAsset};

/// The release feed. Velopack reads the repository's GitHub Releases through the
/// public API — anonymous, hence rate-limited to 60 requests/hour per IP, which a
/// once-per-launch check is nowhere near.
const RELEASE_REPO: &str = "https://github.com/fadion/schemaic";

/// Set this to `1`/`true`/`yes`/`on` to stop the app contacting GitHub at all.
const OPT_OUT_VAR: &str = "SCHEMAIC_NO_UPDATE_CHECK";

/// A development switch, and it **must stay `false` in anything committed**.
///
/// Flip it to `true` to pin the state at [`UpdateState::Ready`] on startup and
/// skip the real check, which is the only way to look at the header's
/// "Restart to update" chip: every other route to that state needs two tagged
/// releases and a genuine update in flight, so the one piece of UI the feature
/// exists to show is otherwise unreachable while working on it.
///
/// The real check has to be *skipped* rather than merely pre-empted — on a dev
/// build it settles to `NotInstalled` → `Idle` within milliseconds and would wipe
/// the pinned state before it was ever drawn.
///
/// Harmless while on: nothing is staged, so the chip is inert and clicking it
/// does nothing (see [`apply_action`], which returns early on an empty `staged`).
/// Shipped as `true` it would show every user a permanent restart offer that
/// silently does nothing, which is why it is called out here rather than left as
/// a quiet constant.
const FORCE_UPDATE_BADGE: bool = false;

/// Build an `UpdateManager` for the release feed.
///
/// Fails when this isn't a Velopack-managed install — a portable-zip extraction,
/// a `cargo run` dev build, a distro package — which is the ordinary case during
/// development and the signal [`CheckGate::NotInstalled`] is derived from, not an
/// error worth showing anyone.
fn manager() -> Result<UpdateManager, velopack::Error> {
    UpdateManager::new(GithubSource::new(RELEASE_REPO, None, false), None, None)
}

/// Start the background update checks and return the action that applies what
/// they staged.
///
/// The returned closure is what the header's "Restart to update" chip calls.
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

    // Off in anything committed — see `FORCE_UPDATE_BADGE`.
    if FORCE_UPDATE_BADGE {
        state.set(UpdateState::Ready {
            version: "0.16.0".to_string(),
        });
        return apply_action(cx, handle.clone(), window, state, staged);
    }

    // Velopack reports download progress over a `std::sync::mpsc::Sender<i16>`,
    // and Floem's `create_signal_from_channel` wants a crossbeam receiver — and
    // has to be called on the UI thread. Hence the pair plus a forwarding thread:
    // `download_updates` blocks its worker for the whole download, so nothing on
    // that thread is free to drain the ticks.
    //
    // Built **once for the process**, not per check, even though checks now
    // repeat: `Sender` is `Clone`, so each round hands Velopack its own handle
    // while the signal, the effect and the forwarder stay single. Per-round would
    // leak a signal and an effect every interval for as long as the app is open.
    // The forwarder parks on `recv` for the app's lifetime, which is what keeping
    // `velo_tx` alive here buys.
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

    // A self-holding closure, so each settled check can schedule the next one —
    // the same shape `main`'s resource sampler and the terminal cursor-blink tick
    // use (`BlinkTick`), not the plain recursive `fn` the health poll uses. It has
    // to be filled in after construction because the closure has to be able to
    // reach itself.
    let again: Recheck = Rc::new(RefCell::new(None));
    {
        let handle = handle.clone();
        let staged = staged.clone();
        let velo_tx = velo_tx.clone();
        let again_inner = again.clone();
        *again.borrow_mut() = Some(Rc::new(move || {
            spawn_check(
                cx,
                &handle,
                state,
                staged.clone(),
                velo_tx.clone(),
                again_inner.clone(),
            );
        }));
    }
    // Clone out of the `RefCell` before calling, rather than calling under a live
    // `borrow()` — the first check re-arms through this same cell.
    let first = again.borrow().clone();
    if let Some(run) = first {
        run();
    }

    apply_action(cx, handle.clone(), window, state, staged)
}

/// Re-arm one more background check. `None` until [`start`] fills it in, because
/// the closure inside has to be able to schedule itself.
type Recheck = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// How long to wait before asking the feed again.
///
/// The cost of a round is two requests — a releases listing and a ~760-byte
/// manifest — against GitHub's anonymous limit of 60 per hour per IP, so this is
/// three orders of magnitude clear of it and could be far shorter without
/// trouble. It is not shorter because nothing is gained: the thing being waited
/// for is a human tagging a release.
const RECHECK_INTERVAL: Duration = Duration::from_secs(3 * 60 * 60);

/// Run one check + download on a worker thread, reporting into `state`, and
/// schedule the next round through `again` if one could still change the answer.
fn spawn_check(
    cx: Scope,
    handle: &tokio::runtime::Handle,
    state: RwSignal<UpdateState>,
    staged: Rc<RefCell<Option<VelopackAsset>>>,
    velo_tx: std::sync::mpsc::Sender<i16>,
    again: Recheck,
) {
    // Two hops back to the UI thread: one when the download starts (so the chip
    // can show progress at all — `with_progress` only moves an in-flight
    // download), one when the whole thing settles. Both are built per round
    // because `create_ext_action` hands back an `FnOnce`.
    let begin = create_ext_action(cx, move |version: String| {
        state.set(UpdateState::Downloading { version, pct: 0 });
    });
    let finish = create_ext_action(
        cx,
        move |(gate, res): (CheckGate, Result<Option<VelopackAsset>, String>)| {
            let settled = match res {
                Ok(Some(asset)) => {
                    let version = asset.Version.clone();
                    tracing::info!("update {version} staged; waiting for a restart");
                    *staged.borrow_mut() = Some(asset);
                    UpdateState::Ready { version }
                }
                Ok(None) => UpdateState::Idle,
                // Deliberately not surfaced: a background poll that couldn't reach
                // GitHub is not something to interrupt the user over, and the next
                // round retries. The log is where this is answerable from.
                Err(e) => {
                    tracing::warn!("update check failed: {e}");
                    UpdateState::Failed { message: e }
                }
            };
            state.set(settled.clone());

            if !should_recheck(gate, &settled) {
                return;
            }
            exec_after(RECHECK_INTERVAL, move |_| {
                // The signal is disposed at shutdown, and a timer that outlives it
                // would read freed memory — the same rule the cursor-blink and
                // resource-sampler ticks follow.
                if state.try_get_untracked().is_none() {
                    return;
                }
                let run = again.borrow().clone();
                if let Some(run) = run {
                    run();
                }
            });
        },
    );

    let opt_out = std::env::var(OPT_OUT_VAR).ok();
    // Invisible (`Checking` has no label), but it keeps the state machine honest:
    // every path out of here settles it back through `finish`, so the state always
    // describes what the updater is actually doing rather than what it last did.
    state.set(UpdateState::Checking);
    handle.spawn_blocking(move || {
        let um = manager();
        let gate = check_gate(opt_out.as_deref(), um.is_ok());
        match gate {
            CheckGate::OptedOut => {
                tracing::info!("update check disabled by {OPT_OUT_VAR}");
                finish((gate, Ok(None)));
                return;
            }
            CheckGate::NotInstalled => {
                tracing::debug!("not a Velopack install — no update check");
                finish((gate, Ok(None)));
                return;
            }
            CheckGate::Allowed => {}
        }
        // `Allowed` is only reachable through `um.is_ok()`.
        let um = um.expect("manager checked by the gate");

        let info = match um.check_for_updates() {
            Ok(UpdateCheck::UpdateAvailable(info)) => info,
            Ok(UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty) => {
                finish((gate, Ok(None)));
                return;
            }
            Err(e) => {
                finish((gate, Err(e.to_string())));
                return;
            }
        };
        // A downgrade means the feed's newest release is *older* than what's
        // installed — a yanked release, or a dev build ahead of the tag. Applying
        // it would silently walk the user backwards, so leave it alone.
        if info.IsDowngrade {
            tracing::info!("remote release is older than the running build — skipping");
            finish((gate, Ok(None)));
            return;
        }

        let asset = info.TargetFullRelease.clone();
        begin(asset.Version.clone());
        match um.download_updates(&info, Some(velo_tx)) {
            Ok(()) => finish((gate, Ok(Some(asset)))),
            Err(e) => finish((gate, Err(e.to_string()))),
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
        // the chip stays on screen until the window actually goes. We're on the
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

#[cfg(test)]
mod tests {
    use super::{FORCE_UPDATE_BADGE, OPT_OUT_VAR, RECHECK_INTERVAL, RELEASE_REPO};

    // The rest of this module is orchestration over Velopack and Floem scopes —
    // a worker thread, two `create_ext_action` hops, a self-holding re-arm
    // closure — and none of it can be driven without a real install and a real
    // feed. Its *decisions* were pushed down into `core::update` (`check_gate`,
    // `should_recheck`, `UpdateState::with_progress`) and are tested there. What
    // is left here is the constants, and one of them is a live hazard.

    /// **The dev switch, shipped.** `FORCE_UPDATE_BADGE` pins the header at
    /// "Restart to update" and skips the real check — the only way to look at
    /// that chip while working on it. Left `true` in a commit it would show
    /// every user a permanent restart offer that does nothing at all, and the
    /// build would be perfectly green: nothing else in the tree reads it, and a
    /// dev build looks correct precisely because that is what it was flipped
    /// for. Its own doc comment says it must stay `false`; this is the part
    /// that notices.
    ///
    /// A `const _: () = assert!(…)` would catch it a step earlier but at the
    /// wrong cost: it would refuse to *compile* while the switch is flipped,
    /// which is exactly when a developer needs the build to work.
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn the_forced_update_badge_is_off_in_a_committed_tree() {
        assert!(
            !FORCE_UPDATE_BADGE,
            "FORCE_UPDATE_BADGE is a local development switch — flip it back before committing"
        );
    }

    /// The opt-out is a documented promise in the README and the privacy
    /// section: renaming the variable silently breaks it for everyone who set
    /// it, and nothing about the app would look different.
    #[test]
    fn the_opt_out_variable_keeps_the_name_that_was_published() {
        assert_eq!(OPT_OUT_VAR, "SCHEMAIC_NO_UPDATE_CHECK");
    }

    /// Two requests per round against GitHub's 60/hour anonymous limit. The
    /// interval could be far shorter without trouble; it must not be so short
    /// that a long session starts approaching it, and it must not be zero,
    /// which would turn the re-arm into a spin.
    #[test]
    fn the_recheck_interval_stays_clear_of_the_anonymous_rate_limit() {
        let per_hour = 2.0 * 3600.0 / RECHECK_INTERVAL.as_secs_f64();
        assert!(RECHECK_INTERVAL.as_secs() > 0);
        assert!(per_hour < 6.0, "{per_hour} requests/hour");
    }

    /// The feed's identity. A wrong owner or repo here checks someone else's
    /// releases, which is the one failure mode of this module that could hand
    /// a user a binary that is not ours.
    #[test]
    fn the_release_feed_points_at_this_repository() {
        assert_eq!(RELEASE_REPO, "https://github.com/fadion/schemaic");
    }
}
