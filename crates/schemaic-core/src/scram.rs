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
//! **A password is hashed exactly when SASLprep cannot rewrite it.** The client
//! normalises the password it types at login with SASLprep (RFC 4013) before
//! deriving anything, and the server does the same to a password handed to it
//! as text: non-ASCII spaces mapped to U+0020, a few characters mapped to
//! nothing, then NFKC — and if the result holds a prohibited character (a
//! control character, a lone bidi mix, an unassigned code point) every
//! implementation falls back to the raw bytes. So a password that the mapping
//! and NFKC leave **unchanged** — all of printable ASCII, and `pässword`,
//! `日本`, even `tab\there` — is hashed as the bytes it already is by every
//! path, whether SASLprep accepts it or falls back ([`saslprep_fixed`]). One it
//! would rewrite (`ｆｕｌｌ` becomes `full`, a no-break space becomes a space)
//! is declined: [`verifier`] answers `None` and the caller sends it as typed,
//! because hashing the rewritten form would rest on this crate's Unicode tables
//! agreeing with every client's, and a disagreement is a lockout.
//!
//! The salt is an argument, never generated here: the account form stamps it
//! into the draft once, so the preview and the Apply run one identical
//! statement and `ChangeSet::emit` stays a pure function.

/// A SCRAM salt. PostgreSQL's own default length.
pub type Salt = [u8; 16];

/// PostgreSQL's default `scram_iterations`.
pub const ITERATIONS: u32 = 4096;

/// The verifier for `password` under `salt`, or `None` for a password that is
/// empty or that SASLprep would rewrite — see the module doc for why those are
/// sent as typed instead.
pub fn verifier(password: &str, salt: &Salt) -> Option<String> {
    verifier_with(password, salt, ITERATIONS)
}

/// [`verifier`] at `iterations` — the server's own `scram_iterations`, which an
/// administrator can raise on PostgreSQL 16 and later. A verifier built at the
/// default under a hardened setting silently weakens every password it sets.
/// `0` is not an iteration count and answers `None`.
pub fn verifier_with(password: &str, salt: &Salt, iterations: u32) -> Option<String> {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;

    if iterations == 0 || password.is_empty() || !saslprep_fixed(password) {
        return None;
    }
    let salted = salted_password(password.as_bytes(), salt, iterations);
    let stored_key = sha256(&hmac(&salted, b"Client Key"));
    let server_key = hmac(&salted, b"Server Key");
    Some(format!(
        "SCRAM-SHA-256${iterations}:{}${}:{}",
        B64.encode(salt),
        B64.encode(stored_key),
        B64.encode(server_key)
    ))
}

/// Does SASLprep leave `password` exactly as it is — its mapping step (RFC 4013
/// §2.1) and NFKC (§2.2) the identity?
///
/// That is the whole question, because what follows those two steps only
/// *checks*: a prohibited or unassigned character makes SASLprep fail, and on
/// failure libpq, `postgres-protocol` and the server's own `pg_saslprep` all
/// use the raw password. So for a fixed password every path hashes the same
/// bytes, and no path's opinion of which characters are prohibited matters.
pub fn saslprep_fixed(password: &str) -> bool {
    use stringprep::tables::{commonly_mapped_to_nothing, non_ascii_space_character};
    use unicode_normalization::UnicodeNormalization;
    // The fast path libpq and the server take: pure ASCII is used unchanged.
    if password.is_ascii() {
        return true;
    }
    password
        .chars()
        .map(|c| if non_ascii_space_character(c) { ' ' } else { c })
        .filter(|&c| !commonly_mapped_to_nothing(c))
        .nfkc()
        .eq(password.chars())
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

    /// A password SASLprep leaves alone is hashed, non-ASCII or not: an
    /// already-composed `pässword` and `日本` pass it unchanged, and a control
    /// character makes it fail, where every implementation falls back to the
    /// raw bytes — the same bytes either way.
    #[test]
    fn a_password_saslprep_leaves_alone_is_hashed() {
        for pw in [
            "pässword",
            "日本",
            "Ελληνικά",
            "tab\there",
            "new\nline",
            "del\u{7f}",
            // Prohibited in stored strings (unassigned), and unchanged by NFKC.
            "x\u{0378}",
        ] {
            assert!(saslprep_fixed(pw), "{pw:?}");
            assert!(verifier(pw, &[0; 16]).is_some(), "{pw:?}");
        }
    }

    /// One SASLprep would rewrite is declined, so the login's rewritten form
    /// never meets a verifier of the raw one: NFKC folds full-width letters and
    /// ligatures, a decomposed `ä` composes, a no-break space becomes a space,
    /// and a soft hyphen is mapped to nothing.
    #[test]
    fn a_password_saslprep_would_rewrite_is_not_hashed() {
        for pw in [
            "ｆｕｌｌ",
            "ﬁne",
            "pa\u{0308}ssword",
            "no\u{a0}break",
            "soft\u{ad}hyphen",
        ] {
            assert!(!saslprep_fixed(pw), "{pw:?}");
            assert_eq!(verifier(pw, &[0; 16]), None, "{pw:?}");
        }
    }

    /// Where SASLprep succeeds, "fixed" is exactly "SASLprep returns it
    /// unchanged" — checked against the `stringprep` crate `postgres-protocol`
    /// logs in with.
    #[test]
    fn fixed_agrees_with_the_clients_saslprep() {
        for pw in ["pässword", "日本", "ｆｕｌｌ", "no\u{a0}break", "plain"] {
            if let Ok(prepped) = stringprep::saslprep(pw) {
                assert_eq!(saslprep_fixed(pw), prepped == pw, "{pw:?}");
            }
        }
    }

    #[test]
    fn an_empty_password_is_not_hashed() {
        assert_eq!(verifier("", &[0; 16]), None);
    }
}
