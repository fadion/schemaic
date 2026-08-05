//! Secret handling for saved connections — keeps credentials out of the plaintext
//! `connections.json` by storing them in the OS keyring instead.
//!
//! Three secrets per connection can be stored: the database password, the SSH
//! tunnel password, and the SSH key passphrase. Each is keyed in the keyring by
//! the connection id + a kind suffix (see [`account`]).
//!
//! This module is the *pure, testable* layer: it defines the [`SecretStore`]
//! seam and the transforms over a [`ConnectionsFile`] (hydrate on load, sanitize
//! on save, forget on delete). The real keyring-backed store lives in the app
//! crate so the heavy `keyring` / D-Bus dependency stays out of the pure core;
//! tests here drive the transforms through an in-memory fake.
//!
//! Design invariant (see CLAUDE.md): after a save, `connections.json` holds **no
//! plaintext secret** whenever the keyring is available — the field is blanked and
//! the value lives in the keyring. If the keyring is *unavailable* (e.g. a
//! headless Linux box with no secret service), we deliberately fall back to
//! leaving the plaintext in the JSON so the app keeps working; that is the one
//! sanctioned plaintext surface and it is best-effort, never silent data loss.

use crate::connection::Connection;
use crate::persist::ConnectionsFile;

/// Which secret of a connection an entry holds. The keyring account name is
/// `conn.{id}.{suffix}`, so the three secrets of one connection never collide and
/// entries for different connections stay independent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecretKind {
    /// The database password ([`Connection::password`]).
    DbPassword,
    /// The SSH tunnel password ([`crate::connection::SshTunnel::password`]).
    SshPassword,
    /// The SSH private-key passphrase ([`crate::connection::SshTunnel::key_passphrase`]).
    SshPassphrase,
}

impl SecretKind {
    /// All three kinds, so callers can iterate a connection's secrets uniformly.
    pub const ALL: [SecretKind; 3] = [
        SecretKind::DbPassword,
        SecretKind::SshPassword,
        SecretKind::SshPassphrase,
    ];

    fn suffix(self) -> &'static str {
        match self {
            SecretKind::DbPassword => "password",
            SecretKind::SshPassword => "ssh_password",
            SecretKind::SshPassphrase => "ssh_passphrase",
        }
    }
}

/// The keyring account name for one connection's secret of a given kind.
pub fn account(id: u64, kind: SecretKind) -> String {
    format!("conn.{id}.{}", kind.suffix())
}

/// Read a connection's secret field for `kind` (borrowing the live value).
fn field(conn: &Connection, kind: SecretKind) -> &str {
    match kind {
        SecretKind::DbPassword => &conn.password,
        SecretKind::SshPassword => &conn.ssh.password,
        SecretKind::SshPassphrase => &conn.ssh.key_passphrase,
    }
}

/// Overwrite a connection's secret field for `kind`.
fn set_field(conn: &mut Connection, kind: SecretKind, value: String) {
    match kind {
        SecretKind::DbPassword => conn.password = value,
        SecretKind::SshPassword => conn.ssh.password = value,
        SecretKind::SshPassphrase => conn.ssh.key_passphrase = value,
    }
}

/// A place secrets can be stored, keyed by an account string. The real
/// implementation is the OS keyring (in the app crate); tests use an in-memory
/// fake. All operations are best-effort: `get` yields `None` and `set` yields
/// `false` when the backend is unavailable, so callers degrade gracefully instead
/// of losing the credential.
pub trait SecretStore {
    /// Fetch a stored secret, or `None` if absent / the store is unavailable.
    fn get(&self, account: &str) -> Option<String>;
    /// Store a secret, returning `false` if the store is unavailable / the write
    /// failed (so the caller can keep the plaintext as a fallback).
    fn set(&self, account: &str, secret: &str) -> bool;
    /// Remove a stored secret (best effort; a missing entry is not an error).
    fn delete(&self, account: &str);
}

/// Fill a connection's empty secret fields from the store, and report whether any
/// *legacy plaintext* was found on disk (a non-empty field), which means the file
/// must be re-saved to migrate that secret into the keyring and blank the disk
/// copy. Pure over the injected store.
///
/// A non-empty field is treated as legacy plaintext and left in place (so the
/// value is never lost even if the keyring is down); an empty field is hydrated
/// from the keyring when an entry exists.
fn hydrate(conn: &mut Connection, store: &dyn SecretStore) -> bool {
    let mut legacy = false;
    for kind in SecretKind::ALL {
        if field(conn, kind).is_empty() {
            if let Some(v) = store.get(&account(conn.id, kind)) {
                set_field(conn, kind, v);
            }
        } else {
            // Plaintext already on disk → needs migration on the next save.
            legacy = true;
        }
    }
    legacy
}

/// Hydrate every connection in the file (see [`hydrate`]). Returns `true` if any
/// connection carried legacy plaintext and the file should be re-saved to migrate
/// it into the keyring.
pub fn hydrate_file(file: &mut ConnectionsFile, store: &dyn SecretStore) -> bool {
    let mut needs_resave = false;
    for conn in &mut file.connections {
        needs_resave |= hydrate(conn, store);
    }
    needs_resave
}

/// Produce the on-disk form of `file`: every secret is moved into the store and
/// blanked in the returned copy. The input is left untouched (the in-memory
/// connections keep their live secrets). Pure over the injected store.
///
/// Per secret: a non-empty value is written to the store and, **only if that
/// write succeeds**, blanked in the disk copy; if the store is unavailable the
/// plaintext is left in the disk copy as a fallback. An empty value deletes any
/// stale store entry, so clearing a password can't be undone by a later hydrate.
pub fn sanitize_file(file: &ConnectionsFile, store: &dyn SecretStore) -> ConnectionsFile {
    let mut disk = file.clone();
    for conn in &mut disk.connections {
        for kind in SecretKind::ALL {
            let acct = account(conn.id, kind);
            let value = field(conn, kind).to_string();
            if value.is_empty() {
                store.delete(&acct);
            } else if store.set(&acct, &value) {
                set_field(conn, kind, String::new());
            }
            // else: store unavailable — keep the plaintext in the disk copy.
        }
    }
    disk
}

/// Remove every stored secret for a deleted connection (best effort).
pub fn forget(id: u64, store: &dyn SecretStore) {
    for kind in SecretKind::ALL {
        store.delete(&account(id, kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::SshTunnel;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// In-memory [`SecretStore`] fake. `available == false` simulates a machine
    /// with no working keyring (get → None, set → false).
    struct MemStore {
        map: RefCell<HashMap<String, String>>,
        available: bool,
    }

    impl MemStore {
        fn new() -> Self {
            MemStore {
                map: RefCell::new(HashMap::new()),
                available: true,
            }
        }
        fn unavailable() -> Self {
            MemStore {
                map: RefCell::new(HashMap::new()),
                available: false,
            }
        }
        fn seeded(pairs: &[(&str, &str)]) -> Self {
            let s = MemStore::new();
            for (k, v) in pairs {
                s.map.borrow_mut().insert(k.to_string(), v.to_string());
            }
            s
        }
    }

    impl SecretStore for MemStore {
        fn get(&self, account: &str) -> Option<String> {
            if !self.available {
                return None;
            }
            self.map.borrow().get(account).cloned()
        }
        fn set(&self, account: &str, secret: &str) -> bool {
            if !self.available {
                return false;
            }
            self.map
                .borrow_mut()
                .insert(account.to_string(), secret.to_string());
            true
        }
        fn delete(&self, account: &str) {
            self.map.borrow_mut().remove(account);
        }
    }

    fn conn(id: u64) -> Connection {
        Connection {
            id,
            name: format!("c{id}"),
            db_type: "MySQL".to_string(),
            host: "h".to_string(),
            port: 3306,
            user: "u".to_string(),
            password: String::new(),
            ssh: SshTunnel::default(),
            color: None,
            prominent_color: false,
            read_only: false,
            environment: crate::connection::Environment::None,
        }
    }

    #[test]
    fn account_names_are_distinct_per_kind_and_id() {
        assert_eq!(account(1, SecretKind::DbPassword), "conn.1.password");
        assert_eq!(account(1, SecretKind::SshPassword), "conn.1.ssh_password");
        assert_eq!(
            account(1, SecretKind::SshPassphrase),
            "conn.1.ssh_passphrase"
        );
        assert_ne!(
            account(1, SecretKind::DbPassword),
            account(2, SecretKind::DbPassword)
        );
    }

    #[test]
    fn hydrate_pulls_secret_from_store_into_empty_field() {
        let store = MemStore::seeded(&[("conn.7.password", "s3cret")]);
        let mut c = conn(7);
        assert!(c.password.is_empty());
        let legacy = hydrate(&mut c, &store);
        assert_eq!(c.password, "s3cret");
        assert!(!legacy, "keyring-sourced secret is not legacy plaintext");
    }

    #[test]
    fn hydrate_flags_legacy_plaintext_for_resave() {
        // Field non-empty on disk, keyring empty → legacy plaintext to migrate.
        let store = MemStore::new();
        let mut c = conn(7);
        c.password = "onDisk".to_string();
        let legacy = hydrate(&mut c, &store);
        assert!(legacy);
        assert_eq!(c.password, "onDisk", "plaintext kept, never lost");
    }

    #[test]
    fn hydrate_empty_field_with_no_entry_stays_empty_and_not_legacy() {
        // A genuinely password-less connection (empty, nothing in keyring).
        let store = MemStore::new();
        let mut c = conn(7);
        let legacy = hydrate(&mut c, &store);
        assert!(c.password.is_empty());
        assert!(!legacy);
    }

    #[test]
    fn sanitize_moves_secret_to_store_and_blanks_disk_copy() {
        let store = MemStore::new();
        let mut c = conn(7);
        c.password = "s3cret".to_string();
        let file = ConnectionsFile {
            connections: vec![c],
            active: Some(7),
        };
        let disk = sanitize_file(&file, &store);
        assert_eq!(disk.connections[0].password, "", "disk copy blanked");
        assert_eq!(
            file.connections[0].password, "s3cret",
            "in-memory secret untouched"
        );
        assert_eq!(store.get("conn.7.password").as_deref(), Some("s3cret"));
    }

    #[test]
    fn sanitize_keeps_plaintext_when_store_unavailable() {
        let store = MemStore::unavailable();
        let mut c = conn(7);
        c.password = "s3cret".to_string();
        let file = ConnectionsFile {
            connections: vec![c],
            active: Some(7),
        };
        let disk = sanitize_file(&file, &store);
        assert_eq!(
            disk.connections[0].password, "s3cret",
            "no keyring → plaintext fallback so the credential isn't lost"
        );
    }

    #[test]
    fn sanitize_deletes_stale_entry_for_cleared_field() {
        // Keyring had an old password; the user cleared the field. Sanitize must
        // delete the stale entry so a later hydrate can't resurrect it.
        let store = MemStore::seeded(&[("conn.7.password", "old")]);
        let file = ConnectionsFile {
            connections: vec![conn(7)], // password empty
            active: Some(7),
        };
        let _ = sanitize_file(&file, &store);
        assert_eq!(store.get("conn.7.password"), None);
    }

    #[test]
    fn roundtrip_sanitize_then_hydrate_restores_every_secret() {
        let store = MemStore::new();
        let mut c = conn(7);
        c.password = "db-pw".to_string();
        c.ssh.password = "ssh-pw".to_string();
        c.ssh.key_passphrase = "kp".to_string();
        let file = ConnectionsFile {
            connections: vec![c],
            active: Some(7),
        };
        // Save: secrets go to keyring, disk copy is blank.
        let disk = sanitize_file(&file, &store);
        assert_eq!(disk.connections[0].password, "");
        assert_eq!(disk.connections[0].ssh.password, "");
        assert_eq!(disk.connections[0].ssh.key_passphrase, "");
        // Load: the blank disk copy hydrates back to the originals.
        let mut reloaded = disk;
        let legacy = hydrate_file(&mut reloaded, &store);
        assert!(!legacy, "keyring-sourced, nothing to migrate");
        assert_eq!(reloaded.connections[0].password, "db-pw");
        assert_eq!(reloaded.connections[0].ssh.password, "ssh-pw");
        assert_eq!(reloaded.connections[0].ssh.key_passphrase, "kp");
    }

    #[test]
    fn hydrate_file_reports_resave_when_any_connection_has_plaintext() {
        let store = MemStore::new();
        let mut clean = conn(1);
        clean.password.clear();
        let mut legacy_c = conn(2);
        legacy_c.password = "plain".to_string();
        let mut file = ConnectionsFile {
            connections: vec![clean, legacy_c],
            active: Some(1),
        };
        assert!(hydrate_file(&mut file, &store));
    }

    #[test]
    fn forget_removes_all_secrets_for_connection() {
        let store = MemStore::seeded(&[
            ("conn.7.password", "a"),
            ("conn.7.ssh_password", "b"),
            ("conn.7.ssh_passphrase", "c"),
            ("conn.8.password", "keep"),
        ]);
        forget(7, &store);
        assert_eq!(store.get("conn.7.password"), None);
        assert_eq!(store.get("conn.7.ssh_password"), None);
        assert_eq!(store.get("conn.7.ssh_passphrase"), None);
        assert_eq!(
            store.get("conn.8.password").as_deref(),
            Some("keep"),
            "other connections untouched"
        );
    }

    #[test]
    fn partial_store_failure_self_heals_on_next_load() {
        // A store that stores the db password but rejects ssh secrets, to model a
        // partial write. The rejected one stays plaintext and is flagged legacy.
        struct PartialStore(RefCell<HashMap<String, String>>);
        impl SecretStore for PartialStore {
            fn get(&self, account: &str) -> Option<String> {
                self.0.borrow().get(account).cloned()
            }
            fn set(&self, account: &str, secret: &str) -> bool {
                if account.contains("ssh") {
                    return false;
                }
                self.0
                    .borrow_mut()
                    .insert(account.to_string(), secret.to_string());
                true
            }
            fn delete(&self, account: &str) {
                self.0.borrow_mut().remove(account);
            }
        }
        let store = PartialStore(RefCell::new(HashMap::new()));
        let mut c = conn(7);
        c.password = "db".to_string();
        c.ssh.password = "ssh".to_string();
        let file = ConnectionsFile {
            connections: vec![c],
            active: Some(7),
        };
        let disk = sanitize_file(&file, &store);
        assert_eq!(disk.connections[0].password, "", "db pw migrated");
        assert_eq!(
            disk.connections[0].ssh.password, "ssh",
            "ssh pw kept plaintext after failed store"
        );
        // Reloading flags the still-plaintext ssh secret for another migration.
        let mut reloaded = disk;
        assert!(hydrate_file(&mut reloaded, &store));
        assert_eq!(reloaded.connections[0].password, "db", "db pw rehydrated");
    }
}
