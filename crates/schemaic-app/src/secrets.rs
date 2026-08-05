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

use keyring::Entry;
use schemaic_core::persist::{self, ConnectionsFile};
use schemaic_core::secrets::{self, SecretStore};

/// Keyring service name under which all of Schemaic's secrets are grouped; the
/// per-secret account string comes from [`schemaic_core::secrets::account`].
const SERVICE: &str = "schemaic";

/// The real secret store: the OS keyring (Windows Credential Manager / Secret
/// Service / Keychain, per the target-gated `keyring` backend).
pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Option<String> {
        Entry::new(SERVICE, account).ok()?.get_password().ok()
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

/// Load saved connections, hydrating each one's secrets from the keyring. If the
/// file still carries legacy plaintext (first launch after upgrading, or after a
/// spell without a keyring), migrate it into the keyring and rewrite the on-disk
/// copy blanked — a one-time self-heal.
pub fn load_connections() -> ConnectionsFile {
    let store = KeyringStore;
    let mut file = persist::load_connections();
    if secrets::hydrate_file(&mut file, &store) {
        persist::save_connections(&secrets::sanitize_file(&file, &store));
        // The pre-migration file (with plaintext secrets) was snapshotted to
        // `.bak` by that save — scrub it so no plaintext credential lingers.
        persist::clear_connections_backup();
    }
    file
}

/// Persist saved connections with their secrets stored in the keyring; the JSON
/// written to disk has every secret field blanked (unless the keyring was
/// unavailable, in which case the plaintext is kept so the credential isn't
/// lost).
pub fn save_connections(file: &ConnectionsFile) {
    persist::save_connections(&secrets::sanitize_file(file, &KeyringStore));
}

/// Forget a deleted connection's stored secrets (best effort).
pub fn forget_connection(id: u64) {
    secrets::forget(id, &KeyringStore);
}
