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

/// Why a [`SecretStore`] read failed. Carries the backend's message for display;
/// the *fact* of the error is what callers branch on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A place secrets can be stored, keyed by an account string. The real
/// implementation is the OS keyring (in the app crate); tests use an in-memory
/// fake. Writes are best-effort: `set` yields `false` when the backend is
/// unavailable, so callers keep the plaintext rather than lose the credential.
pub trait SecretStore {
    /// Fetch a stored secret.
    ///
    /// **`Ok(None)` and `Err` are not interchangeable, and collapsing them
    /// destroys credentials.** `Ok(None)` means there is no such entry; `Err`
    /// means the store exists but could not be read — a locked keyring, a denied
    /// prompt, a transient platform failure. Both leave the connection's field
    /// empty, but only the first means *the user has no password here*; the
    /// second means *we don't know*. [`sanitize_file`] deletes on the first and
    /// must not on the second.
    fn get(&self, account: &str) -> Result<Option<String>, StoreError>;
    /// Store a secret, returning `false` if the store is unavailable / the write
    /// failed (so the caller can keep the plaintext as a fallback).
    fn set(&self, account: &str, secret: &str) -> bool;
    /// Remove a stored secret (best effort; a missing entry is not an error).
    fn delete(&self, account: &str);
}

/// What a load learned about the store, which the matching save needs to know.
///
/// The two fields answer different questions and both are consumed at save time:
/// `needs_resave` asks *should we rewrite the file now*, `unreadable` asks *which
/// entries must this save leave alone*.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hydration {
    /// Some connection carried legacy plaintext on disk, so the file should be
    /// re-saved to migrate it into the store and blank the disk copy.
    pub needs_resave: bool,
    /// Secrets whose store read *failed* — as opposed to being absent. Their
    /// in-memory fields are empty for a reason that has nothing to do with the
    /// user, so [`sanitize_file`] must not treat them as cleared.
    pub unreadable: Vec<(u64, SecretKind)>,
}

impl Hydration {
    /// Did this connection's secret of `kind` fail to read?
    pub fn is_unreadable(&self, id: u64, kind: SecretKind) -> bool {
        self.unreadable.iter().any(|&(i, k)| i == id && k == kind)
    }

    /// Any secret at all failed to read — the app surfaces this, because it is
    /// the real reason connections stop authenticating.
    pub fn any_unreadable(&self) -> bool {
        !self.unreadable.is_empty()
    }

    /// Drop the entries `file` has since supplied a value for.
    ///
    /// Call after a save. Once a field is non-empty the save has written it to
    /// the store, so it is no longer "empty because we couldn't read it" — and
    /// leaving it marked would suppress the delete if the user later clears it
    /// on purpose, letting a stale entry be resurrected by the next hydrate.
    pub fn resolve_against(&mut self, file: &ConnectionsFile) {
        self.unreadable.retain(|&(id, kind)| {
            file.connections
                .iter()
                .find(|c| c.id == id)
                .is_some_and(|c| field(c, kind).is_empty())
        });
    }

    /// Drop every entry for a deleted connection, so a reused id can't inherit
    /// its protection (the delete-on-empty branch exists for exactly that case).
    pub fn forget(&mut self, id: u64) {
        self.unreadable.retain(|&(i, _)| i != id);
    }
}

/// Fill a connection's empty secret fields from the store, and report whether any
/// *legacy plaintext* was found on disk (a non-empty field), which means the file
/// must be re-saved to migrate that secret into the keyring and blank the disk
/// copy. Pure over the injected store.
///
/// A non-empty field is treated as legacy plaintext and left in place (so the
/// value is never lost even if the keyring is down); an empty field is hydrated
/// from the keyring when an entry exists, and recorded in `out.unreadable` when
/// the read fails.
fn hydrate(conn: &mut Connection, store: &dyn SecretStore, out: &mut Hydration) {
    for kind in SecretKind::ALL {
        if field(conn, kind).is_empty() {
            match store.get(&account(conn.id, kind)) {
                Ok(Some(v)) => set_field(conn, kind, v),
                Ok(None) => {}
                // Don't guess. The save path needs to know this field is empty
                // because we couldn't read it, not because there's nothing there.
                Err(_) => out.unreadable.push((conn.id, kind)),
            }
        } else {
            // Plaintext already on disk → needs migration on the next save.
            out.needs_resave = true;
        }
    }
}

/// Hydrate every connection in the file (see [`hydrate`]).
///
/// The returned [`Hydration`] must be handed to the [`sanitize_file`] of any
/// later save of this same file, or a transient store failure at load turns the
/// next ordinary save into a deletion of every credential it couldn't read.
pub fn hydrate_file(file: &mut ConnectionsFile, store: &dyn SecretStore) -> Hydration {
    let mut out = Hydration::default();
    for conn in &mut file.connections {
        hydrate(conn, store, &mut out);
    }
    out
}

/// Produce the on-disk form of `file`: every secret is moved into the store and
/// blanked in the returned copy. The input is left untouched (the in-memory
/// connections keep their live secrets). Pure over the injected store.
///
/// Per secret: a non-empty value is written to the store and, **only if that
/// write succeeds**, blanked in the disk copy; if the store is unavailable the
/// plaintext is left in the disk copy as a fallback. An empty value deletes any
/// stale store entry, so clearing a password can't be undone by a later hydrate.
///
/// `hydration` is what the load of this file learned, and it exists for one
/// branch: **a secret whose read failed is not a cleared secret.** Without it the
/// empty field is overloaded — a locked keyring at startup looks exactly like a
/// user who cleared every password, and the next unrelated save (a read-only
/// toggle is enough, since the whole file is rewritten) deletes credentials that
/// cannot be recovered. Uncertainty resolves to *don't destroy*. Pass
/// `&Hydration::default()` only when the file was not hydrated from this store.
pub fn sanitize_file(
    file: &ConnectionsFile,
    store: &dyn SecretStore,
    hydration: &Hydration,
) -> ConnectionsFile {
    let mut disk = file.clone();
    for conn in &mut disk.connections {
        for kind in SecretKind::ALL {
            let acct = account(conn.id, kind);
            let value = field(conn, kind).to_string();
            if value.is_empty() {
                if hydration.is_unreadable(conn.id, kind) {
                    // Empty only because we couldn't read it. Leave the stored
                    // entry exactly as it was.
                    continue;
                }
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

    /// In-memory [`SecretStore`] fake. `available == false` simulates a keyring
    /// that exists but can't be reached (get → Err, set → false) — note that is
    /// *unreadable*, not *empty*, which is the distinction the module turns on.
    /// Availability is a `Cell` so one store can fail a load and serve the save.
    struct MemStore {
        map: RefCell<HashMap<String, String>>,
        available: std::cell::Cell<bool>,
    }

    impl MemStore {
        fn new() -> Self {
            MemStore {
                map: RefCell::new(HashMap::new()),
                available: std::cell::Cell::new(true),
            }
        }
        fn unavailable() -> Self {
            let s = MemStore::new();
            s.available.set(false);
            s
        }
        fn seeded(pairs: &[(&str, &str)]) -> Self {
            let s = MemStore::new();
            for (k, v) in pairs {
                s.map.borrow_mut().insert(k.to_string(), v.to_string());
            }
            s
        }
        fn set_available(&self, yes: bool) {
            self.available.set(yes);
        }
        /// Read past the availability flag, to assert on what is actually stored.
        fn stored(&self, account: &str) -> Option<String> {
            self.map.borrow().get(account).cloned()
        }
    }

    impl SecretStore for MemStore {
        fn get(&self, account: &str) -> Result<Option<String>, StoreError> {
            if !self.available.get() {
                return Err(StoreError("keyring locked".into()));
            }
            Ok(self.map.borrow().get(account).cloned())
        }
        fn set(&self, account: &str, secret: &str) -> bool {
            if !self.available.get() {
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

    /// Sanitize a file that wasn't hydrated from this store (the common case in
    /// these tests) — nothing is known to be unreadable.
    fn sanitize(file: &ConnectionsFile, store: &dyn SecretStore) -> ConnectionsFile {
        sanitize_file(file, store, &Hydration::default())
    }

    fn hydrate_one(c: &mut Connection, store: &dyn SecretStore) -> Hydration {
        let mut out = Hydration::default();
        hydrate(c, store, &mut out);
        out
    }

    #[test]
    fn hydrate_pulls_secret_from_store_into_empty_field() {
        let store = MemStore::seeded(&[("conn.7.password", "s3cret")]);
        let mut c = conn(7);
        assert!(c.password.is_empty());
        let out = hydrate_one(&mut c, &store);
        assert_eq!(c.password, "s3cret");
        assert!(
            !out.needs_resave,
            "keyring-sourced secret is not legacy plaintext"
        );
        assert!(!out.any_unreadable());
    }

    #[test]
    fn hydrate_flags_legacy_plaintext_for_resave() {
        // Field non-empty on disk, keyring empty → legacy plaintext to migrate.
        let store = MemStore::new();
        let mut c = conn(7);
        c.password = "onDisk".to_string();
        let out = hydrate_one(&mut c, &store);
        assert!(out.needs_resave);
        assert_eq!(c.password, "onDisk", "plaintext kept, never lost");
    }

    #[test]
    fn hydrate_empty_field_with_no_entry_stays_empty_and_not_legacy() {
        // A genuinely password-less connection (empty, nothing in keyring).
        let store = MemStore::new();
        let mut c = conn(7);
        let out = hydrate_one(&mut c, &store);
        assert!(c.password.is_empty());
        assert!(!out.needs_resave);
        assert!(
            !out.any_unreadable(),
            "an absent entry is not an unreadable one"
        );
    }

    #[test]
    fn hydrate_records_every_unreadable_secret_and_claims_no_resave() {
        let store = MemStore::unavailable();
        let mut c = conn(7);
        let out = hydrate_one(&mut c, &store);
        assert_eq!(
            out.unreadable,
            vec![
                (7, SecretKind::DbPassword),
                (7, SecretKind::SshPassword),
                (7, SecretKind::SshPassphrase),
            ]
        );
        assert!(
            !out.needs_resave,
            "a failed read is no reason to rewrite the file"
        );
    }

    /// The finding: a locked keyring at startup leaves every field empty, and the
    /// next ordinary save — a read-only toggle rewrites the whole file — used to
    /// read that as "the user cleared every password" and delete them all.
    #[test]
    fn a_failed_hydrate_must_not_delete_the_stored_secret() {
        let store = MemStore::seeded(&[("conn.7.password", "s3cret")]);
        let mut file = ConnectionsFile {
            connections: vec![conn(7)],
            active: Some(7),
        };

        store.set_available(false); // keyring locked at startup
        let hydration = hydrate_file(&mut file, &store);
        assert!(file.connections[0].password.is_empty(), "read failed");
        assert!(hydration.is_unreadable(7, SecretKind::DbPassword));

        store.set_available(true); // user unlocks it, then edits something else
        let disk = sanitize_file(&file, &store, &hydration);

        assert_eq!(
            store.stored("conn.7.password").as_deref(),
            Some("s3cret"),
            "an unreadable secret must never be deleted"
        );
        assert_eq!(
            disk.connections[0].password, "",
            "and no plaintext appears on disk either"
        );
    }

    /// Once the user supplies a value, the protection must lift — otherwise a
    /// later deliberate clear wouldn't delete the entry, and the next hydrate
    /// would resurrect it.
    #[test]
    fn resolve_against_lifts_protection_once_a_value_is_supplied() {
        let mut hydration = Hydration {
            needs_resave: false,
            unreadable: vec![(7, SecretKind::DbPassword), (7, SecretKind::SshPassword)],
        };
        let mut c = conn(7);
        c.password = "typed-it-in".to_string(); // ssh password still empty
        let file = ConnectionsFile {
            connections: vec![c],
            active: Some(7),
        };

        hydration.resolve_against(&file);
        assert!(!hydration.is_unreadable(7, SecretKind::DbPassword));
        assert!(
            hydration.is_unreadable(7, SecretKind::SshPassword),
            "still unread, still protected"
        );
    }

    #[test]
    fn resolve_against_drops_entries_for_a_removed_connection() {
        let mut hydration = Hydration {
            needs_resave: false,
            unreadable: vec![(7, SecretKind::DbPassword)],
        };
        let empty = ConnectionsFile {
            connections: vec![],
            active: None,
        };
        hydration.resolve_against(&empty);
        assert!(!hydration.any_unreadable(), "gone with the connection");
    }

    #[test]
    fn forget_clears_protection_so_a_reused_id_cannot_inherit_it() {
        let mut hydration = Hydration {
            needs_resave: false,
            unreadable: vec![(7, SecretKind::DbPassword), (8, SecretKind::DbPassword)],
        };
        hydration.forget(7);
        assert!(!hydration.is_unreadable(7, SecretKind::DbPassword));
        assert!(hydration.is_unreadable(8, SecretKind::DbPassword));
    }

    /// The delete-on-empty branch must survive the fix: it is what stops a reused
    /// connection id from inheriting a deleted connection's password.
    #[test]
    fn a_cleared_field_still_deletes_when_the_read_succeeded() {
        let store = MemStore::seeded(&[("conn.7.password", "old")]);
        let mut file = ConnectionsFile {
            connections: vec![conn(7)],
            active: Some(7),
        };
        // A *successful* hydrate of a connection whose entry the user removed.
        store.delete("conn.7.password");
        let hydration = hydrate_file(&mut file, &store);
        assert!(!hydration.any_unreadable());

        store.set("conn.7.password", "old");
        let _ = sanitize_file(&file, &store, &hydration);
        assert_eq!(store.stored("conn.7.password"), None);
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
        let disk = sanitize(&file, &store);
        assert_eq!(disk.connections[0].password, "", "disk copy blanked");
        assert_eq!(
            file.connections[0].password, "s3cret",
            "in-memory secret untouched"
        );
        assert_eq!(store.stored("conn.7.password").as_deref(), Some("s3cret"));
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
        let disk = sanitize(&file, &store);
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
        let _ = sanitize(&file, &store);
        assert_eq!(store.stored("conn.7.password"), None);
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
        let disk = sanitize(&file, &store);
        assert_eq!(disk.connections[0].password, "");
        assert_eq!(disk.connections[0].ssh.password, "");
        assert_eq!(disk.connections[0].ssh.key_passphrase, "");
        // Load: the blank disk copy hydrates back to the originals.
        let mut reloaded = disk;
        let out = hydrate_file(&mut reloaded, &store);
        assert!(!out.needs_resave, "keyring-sourced, nothing to migrate");
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
        assert!(hydrate_file(&mut file, &store).needs_resave);
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
        assert_eq!(store.stored("conn.7.password"), None);
        assert_eq!(store.stored("conn.7.ssh_password"), None);
        assert_eq!(store.stored("conn.7.ssh_passphrase"), None);
        assert_eq!(
            store.stored("conn.8.password").as_deref(),
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
            fn get(&self, account: &str) -> Result<Option<String>, StoreError> {
                Ok(self.0.borrow().get(account).cloned())
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
        let disk = sanitize(&file, &store);
        assert_eq!(disk.connections[0].password, "", "db pw migrated");
        assert_eq!(
            disk.connections[0].ssh.password, "ssh",
            "ssh pw kept plaintext after failed store"
        );
        // Reloading flags the still-plaintext ssh secret for another migration.
        let mut reloaded = disk;
        assert!(hydrate_file(&mut reloaded, &store).needs_resave);
        assert_eq!(reloaded.connections[0].password, "db", "db pw rehydrated");
    }
}
