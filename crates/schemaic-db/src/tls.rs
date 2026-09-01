//! Turning a [`TlsPlan`] into the two networked drivers' TLS configuration.
//!
//! The plan is made once, in `schemaic_core::connection::Tls::plan` — five
//! `sslmode` levels collapsed into the four decisions a handshake is actually
//! made of. This module only *translates*, because the drivers spell the same
//! decisions very differently: `mysql_async` takes an [`SslOpts`] carrying two
//! `danger_*` toggles, while `tokio_postgres` takes an `SslMode` for the
//! negotiation and leaves every verification question to the rustls
//! [`ClientConfig`] behind it. Deriving each from the five modes separately is
//! how `verify-ca` ends up meaning one thing on MySQL and another on Postgres.
//!
//! Both engines run on one rustls, so a certificate accepted by one is accepted
//! by the other — see the workspace `Cargo.toml` for why the crypto provider
//! must stay single. Keeping that true takes work in one place: `mysql_async`
//! builds its own root store and adds its compiled-in roots unless told not to,
//! so [`mysql_ssl_opts`] switches those off and hands it the same anchors
//! [`client_config`] gives Postgres. Without that, naming a private CA would
//! *narrow* the trust on one engine and *widen* it on the other.
//!
//! **The anchor count is smaller than the certificate manager's, and that is
//! correct.** Trust anchors come from the OS store ([`default_roots`]), and
//! `rustls-native-certs` returns only the roots valid for *server*
//! authentication — a Windows box measured here holds 69 roots and yields 37,
//! the rest being code-signing and timestamping CAs that have no business
//! vouching for a database server. None were rejected by [`usable_roots`] on
//! that machine, and `ISRG Root X1` — what Let's Encrypt, and so most hosted
//! providers, chain to — was among the 37. So a low number is not itself a
//! symptom; check whether the *specific* root is present before treating it as
//! one.
//!
//! Windows does also populate its root program lazily, fetching roots on demand
//! through a component we do not go through, so a certificate every browser on
//! the box accepts can still fail `verify-ca` here with `UnknownIssuer`. That is
//! a real failure mode and a different one from the count above. Naming the
//! provider's CA file is the immediate answer either way.
//!
//! **The verifier ladder is the load-bearing part.** rustls verifies the chain
//! and the name together inside `verify_server_cert`, so `verify-ca` — trust the
//! chain, ignore the name — cannot be expressed by configuration. It is
//! [`NameAgnosticVerifier`]: the real webpki verifier, with the *one* error it is
//! allowed to forgive named explicitly. Forgiving it by accepting everything
//! would silently turn `verify-ca` into `require`.

use std::sync::Arc;

use mysql_async::{ClientIdentity, SslOpts};
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use schemaic_core::connection::TlsPlan;
use tokio_postgres::config::SslMode as PgSslMode;
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::DbError;

/// `mysql_async`'s TLS options for this plan.
///
/// The two `danger_*` toggles are the same two booleans the plan carries, which
/// is the whole reason the plan carries booleans rather than a mode.
pub(crate) fn mysql_ssl_opts(plan: &TlsPlan) -> SslOpts {
    let mut opts = SslOpts::default()
        .with_danger_accept_invalid_certs(plan.accept_invalid_certs)
        .with_danger_skip_domain_validation(plan.skip_hostname_check);

    if let Some(name) = &plan.hostname_override {
        opts = opts.with_danger_tls_hostname_override(Some(name.clone()));
    }

    // Anchors, but **only for a mode that has something to verify**. Two reasons
    // it is guarded rather than unconditional, and neither is visible from the
    // handshake: `mysql_async` builds a `WebPkiServerVerifier` even when it will
    // not consult it, and that builder fails on an empty root store — so
    // switching its built-in roots off for `prefer` would break the default mode
    // of every connection. And copying a whole trust store into the options is
    // real work on a path that runs once per operation.
    if !plan.accept_invalid_certs {
        match &plan.root_ca {
            // Naming a CA means "trust exactly this". Without disabling the
            // driver's own roots it would mean "this *as well as* every public
            // CA", which narrows the trust on Postgres and widens it here.
            Some(ca) => {
                opts = opts
                    .with_disable_built_in_roots(true)
                    .with_root_certs(vec![std::path::PathBuf::from(ca).into()]);
            }
            // No file: the same anchors `client_config` gives Postgres. When we
            // fell back to the bundled set there is nothing to copy — the
            // driver's own built-ins are that set already.
            None => {
                let roots = default_roots();
                if roots.source == RootSource::Os {
                    // As raw DER bytes: the driver's loader tries PEM first and
                    // falls back to reading a whole non-empty buffer as one
                    // certificate, which is what these are. `PathOrBuf` is not
                    // nameable from outside the crate, hence the `into`.
                    opts = opts.with_disable_built_in_roots(true).with_root_certs(
                        roots
                            .certs
                            .iter()
                            .map(|c| c.as_ref().to_vec().into())
                            .collect(),
                    );
                }
            }
        }
    }
    if let Some((cert, key)) = &plan.client_identity {
        opts = opts.with_client_identity(Some(ClientIdentity::new(
            std::path::PathBuf::from(cert).into(),
            std::path::PathBuf::from(key).into(),
        )));
    }
    opts
}

/// Check the files a plan names, before a driver that would report them badly
/// gets to try.
///
/// `mysql_async` hands the paths to rustls itself and surfaces whatever comes
/// back, so a mistyped CA path arrives as `Input/output error: Input/output
/// error: The system cannot find the path specified. (os error 3)` — no file
/// name, doubled, and indistinguishable from a network fault. A wrong path is
/// the single likeliest way to misconfigure this feature, so it is worth one
/// `read` to be able to say which one.
///
/// The Postgres side needs no such call: it loads the same files itself, through
/// [`client_config`], and already fails with the path in the message.
pub(crate) fn preflight(plan: &TlsPlan) -> Result<(), DbError> {
    if let Some(ca) = &plan.root_ca {
        file_root_store(ca)?;
    }
    if let Some((cert, key)) = &plan.client_identity {
        read_certs(cert)?;
        read_key(key)?;
    }
    Ok(())
}

/// How `tokio_postgres` should negotiate — the *transport* half only.
///
/// Postgres has three levels where we have five, and the missing two are not a
/// gap: `verify-ca` and `verify-full` negotiate exactly as `require` does and
/// differ only in what the verifier behind them accepts. Mapping them to
/// `Prefer` would let a verifying connection silently end up in plaintext, which
/// is the failure this whole setting exists to prevent.
pub(crate) fn pg_ssl_mode(plan: Option<&TlsPlan>) -> PgSslMode {
    match plan {
        None => PgSslMode::Disable,
        Some(p) if p.fallback_to_plaintext => PgSslMode::Prefer,
        Some(_) => PgSslMode::Require,
    }
}

/// A rustls connector honouring this plan's verification decisions.
pub(crate) fn pg_connector(plan: &TlsPlan) -> Result<MakeRustlsConnect, DbError> {
    Ok(MakeRustlsConnect::new(client_config(plan)?))
}

/// The rustls client configuration for a plan: which roots, which verifier, and
/// whether we present an identity of our own.
fn client_config(plan: &TlsPlan) -> Result<ClientConfig, DbError> {
    // Named explicitly rather than taken from crate features. The workspace
    // keeps exactly one provider in the tree, and saying so here means a second
    // one arriving through somebody's dependency is a compile-time choice we
    // already made instead of a panic on the first handshake.
    let provider = Arc::new(rustls::crypto::ring::default_provider());

    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| DbError::Connect(format!("TLS setup failed: {e}")))?;

    let verifier: Arc<dyn ServerCertVerifier> = if plan.accept_invalid_certs {
        Arc::new(NoVerification(provider))
    } else {
        let roots = match plan.root_ca.as_deref() {
            Some(path) => Arc::new(file_root_store(path)?),
            None => default_roots().store.clone(),
        };
        let webpki = WebPkiServerVerifier::builder_with_provider(roots, provider)
            .build()
            .map_err(|e| DbError::Connect(format!("TLS trust store is unusable: {e}")))?;
        match (plan.skip_hostname_check, &plan.hostname_override) {
            (true, _) => Arc::new(NameAgnosticVerifier(webpki)),
            (false, Some(name)) => Arc::new(FixedNameVerifier {
                inner: webpki,
                name: ServerName::try_from(name.clone()).map_err(|e| {
                    DbError::Connect(format!("{name} is not a name a certificate can carry: {e}"))
                })?,
            }),
            (false, None) => webpki,
        }
    };

    let cfg = builder
        .dangerous()
        .with_custom_certificate_verifier(verifier);

    match &plan.client_identity {
        None => Ok(cfg.with_no_client_auth()),
        Some((cert, key)) => {
            let chain = read_certs(cert)?;
            let key = read_key(key)?;
            cfg.with_client_auth_cert(chain, key)
                .map_err(|e| DbError::Connect(format!("client certificate rejected: {e}")))
        }
    }
}

/// Which anchors an *empty* CA path resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RootSource {
    /// The operating system's own certificate store.
    Os,
    /// `webpki-roots` — Mozilla's set, compiled in.
    Bundled,
}

/// The bundled set is a **fallback**, never a supplement.
///
/// Unioning the two would be the tempting shape and is the wrong one: reading
/// the OS store is worth doing precisely because an administrator's decisions
/// there are binding, and a CA they *removed* has to stop being trusted. A
/// compiled-in set quietly underneath would undo exactly that.
///
/// The fallback is for a machine with no store to read — a scratch container
/// with no `ca-certificates` package. Trusting nothing there would be
/// defensible and would also make the app useless with an error naming no
/// cause.
fn root_source(os_cert_count: usize) -> RootSource {
    match os_cert_count {
        0 => RootSource::Bundled,
        _ => RootSource::Os,
    }
}

/// Trust anchors, and the certificates they were built from.
///
/// Both, because the two drivers want different things from the same decision:
/// Postgres verifies against a [`RootCertStore`] we hand rustls directly, while
/// `mysql_async` builds its own store internally and will only take certificates
/// as bytes. Deriving them separately is how the engines come to disagree about
/// what is trusted.
struct DefaultRoots {
    store: Arc<RootCertStore>,
    certs: Vec<CertificateDer<'static>>,
    source: RootSource,
}

/// Filter `certs` down to the ones rustls will accept as trust anchors,
/// returning the store and the certificates that actually went into it.
///
/// One at a time rather than in bulk, because a real trust store contains
/// entries rustls refuses — anchors a platform accumulated over a decade — and
/// one of them must not be the difference between a working machine and one that
/// can open no verified connection at all.
fn usable_roots(
    certs: Vec<CertificateDer<'static>>,
) -> (RootCertStore, Vec<CertificateDer<'static>>) {
    let mut store = RootCertStore::empty();
    let kept = certs
        .into_iter()
        .filter(|cert| store.add(cert.clone()).is_ok())
        .collect();
    (store, kept)
}

/// The anchors an empty CA path verifies against — the OS store, read **once**.
///
/// Once because of the one-connection-per-operation invariant: this is on the
/// path of every query, schema refresh and health poll, and enumerating the
/// Windows certificate store or walking `/etc/ssl/certs` per connection would be
/// a cost paid thousands of times for an answer that does not change while the
/// app is running. The trade is that a certificate installed *during* a session
/// needs a restart, which is the same deal every other TLS client makes.
fn default_roots() -> &'static DefaultRoots {
    static ROOTS: std::sync::OnceLock<DefaultRoots> = std::sync::OnceLock::new();
    ROOTS.get_or_init(|| {
        let loaded = rustls_native_certs::load_native_certs();
        for err in &loaded.errors {
            // Not fatal on its own: a store can be partly readable, and the
            // count below is what decides whether we got anything usable.
            tracing::debug!("reading the OS certificate store: {err}");
        }

        if root_source(loaded.certs.len()) == RootSource::Os {
            let (store, certs) = usable_roots(loaded.certs);
            // An OS store that yielded certificates rustls can do nothing with
            // still leaves us with no anchors, and `mysql_async` cannot even
            // *build* a verifier from an empty store — so the fallback is asked
            // about the usable count, not the loaded one.
            if !store.is_empty() {
                tracing::debug!("{} trust anchors from the OS store", certs.len());
                return DefaultRoots {
                    store: Arc::new(store),
                    certs,
                    source: RootSource::Os,
                };
            }
            tracing::warn!("the OS certificate store held nothing usable; using bundled roots");
        }

        // `webpki_roots` hands out `TrustAnchor`s — a subject and a public key,
        // not certificates — so there is nothing here to give `mysql_async` as
        // bytes. That is exactly why this case leaves the driver's own roots
        // switched **on**: its built-in set *is* this set, so both engines still
        // verify against the same anchors without anything being copied.
        //
        // **Which is true only because the workspace and the driver take the
        // same `webpki-roots` major.** They did not: the workspace held 0.26
        // and `mysql_async` 1.0, so this arm quietly verified PostgreSQL
        // against one Mozilla snapshot and MySQL against another, under a
        // module doc promising the opposite. `cargo deny`'s `multiple-versions`
        // is set to `warn`, so it reports a second copy rather than failing the
        // build — this comment is the reason to act on that warning when it
        // names `webpki-roots`.
        let mut bundled = RootCertStore::empty();
        bundled.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        DefaultRoots {
            store: Arc::new(bundled),
            certs: Vec::new(),
            source: RootSource::Bundled,
        }
    })
}

/// Trust anchors from a CA file the user named — **and nothing else**.
///
/// "Trust exactly this" is what the field is for, and what libpq's
/// `sslrootcert` means: naming a private CA narrows the trust rather than adding
/// to it.
fn file_root_store(path: &str) -> Result<RootCertStore, DbError> {
    let certs = read_certs(path)?;
    if certs.is_empty() {
        return Err(DbError::Connect(format!(
            "CA file {path} contains no certificates"
        )));
    }
    let count = certs.len();
    let (store, kept) = usable_roots(certs);
    if kept.len() != count {
        return Err(DbError::Connect(format!(
            "CA file {path} contains a certificate that cannot be used as a trust anchor"
        )));
    }
    Ok(store)
}

/// PEM parsing goes through `rustls-pki-types`' own `PemObject`, not the
/// `rustls-pemfile` crate — which is unmaintained (RUSTSEC-2025-0134) precisely
/// because this is where it moved.
///
/// The file is read here rather than through `pem_file_iter` so the error can
/// name the path: "cannot read certificate file …" is the message for the most
/// common way to misconfigure this, and the library's own is not.
/// **A file with no `CERTIFICATE` section is an error, not an empty list.**
/// `pem_slice_iter` yields nothing for a DER `.crt` — which is what Windows'
/// *Export certificate* writes by default — so `Ok(vec![])` came back and
/// `preflight` passed it, defeating the one thing preflight is for. The engines
/// then disagreed about the same file: PostgreSQL failed with `client
/// certificate rejected` naming nothing, and MySQL **succeeded with the
/// identity presented**, because `mysql_async` decides PEM-vs-DER by asking
/// whether the bytes are UTF-8 and hands the raw file to rustls when they are
/// not. `file_root_store` twenty lines above already had this check; the client
/// certificate did not.
fn read_certs(path: &str) -> Result<Vec<CertificateDer<'static>>, DbError> {
    let pem = std::fs::read(path)
        .map_err(|e| DbError::Connect(format!("cannot read certificate file {path}: {e}")))?;
    parse_certs(path, &pem)
}

/// [`read_certs`] without the read, so the decision above can be asserted
/// without a file — the suite's no-filesystem rule, and the reason the
/// emptiness check was untestable where it was missing.
fn parse_certs(path: &str, pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, DbError> {
    let certs = CertificateDer::pem_slice_iter(pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| DbError::Connect(format!("{path} is not a PEM certificate file: {e}")))?;
    if certs.is_empty() {
        return Err(DbError::Connect(format!(
            "{path} holds no PEM certificate — a DER file has to be converted first \
             (openssl x509 -inform der -in {path} -out cert.pem)"
        )));
    }
    Ok(certs)
}

fn read_key(path: &str) -> Result<PrivateKeyDer<'static>, DbError> {
    let pem = std::fs::read(path)
        .map_err(|e| DbError::Connect(format!("cannot read key file {path}: {e}")))?;
    parse_key(path, &pem)
}

/// [`read_key`] without the read, so the refusal below can be asserted without a
/// file.
///
/// **An encrypted key is named, not blamed on the file.** Schemaic does not
/// decrypt one: the form used to collect a passphrase, store it in the keyring,
/// and hand it to nobody — `TlsPlan` carries no passphrase — so the connect
/// failed with `"<path> is not a PEM private key"`, which sends the user to
/// check a file that is perfectly good. The row is gone (see
/// `schemaic_core::connection::Tls`) and this says what to do instead.
fn parse_key(path: &str, pem: &[u8]) -> Result<PrivateKeyDer<'static>, DbError> {
    if let Ok(text) = std::str::from_utf8(pem)
        && text.contains("ENCRYPTED PRIVATE KEY")
    {
        return Err(DbError::Connect(format!(
            "{path} is a passphrase-protected private key, which Schemaic cannot open. \
             Decrypt it first: openssl pkcs8 -in {path} -out client-key.pem"
        )));
    }
    PrivateKeyDer::from_pem_slice(pem)
        .map_err(|e| DbError::Connect(format!("{path} is not a PEM private key: {e}")))
}

/// Verifies nothing at all — for `prefer` and `require`, which encrypt without
/// checking who they are encrypting *to*.
///
/// The signature checks still run against whatever key the peer presented; what
/// is skipped is any question of whether that key belongs to anyone we trust.
#[derive(Debug)]
struct NoVerification(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for NoVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// `verify-ca`: the real webpki verifier, forgiving exactly one error.
///
/// rustls checks the chain and the host name in the same call, so this mode
/// cannot be configured — only wrapped. It matters that the wrapper names
/// [`rustls::CertificateError::NotValidForName`] instead of falling back to
/// accepting everything on any failure: an expired certificate, an unknown CA
/// and a revoked one must all still be refused here, and a `verify-ca` that
/// swallowed them would be `require` wearing a stronger label.
#[derive(Debug)]
struct NameAgnosticVerifier(Arc<WebPkiServerVerifier>);

impl ServerCertVerifier for NameAgnosticVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self
            .0
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
        {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::NotValidForName
                | rustls::CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

/// `verify-full` through an SSH tunnel: the real webpki verifier, asked about
/// the host the user configured rather than the `127.0.0.1` we are dialling.
///
/// Substituting the name rather than skipping the check is the point. Falling
/// back to [`NameAgnosticVerifier`] whenever a tunnel is involved would make
/// `verify-full` mean `verify-ca` for every tunnelled connection — silently, and
/// exactly for the users who took the most care.
///
/// The name on the wire (SNI) is still the tunnel endpoint, so a server that
/// *routes* by SNI — Neon, PlanetScale — cannot be reached through a tunnel and
/// verified at the same time. Nothing here can fix that: the tunnel is the
/// thing hiding the name from the server.
#[derive(Debug)]
struct FixedNameVerifier {
    inner: Arc<WebPkiServerVerifier>,
    name: ServerName<'static>,
}

impl ServerCertVerifier for FixedNameVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        self.inner
            .verify_server_cert(end_entity, intermediates, &self.name, ocsp_response, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemaic_core::connection::{SslMode, Tls};

    fn plan_for(mode: SslMode) -> TlsPlan {
        Tls {
            mode,
            ..Tls::default()
        }
        .plan()
        .expect("mode handshakes")
    }

    /// The mapping that decides whether a verifying connection can silently end
    /// up unencrypted. `Prefer` is the only mode allowed to fall back, so it is
    /// the only one that may map to Postgres' `Prefer`.
    #[test]
    fn only_prefer_maps_to_a_postgres_mode_that_can_fall_back() {
        assert_eq!(pg_ssl_mode(None), PgSslMode::Disable);
        assert_eq!(
            pg_ssl_mode(Some(&plan_for(SslMode::Prefer))),
            PgSslMode::Prefer
        );
        for m in [SslMode::Require, SslMode::VerifyCa, SslMode::VerifyFull] {
            assert_eq!(
                pg_ssl_mode(Some(&plan_for(m))),
                PgSslMode::Require,
                "{m:?} must not be allowed to fall back"
            );
        }
    }

    /// A configuration is built for every mode without a server, so a mode that
    /// cannot even be *configured* fails here rather than at a user's connect.
    #[test]
    fn every_mode_builds_a_client_config() {
        for m in SslMode::ALL {
            let Some(plan) = (Tls {
                mode: m,
                ..Tls::default()
            })
            .plan() else {
                continue; // Disable never reaches rustls.
            };
            assert!(client_config(&plan).is_ok(), "{m:?}");
        }
    }

    /// **A file with no PEM certificate in it is an error, not an empty list.**
    ///
    /// `read_certs` returned `Ok(vec![])` for a DER `.crt` — which is what
    /// Windows' *Export certificate* writes by default — so `preflight`
    /// **passed** it, defeating the one thing preflight exists for. The engines
    /// then disagreed about the same file: PostgreSQL failed with `client
    /// certificate rejected` naming nothing, and MySQL succeeded *with the
    /// identity presented*, because `mysql_async` decides PEM-vs-DER by asking
    /// whether the bytes are UTF-8. `file_root_store` twenty lines away already
    /// had this check.
    ///
    /// The DER stand-in is a byte string that is not UTF-8 and holds no PEM
    /// header, which is exactly the shape of the real file.
    #[test]
    fn a_certificate_file_with_no_pem_section_is_refused_rather_than_empty() {
        let der = [0x30u8, 0x82, 0x02, 0x5c, 0x30, 0x82, 0x01, 0xc5, 0xa0, 0x03];
        let err = parse_certs("/tmp/client.crt", &der)
            .expect_err("a DER file must not pass as zero certificates");
        let DbError::Connect(msg) = err else {
            panic!("a connect error is what preflight reports");
        };
        assert!(msg.contains("/tmp/client.crt"), "{msg}");
        assert!(msg.contains("DER"), "{msg}");

        // An empty file and a file of comments take the same door.
        assert!(parse_certs("/tmp/empty.crt", b"").is_err());
        assert!(parse_certs("/tmp/notes.crt", b"# nothing here\n").is_err());
    }

    /// **An encrypted client key is named as such.** The form used to collect a
    /// passphrase for one, store it in the OS keyring, and hand it to nobody —
    /// `TlsPlan` has no passphrase field — so the connect failed with
    /// "<path> is not a PEM private key", sending the user to check a file that
    /// is perfectly good. The row is withdrawn; this is what replaced it.
    #[test]
    fn a_passphrase_protected_key_is_named_rather_than_called_malformed() {
        let encrypted = b"-----BEGIN ENCRYPTED PRIVATE KEY-----\nMIIBxxxx\n\
                          -----END ENCRYPTED PRIVATE KEY-----\n";
        let err = parse_key("/tmp/client.key", encrypted)
            .expect_err("an encrypted key cannot be opened here");
        let DbError::Connect(msg) = err else {
            panic!("a connect error is what preflight reports");
        };
        assert!(msg.contains("passphrase"), "{msg}");
        assert!(msg.contains("openssl pkcs8"), "the way out: {msg}");
        assert!(
            !msg.contains("is not a PEM private key"),
            "that message blames the file: {msg}"
        );

        // An ordinary malformed key keeps the old message, which is right for it.
        let err = parse_key("/tmp/client.key", b"hello").expect_err("not a key");
        assert!(format!("{err:?}").contains("not a PEM private key"));
    }

    /// A named CA file that isn't there is a *connect* error naming the path,
    /// not a panic and not a silent fall back to the public roots — which would
    /// quietly verify against 150 CAs the user did not choose.
    #[test]
    fn a_missing_ca_file_is_an_error_naming_the_path() {
        let plan = TlsPlan {
            root_ca: Some("/no/such/ca.crt".to_string()),
            ..plan_for(SslMode::VerifyFull)
        };
        let err = client_config(&plan).expect_err("must not fall back to the public roots");
        assert!(
            format!("{err:?}").contains("/no/such/ca.crt"),
            "the message names the file: {err:?}"
        );
    }

    /// The empty CA path has to produce a usable store whichever source wins —
    /// an empty `RootCertStore` fails every handshake against a correctly
    /// configured hosted server, and `mysql_async` will not even *build* a
    /// verifier from one once we have switched its own roots off.
    #[test]
    fn the_default_roots_are_never_empty() {
        let roots = default_roots();
        assert!(!roots.store.is_empty(), "no trust anchors at all");

        match roots.source {
            // Read from the OS: the certificates we hand `mysql_async` are
            // exactly the anchors Postgres verifies against, or the engines are
            // trusting overlapping sets rather than one.
            RootSource::Os => {
                assert!(!roots.certs.is_empty());
                assert_eq!(roots.certs.len(), roots.store.len());
            }
            // Fallen back: there is nothing to hand over, because the driver's
            // own built-in set already *is* this set — which holds only while
            // the workspace and `mysql_async` resolve `webpki-roots` to one
            // version. They did not (0.26 here, 1.0 there), so this arm
            // verified the two engines against two Mozilla snapshots while
            // asserting they agreed. Nothing in the crate can see that, so the
            // guard is the dependency: see the workspace `Cargo.toml`.
            RootSource::Bundled => assert!(roots.certs.is_empty()),
        }
    }

    /// The bundled set is a **fallback**, not the default. An OS store that
    /// offered certificates is the whole point of reading it: a CA an
    /// administrator removed there has to actually stop being trusted, which
    /// quietly unioning the compiled-in roots underneath would undo.
    #[test]
    fn the_bundled_roots_are_only_a_fallback() {
        assert_eq!(root_source(12), RootSource::Os);
        assert_eq!(root_source(1), RootSource::Os);
        // Nothing loadable — a minimal container with no ca-certificates
        // package. Trusting nothing would be defensible and would also make the
        // app useless there, with an error naming no cause.
        assert_eq!(root_source(0), RootSource::Bundled);
    }

    /// Real trust stores contain entries rustls refuses — expired anchors,
    /// oddities a platform accumulated over a decade. Dropping them one at a
    /// time is the difference between "one bad certificate in the Windows store"
    /// and "this machine cannot open a verified connection".
    #[test]
    fn an_unusable_certificate_is_dropped_rather_than_fatal() {
        let good = default_roots().certs[0].clone();
        let junk = CertificateDer::from(vec![0x30, 0x00, 0xff, 0xff]);

        let (store, kept) = usable_roots(vec![junk.clone(), good.clone()]);
        assert_eq!(kept.len(), 1, "the good one survives");
        assert_eq!(store.len(), 1);
        assert_eq!(kept[0], good);

        let (empty, none) = usable_roots(vec![junk]);
        assert!(empty.is_empty() && none.is_empty());
    }

    /// **The two engines must narrow to the same set.** Naming a CA file means
    /// "trust exactly this" — that is what the field is *for*, and what libpq's
    /// `sslrootcert` means. `mysql_async` adds its own built-in roots to
    /// whatever we hand it unless told not to, so without this a private CA
    /// would narrow the trust on Postgres and merely *widen* it on MySQL, which
    /// no local test-bed can show: our own server's certificate is refused by
    /// the public roots either way, so both engines look correct until the
    /// server is a hosted one with a publicly-signed certificate.
    #[test]
    fn a_named_ca_file_is_the_only_anchor_on_both_engines() {
        let plan = TlsPlan {
            root_ca: Some("/etc/ca.crt".to_string()),
            ..plan_for(SslMode::VerifyFull)
        };
        assert!(
            mysql_ssl_opts(&plan).disable_built_in_roots(),
            "MySQL would otherwise trust the named CA *plus* every public one"
        );

        // The Postgres side narrows by construction — the store is built from
        // the file alone — so one certificate in means one anchor out.
        let (store, _) = usable_roots(vec![default_roots().certs[0].clone()]);
        assert_eq!(store.len(), 1);
    }

    /// A verifying mode with no file named still hands the driver *our* anchors
    /// rather than leaving it on its own compiled-in set, or the OS store would
    /// be read for Postgres and ignored for MySQL.
    #[test]
    fn a_verifying_mode_hands_mysql_our_own_anchors() {
        let roots = default_roots();
        let opts = mysql_ssl_opts(&plan_for(SslMode::VerifyCa));
        match roots.source {
            RootSource::Os => {
                assert!(opts.disable_built_in_roots(), "or the OS store is ignored");
                assert_eq!(
                    opts.root_certs().len(),
                    roots.certs.len(),
                    "the same anchors Postgres verifies against"
                );
            }
            // The driver's own built-in roots are the bundled set, so leaving
            // them on *is* agreeing with Postgres here — and there is nothing to
            // copy per connection.
            RootSource::Bundled => {
                assert!(!opts.disable_built_in_roots());
                assert!(opts.root_certs().is_empty());
            }
        }
    }

    /// **An assumption about somebody else's parser, pinned.**
    ///
    /// The OS store hands us DER, and `mysql_async` has no DER entry point: its
    /// loader iterates PEM sections and falls back to reading a whole non-empty
    /// buffer as one certificate *only when it saw none*. So the anchors reach
    /// MySQL at all only because DER carries nothing that scanner recognises —
    /// if it ever returned an error instead of ending, that `cert?` would fail
    /// every verified MySQL connection, and no test of ours would be looking.
    #[test]
    fn a_der_certificate_carries_no_pem_section_for_the_driver_to_trip_on() {
        let der = default_roots().certs[0].as_ref();
        let mut sections = CertificateDer::pem_slice_iter(der);
        assert!(
            sections.next().is_none(),
            "the driver would take this as PEM and error instead of falling back"
        );
    }

    /// **And a non-verifying mode must leave them alone.** `mysql_async` builds
    /// a `WebPkiServerVerifier` even when it will not consult it, and that
    /// builder fails on an empty root store — so switching the built-in roots
    /// off for `prefer` (which names no CA and needs no anchors) would break the
    /// default mode of every connection. Nothing about the handshake says so;
    /// this is a property of the driver's construction order.
    #[test]
    fn a_non_verifying_mode_leaves_the_drivers_own_roots_alone() {
        for m in [SslMode::Prefer, SslMode::Require] {
            let opts = mysql_ssl_opts(&plan_for(m));
            assert!(
                !opts.disable_built_in_roots(),
                "{m:?} would leave the driver with no anchors to build a verifier from"
            );
            assert!(
                opts.root_certs().is_empty(),
                "{m:?} verifies nothing, so copying the whole trust store per \
                 connection is pure cost"
            );
        }
    }

    #[test]
    fn a_ca_file_that_is_not_a_certificate_is_rejected() {
        let dir = std::env::temp_dir().join("schemaic-tls-tests");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("not-a-cert.pem");
        std::fs::write(&path, b"hello, not a certificate").expect("write");

        let err = file_root_store(path.to_str().unwrap())
            .expect_err("a file with no certificates in it is not a trust store");
        // The refusal moved one layer down — `read_certs` now rejects a file
        // with no PEM section rather than returning an empty list, so the CA
        // path and the client-certificate path answer the same way about the
        // same file. What matters here is unchanged: it is refused, and the
        // message names the file.
        assert!(
            format!("{err:?}").contains("not-a-cert.pem"),
            "the message has to name the file: {err:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// **The assumption `skip_domain_validation` rests on, pinned — and it is
    /// false today.**
    ///
    /// `mysql_async` 0.37 does not ask rustls to skip the name check. It runs
    /// the full verification and then *forgives* the failure by matching the
    /// error's text: `e.to_string().contains("NotValidForName")`. rustls 0.23
    /// raises `CertificateError::NotValidForNameContext { .. }`, whose `Display`
    /// reads "certificate not valid for name …" and contains no such substring
    /// — so the arm never fires and `Verify CA` on MySQL/MariaDB also rejects a
    /// host-name mismatch. Measured twice against the same server, same CA,
    /// same binary, differing only in the name dialled.
    ///
    /// The test asserts what is true **now**, so it turns red the day the
    /// drivers agree again — which is the day `SslMode::caveat`'s sentence and
    /// the test-bed README's `wrongname` row come back out. Asserting the
    /// property we *want* would leave a red test standing over somebody else's
    /// release schedule; asserting the property we *have* makes the fix
    /// announce itself.
    #[test]
    fn the_driver_still_reads_the_verifier_error_by_its_words() {
        let e = rustls::Error::InvalidCertificate(rustls::CertificateError::NotValidForName);
        assert!(
            e.to_string().contains("NotValidForName"),
            "the bare variant is the one spelling the driver does recognise: {e}"
        );

        // And the one rustls actually raises, which it does not.
        let ctx =
            rustls::Error::InvalidCertificate(rustls::CertificateError::NotValidForNameContext {
                expected: rustls::pki_types::ServerName::try_from("db.example")
                    .expect("a name")
                    .to_owned(),
                presented: vec!["other.example".to_string()],
            });
        assert!(
            !ctx.to_string().contains("NotValidForName"),
            "the drivers agree again — take out `SslMode::caveat`'s sentence, restore the \
             test-bed README's `wrongname` row, and delete this test: {ctx}"
        );
    }

    /// MySQL's two toggles are the plan's two booleans, and crossing them is the
    /// bug that would make `verify-ca` check the name instead of the chain.
    ///
    /// **Green while the bug it names was live**, which is why the test above
    /// exists: this asserts the flag Schemaic sets, and the defect is in what
    /// the driver does with it.
    #[test]
    fn the_mysql_toggles_are_not_crossed() {
        let ca = mysql_ssl_opts(&plan_for(SslMode::VerifyCa));
        assert!(!ca.accept_invalid_certs(), "the chain is checked");
        assert!(ca.skip_domain_validation(), "the name is not");

        let full = mysql_ssl_opts(&plan_for(SslMode::VerifyFull));
        assert!(!full.accept_invalid_certs());
        assert!(!full.skip_domain_validation());

        let require = mysql_ssl_opts(&plan_for(SslMode::Require));
        assert!(require.accept_invalid_certs());
        assert!(require.skip_domain_validation());
    }

    /// MySQL's own report for this is `Input/output error: Input/output error:
    /// The system cannot find the path specified` — with no file named, and
    /// indistinguishable from the server being unreachable. A wrong path is the
    /// likeliest way to misconfigure the feature, so the preflight has to name
    /// it.
    #[test]
    fn the_preflight_names_a_file_that_is_not_there() {
        let plan = TlsPlan {
            root_ca: Some("/no/such/ca.crt".to_string()),
            ..plan_for(SslMode::VerifyFull)
        };
        let err = preflight(&plan).expect_err("a missing CA must not reach the driver");
        assert!(format!("{err:?}").contains("/no/such/ca.crt"), "{err:?}");

        let plan = TlsPlan {
            client_identity: Some(("/no/such/c.crt".to_string(), "/no/such/c.key".to_string())),
            ..plan_for(SslMode::Require)
        };
        let err = preflight(&plan).expect_err("a missing client certificate is just as fatal");
        assert!(format!("{err:?}").contains("/no/such/c.crt"), "{err:?}");
    }

    /// A plan naming no files has nothing to check, and must not invent a
    /// failure — this is every `prefer` and `require` connection.
    #[test]
    fn the_preflight_passes_a_plan_with_no_files() {
        assert!(preflight(&plan_for(SslMode::Prefer)).is_ok());
        assert!(preflight(&plan_for(SslMode::Require)).is_ok());
        assert!(
            preflight(&plan_for(SslMode::VerifyFull)).is_ok(),
            "an empty CA path means the public roots, not a missing file"
        );
    }

    #[test]
    fn the_mysql_options_carry_the_named_ca_and_identity() {
        let plan = TlsPlan {
            root_ca: Some("/etc/ca.crt".to_string()),
            client_identity: Some(("/c.crt".to_string(), "/c.key".to_string())),
            ..plan_for(SslMode::VerifyFull)
        };
        let opts = mysql_ssl_opts(&plan);
        assert_eq!(opts.root_certs().len(), 1);
        assert!(opts.client_identity().is_some());
    }
}
