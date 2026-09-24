//! PostgreSQL's SCRAM-SHA-256 password verifier, computed here so a role's
//! password never reaches the server as text.
//!
//! `ALTER ROLE "x" PASSWORD 'hunter2'` is logged verbatim by PostgreSQL under
//! `log_statement = 'ddl'`, pgaudit, or any `log_min_duration_statement` the
//! statement happens to exceed, and it sits in `pg_stat_activity.query` while
//! it runs. PostgreSQL 10 and later store a password that is *already* a
//! verifier exactly as given, whatever `password_encryption` says — which is
//! what `psql`'s `\password` relies on — so sending
//! `PASSWORD 'SCRAM-SHA-256$4096:…'` sets the same password with nothing
//! recoverable in the statement. A `pg_hba` line reading `md5` authenticates a
//! SCRAM-stored password by switching to SCRAM on its own.
//!
//! The format is PostgreSQL's (`scram_build_secret`):
//! `SCRAM-SHA-256$<iterations>:<salt>$<StoredKey>:<ServerKey>`, each part
//! standard base64, where (RFC 5802 / RFC 7677)
//!
//! - `SaltedPassword = PBKDF2-HMAC-SHA-256(password, salt, iterations)`
//! - `StoredKey = SHA-256(HMAC(SaltedPassword, "Client Key"))`
//! - `ServerKey = HMAC(SaltedPassword, "Server Key")`
//!
//! **Only printable ASCII is hashed.** The client normalises the password it
//! types at login with SASLprep before deriving anything. SASLprep is the
//! identity on 0x20–0x7E; outside it, it *may* rewrite the password — NFKC
//! (`ｆｕｌｌ` becomes `full`), non-ASCII spaces mapped to U+0020, some
//! characters deleted — while an already-normalised password such as `pässword`
//! passes through unchanged, and one holding a prohibited character is used as
//! raw bytes. This module does not implement SASLprep, so it cannot tell which
//! case a password is in, and it declines rather than risk a verifier the login
//! would not match: [`verifier`] answers `None` there and the caller sends the
//! password as typed, which is what the app did before this module existed. The
//! decline is a missing implementation, not an impossibility — and until it is
//! filled, such a password still reaches the server as text.
//!
//! The salt is an argument, never generated here: the account form stamps it
//! into the draft once, so the preview and the Apply run one identical
//! statement and `ChangeSet::emit` stays a pure function.

/// A SCRAM salt. PostgreSQL's own default length.
pub type Salt = [u8; 16];

/// PostgreSQL's default `scram_iterations`.
pub const ITERATIONS: u32 = 4096;

/// The verifier for `password` under `salt`, or `None` for a password that is
/// empty or not wholly printable ASCII — see the module doc for why those are
/// sent as typed instead.
pub fn verifier(password: &str, salt: &Salt) -> Option<String> {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;

    if password.is_empty() || !password.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        return None;
    }
    let salted = salted_password(password.as_bytes(), salt, ITERATIONS);
    let stored_key = sha256(&hmac(&salted, b"Client Key"));
    let server_key = hmac(&salted, b"Server Key");
    Some(format!(
        "SCRAM-SHA-256${ITERATIONS}:{}${}:{}",
        B64.encode(salt),
        B64.encode(stored_key),
        B64.encode(server_key)
    ))
}

/// PBKDF2-HMAC-SHA-256 (RFC 8018) for the single 32-byte block SCRAM uses —
/// the output is exactly one HMAC wide, so there is one block, index 1.
fn salted_password(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut first = Vec::with_capacity(salt.len() + 4);
    first.extend_from_slice(salt);
    first.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac(password, &first);
    let mut out = u;
    for _ in 1..iterations {
        u = hmac(password, &u);
        for (o, b) in out.iter_mut().zip(u) {
            *o ^= b;
        }
    }
    out
}

fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    use hmac::{KeyInit, Mac};
    // HMAC takes a key of any length, so this cannot fail.
    let mut mac =
        hmac::Hmac::<sha2::Sha256>::new_from_slice(key).expect("HMAC accepts a key of any length");
    mac.update(msg);
    mac.finalize().into_bytes().into()
}

fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(data).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;

    fn salt_of(b64: &str) -> Salt {
        B64.decode(b64).unwrap().try_into().unwrap()
    }

    /// **An oracle from PostgreSQL itself**: `SET password_encryption =
    /// 'scram-sha-256'; CREATE ROLE … PASSWORD 'correct horse ~ battery!'`,
    /// then `rolpassword` read back from `pg_authid`, on PostgreSQL 16.14. Same
    /// password, same salt, so the verifier has to be byte-for-byte the one the
    /// server built — the space, `~` and `!` sit on the edges of the printable
    /// range this module accepts.
    #[test]
    fn matches_a_verifier_postgresql_built() {
        let salt = salt_of("OwYoELizRSaxs1J6Wf1JoA==");
        assert_eq!(
            verifier("correct horse ~ battery!", &salt).as_deref(),
            Some(
                "SCRAM-SHA-256$4096:OwYoELizRSaxs1J6Wf1JoA==$\
                 xYGh+XvNiSb5SjECtzg1qcdjNxKitIfPaHYT28bIOTA=:\
                 mGNT0iEbx0dueCyHnolH79rr4hnYlsRM/QT8saEjywU="
            )
        );
    }

    /// RFC 7677 §3's exchange, user `user`, password `pencil`. The RFC gives the
    /// client proof and server signature rather than the stored keys, so this
    /// rebuilds both from the verifier's own `StoredKey`/`ServerKey` and the
    /// exchange's AuthMessage — which is what a server does at login.
    #[test]
    fn logs_in_to_the_rfc_7677_exchange() {
        let salt = salt_of("W22ZaJ0SNY7soEsUEjb6gQ==");
        let v = verifier("pencil", &salt).unwrap();
        let (_, keys) = v.split_once('$').unwrap().1.split_once('$').unwrap();
        let (stored_b64, server_b64) = keys.split_once(':').unwrap();
        let stored = B64.decode(stored_b64).unwrap();
        let server_key = B64.decode(server_b64).unwrap();

        let auth = "n=user,r=rOprNGfwEbeRWgbNEkqO,\
                    r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,\
                    s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096,\
                    c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0";

        // The server's check of the client's proof: ClientKey = proof XOR
        // HMAC(StoredKey, AuthMessage), and SHA-256(ClientKey) must be StoredKey.
        let proof = B64
            .decode("dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=")
            .unwrap();
        let client_sig = hmac(&stored, auth.as_bytes());
        let client_key: Vec<u8> = proof.iter().zip(client_sig).map(|(p, s)| p ^ s).collect();
        assert_eq!(sha256(&client_key).to_vec(), stored);

        // And the signature the server sends back.
        assert_eq!(
            B64.encode(hmac(&server_key, auth.as_bytes())),
            "6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4="
        );
    }

    #[test]
    fn the_verifier_has_postgresqls_shape() {
        let v = verifier("hunter2", &[7; 16]).unwrap();
        let rest = v.strip_prefix("SCRAM-SHA-256$4096:").unwrap();
        let (salt, keys) = rest.split_once('$').unwrap();
        let (stored, server) = keys.split_once(':').unwrap();
        assert_eq!(B64.decode(salt).unwrap(), vec![7; 16]);
        assert_eq!(B64.decode(stored).unwrap().len(), 32);
        assert_eq!(B64.decode(server).unwrap().len(), 32);
    }

    #[test]
    fn the_same_password_and_salt_give_the_same_verifier() {
        // What lets the preview and the Apply run one identical statement.
        assert_eq!(verifier("hunter2", &[1; 16]), verifier("hunter2", &[1; 16]));
    }

    #[test]
    fn a_different_salt_gives_a_different_verifier() {
        assert_ne!(verifier("hunter2", &[1; 16]), verifier("hunter2", &[2; 16]));
    }

    #[test]
    fn a_different_password_gives_a_different_verifier() {
        assert_ne!(verifier("hunter2", &[1; 16]), verifier("hunter3", &[1; 16]));
    }

    #[test]
    fn the_whole_printable_range_is_hashed() {
        let every: String = (0x20u8..=0x7e).map(char::from).collect();
        assert!(verifier(&every, &[0; 16]).is_some());
        assert!(verifier("it's \\ a \"test\"", &[0; 16]).is_some());
    }

    #[test]
    fn outside_printable_ascii_is_not_hashed() {
        // Not because each would be rewritten — `pässword` and `日本` pass
        // SASLprep unchanged, and the control characters are prohibited so the
        // raw bytes are used — but because telling those apart from `ｆｕｌｌ`
        // (NFKC to `full`) needs a SASLprep this module does not have.
        for pw in [
            "pässword",
            "tab\there",
            "new\nline",
            "del\u{7f}",
            "ｆｕｌｌ",
            "日本",
        ] {
            assert_eq!(verifier(pw, &[0; 16]), None, "{pw:?}");
        }
    }

    #[test]
    fn an_empty_password_is_not_hashed() {
        assert_eq!(verifier("", &[0; 16]), None);
    }
}
