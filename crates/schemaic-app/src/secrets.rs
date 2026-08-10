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

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<String>, StoreError> {
        let entry = Entry::new(SERVICE, account).map_err(|e| StoreError(e.to_string()))?;
        match entry.get_password() {
            Ok(p) => Ok(Some(p)),
            // The only error that means "there is no secret here". Everything
            // else — a locked keyring, a denied prompt, a platform failure — is
            // an error, and must not read as an absent entry (see the trait).
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(StoreError(e.to_string())),
        }
    }

    fn set(&self, account: &str, secret: &str) -> bool {
        Entry::new(SERVICE, account)
            .and_then(|e| e.set_password(secret))
            .is_ok()
    }

    fn delete(&self, account: &str) {
        if let Ok(e) = Entry::new(SERVICE, account) {
            // A missing entry surfaces as `NoEntry` — not a real failure.
            let _ = e.delete_credential();
        }
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
        persist::save_connections(&secrets::sanitize_file(&file, &store, &hydration));
        // The pre-migration file (with plaintext secrets) was snapshotted to
        // `.bak` by that save — scrub it so no plaintext credential lingers.
        persist::clear_connections_backup();
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
pub fn save_connections(file: &ConnectionsFile) {
    let mut hydration = last_hydration()
        .lock()
        .map(|h| h.clone())
        .unwrap_or_default();
    persist::save_connections(&secrets::sanitize_file(file, &KeyringStore, &hydration));
    // A secret the user has since typed in is no longer unread; leaving it
    // marked would suppress the delete when they later clear it on purpose.
    hydration.resolve_against(file);
    if let Ok(mut slot) = last_hydration().lock() {
        *slot = hydration;
    }
}

/// Forget a deleted connection's stored secrets (best effort).
pub fn forget_connection(id: u64) {
    secrets::forget(id, &KeyringStore);
    if let Ok(mut slot) = last_hydration().lock() {
        slot.forget(id);
    }
}
