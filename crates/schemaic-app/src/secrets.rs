//! OS-keyring-backed [`SecretStore`] plus the connection load/save/forget
//! wrappers the app uses so `connections.json` never persists a plaintext
//! secret.
//!
//! The pure transforms (hydrate / sanitize / forget) live in
//! [`schemaic_core::secrets`]; this module supplies the real store (the OS
//! keyring) and threads it through [`persist`]'s file I/O. Every keyring
//! operation is best-effort: any backend error maps to a graceful miss/failure
//! so a machine with no working keyring degrades to the legacy plaintext path
//! rather than breaking (see [`schemaic_core::secrets::sanitize_file`]).

use std::sync::{Mutex, OnceLock};

use keyring::Entry;
use schemaic_core::persist::{self, ConnectionsFile};
use schemaic_core::secrets::{self, Hydration, SecretStore, StoreError};

/// Keyring service name under which all of Schemaic's secrets are grouped; the
/// per-secret account string comes from [`schemaic_core::secrets::account`].
const SERVICE: &str = "schemaic";

/// The real secret store: the OS keyring (Windows Credential Manager / Secret
/// Service / Keychain, per the target-gated `keyring` backend).
pub struct KeyringStore;

/// **The classification the whole secret-protection scheme rests on**, as a
/// function of the backend's answer alone.
///
/// `Ok(None)` and `Err` are not interchangeable and collapsing them destroys
/// credentials: `sanitize_file` deletes the stored entry on the first and must
/// not on the second. `NoEntry` is the only error that means *there is no secret
/// here*; everything else — a locked keyring, a denied prompt, a platform
/// failure — means *we could not read it*.
///
/// It is a free function so it can be **tested**, which is the whole reason it
/// was pulled out of the method. Core pins both consequences of this
/// classification, but only ever against a `FakeStore` that reports errors
/// correctly by construction — collapse the match here and every one of those
/// tests still passes, which is exactly the shape CLAUDE.md's TDD rule warns
/// about, with the seam at a trait impl rather than a function call.
fn classify_get(answer: Result<String, keyring::Error>) -> Result<Option<String>, StoreError> {
    match answer {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(StoreError(e.to_string())),
    }
}

/// Whether a delete left the entry **definitely gone**, from the backend's
/// answer alone. [`classify_get`]'s rule in the other direction: a missing entry
/// is the outcome the caller wanted, and anything else is the store failing to
/// answer — which must not read as a success, or a password the user cleared
/// comes back on the next launch.
fn classify_delete(answer: Result<(), keyring::Error>) -> bool {
    matches!(answer, Ok(()) | Err(keyring::Error::NoEntry))
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<String>, StoreError> {
        let entry = Entry::new(SERVICE, account).map_err(|e| StoreError(e.to_string()))?;
        classify_get(entry.get_password())
    }

    fn set(&self, account: &str, secret: &str) -> bool {
        Entry::new(SERVICE, account)
            .and_then(|e| e.set_password(secret))
            .is_ok()
    }

    fn delete(&self, account: &str) -> bool {
        let Ok(e) = Entry::new(SERVICE, account) else {
            return false;
        };
        classify_delete(e.delete_credential())
    }
}

/// What the last [`load_connections`] learned about the keyring.
///
/// Process-level because the save path can't reach the load's return value:
/// [`save_connections`] is called from the read-only toggle, the active-connection
/// switch, `save_conn` and the delete path, none of which carry connection state.
/// Without it a save cannot tell "the user cleared this password" from "we
/// couldn't read it at startup", and the first reading deletes it.
fn last_hydration() -> &'static Mutex<Hydration> {
    static H: OnceLock<Mutex<Hydration>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(Hydration::default()))
}

/// Load saved connections, hydrating each one's secrets from the keyring. If the
/// file still carries legacy plaintext (first launch after upgrading, or after a
/// spell without a keyring), migrate it into the keyring and rewrite the on-disk
/// copy blanked — a one-time self-heal.
pub fn load_connections() -> ConnectionsFile {
    let store = KeyringStore;
    let mut file = persist::load_connections();
    let hydration = secrets::hydrate_file(&mut file, &store);
    if hydration.needs_resave {
        let sanitized = secrets::sanitize_file(&file, &store, &hydration);
        persist::save_connections(&sanitized.file);
        // The pre-migration file (with plaintext secrets) was snapshotted to
        // `.bak` by that save — scrub it so no plaintext credential lingers.
        persist::clear_connections_backup();
        // The migration is the one save that can leave plaintext behind without
        // the user having done anything, so it reports on the same channel.
        if let Some(notice) = sanitized.notice() {
            persist::queue_notice(notice);
        }
    }
    // **Said, not only recorded.** `any_unreadable` had a doc claiming the app
    // surfaces this and no caller anywhere; the failure reached neither a banner
    // nor a log line, so the app came up with every password field blank, every
    // connection failing with the server's own *Access denied*, and nothing
    // saying the keyring was the reason. This is the startup notice channel the
    // config-recovery modal already drains.
    if let Some(notice) = hydration.notice() {
        persist::queue_notice(notice);
    }
    if let Ok(mut slot) = last_hydration().lock() {
        *slot = hydration;
    }
    file
}

/// Persist saved connections with their secrets stored in the keyring; the JSON
/// written to disk has every secret field blanked (unless the keyring was
/// unavailable, in which case the plaintext is kept so the credential isn't
/// lost).
/// Returns what the user has to be told about this save, once per session per
/// kind of problem — `None` when there is nothing to say, which is every save on
/// a machine whose keyring works.
///
/// Once per session because the alternative is a modal on every read-only
/// toggle and every connection switch for as long as the keyring is down, which
/// is a notice nobody reads.
#[must_use = "a `Some` is what the user is told; dropping it is how this went unsurfaced before"]
pub fn save_connections(file: &ConnectionsFile) -> Option<String> {
    // **`into_inner`, not `unwrap_or_default`.** A poisoned mutex used to hand
    // the save a *default* `Hydration` — nothing marked unreadable — and the
    // save then read every empty field as a password the user had cleared and
    // deleted it from the keyring. The failure direction of a lock has to be
    // "keep what we knew", not "know nothing".
    let mut hydration = last_hydration()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let sanitized = secrets::sanitize_file(file, &KeyringStore, &hydration);
    persist::save_connections(&sanitized.file);
    // A secret the user has since typed in is no longer unread; leaving it
    // marked would suppress the delete when they later clear it on purpose.
    hydration.resolve_against(file);
    // And a secret this save just *wrote* is one the store is now known to
    // hold, exactly as if a load had read it. `Hydration::stored` used to be
    // pushed to by a load and by nothing else, so a password typed into a
    // connection that had none and cleared again in the same session had its
    // refused delete go unreported — and came back on the next launch.
    hydration.absorb_stored(&sanitized);
    if let Ok(mut slot) = last_hydration().lock() {
        *slot = hydration;
    }
    let notice = sanitized.notice()?;
    told_once(&notice).then_some(notice)
}

/// Has this notice not been shown yet this session?
///
/// Keyed on the text rather than on a flag per kind, so a save that starts
/// carrying a *different* problem is still heard.
fn told_once(notice: &str) -> bool {
    static SEEN: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    SEEN.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(notice.to_string())
}

/// Forget a deleted connection's stored secrets. Returns whether they are all
/// definitely gone; `false` means the keyring would not answer and an entry may
/// outlive the connection — which matters, because ids are reused.
#[must_use = "a `false` is what the user is told; dropping it left the deleted               connection's secrets in the keyring for the next id to inherit"]
pub fn forget_connection(id: u64) -> bool {
    let gone = secrets::forget(id, &KeyringStore);
    if !gone {
        tracing::warn!(
            conn = id,
            "the OS keyring would not delete a removed connection's secrets"
        );
    }
    if let Ok(mut slot) = last_hydration().lock() {
        slot.forget(id);
    }
    gone
}

#[cfg(test)]
mod tests {
    use super::{classify_delete, classify_get};

    /// **The one classification the whole scheme rests on, finally asserted
    /// against the real backend's error type.** Every test that names the
    /// property this decides — `an_unreadable_secret_must_never_be_deleted`, and
    /// its sibling saying an absent entry is not an unreadable one — runs
    /// against a fake that reports errors correctly *by construction*. Collapse
    /// this match and all of them go on passing while the real store deletes the
    /// user's credentials.
    #[test]
    fn only_a_missing_entry_reads_as_no_secret() {
        assert_eq!(
            classify_get(Ok("s3cret".into())),
            Ok(Some("s3cret".to_string()))
        );
        assert_eq!(classify_get(Err(keyring::Error::NoEntry)), Ok(None));
    }

    /// Everything that is not `NoEntry` is *we could not read it*, and the
    /// backend's own words go with it — that message is the whole of what the
    /// user is told about why their connections stopped authenticating.
    #[test]
    fn every_other_backend_error_is_an_unreadable_secret_and_carries_its_reason() {
        let cases = [
            keyring::Error::NoStorageAccess(Box::new(std::io::Error::other("keyring is locked"))),
            keyring::Error::PlatformFailure(Box::new(std::io::Error::other("service down"))),
            keyring::Error::BadEncoding(vec![0xff]),
            keyring::Error::TooLong("service".into(), 32),
            keyring::Error::Invalid("target".into(), "empty".into()),
            keyring::Error::Ambiguous(vec![]),
        ];
        for e in cases {
            let expected = e.to_string();
            let got = classify_get(Err(e));
            let err = got.expect_err("must not read as an absent entry");
            assert_eq!(err.0, expected, "the backend's message must survive");
        }
    }

    /// The delete side of the same rule. A delete that quietly reported success
    /// left the entry in the keyring while the disk copy said empty, so the next
    /// launch hydrated a password the user had deliberately cleared back in.
    #[test]
    fn a_delete_is_only_done_when_the_entry_is_certainly_gone() {
        assert!(classify_delete(Ok(())));
        assert!(
            classify_delete(Err(keyring::Error::NoEntry)),
            "nothing there is the outcome the caller wanted"
        );
        assert!(!classify_delete(Err(keyring::Error::NoStorageAccess(
            Box::new(std::io::Error::other("locked"))
        ))));
        assert!(!classify_delete(Err(keyring::Error::PlatformFailure(
            Box::new(std::io::Error::other("down"))
        ))));
    }
}
