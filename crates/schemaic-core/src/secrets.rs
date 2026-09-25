//! Secret handling for saved connections — keeps credentials out of the plaintext
//! `connections.json` by storing them in the OS keyring instead.
//!
//! Three secrets per connection can be stored: the database password, the SSH
//! tunnel password and the SSH key passphrase. Each is keyed in the keyring by
//! the connection id + a kind suffix (see [`account`]).
//!
//! A fourth — the TLS client-key passphrase — was collected and stored by an
//! earlier build for a feature that could never work, and is withdrawn; see
//! [`RETIRED_SECRET_SUFFIXES`], which is how an entry it wrote is cleaned off
//! the machines that have one.
//!
//! This module is the *pure, testable* layer: it defines the [`SecretStore`]
//! seam and the transforms over a [`ConnectionsFile`] (hydrate on load, sanitize
//! on save, forget on delete). The real keyring-backed store lives in the app
//! crate so the heavy `keyring` / D-Bus dependency stays out of the pure core;
//! tests here drive the transforms through an in-memory fake.
//!
//! Design invariant (see `docs/architecture.md`): after a save, `connections.json` holds **no
//! plaintext secret** whenever the keyring is available — the field is blanked and
//! the value lives in the keyring. If the keyring is *unavailable* (e.g. a
//! headless Linux box with no secret service), we deliberately fall back to
//! leaving the plaintext in the JSON so the app keeps working; that is the one
//! sanctioned plaintext surface and it is best-effort, never silent data loss.

use crate::connection::Connection;
use crate::persist::ConnectionsFile;

/// Which secret of a connection an entry holds. The keyring account name is
/// `conn.{id}.{suffix}`, so one connection's secrets never collide and
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

/// Account suffixes this app **no longer writes**, swept wherever entries are
/// deleted.
///
/// A withdrawn secret is not the same as one that was never there. `tls_key
/// _passphrase` was collected and stored by an earlier build for a feature that
/// could not work (see `connection::Tls`), so a user who typed one has an entry
/// in their keyring that nothing would ever read and nothing would ever delete.
/// Removing the [`SecretKind`] arm alone would strand it there permanently.
pub const RETIRED_SECRET_SUFFIXES: [&str; 1] = ["tls_key_passphrase"];

/// The keyring account for a retired secret of a connection.
fn retired_account(id: u64, suffix: &str) -> String {
    format!("conn.{id}.{suffix}")
}

impl SecretKind {
    /// Every kind, so callers can iterate a connection's secrets uniformly —
    /// which is what makes adding one here enough to have it hydrated, sanitized
    /// and forgotten everywhere.
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
    /// Remove a stored secret, returning whether the entry is now **definitely
    /// gone** — `true` for a delete that succeeded and for one that found
    /// nothing, `false` when the store could not be reached.
    ///
    /// It used to return `()`, "best effort; a missing entry is not an error",
    /// while its sibling `set` returned `bool` for exactly the reason this now
    /// does. The asymmetry was the defect: [`sanitize_file`]'s own doc promises
    /// that *clearing a password can't be undone by a later hydrate*, and that
    /// promise rests entirely on this call — a delete that quietly failed left
    /// the entry in the keyring while the disk copy said empty, so the next
    /// launch hydrated the deleted password back in and the connection went on
    /// authenticating with a credential the user had removed.
    fn delete(&self, account: &str) -> bool;
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
    /// Secrets the store actually **held a value for** at load.
    ///
    /// What makes a refused delete worth reporting. Every empty field asks the
    /// store to delete, and on a machine with no keyring at all every one of
    /// those refusals is meaningless — there was nothing there. A refusal only
    /// means *your clear did not take* for a field this load read a value out
    /// of, and saying it for the others would be three false alarms per
    /// connection on exactly the machines that already have a real problem.
    pub stored: Vec<(u64, SecretKind)>,
    /// One backend message from those failed reads, for the sentence the user
    /// is shown.
    ///
    /// [`StoreError`]'s own doc says it *"carries the backend's message for
    /// display"*, and the only site in the workspace that builds one from a real
    /// backend used to drop it on a wildcard — so the app opened with every
    /// password field blank, every connection failing with the server's own
    /// *Access denied*, and nothing anywhere saying the keyring was the reason.
    /// The user's rational next move is to conclude the passwords are gone and
    /// retype them, and a misremembered one is then written over the still-intact
    /// stored secret.
    pub error: Option<String>,
}

impl Hydration {
    /// Did this connection's secret of `kind` fail to read?
    pub fn is_unreadable(&self, id: u64, kind: SecretKind) -> bool {
        self.unreadable.iter().any(|&(i, k)| i == id && k == kind)
    }

    /// Did the store hold a value for this one at load? See [`Hydration::stored`].
    pub fn was_stored(&self, id: u64, kind: SecretKind) -> bool {
        self.stored.iter().any(|&(i, k)| i == id && k == kind)
    }

    /// Any secret at all failed to read — the app surfaces this, because it is
    /// the real reason connections stop authenticating.
    pub fn any_unreadable(&self) -> bool {
        !self.unreadable.is_empty()
    }

    /// What to tell the user when the keyring would not answer, or `None` when
    /// it answered everything.
    ///
    /// **Both halves of the sentence are load-bearing.** "Unavailable this
    /// session" is why the connections stopped working; "were not deleted" is
    /// what stops the user retyping a half-remembered password over a stored one
    /// that is perfectly intact. The wording lives here, with the fact, rather
    /// than in whichever surface happens to show it — this used to have a
    /// documented caller that did not exist, and the failure reached neither a
    /// banner nor a log line.
    pub fn notice(&self) -> Option<String> {
        if !self.any_unreadable() {
            return None;
        }
        let n = self.unreadable.len();
        let reason = match &self.error {
            Some(e) => format!(" ({e})"),
            None => String::new(),
        };
        Some(format!(
            "Schemaic could not read {n} saved {} from the OS keyring{reason}.\n\
             Those passwords are unavailable this session and were **not** deleted — \
             leave the fields blank and they will come back once the keyring is reachable. \
             Typing a new one saves over the stored secret.",
            crate::text::plural(n, "secret", "secrets"),
        ))
    }

    /// [`Hydration::notice`] for the headless CLI's stderr: plain text, the
    /// secrets named, and the one way round a locked keyring the command line
    /// has — which is offered only when it would help, since `--password-stdin`
    /// supplies the database password and nothing else.
    pub fn cli_notice(&self) -> Option<String> {
        if !self.any_unreadable() {
            return None;
        }
        let names: Vec<&str> = SecretKind::ALL
            .into_iter()
            .filter(|&k| self.unreadable.iter().any(|&(_, u)| u == k))
            .map(|k| match k {
                SecretKind::DbPassword => "database password",
                SecretKind::SshPassword => "SSH password",
                SecretKind::SshPassphrase => "SSH key passphrase",
            })
            .collect();
        let reason = match &self.error {
            Some(e) => format!(" ({e})"),
            None => String::new(),
        };
        let way_round = if self
            .unreadable
            .iter()
            .any(|&(_, k)| k == SecretKind::DbPassword)
        {
            "; unlock the keyring, or pipe the password in with --password-stdin"
        } else {
            "; unlock the keyring and try again"
        };
        Some(format!(
            "could not read the connection's {} from the OS keyring{reason}; \
             nothing was deleted{way_round}",
            names.join(" and ")
        ))
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

    /// Take on what a save just wrote to the store.
    ///
    /// The other half of [`Hydration::stored`]: a load records what it *read*,
    /// and this records what a [`sanitize_file`] **wrote**. Both make the entry
    /// one the store is known to hold, which is the question `was_stored` is
    /// asked — and without this half a secret typed and cleared inside one
    /// session had its refused delete pass in silence.
    pub fn absorb_stored(&mut self, saved: &Sanitized) {
        for &pair in &saved.stored {
            if !self.stored.contains(&pair) {
                self.stored.push(pair);
            }
        }
    }

    /// Drop every entry for a deleted connection, so a reused id can't inherit
    /// its protection (the delete-on-empty branch exists for exactly that case)
    /// or be reported against a stored secret that was its predecessor's.
    pub fn forget(&mut self, id: u64) {
        self.unreadable.retain(|&(i, _)| i != id);
        self.stored.retain(|&(i, _)| i != id);
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
    hydrate_kinds(conn, store, out, &[]);
}

/// [`hydrate`], leaving out the kinds in `skip`.
fn hydrate_kinds(
    conn: &mut Connection,
    store: &dyn SecretStore,
    out: &mut Hydration,
    skip: &[SecretKind],
) {
    for kind in SecretKind::ALL.into_iter().filter(|k| !skip.contains(k)) {
        if field(conn, kind).is_empty() {
            match store.get(&account(conn.id, kind)) {
                Ok(Some(v)) => {
                    out.stored.push((conn.id, kind));
                    set_field(conn, kind, v);
                }
                Ok(None) => {}
                // Don't guess. The save path needs to know this field is empty
                // because we couldn't read it, not because there's nothing there
                // — and the *reason* is what the user has to be told, so it is
                // kept rather than dropped on a wildcard.
                Err(e) => {
                    tracing::warn!(
                        conn = conn.id,
                        kind = ?kind,
                        error = %e,
                        "could not read a secret from the OS keyring"
                    );
                    out.error.get_or_insert_with(|| e.0.clone());
                    out.unreadable.push((conn.id, kind));
                }
            }
        } else {
            // Plaintext already on disk → needs migration on the next save.
            out.needs_resave = true;
        }
    }
}

/// Hydrate **one** connection, leaving out the kinds in `supplied` — the
/// headless CLI's load.
///
/// The app hydrates the whole file because it may show any connection; a
/// command runs against one, and reading every other connection's secrets was
/// a keyring round trip (and on macOS, potentially a keychain prompt) per
/// secret it would never use. `supplied` is what the caller got another way —
/// `--password-stdin` — which is not read at all, so a locked keyring cannot
/// report it unreadable.
///
/// **Nor is a secret the connection cannot use.** A connection with no tunnel
/// has no SSH password to read, and asking for one on a locked keyring told the
/// user to unlock it and retry a command that had just succeeded — see
/// [`unused_kinds`].
pub fn hydrate_connection(
    conn: &mut Connection,
    store: &dyn SecretStore,
    supplied: &[SecretKind],
) -> Hydration {
    let mut out = Hydration::default();
    let mut skip = unused_kinds(conn);
    skip.extend_from_slice(supplied);
    hydrate_kinds(conn, store, &mut out, &skip);
    out
}

/// The secret kinds `conn` has no use for as configured: both SSH secrets when
/// it has no tunnel, and the one its SSH auth method does not take — the
/// passphrase for a password login, the password for a key, both for the agent.
///
/// The app still hydrates every kind (a form can switch the auth method and
/// should find the secret there); this is for a caller that only connects.
pub fn unused_kinds(conn: &Connection) -> Vec<SecretKind> {
    use crate::connection::SshAuth;
    if !conn.ssh.enabled {
        return vec![SecretKind::SshPassword, SecretKind::SshPassphrase];
    }
    match conn.ssh.auth {
        SshAuth::Password => vec![SecretKind::SshPassphrase],
        SshAuth::KeyPair => vec![SecretKind::SshPassword],
        SshAuth::Agent => vec![SecretKind::SshPassword, SecretKind::SshPassphrase],
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
) -> Sanitized {
    let mut out = Sanitized {
        file: file.clone(),
        ..Sanitized::default()
    };
    for conn in &mut out.file.connections {
        // A secret this app no longer writes still has to be cleaned up on
        // whatever machine an older build wrote it — see
        // `RETIRED_SECRET_SUFFIXES`. Every save, not only a delete: a user who
        // typed a TLS key passphrase may never delete that connection.
        //
        // Not reported: this sweep runs on machines that never had one, so it
        // has nothing to say when it cannot reach a store, and the next save
        // retries it.
        for suffix in RETIRED_SECRET_SUFFIXES {
            store.delete(&retired_account(conn.id, suffix));
        }
        for kind in SecretKind::ALL {
            let acct = account(conn.id, kind);
            let value = field(conn, kind).to_string();
            if value.is_empty() {
                if hydration.is_unreadable(conn.id, kind) {
                    // Empty only because we couldn't read it. Leave the stored
                    // entry exactly as it was.
                    continue;
                }
                if !store.delete(&acct) && hydration.was_stored(conn.id, kind) {
                    // The disk copy says empty and the keyring still holds the
                    // old value, so the next hydrate would restore a password
                    // the user deliberately cleared. The next save retries the
                    // delete — the field stays empty and stays readable — but
                    // until one lands the user has to be told the clear did not
                    // take, or they meet it two launches later.
                    //
                    // Only for a field the load *read a value out of*: a refused
                    // delete of an entry that was never there is not news, and
                    // on a machine with no keyring every empty field would raise
                    // one.
                    out.undeleted.push((conn.id, kind));
                }
            } else if store.set(&acct, &value) {
                // The store now holds it, whoever read it first — see
                // `Sanitized::stored`.
                out.stored.push((conn.id, kind));
                set_field(conn, kind, String::new());
            } else {
                // Store unavailable — keep the plaintext in the disk copy, which
                // is the sanctioned fallback, and **say so**. It goes into the
                // same folder the Settings modal offers an *Open folder* button
                // for, next to the log a support request asks for.
                out.in_the_clear.push((conn.id, kind));
            }
        }
    }
    out
}

/// What a save learned while moving secrets into the store — the write-side
/// counterpart of [`Hydration`].
///
/// Both lists used to be nothing at all: the sanitized file was returned and the
/// two facts it had just established were discarded at the only call site. The
/// app's stated invariant is that connection secrets live in the OS keyring and
/// not in `connections.json`, so a machine where that is quietly untrue is
/// exactly the machine whose user needs to hear it.
#[derive(Clone, Debug, Default)]
pub struct Sanitized {
    /// The copy to write to disk.
    pub file: ConnectionsFile,
    /// Secrets the store would not take, so the disk copy carries them in the
    /// clear.
    pub in_the_clear: Vec<(u64, SecretKind)>,
    /// Secrets the user cleared that the store would not delete, so the stored
    /// entry outlives the blanked disk field.
    pub undeleted: Vec<(u64, SecretKind)>,
    /// Secrets this save **wrote** to the store, which it therefore now holds.
    ///
    /// Folded into [`Hydration::stored`] by the caller. Without it `was_stored`
    /// answered only for what a *load* read, so a password typed and saved in
    /// one session and cleared in the same one had its refused delete go
    /// unreported — and came back on the next launch.
    pub stored: Vec<(u64, SecretKind)>,
}

impl Sanitized {
    /// What to tell the user about this save, or `None` when it did what it says
    /// on the tin.
    ///
    /// One sentence per fact, and each names the consequence rather than the
    /// mechanism: the user cannot act on "the keyring returned an error", and
    /// can act on "your passwords are in a file you are about to send someone".
    pub fn notice(&self) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if !self.in_the_clear.is_empty() {
            let n = self.in_the_clear.len();
            parts.push(format!(
                "Schemaic could not reach the OS keyring, so {n} {} \
                 saved **in plain text** in `connections.json`.\n\
                 That file is in the folder Settings → General's *Open folder* button opens, \
                 beside the log — check before sharing it. Saving again once the keyring is \
                 reachable moves them back in.",
                crate::text::plural(n, "password was", "passwords were"),
            ));
        }
        if !self.undeleted.is_empty() {
            let n = self.undeleted.len();
            parts.push(format!(
                "{n} cleared {} still in the OS keyring — Schemaic could not delete {}.\n\
                 The field is blank on disk, but the next launch will fill it back in. \
                 Save again once the keyring is reachable.",
                crate::text::plural(n, "password is", "passwords are"),
                crate::text::plural(n, "it", "them"),
            ));
        }
        (!parts.is_empty()).then(|| parts.join("\n\n"))
    }
}

/// Remove every stored secret for a deleted connection. Returns whether they are
/// all definitely gone — `false` means at least one entry may still be in the
/// keyring.
///
/// **That used to matter because ids were reused**, so a surviving entry would be
/// handed to the next connection created;
/// [`crate::connection::Connection::next_id_after`] closed that. It still
/// matters, for the reason it would have anyway: the user confirmed a deletion
/// that said the credential was unrecoverable, and it is still at rest.
pub fn forget(id: u64, store: &dyn SecretStore) -> bool {
    let mut gone = true;
    for kind in SecretKind::ALL {
        gone &= store.delete(&account(id, kind));
    }
    for suffix in RETIRED_SECRET_SUFFIXES {
        gone &= store.delete(&retired_account(id, suffix));
    }
    gone
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
        fn delete(&self, account: &str) -> bool {
            if !self.available.get() {
                return false;
            }
            self.map.borrow_mut().remove(account);
            true
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
            file: String::new(),
            database: String::new(),
            ssh: SshTunnel::default(),
            tls: crate::connection::Tls::default(),
            color: None,
            prominent_color: false,
            read_only: false,
            cli_access: false,
            environment: crate::connection::Environment::None,
            ai_data: None,
            folder: String::new(),
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

    /// Every kind must have its own slot: two sharing a suffix would have one
    /// secret overwrite the other on save and come back as the wrong credential
    /// on load. Asserted over `ALL` so a kind added later has to earn a suffix
    /// rather than inherit a collision.
    #[test]
    fn no_two_kinds_share_a_slot() {
        let mut names: Vec<String> = SecretKind::ALL.iter().map(|k| account(3, *k)).collect();
        let total = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), total, "two kinds share one keyring account");
    }

    /// **A withdrawn secret has to be cleaned off the machines that stored
    /// it.** The TLS client-key passphrase was collected and written to the
    /// keyring by an earlier build for a feature that could never work; removing
    /// the `SecretKind` arm alone would strand that entry there permanently,
    /// unread and undeletable through the app.
    ///
    /// Swept on every ordinary save, not only on delete: a user who typed one
    /// may never delete that connection.
    #[test]
    fn a_retired_secret_is_deleted_from_the_keyring() {
        let store = MemStore::new();
        assert!(store.set("conn.4.tls_key_passphrase", "let me in"));
        assert!(store.stored("conn.4.tls_key_passphrase").is_some());

        let file = ConnectionsFile {
            connections: vec![conn(4)],
            active: Some(4),
            highest_id: 0,
        };
        sanitize(&file, &store);
        assert_eq!(
            store.stored("conn.4.tls_key_passphrase"),
            None,
            "an ordinary save must clear it"
        );

        // And the delete path too, for a connection that is removed before it
        // is ever saved again.
        assert!(store.set("conn.5.tls_key_passphrase", "let me in"));
        forget(5, &store);
        assert_eq!(store.stored("conn.5.tls_key_passphrase"), None);
    }

    /// The sweep list and the live list must not overlap, or a save would delete
    /// a secret it had just written.
    #[test]
    fn no_retired_suffix_is_also_a_live_one() {
        for suffix in RETIRED_SECRET_SUFFIXES {
            for kind in SecretKind::ALL {
                assert_ne!(account(9, kind), retired_account(9, suffix), "{suffix}");
            }
        }
    }

    /// Sanitize a file that wasn't hydrated from this store (the common case in
    /// these tests) — nothing is known to be unreadable.
    fn sanitize(file: &ConnectionsFile, store: &dyn SecretStore) -> ConnectionsFile {
        sanitize_file(file, store, &Hydration::default()).file
    }

    fn hydrate_one(c: &mut Connection, store: &dyn SecretStore) -> Hydration {
        let mut out = Hydration::default();
        hydrate(c, store, &mut out);
        out
    }

    /// **The CLI asks the keyring only for what it will use.** A secret the
    /// caller already has — `--password-stdin` — is not read, so a locked
    /// keyring cannot report it unreadable and a macOS keychain has no item to
    /// prompt for.
    #[test]
    fn hydrating_one_connection_skips_what_the_caller_supplied() {
        let store = MemStore::unavailable();
        let mut c = conn(3);
        // A tunnel, so the SSH password is one it does use.
        c.ssh.enabled = true;
        let out = hydrate_connection(&mut c, &store, &[SecretKind::DbPassword]);
        assert!(!out.is_unreadable(3, SecretKind::DbPassword));
        assert!(out.is_unreadable(3, SecretKind::SshPassword));
    }

    #[test]
    fn hydrating_one_connection_fills_it_from_the_store() {
        let store = MemStore::seeded(&[("conn.3.password", "s3cret")]);
        let mut c = conn(3);
        let out = hydrate_connection(&mut c, &store, &[]);
        assert_eq!(c.password, "s3cret");
        assert!(out.cli_notice().is_none());
    }

    /// **The CLI's wording, not the app's.** The app's notice is Markdown and
    /// talks about leaving form fields blank; on stderr that is `**not**` and
    /// advice about a form the reader is not looking at. This one names the
    /// secret and the way round it.
    #[test]
    fn the_cli_notice_names_the_secret_and_the_way_round_it() {
        let store = MemStore::unavailable();
        let mut c = conn(3);
        let notice = hydrate_connection(&mut c, &store, &[])
            .cli_notice()
            .expect("a locked keyring is reported");
        assert!(notice.contains("database password"), "{notice}");
        assert!(notice.contains("keyring locked"), "{notice}");
        assert!(notice.contains("--password-stdin"), "{notice}");
        assert!(!notice.contains("**"), "no Markdown on stderr: {notice}");
        assert!(!notice.contains("field"), "no form advice: {notice}");
    }

    /// Only an SSH secret unreadable: `--password-stdin` supplies the database
    /// password and cannot help, so it is not offered.
    #[test]
    fn the_cli_notice_does_not_offer_stdin_for_an_ssh_secret() {
        let store = MemStore::unavailable();
        let mut c = conn(3);
        c.ssh.enabled = true;
        let notice = hydrate_connection(&mut c, &store, &[SecretKind::DbPassword])
            .cli_notice()
            .expect("the SSH password is still unreadable");
        assert!(notice.contains("SSH password"), "{notice}");
        // A password login has no passphrase to be missing.
        assert!(!notice.contains("passphrase"), "{notice}");
        assert!(!notice.contains("--password-stdin"), "{notice}");
    }

    /// **A connection with no tunnel has no SSH secret to be unreadable.** It
    /// warned about two, and to retry a command that had succeeded.
    #[test]
    fn a_connection_with_no_tunnel_is_not_asked_for_ssh_secrets() {
        let store = MemStore::unavailable();
        let mut c = conn(3);
        assert!(!c.ssh.enabled);
        let got = hydrate_connection(&mut c, &store, &[SecretKind::DbPassword]);
        assert_eq!(got.cli_notice(), None);
    }

    #[test]
    fn the_unused_kinds_follow_the_ssh_auth_method() {
        use crate::connection::SshAuth;
        let mut c = conn(1);
        assert_eq!(
            unused_kinds(&c),
            vec![SecretKind::SshPassword, SecretKind::SshPassphrase]
        );
        c.ssh.enabled = true;
        c.ssh.auth = SshAuth::KeyPair;
        assert_eq!(unused_kinds(&c), vec![SecretKind::SshPassword]);
        c.ssh.auth = SshAuth::Password;
        assert_eq!(unused_kinds(&c), vec![SecretKind::SshPassphrase]);
        c.ssh.auth = SshAuth::Agent;
        assert_eq!(unused_kinds(&c).len(), 2);
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

    /// **The failure has to reach the user, and with the backend's own words.**
    /// `StoreError`'s doc says it carries the message for display, and the only
    /// site in the workspace that builds one from a real backend dropped it on a
    /// wildcard — so a locked keyring showed up as every password field blank,
    /// every connection failing with the server's own *Access denied*, and
    /// nothing anywhere naming the cause. `any_unreadable`'s own doc claimed the
    /// app surfaced this; it had no caller at all.
    #[test]
    fn an_unreadable_keyring_reports_why_and_says_the_secrets_are_still_there() {
        let store = MemStore::new();
        let mut c = conn(7);
        c.password = "s3cret".to_string();
        let file = ConnectionsFile {
            connections: vec![c],
            active: Some(7),
            highest_id: 0,
        };
        let saved = sanitize(&file, &store);
        assert_eq!(saved.connections[0].password, "");

        store.set_available(false);
        let mut reloaded = saved.clone();
        let hydration = hydrate_file(&mut reloaded, &store);

        assert!(hydration.any_unreadable());
        assert!(
            hydration.error.is_some(),
            "the backend's reason is what the user is told"
        );
        let notice = hydration.notice().expect("there is something to say");
        assert!(
            notice.contains("not** deleted"),
            "the half that stops the user retyping over an intact secret: {notice}"
        );
        assert!(
            notice.contains(&hydration.error.clone().unwrap()),
            "the backend's message reaches the sentence: {notice}"
        );
        // A store that answers everything has nothing to say.
        store.set_available(true);
        let mut fine = saved.clone();
        assert_eq!(hydrate_file(&mut fine, &store).notice(), None);
    }

    /// The write side of the same silence (A3-L5-03). The plaintext fallback is
    /// sanctioned; the user never being told is not — `connections.json` sits in
    /// the folder Settings offers an *Open folder* button for, next to the log a
    /// support request asks for.
    #[test]
    fn a_password_left_in_the_clear_is_reported_by_the_save_that_left_it() {
        let store = MemStore::new();
        store.set_available(false);
        let mut c = conn(7);
        c.password = "s3cret".to_string();
        let file = ConnectionsFile {
            connections: vec![c],
            active: Some(7),
            highest_id: 0,
        };
        let out = sanitize_file(&file, &store, &Hydration::default());
        assert_eq!(
            out.file.connections[0].password, "s3cret",
            "the fallback itself is unchanged — the credential is not lost"
        );
        assert_eq!(out.in_the_clear, vec![(7, SecretKind::DbPassword)]);
        let notice = out.notice().expect("there is something to say");
        assert!(notice.contains("plain text"), "{notice}");
        assert!(notice.contains("connections.json"), "{notice}");

        // And a working store says nothing.
        store.set_available(true);
        let out = sanitize_file(&file, &store, &Hydration::default());
        assert!(out.in_the_clear.is_empty());
        assert_eq!(out.notice(), None);
    }

    /// **A delete that quietly failed unclears a cleared password.**
    /// `sanitize_file`'s own doc promises that clearing a password can't be
    /// undone by a later hydrate, and that promise rested entirely on a call
    /// that returned `()`. The disk copy said empty, the keyring still held the
    /// value, and the next launch filled it back in.
    #[test]
    fn a_clear_that_the_keyring_refused_is_reported_and_retried() {
        let store = MemStore::new();
        let mut c = conn(7);
        c.password = "s3cret".to_string();
        let file = ConnectionsFile {
            connections: vec![c],
            active: Some(7),
            highest_id: 0,
        };
        let saved = sanitize(&file, &store);
        assert_eq!(store.stored("conn.7.password").as_deref(), Some("s3cret"));

        // Next launch: the load reads the password back, so the store is known
        // to hold one. Nothing is unreadable — this load *worked* — so a later
        // empty field really is a deliberate clear.
        let mut loaded = saved.clone();
        let hydration = hydrate_file(&mut loaded, &store);
        assert!(hydration.was_stored(7, SecretKind::DbPassword));
        assert!(!hydration.any_unreadable());

        // The user clears the field and the store then refuses the delete.
        store.set_available(false);
        let cleared = saved.clone(); // password already blank in the disk copy
        let out = sanitize_file(&cleared, &store, &hydration);
        assert_eq!(out.undeleted, vec![(7, SecretKind::DbPassword)]);
        assert!(
            !out.undeleted.contains(&(7, SecretKind::SshPassword)),
            "an entry that was never there is not a refused clear"
        );
        let notice = out.notice().expect("there is something to say");
        assert!(notice.contains("still in the OS keyring"), "{notice}");
        assert_eq!(
            store.stored("conn.7.password").as_deref(),
            Some("s3cret"),
            "and the entry really is still there, which is why it is said"
        );

        // The next save retries — the field is still empty and still readable —
        // and this one lands, so there is nothing left to report.
        store.set_available(true);
        let out = sanitize_file(&cleared, &store, &hydration);
        assert!(out.undeleted.is_empty());
        assert_eq!(out.notice(), None);
        assert_eq!(store.stored("conn.7.password"), None, "gone for good");
    }

    /// **The same refusal, for a secret this session *wrote* rather than read.**
    ///
    /// `Hydration::stored` was pushed to by a **load** and by nothing else, so a
    /// password typed into a connection that had none, saved successfully, and
    /// then cleared in the same session had `was_stored` answer `false` — and
    /// the refused delete went unreported. Disk says blank, the keyring still
    /// holds the value, and the next launch hydrates it straight back in: the
    /// connection goes on authenticating with the credential the user
    /// deliberately removed, which is the failure the whole mechanism exists to
    /// end.
    ///
    /// The narrowing the guard was written for — a machine with no keyring, where
    /// every empty field asks for a delete and every refusal is meaningless — is
    /// an argument about entries that were **never written**. This one was
    /// written, and the store said so.
    ///
    /// Asserted over the composition (`sanitize_file` → `Hydration` →
    /// `sanitize_file`), because either half alone is green.
    #[test]
    fn a_secret_typed_and_saved_this_session_is_a_stored_one() {
        let store = MemStore::new();
        // The load found nothing, so nothing is `stored` and nothing unreadable.
        let mut file = ConnectionsFile {
            connections: vec![conn(7)],
            active: Some(7),
            highest_id: 0,
        };
        let mut hydration = hydrate_file(&mut file, &store);
        assert!(!hydration.was_stored(7, SecretKind::DbPassword));

        // The user types a password and saves. The store takes it.
        file.connections[0].password = "s3cret".to_string();
        let out = sanitize_file(&file, &store, &hydration);
        assert_eq!(out.stored, vec![(7, SecretKind::DbPassword)]);
        assert_eq!(store.stored("conn.7.password").as_deref(), Some("s3cret"));
        // What the app does with it at the save site.
        hydration.absorb_stored(&out);
        hydration.resolve_against(&file);
        assert!(hydration.was_stored(7, SecretKind::DbPassword));

        // They clear it again; the keyring has meanwhile relocked.
        store.set_available(false);
        let cleared = out.file.clone();
        let out = sanitize_file(&cleared, &store, &hydration);
        assert_eq!(out.undeleted, vec![(7, SecretKind::DbPassword)]);
        assert!(out.notice().is_some(), "and the user is told");
    }

    /// A connection's id is reused, so a `forget` the keyring refused is worth a
    /// return value: the next connection to take that id would hydrate the
    /// deleted one's password.
    #[test]
    fn forget_says_whether_the_secrets_are_really_gone() {
        let store = MemStore::new();
        let mut c = conn(7);
        c.password = "s3cret".to_string();
        let file = ConnectionsFile {
            connections: vec![c],
            active: Some(7),
            highest_id: 0,
        };
        let _ = sanitize(&file, &store);
        store.set_available(false);
        assert!(!forget(7, &store));
        store.set_available(true);
        assert!(forget(7, &store));
        assert_eq!(store.stored("conn.7.password"), None);
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
            highest_id: 0,
        };

        store.set_available(false); // keyring locked at startup
        let hydration = hydrate_file(&mut file, &store);
        assert!(file.connections[0].password.is_empty(), "read failed");
        assert!(hydration.is_unreadable(7, SecretKind::DbPassword));

        store.set_available(true); // user unlocks it, then edits something else
        let disk = sanitize_file(&file, &store, &hydration).file;

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
            unreadable: vec![(7, SecretKind::DbPassword), (7, SecretKind::SshPassword)],
            ..Hydration::default()
        };
        let mut c = conn(7);
        c.password = "typed-it-in".to_string(); // ssh password still empty
        let file = ConnectionsFile {
            connections: vec![c],
            active: Some(7),
            highest_id: 0,
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
            unreadable: vec![(7, SecretKind::DbPassword)],
            ..Hydration::default()
        };
        let empty = ConnectionsFile {
            connections: vec![],
            active: None,
            highest_id: 0,
        };
        hydration.resolve_against(&empty);
        assert!(!hydration.any_unreadable(), "gone with the connection");
    }

    #[test]
    fn forget_clears_protection_so_a_reused_id_cannot_inherit_it() {
        let mut hydration = Hydration {
            unreadable: vec![(7, SecretKind::DbPassword), (8, SecretKind::DbPassword)],
            ..Hydration::default()
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
            highest_id: 0,
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
            highest_id: 0,
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
            highest_id: 0,
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
            highest_id: 0,
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
            highest_id: 0,
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
            highest_id: 0,
        };
        assert!(hydrate_file(&mut file, &store).needs_resave);
    }

    #[test]
    fn forget_removes_all_secrets_for_connection() {
        let store = MemStore::seeded(&[
            ("conn.7.password", "a"),
            ("conn.7.ssh_password", "b"),
            ("conn.7.ssh_passphrase", "c"),
            ("conn.7.tls_key_passphrase", "d"),
            ("conn.8.password", "keep"),
        ]);
        forget(7, &store);
        assert_eq!(store.stored("conn.7.password"), None);
        assert_eq!(store.stored("conn.7.ssh_password"), None);
        assert_eq!(store.stored("conn.7.ssh_passphrase"), None);
        assert_eq!(store.stored("conn.7.tls_key_passphrase"), None);
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
            fn delete(&self, account: &str) -> bool {
                self.0.borrow_mut().remove(account);
                true
            }
        }
        let store = PartialStore(RefCell::new(HashMap::new()));
        let mut c = conn(7);
        c.password = "db".to_string();
        c.ssh.password = "ssh".to_string();
        let file = ConnectionsFile {
            connections: vec![c],
            active: Some(7),
            highest_id: 0,
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
