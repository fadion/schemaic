//! Connection health-poll policy — pure, no timer, no DB, no UI.
//!
//! The app health-checks the active connection on a repeating timer so
//! [`ConnStatus`] is trustworthy rather than a souvenir of the last time the
//! user switched connections. *When* to actually ping, and *how long* to wait
//! before the next tick, is a policy decision with several competing pulls — a
//! dead host shouldn't be hammered, a background window shouldn't be opening
//! connections at all, an SSH-tunnelled link is more expensive to probe than a
//! local socket — so it lives here, decided by [`tick`] over a [`TickCtx`]
//! snapshot and covered by unit tests.
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
        next: base,
    };
    if ctx.tunnel_pending {
        // Re-arm on the *base* interval, not the tunnelled one: this costs
        // nothing (no network at all) and is how the app notices the tunnel came
        // up, so it shouldn't be the slowest path.
        return Tick {
            skip: Some(SkipReason::TunnelPending),
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
