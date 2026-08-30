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
//! must stay single.
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
    if let Some(ca) = &plan.root_ca {
        opts = opts.with_root_certs(vec![std::path::PathBuf::from(ca).into()]);
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
        root_store(Some(ca))?;
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
        let roots = root_store(plan.root_ca.as_deref())?;
        let webpki = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider)
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

/// The roots to trust: the named PEM file, or the **bundled** public set.
///
/// An empty `root_ca` is not "trust nothing" — it is a hosted server whose
/// certificate is already publicly signed, which is the common case the file
/// exists to *narrow*.
///
/// Those roots are `webpki-roots` — Mozilla's set, compiled in — and **not the
/// operating system's certificate store**. Worth stating because the difference
/// is invisible until it isn't: a company CA installed machine-wide, or the one
/// a TLS-inspecting proxy injects, is trusted by every browser on the box and
/// not by this. Such a connection has to name the CA file by path. The choice is
/// deliberate for now — one trust set on both engines and all three platforms,
/// with nothing that varies by how the machine was provisioned — and the same
/// set `mysql_async` uses internally, so the two engines cannot disagree about
/// which public CAs exist.
fn root_store(ca_path: Option<&str>) -> Result<RootCertStore, DbError> {
    let mut store = RootCertStore::empty();
    let Some(path) = ca_path else {
        store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        return Ok(store);
    };

    let certs = read_certs(path)?;
    let count = certs.len();
    for cert in certs {
        store
            .add(cert)
            .map_err(|e| DbError::Connect(format!("CA file {path} is not usable: {e}")))?;
    }
    if count == 0 {
        return Err(DbError::Connect(format!(
            "CA file {path} contains no certificates"
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
fn read_certs(path: &str) -> Result<Vec<CertificateDer<'static>>, DbError> {
    let pem = std::fs::read(path)
        .map_err(|e| DbError::Connect(format!("cannot read certificate file {path}: {e}")))?;
    CertificateDer::pem_slice_iter(&pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| DbError::Connect(format!("{path} is not a PEM certificate file: {e}")))
}

fn read_key(path: &str) -> Result<PrivateKeyDer<'static>, DbError> {
    let pem = std::fs::read(path)
        .map_err(|e| DbError::Connect(format!("cannot read key file {path}: {e}")))?;
    PrivateKeyDer::from_pem_slice(&pem)
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

    /// The empty CA path means the bundled public roots, and it has to actually
    /// produce a usable store — an empty `RootCertStore` would fail every
    /// handshake against a correctly-configured hosted server.
    #[test]
    fn no_ca_file_means_the_public_roots_are_loaded() {
        let store = root_store(None).expect("builds");
        assert!(!store.is_empty(), "the bundled roots are missing");
    }

    #[test]
    fn a_ca_file_that_is_not_a_certificate_is_rejected() {
        let dir = std::env::temp_dir().join("schemaic-tls-tests");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("not-a-cert.pem");
        std::fs::write(&path, b"hello, not a certificate").expect("write");

        let err = root_store(Some(path.to_str().unwrap()))
            .expect_err("a file with no certificates in it is not a trust store");
        assert!(format!("{err:?}").contains("no certificates"), "{err:?}");
        let _ = std::fs::remove_file(&path);
    }

    /// MySQL's two toggles are the plan's two booleans, and crossing them is the
    /// bug that would make `verify-ca` check the name instead of the chain.
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
