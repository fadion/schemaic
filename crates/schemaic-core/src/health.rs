//! Connection health-poll policy — pure, no timer, no DB, no UI.
//!
//! The app health-checks the active connection on a repeating timer so
//! [`ConnStatus`] is trustworthy rather than a souvenir of the last time the
//! user switched connections. *When* to actually ping, and *how long* to wait
//! before the next tick, is a policy decision with several competing pulls — a
//! dead host shouldn't be hammered, a background window shouldn't be opening
//! connections at all, an SSH-tunnelled link is more expensive to probe than a
//! local socket — so it lives here, decided by [`tick`] over a [`TickCtx`]
//! snapshot and covered by unit tests. What each piece of news — a check, a
//! schema load, an edit of the settings — does to the verdict is [`fold`].
//!
//! The app owns the parts that can't be pure: reading the snapshot, calling
//! `Db::ping`, and re-arming the timer with [`Tick::next`].

use std::time::Duration;

use crate::connection::ConnStatus;

/// Timer knobs. [`Default`] is the shipped policy; no user-facing setting yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthCfg {
    /// Gap between checks of a healthy, direct connection.
    pub base: Duration,
    /// Ceiling the failure backoff climbs to (a host that's been down for hours
    /// is still re-checked this often, so recovery is noticed on its own).
    pub max_backoff: Duration,
    /// Multiplier applied to `base` for an SSH-tunnelled connection. Each ping
    /// is a fresh channel through the tunnel plus a fresh DB handshake, so it's
    /// worth noticeably more than a loopback `SELECT 1`.
    pub tunnel_factor: u32,
}

impl Default for HealthCfg {
    fn default() -> Self {
        Self {
            base: Duration::from_secs(10),
            max_backoff: Duration::from_secs(120),
            tunnel_factor: 3,
        }
    }
}

/// Everything about the moment a tick fires that changes what to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickCtx {
    /// Last known status of the active connection.
    pub status: ConnStatus,
    /// Consecutive failed pings so far (see [`record`]).
    pub failures: u32,
    /// A query is already in flight against this connection — it's a live probe
    /// of reachability on its own, so a ping alongside it is pure duplication.
    pub busy: bool,
    /// The window has OS focus. An unfocused app isn't about to be used, so the
    /// status doesn't need to be fresh; the app re-checks on regaining focus.
    pub focused: bool,
    /// The active connection reaches its server through an SSH tunnel.
    pub tunnelled: bool,
    /// …and that tunnel isn't established yet. Nothing to ping *through* — the
    /// existing status stands until the tunnel comes up.
    pub tunnel_pending: bool,
    /// The active connection's last schema load **failed on reaching the
    /// server** and cleared its tree — a connect failure, not a refusal of the
    /// app's own (a keychain read the user declined, init SQL the guard
    /// refused), which nothing changes on its own and a retry would only ask
    /// about again.
    pub tree_failed: bool,
    /// Consecutive automatic reloads ([`Tick::reload`]) that failed in turn —
    /// their own backoff, apart from [`Self::failures`], which counts checks.
    pub reload_failures: u32,
}

/// Why a tick decided not to ping. Purely descriptive (logging / tests) — the
/// timer treats every skip the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// SSH connection whose tunnel hasn't come up yet.
    TunnelPending,
    /// Window doesn't have focus.
    Unfocused,
    /// A query is already running against this connection.
    Busy,
}

/// A tick's outcome: ping or not, and when to come back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tick {
    /// `None` = ping now.
    pub skip: Option<SkipReason>,
    /// Load the schema again instead — the one way a tunnelled connection whose
    /// load failed before its tunnel opened can recover. Its tunnel is opened by
    /// the load and by nothing else, so with none to ping through no check can
    /// ever pass and [`reloads_failed_tree`] never fires.
    pub reload: bool,
    /// Delay before the next tick. Always set — the timer must re-arm on every
    /// path, including skips, or one quiet moment ends polling for the session.
    pub next: Duration,
}

impl Tick {
    /// Should this tick actually ping?
    pub fn ping(self) -> bool {
        self.skip.is_none()
    }
}

/// Decide what this tick does.
///
/// Note the delay is computed from the failure count *as of now* — the ping this
/// tick may fire hasn't answered yet — so a newly-dead server gets one extra
/// check at the base interval before the backoff engages. That's deliberate: it
/// keeps the timer re-arm on the tick itself rather than on the ping callback,
/// which can't be relied on to run (a check with no active connection, or a
/// tunnel that dropped mid-flight, returns without reporting). A timer that
/// re-arms only from a callback stops forever the first time one doesn't come.
pub fn tick(cfg: HealthCfg, ctx: TickCtx) -> Tick {
    let base = interval(cfg, ctx.tunnelled);
    let skip = |why| Tick {
        skip: Some(why),
        reload: false,
        next: base,
    };
    if ctx.tunnel_pending {
        // **Unless the load that opens the tunnel is what failed.** Nothing
        // else opens one, so waiting for it to come up waits for ever over an
        // empty tree. Reload instead, behind the gates a ping has, backed off
        // at the tunnelled interval on the reloads that failed — each is an SSH
        // handshake against a host that may be down for hours.
        if ctx.tree_failed && ctx.focused && !ctx.busy {
            return Tick {
                skip: Some(SkipReason::TunnelPending),
                reload: true,
                next: backoff(base, ctx.reload_failures, cfg.max_backoff),
            };
        }
        // Re-arm on the *base* interval, not the tunnelled one: this costs
        // nothing (no network at all) and is how the app notices the tunnel came
        // up, so it shouldn't be the slowest path.
        return Tick {
            skip: Some(SkipReason::TunnelPending),
            reload: false,
            next: cfg.base,
        };
    }
    if !ctx.focused {
        return skip(SkipReason::Unfocused);
    }
    if ctx.busy {
        return skip(SkipReason::Busy);
    }
    Tick {
        skip: None,
        reload: false,
        next: backoff(base, ctx.failures, cfg.max_backoff),
    }
}

/// The healthy-connection interval, tunnel surcharge included.
pub fn interval(cfg: HealthCfg, tunnelled: bool) -> Duration {
    if tunnelled {
        cfg.base.saturating_mul(cfg.tunnel_factor.max(1))
    } else {
        cfg.base
    }
}

/// Exponential backoff: `base * 2^failures`, capped at `max`.
///
/// The cap can never pull the delay *below* `base` — a `max` misconfigured under
/// the interval would otherwise make a failing connection poll faster than a
/// healthy one.
pub fn backoff(base: Duration, failures: u32, max: Duration) -> Duration {
    if failures == 0 {
        return base;
    }
    let factor = 1u32.checked_shl(failures.min(31)).unwrap_or(u32::MAX);
    base.saturating_mul(factor).min(max.max(base))
}

/// Fold a ping result into the consecutive-failure count: any success clears it.
pub fn record(failures: u32, ok: bool) -> u32 {
    if ok { 0 } else { failures.saturating_add(1) }
}

/// What the app knows about the active connection's reachability: the verdict
/// the header shows ("Disconnected · Retry" when it is down) and the
/// consecutive-failure count [`tick`] backs off on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Health {
    /// The verdict.
    pub status: ConnStatus,
    /// Consecutive failed checks behind it (see [`record`]).
    pub failures: u32,
}

/// Something that has just told the app about the active connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// A health check landed, and the app's landing guard let it write: `true`
    /// if the server answered.
    Checked(bool),
    /// The schema load listed the connection's databases: a sign-in with the
    /// connection's settings as they now stand, and a query after it, have just
    /// succeeded.
    Listed,
    /// The connection's settings were saved.
    Edited,
}

impl Evidence {
    /// Is every health check already in flight stale once this has happened?
    ///
    /// Only an **edit**. A check out at the time pings with the settings as they
    /// were — the wrong password the user has just corrected — and its failure,
    /// landing after the save, would put the header straight back.
    ///
    /// **Not a load**, which is the other half and is just as deliberate. A
    /// check started after the load did is newer evidence than the load, so a
    /// failure it brings back must still land: retiring it would let a
    /// successful listing mask the server going away a moment later.
    pub fn retires_checks_in_flight(self) -> bool {
        matches!(self, Evidence::Edited)
    }
}

/// Fold one piece of [`Evidence`] into what the app knows.
///
/// The case this exists for: a connection showing "Disconnected" because its
/// saved password was wrong, the user fixes it and saves, and the schema tree
/// fills in while the header goes on saying "Disconnected · Retry" — for as
/// long as the poll's backoff, grown on every failed check, took to come round.
/// Two facts were missing, and both are folded here:
///
/// - **A listing is a check that passed.** It signed in and ran a query against
///   this connection with its current settings, which is everything `ping`
///   proves, so it clears the verdict and the backoff behind it.
/// - **An edit forgets the old settings' backoff**, which says nothing about the
///   new ones, and the app re-checks at once rather than at the next tick. The
///   verdict itself stands until that check (or the load's listing) answers:
///   clearing it made a colour-only save of a connection that really is down
///   hide "Disconnected" for as long as the check took, then show it again. The
///   checks already in flight are what the edit makes stale, and the app retires
///   those ([`Evidence::retires_checks_in_flight`]).
///
/// A load that *failed* is deliberately not evidence: a listing can fail on a
/// privilege the ping does not need, and the tree already says so in its own
/// words.
pub fn fold(h: Health, e: Evidence) -> Health {
    match e {
        Evidence::Checked(ok) => Health {
            status: if ok {
                ConnStatus::Connected
            } else {
                ConnStatus::Disconnected
            },
            failures: record(h.failures, ok),
        },
        Evidence::Listed => fold(h, Evidence::Checked(true)),
        Evidence::Edited => Health {
            status: h.status,
            failures: 0,
        },
    }
}

/// Should the evidence that has just landed reload a schema tree whose last load
/// **failed**? `before` is the verdict the evidence is folded over.
///
/// The case it exists for: a load that fails at launch — the server not up yet,
/// a tunnel still coming up, a keychain dialog nobody had answered — clears the
/// tree, and nothing loaded it again. The health poll recovered on its own and
/// the header cleared "Disconnected", over a tree that stayed empty until the
/// user found Refresh.
///
/// **Only a check, and only one that changes the verdict to Connected.** A
/// listing *is* the load, and an edit starts its own. A check that finds the
/// connection already Connected says nothing new — and reloading on it would
/// loop against a load that fails on something the ping does not need (a
/// privilege, a catalogue the account cannot read): fail, check, reload, fail,
/// every poll. A real recovery is a change of verdict, so that is the trigger.
pub fn reloads_failed_tree(before: ConnStatus, e: Evidence, tree_failed: bool) -> bool {
    tree_failed && e == Evidence::Checked(true) && before != ConnStatus::Connected
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The recovery the poll found reloads the tree it left empty, from either
    /// verdict short of Connected.
    #[test]
    fn a_check_that_recovers_the_connection_reloads_a_failed_tree() {
        for before in [ConnStatus::Disconnected, ConnStatus::Unknown] {
            assert!(
                reloads_failed_tree(before, Evidence::Checked(true), true),
                "{before:?} → Connected left the failed tree empty"
            );
        }
    }

    /// No failed load, nothing to reload: the tree a recovery finds is fine.
    #[test]
    fn a_recovery_leaves_a_tree_that_loaded_alone() {
        assert!(!reloads_failed_tree(
            ConnStatus::Disconnected,
            Evidence::Checked(true),
            false
        ));
    }

    /// A check that finds the connection already Connected is not a recovery —
    /// reloading on it would retry a load that fails on a privilege the ping
    /// does not need, at every poll.
    #[test]
    fn a_check_that_changes_nothing_does_not_retry_a_failed_load() {
        assert!(!reloads_failed_tree(
            ConnStatus::Connected,
            Evidence::Checked(true),
            true
        ));
    }

    /// A failed check, a listing and an edit are never the trigger: the first
    /// is no recovery, the second is the load itself, the third starts its own.
    #[test]
    fn only_a_passing_check_reloads() {
        for e in [Evidence::Checked(false), Evidence::Listed, Evidence::Edited] {
            for before in [
                ConnStatus::Disconnected,
                ConnStatus::Unknown,
                ConnStatus::Connected,
            ] {
                assert!(
                    !reloads_failed_tree(before, e, true),
                    "{e:?} from {before:?}"
                );
            }
        }
    }

    /// A ctx that pings: focused, idle, direct, healthy.
    fn ready() -> TickCtx {
        TickCtx {
            status: ConnStatus::Connected,
            focused: true,
            ..TickCtx::default()
        }
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn healthy_idle_connection_pings_on_the_base_interval() {
        let t = tick(HealthCfg::default(), ready());
        assert!(t.ping());
        assert_eq!(t.skip, None);
        assert_eq!(t.next, secs(10));
    }

    #[test]
    fn unknown_status_still_pings() {
        // Startup: nothing checked yet is exactly when a check is most wanted.
        let ctx = TickCtx {
            status: ConnStatus::Unknown,
            ..ready()
        };
        assert!(tick(HealthCfg::default(), ctx).ping());
    }

    #[test]
    fn unfocused_window_skips_the_ping() {
        let ctx = TickCtx {
            focused: false,
            ..ready()
        };
        let t = tick(HealthCfg::default(), ctx);
        assert!(!t.ping());
        assert_eq!(t.skip, Some(SkipReason::Unfocused));
        assert_eq!(t.next, secs(10));
    }

    #[test]
    fn a_running_query_skips_the_ping() {
        let ctx = TickCtx {
            busy: true,
            ..ready()
        };
        let t = tick(HealthCfg::default(), ctx);
        assert_eq!(t.skip, Some(SkipReason::Busy));
    }

    #[test]
    fn a_pending_tunnel_skips_the_ping() {
        let ctx = TickCtx {
            tunnelled: true,
            tunnel_pending: true,
            ..ready()
        };
        let t = tick(HealthCfg::default(), ctx);
        assert_eq!(t.skip, Some(SkipReason::TunnelPending));
        // Base interval, not the tunnelled one — this tick is free.
        assert_eq!(t.next, secs(10));
    }

    #[test]
    fn a_pending_tunnel_outranks_the_other_skips() {
        // All three skip conditions at once: the cheapest, most specific reason
        // is the one reported, so the log/tests don't depend on field order.
        let ctx = TickCtx {
            focused: false,
            busy: true,
            tunnelled: true,
            tunnel_pending: true,
            ..ready()
        };
        assert_eq!(
            tick(HealthCfg::default(), ctx).skip,
            Some(SkipReason::TunnelPending)
        );
    }

    #[test]
    fn unfocused_outranks_busy() {
        let ctx = TickCtx {
            focused: false,
            busy: true,
            ..ready()
        };
        assert_eq!(
            tick(HealthCfg::default(), ctx).skip,
            Some(SkipReason::Unfocused)
        );
    }

    #[test]
    fn a_tunnelled_connection_polls_less_often() {
        let ctx = TickCtx {
            tunnelled: true,
            ..ready()
        };
        let t = tick(HealthCfg::default(), ctx);
        assert!(t.ping());
        assert_eq!(t.next, secs(30));
    }

    #[test]
    fn a_tunnelled_skip_also_uses_the_longer_interval() {
        let ctx = TickCtx {
            tunnelled: true,
            busy: true,
            ..ready()
        };
        assert_eq!(tick(HealthCfg::default(), ctx).next, secs(30));
    }

    /// A tunnelled connection whose load failed before its tunnel opened.
    fn tunnel_never_opened() -> TickCtx {
        TickCtx {
            status: ConnStatus::Unknown,
            tunnelled: true,
            tunnel_pending: true,
            tree_failed: true,
            reload_failures: 1,
            ..ready()
        }
    }

    /// **A load that failed before its tunnel opened is retried by the poll.**
    /// The load is the only thing that opens a tunnel, and a check with no
    /// tunnel to ping through returns without pinging — so before this the
    /// tree stayed empty for good, and the reload a recovering check triggers
    /// (`reloads_failed_tree`) could never fire for it.
    #[test]
    fn a_tunnel_that_never_opened_is_reopened_by_reloading() {
        let t = tick(HealthCfg::default(), tunnel_never_opened());
        assert!(
            t.reload,
            "the poll must retry the load that opens the tunnel"
        );
        assert!(!t.ping(), "there is still nothing to ping through");
    }

    /// The retries back off on their own count, at the tunnelled interval: each
    /// is an SSH handshake against a host that may be down for hours.
    #[test]
    fn reopening_a_tunnel_backs_off_on_the_reloads_that_failed() {
        let next = |reload_failures| {
            tick(
                HealthCfg::default(),
                TickCtx {
                    reload_failures,
                    ..tunnel_never_opened()
                },
            )
            .next
        };
        assert_eq!(next(0), secs(30));
        assert_eq!(next(1), secs(60));
        assert_eq!(next(2), secs(120));
        assert_eq!(next(9), secs(120));
    }

    /// Not in the background and not over a running query — the same two
    /// gates a ping has — and not where nothing failed.
    #[test]
    fn a_tunnel_is_only_reopened_for_a_failed_tree_in_a_focused_idle_window() {
        let cases = [
            TickCtx {
                focused: false,
                ..tunnel_never_opened()
            },
            TickCtx {
                busy: true,
                ..tunnel_never_opened()
            },
            TickCtx {
                tree_failed: false,
                ..tunnel_never_opened()
            },
        ];
        for ctx in cases {
            let t = tick(HealthCfg::default(), ctx);
            assert!(!t.reload, "{ctx:?}");
            assert_eq!(t.skip, Some(SkipReason::TunnelPending), "{ctx:?}");
            assert_eq!(t.next, secs(10), "{ctx:?}");
        }
    }

    /// With the tunnel up the ping can reach the server, and a passing one
    /// reloads through `reloads_failed_tree` — the tick has no reload to add.
    #[test]
    fn a_failed_tree_over_an_open_tunnel_is_left_to_the_check() {
        let t = tick(
            HealthCfg::default(),
            TickCtx {
                tunnel_pending: false,
                ..tunnel_never_opened()
            },
        );
        assert!(t.ping());
        assert!(!t.reload);
    }

    #[test]
    fn failures_back_the_interval_off_exponentially() {
        let cfg = HealthCfg::default();
        let next = |failures| {
            tick(
                cfg,
                TickCtx {
                    status: ConnStatus::Disconnected,
                    failures,
                    ..ready()
                },
            )
            .next
        };
        assert_eq!(next(0), secs(10));
        assert_eq!(next(1), secs(20));
        assert_eq!(next(2), secs(40));
        assert_eq!(next(3), secs(80));
    }

    #[test]
    fn backoff_stops_at_the_ceiling() {
        let cfg = HealthCfg::default();
        for failures in [4, 5, 20, u32::MAX] {
            let t = tick(
                cfg,
                TickCtx {
                    status: ConnStatus::Disconnected,
                    failures,
                    ..ready()
                },
            );
            // Still pinging — a host that's been down for hours must still be
            // able to come back on its own.
            assert!(t.ping(), "failures={failures}");
            assert_eq!(t.next, secs(120), "failures={failures}");
        }
    }

    #[test]
    fn a_down_connection_with_no_failures_recorded_uses_the_base_interval() {
        // `status` alone never drives the delay — the failure count does, so a
        // status set by some other path can't skew the timer.
        let ctx = TickCtx {
            status: ConnStatus::Disconnected,
            failures: 0,
            ..ready()
        };
        assert_eq!(tick(HealthCfg::default(), ctx).next, secs(10));
    }

    #[test]
    fn backoff_of_zero_failures_is_the_base() {
        assert_eq!(backoff(secs(10), 0, secs(120)), secs(10));
    }

    #[test]
    fn backoff_never_drops_below_the_base_interval() {
        // A ceiling under the interval must not make a failing connection poll
        // *faster* than a healthy one.
        assert_eq!(backoff(secs(30), 3, secs(5)), secs(30));
    }

    #[test]
    fn backoff_saturates_instead_of_overflowing() {
        assert_eq!(backoff(secs(10), u32::MAX, secs(120)), secs(120));
        assert_eq!(backoff(Duration::MAX, 40, Duration::MAX), Duration::MAX);
    }

    #[test]
    fn interval_ignores_a_zero_tunnel_factor() {
        let cfg = HealthCfg {
            tunnel_factor: 0,
            ..HealthCfg::default()
        };
        assert_eq!(interval(cfg, true), secs(10));
        assert_eq!(interval(cfg, false), secs(10));
    }

    #[test]
    fn record_counts_consecutive_failures_and_any_success_clears() {
        assert_eq!(record(0, false), 1);
        assert_eq!(record(1, false), 2);
        assert_eq!(record(7, true), 0);
        assert_eq!(record(0, true), 0);
    }

    #[test]
    fn record_saturates() {
        assert_eq!(record(u32::MAX, false), u32::MAX);
    }

    fn down(failures: u32) -> Health {
        Health {
            status: ConnStatus::Disconnected,
            failures,
        }
    }

    #[test]
    fn a_check_folds_its_verdict_and_counts_toward_the_backoff() {
        assert_eq!(
            fold(down(2), Evidence::Checked(false)),
            Health {
                status: ConnStatus::Disconnected,
                failures: 3
            }
        );
        assert_eq!(
            fold(down(2), Evidence::Checked(true)),
            Health {
                status: ConnStatus::Connected,
                failures: 0
            }
        );
    }

    /// The reported bug: the password is fixed, the tree fills in, and the
    /// header still says "Disconnected · Retry" until a backed-off poll comes
    /// round. Listing the databases *is* a successful check.
    #[test]
    fn a_listed_load_clears_a_failed_verdict_and_its_backoff() {
        assert_eq!(
            fold(down(4), Evidence::Listed),
            Health {
                status: ConnStatus::Connected,
                failures: 0
            }
        );
    }

    #[test]
    fn a_listed_load_of_an_unchecked_connection_says_connected() {
        assert_eq!(
            fold(Health::default(), Evidence::Listed).status,
            ConnStatus::Connected
        );
    }

    /// An edit forgets the old settings' backoff but leaves the verdict to the
    /// check the save runs at once. Clearing it instead made a colour-only save
    /// of a connection that really is down hide "Disconnected" for as long as
    /// that check took, then show it again.
    #[test]
    fn an_edit_keeps_a_failed_verdict_and_forgets_its_backoff() {
        assert_eq!(fold(down(4), Evidence::Edited), down(0));
    }

    #[test]
    fn an_edit_keeps_a_passing_verdict_too() {
        let up = Health {
            status: ConnStatus::Connected,
            failures: 0,
        };
        assert_eq!(fold(up, Evidence::Edited), up);
    }

    /// A check out at the time of a save pinged with the old settings; its
    /// failure must not land on top of the new ones.
    #[test]
    fn an_edit_retires_every_check_in_flight() {
        assert!(Evidence::Edited.retires_checks_in_flight());
    }

    /// The reverse must not happen: a check started after a load is newer
    /// evidence, and a failure it brings back must still be able to land.
    #[test]
    fn a_load_or_a_check_retires_nothing() {
        assert!(!Evidence::Listed.retires_checks_in_flight());
        assert!(!Evidence::Checked(true).retires_checks_in_flight());
        assert!(!Evidence::Checked(false).retires_checks_in_flight());
    }

    /// A failure after a listing is the newer news, and wins.
    #[test]
    fn a_failed_check_after_a_listing_still_says_disconnected() {
        let after = fold(fold(down(1), Evidence::Listed), Evidence::Checked(false));
        assert_eq!(
            after,
            Health {
                status: ConnStatus::Disconnected,
                failures: 1
            }
        );
    }
}
