//! Is the process that left this behind still running?
//!
//! Two unrelated pieces of the AI machinery ask the same question and used to
//! answer it two different ways, both wrong in one direction:
//!
//! - [`crate::antigravity`]'s startup sweep withdraws a registration and its
//!   allow-rules unless a *live* instance claims them. It answered with a marker
//!   file holding a pid, plus a special case for its own pid that assumed the
//!   detached sweep thread had finished before any session could install.
//! - [`crate::ai`]'s temp sweep deletes MCP config and endpoint files — which
//!   carry the database password — and answered with a 24-hour mtime. A session
//!   older than a day plus a second window was enough to delete a *live*
//!   instance's endpoint file, while abandoned siblings with the wrong suffix
//!   were never collected at all.
//!
//! One question, one answer. An [`Owner`] is a pid **and** the start time of the
//! process that held it, because pids are reissued: a crashed instance's claim
//! eventually names some unrelated live process, and anything trusting the
//! number alone would decline to clean up for the rest of that pid's life —
//! turning a transient crash into a permanent standing grant, or a permanent
//! plaintext credential on disk.
//!
//! **An unreadable or absent owner is *not* a claim.** Both callers must be able
//! to clean up after a crash they cannot identify; refusing to act on a garbled
//! marker would strand exactly what the sweep exists to remove.

/// The process that owns something outside its own memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    pub(crate) pid: u32,
    /// The pid's start time, as the OS reports it. Without this a reissued pid
    /// makes a dead claim look live forever.
    pub(crate) started: u64,
}

/// This process, if its start time can be read.
///
/// `None` means the claim could not be told from a pid-reuse, and a claim that
/// cannot expire is worse than none — so every caller treats it as "do not
/// claim" rather than falling back to the bare pid.
pub(crate) fn me() -> Option<Owner> {
    let pid = std::process::id();
    Some(Owner {
        pid,
        started: process_start(pid)?,
    })
}

/// A process's start time, if that pid is running at all.
///
/// The impure half of [`is_live`], kept to one line of answer so the decision
/// itself stays testable without a process to look at.
pub(crate) fn process_start(pid: u32) -> Option<u64> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let p = Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[p]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(p).map(|pr| pr.start_time())
}

/// Is `owner` still the process on that pid? `live` is that pid's start time, if
/// it is running at all.
///
/// **Pure, and the whole decision.** Both sweeps call it with the same shape of
/// argument, so neither can grow its own idea of what "still running" means.
pub(crate) fn is_live(owner: Owner, live: Option<u64>) -> bool {
    live == Some(owner.started)
}

/// May a sweep remove state claimed by `marker`?
///
/// `None` — absent, unreadable, garbled — is no claim at all, in the direction
/// that lets a crash be cleaned up.
///
/// **There is deliberately no "our own pid" arm.** One used to sit here, on the
/// reasoning that a sweep runs at startup and so cannot be the instance in the
/// middle of a session. The sweep is a *detached thread* that has to run
/// `detect_bin`, a `sysinfo` refresh and a subprocess before it gets this far,
/// and a user who presses Enter in the AI panel first has already installed a
/// marker naming this pid — which that arm then swept, out from under the live
/// session that had just written it. The start-time comparison answers our own
/// process correctly with no special case: a marker written by *this* process
/// carries *this* start time and is live; one left by a previous process on the
/// same pid carries a different one and is not.
pub(crate) fn may_sweep(marker: Option<Owner>, live: Option<u64>) -> bool {
    match marker {
        None => true,
        Some(o) => !is_live(o, live),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug the start time exists for: the OS hands pids out again, so a
    /// crashed instance's claim eventually names some unrelated live process.
    #[test]
    fn a_reissued_pid_is_not_the_owner() {
        let o = Owner {
            pid: 1234,
            started: 900,
        };
        assert!(is_live(o, Some(900)));
        assert!(!is_live(o, Some(901)), "a reissued pid read as live");
        assert!(!is_live(o, None), "a dead pid read as live");
    }

    #[test]
    fn an_absent_or_unreadable_claim_never_blocks_a_sweep() {
        assert!(may_sweep(None, None));
        assert!(may_sweep(None, Some(900)));
    }

    /// **The arm that used to be a special case.** `may_sweep` once answered
    /// `true` for any marker naming our own pid, on the assumption that the
    /// startup sweep runs before any session installs. It does not: the sweep is
    /// detached and has a `detect_bin`, a `sysinfo` refresh and a subprocess
    /// ahead of it, so a user who asks a question first has already written a
    /// marker naming this pid — with this pid's start time. Composed, the old
    /// arm swept a live session's own claim.
    #[test]
    fn our_own_live_claim_is_not_sweepable_even_though_it_names_our_pid() {
        let me = Owner {
            pid: 4242,
            started: 900,
        };
        assert!(
            !may_sweep(Some(me), Some(900)),
            "swept a claim this very process is holding"
        );
        // A previous process on the same pid is a different start time, and that
        // one *is* a crash's leftovers.
        assert!(may_sweep(
            Some(Owner {
                pid: 4242,
                started: 111
            }),
            Some(900)
        ));
    }
}
