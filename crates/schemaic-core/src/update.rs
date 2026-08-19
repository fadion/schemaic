//! Auto-update state model for the status-bar update indicator.
//!
//! Pure: the app boundary owns the Velopack `UpdateManager` (network + file I/O,
//! and a *synchronous* API, so it runs on a worker thread), and feeds what it
//! learns through [`CheckGate`] and [`UpdateState`]. Nothing here talks to
//! Velopack, so the decisions worth getting right — whether to check at all, what
//! the header chip shows, and which ticks are allowed to move the state back —
//! are testable without an installed app or a release feed.

/// Whether a background update check may run, and if not, why.
///
/// The app asks this at the start of every check round. `Allowed` is the only
/// variant that leads to a check; the other two are ordinary, expected outcomes
/// rather than errors, so neither is surfaced to the user — and per
/// [`should_recheck`] either of them also ends the polling, since neither can
/// turn into `Allowed` while the process is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckGate {
    /// Check away.
    Allowed,
    /// `SCHEMAIC_NO_UPDATE_CHECK` is set — the user opted out.
    OptedOut,
    /// Not a Velopack-installed build: a portable-zip extraction, a `cargo run`
    /// dev build, or a distro package. There is no update path to poll, and
    /// Velopack's `UpdateManager::new` fails on exactly this, so we don't build one.
    NotInstalled,
}

/// Decide whether a check round runs.
///
/// `opt_out` is the raw `SCHEMAIC_NO_UPDATE_CHECK` value (`None` when unset);
/// `installed` is Velopack's own verdict on whether this is a managed install.
/// Opt-out wins over everything, so setting the variable silences the feature
/// completely rather than merely on installs.
pub fn check_gate(opt_out: Option<&str>, installed: bool) -> CheckGate {
    if opt_out_requested(opt_out) {
        CheckGate::OptedOut
    } else if installed {
        CheckGate::Allowed
    } else {
        CheckGate::NotInstalled
    }
}

/// Whether an env-var value means "don't check".
///
/// Presence alone is *not* enough: `SCHEMAIC_NO_UPDATE_CHECK=0` in a shell profile
/// should mean what it says. Accepts `1` / `true` / `yes` / `on` case-insensitively
/// with surrounding whitespace trimmed; everything else — including the empty
/// string a bare `set VAR=` leaves behind — is not an opt-out.
pub fn opt_out_requested(raw: Option<&str>) -> bool {
    matches!(
        raw.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

/// Where the background updater has got to.
///
/// The app drives this straight through — [`Idle`](Self::Idle) →
/// [`Checking`](Self::Checking) → [`Downloading`](Self::Downloading) →
/// [`Ready`](Self::Ready) — because Velopack's delta packages are small enough
/// that asking permission to download would cost the user more attention than the
/// download costs bandwidth. The one point the user is asked is the restart.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdateState {
    /// No check has run, or one finished with nothing to do.
    #[default]
    Idle,
    /// Asking the release feed. Deliberately invisible in the UI.
    Checking,
    /// Fetching the update. `pct` is always 0..=100.
    Downloading { version: String, pct: u8 },
    /// Staged on disk and waiting for a restart to apply.
    Ready { version: String },
    /// The check or download failed. Deliberately invisible in the UI — a
    /// background poll that couldn't reach GitHub is not the user's problem, and
    /// the next round retries (see [`should_recheck`], for which a failure is the
    /// one negative outcome that re-arms). Kept as a state so it can be logged
    /// and tested.
    Failed { message: String },
}

impl UpdateState {
    /// The chip's text, or `None` when this state shows nothing at all.
    ///
    /// Only the two states the user can act on or is waiting for produce text.
    /// A silent background check that finds nothing — or fails — leaves the
    /// header exactly as it was.
    pub fn label(&self) -> Option<String> {
        match self {
            Self::Downloading { pct, .. } => Some(format!("Updating… {pct}%")),
            Self::Ready { .. } => Some("Restart to update".to_string()),
            Self::Idle | Self::Checking | Self::Failed { .. } => None,
        }
    }

    /// Whether clicking the chip should do something — true only once
    /// an update is staged and the click means "restart and apply".
    pub fn is_actionable(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    /// The version this state refers to, when it refers to one.
    pub fn version(&self) -> Option<&str> {
        match self {
            Self::Downloading { version, .. } | Self::Ready { version } => Some(version),
            _ => None,
        }
    }

    /// Fold in a progress tick from Velopack's `Sender<i16>`.
    ///
    /// Ticks are ignored unless a download is actually in flight. That matters
    /// because the channel outlives the download: a tick that arrives after
    /// [`Ready`](Self::Ready) must not drag the chip back to "Updating… 100%"
    /// and lose the restart affordance the user was about to click.
    pub fn with_progress(&self, raw_pct: i16) -> Self {
        match self {
            Self::Downloading { version, .. } => Self::Downloading {
                version: version.clone(),
                pct: clamp_pct(raw_pct),
            },
            other => other.clone(),
        }
    }
}

/// Whether another background check is worth arming after one has settled.
///
/// The check used to run once per launch, which quietly meant a session left open
/// never learned about anything: a release published two hours into a working day
/// went unnoticed until the app happened to be restarted, which for a database
/// client can be days. So checks repeat — but only while repeating can still
/// change the answer.
///
/// Two outcomes end the polling for good. A **staged** update is the obvious one:
/// there is nothing better to find until the user restarts, and re-checking would
/// only risk replacing the offer they are looking at. A **refused gate** is the
/// less obvious one — neither `OptedOut` nor `NotInstalled` can become `Allowed`
/// inside a running process, so re-asking is pure waste, and on a dev build it
/// would be a timer that can never do anything.
///
/// A *failed* check does re-arm. It is the one negative outcome that is expected
/// to be temporary — the network comes back — and the failure is invisible
/// anyway, so retrying costs the user nothing.
pub fn should_recheck(gate: CheckGate, settled: &UpdateState) -> bool {
    gate == CheckGate::Allowed && matches!(settled, UpdateState::Idle | UpdateState::Failed { .. })
}

/// Clamp Velopack's `i16` progress into a percentage.
///
/// The channel carries a plain `i16`, so nothing at the type level stops a
/// sentinel like `-1` or an overshooting `101` from reaching us.
pub fn clamp_pct(raw: i16) -> u8 {
    raw.clamp(0, 100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opt_out_accepts_the_truthy_spellings() {
        for v in ["1", "true", "yes", "on", "TRUE", "Yes", "ON", " 1 "] {
            assert!(opt_out_requested(Some(v)), "{v:?} should opt out");
        }
    }

    #[test]
    fn opt_out_rejects_unset_empty_and_falsey() {
        for v in [
            None,
            Some(""),
            Some(" "),
            Some("0"),
            Some("false"),
            Some("no"),
        ] {
            assert!(!opt_out_requested(v), "{v:?} should not opt out");
        }
    }

    #[test]
    fn gate_allows_an_installed_build_with_no_opt_out() {
        assert_eq!(check_gate(None, true), CheckGate::Allowed);
    }

    #[test]
    fn gate_blocks_an_uninstalled_build() {
        assert_eq!(check_gate(None, false), CheckGate::NotInstalled);
    }

    #[test]
    fn gate_opt_out_wins_over_installed_state() {
        assert_eq!(check_gate(Some("1"), true), CheckGate::OptedOut);
        assert_eq!(check_gate(Some("1"), false), CheckGate::OptedOut);
    }

    #[test]
    fn gate_ignores_a_falsey_opt_out_value() {
        assert_eq!(check_gate(Some("0"), true), CheckGate::Allowed);
    }

    #[test]
    fn idle_and_checking_show_nothing() {
        assert_eq!(UpdateState::default(), UpdateState::Idle);
        assert_eq!(UpdateState::Idle.label(), None);
        assert_eq!(UpdateState::Checking.label(), None);
    }

    #[test]
    fn a_failed_check_stays_silent() {
        let s = UpdateState::Failed {
            message: "dns error".into(),
        };
        assert_eq!(s.label(), None);
        assert!(!s.is_actionable());
    }

    #[test]
    fn downloading_shows_percent_and_is_not_clickable() {
        let s = UpdateState::Downloading {
            version: "0.16.0".into(),
            pct: 40,
        };
        assert_eq!(s.label().as_deref(), Some("Updating… 40%"));
        assert!(!s.is_actionable());
        assert_eq!(s.version(), Some("0.16.0"));
    }

    #[test]
    fn ready_offers_the_restart() {
        let s = UpdateState::Ready {
            version: "0.16.0".into(),
        };
        assert_eq!(s.label().as_deref(), Some("Restart to update"));
        assert!(s.is_actionable());
        assert_eq!(s.version(), Some("0.16.0"));
    }

    #[test]
    fn version_is_absent_where_there_is_none() {
        assert_eq!(UpdateState::Idle.version(), None);
        assert_eq!(UpdateState::Checking.version(), None);
        assert_eq!(
            UpdateState::Failed {
                message: "x".into()
            }
            .version(),
            None
        );
    }

    #[test]
    fn recheck_continues_while_nothing_has_been_found() {
        assert!(should_recheck(CheckGate::Allowed, &UpdateState::Idle));
    }

    /// The reason polling exists at all: a session left open for hours found
    /// nothing on launch, and the next release must still reach it.
    #[test]
    fn recheck_retries_after_a_failure() {
        let failed = UpdateState::Failed {
            message: "dns error".into(),
        };
        assert!(should_recheck(CheckGate::Allowed, &failed));
    }

    /// Once an update is staged there is nothing better to find until the user
    /// restarts, and a further round could only disturb the offer on screen.
    #[test]
    fn recheck_stops_once_something_is_staged() {
        let ready = UpdateState::Ready {
            version: "0.16.1".into(),
        };
        assert!(!should_recheck(CheckGate::Allowed, &ready));
    }

    #[test]
    fn recheck_stops_while_a_download_is_in_flight() {
        let busy = UpdateState::Downloading {
            version: "0.16.1".into(),
            pct: 40,
        };
        assert!(!should_recheck(CheckGate::Allowed, &busy));
    }

    /// Neither refusal can change inside a running process, so a timer that
    /// re-asks is one that can never do anything — on a dev build, forever.
    #[test]
    fn a_refused_gate_never_re_arms() {
        for gate in [CheckGate::OptedOut, CheckGate::NotInstalled] {
            assert!(!should_recheck(gate, &UpdateState::Idle), "{gate:?}");
            assert!(
                !should_recheck(
                    gate,
                    &UpdateState::Failed {
                        message: "x".into()
                    }
                ),
                "{gate:?}"
            );
        }
    }

    #[test]
    fn clamp_pct_bounds_both_ends() {
        assert_eq!(clamp_pct(-1), 0);
        assert_eq!(clamp_pct(i16::MIN), 0);
        assert_eq!(clamp_pct(0), 0);
        assert_eq!(clamp_pct(55), 55);
        assert_eq!(clamp_pct(100), 100);
        assert_eq!(clamp_pct(101), 100);
        assert_eq!(clamp_pct(i16::MAX), 100);
    }

    #[test]
    fn progress_updates_an_in_flight_download() {
        let s = UpdateState::Downloading {
            version: "0.16.0".into(),
            pct: 0,
        };
        assert_eq!(
            s.with_progress(73),
            UpdateState::Downloading {
                version: "0.16.0".into(),
                pct: 73,
            }
        );
    }

    #[test]
    fn progress_clamps_out_of_range_ticks() {
        let s = UpdateState::Downloading {
            version: "0.16.0".into(),
            pct: 10,
        };
        assert_eq!(s.with_progress(-1).label().as_deref(), Some("Updating… 0%"));
        assert_eq!(
            s.with_progress(999).label().as_deref(),
            Some("Updating… 100%")
        );
    }

    /// The regression this method exists for: Velopack's progress channel can
    /// still deliver a queued tick after `download_updates` returns, and folding
    /// it in blindly would replace the "Restart to update" affordance with
    /// "Updating… 100%" — a dead chip the user can't click.
    #[test]
    fn a_late_tick_cannot_rewind_a_staged_update() {
        let ready = UpdateState::Ready {
            version: "0.16.0".into(),
        };
        assert_eq!(ready.with_progress(100), ready);
        assert!(ready.with_progress(100).is_actionable());
    }

    #[test]
    fn progress_is_inert_in_the_states_with_no_download() {
        assert_eq!(UpdateState::Idle.with_progress(50), UpdateState::Idle);
        assert_eq!(
            UpdateState::Checking.with_progress(50),
            UpdateState::Checking
        );
        let failed = UpdateState::Failed {
            message: "offline".into(),
        };
        assert_eq!(failed.with_progress(50), failed);
    }
}
