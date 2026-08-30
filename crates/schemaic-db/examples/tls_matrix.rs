//! Walk every TLS mode against a live server and print what each one did.
//!
//! The unit tests in `db::tls` cover the decisions; this covers the handshake,
//! which no pure test can. Point it at the test-bed in `scripts/tls-testbed`
//! (whose README has the matrix this is meant to reproduce), swap the server's
//! certificate underneath it, and run it again:
//!
//! ```text
//! cargo run -p schemaic-db --example tls_matrix
//! sudo bash scripts/tls-testbed/swap-server-cert.sh wrongname both
//! cargo run -p schemaic-db --example tls_matrix
//! ```
//!
//! Every setting is an environment variable, because the certificates live
//! wherever the test-bed put them — and on Windows against servers in WSL, the
//! CA has to be named by its *Windows* path:
//!
//! | variable | default |
//! |---|---|
//! | `TLS_HOST` | `schemaic-tls.test` |
//! | `TLS_CA` | `/etc/schemaic-tls/ca.crt`, or `os` for the OS trust store |
//! | `TLS_WRONG_CA` | `TLS_CA` with `ca.crt` → `otherca.crt`; `os` likewise |
//! | `TLS_USER` / `TLS_PASSWORD` | `schemaic` / `schemaic` |
//! | `TLS_MYSQL_PORT` / `TLS_PG_PORT` | `3306` / `5432` (`0` skips the engine) |
//!
//! **`os` is spelled out rather than written as an empty value** because an
//! empty environment variable cannot be set from PowerShell at all — `$env:X =
//! ''` *removes* the variable — so the obvious spelling would silently fall back
//! to the default path on the shell most likely to be running this on Windows.
//!
//! Each mode is tried under two CA settings, printed above the table. Against a
//! local test-bed those are the CA that signed the server and one that did not,
//! and the second column is the one that matters: a verifying mode that connects
//! there is not verifying anything. Against a hosted provider with a private CA
//! (Aiven), set the second to `os` — its refusal is the proof that a named file
//! *replaces* the trust set rather than adding to it.

use std::time::Duration;

use schemaic_core::connection::{Connection, Environment, SshTunnel, SslMode, Tls};
use schemaic_db::Db;

fn env(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_string())
}

/// A CA setting: a file path, or an empty string meaning the OS trust store.
///
/// `os` is the portable spelling of "no file". An empty *value* means the same
/// thing and works from a POSIX shell, but PowerShell deletes a variable
/// assigned `''`, so that spelling would quietly resolve to the default path.
fn ca_setting(key: &str, fallback: &str) -> String {
    let value = env(key, fallback);
    match value.trim().eq_ignore_ascii_case("os") {
        true => String::new(),
        false => value,
    }
}

/// How a CA setting reads in the table header.
fn ca_label(path: &str) -> &str {
    match path.is_empty() {
        true => "the OS trust store",
        false => path,
    }
}

fn port(key: &str, fallback: u16) -> u16 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

fn connection(db_type: &str, host: &str, port: u16, tls: Tls) -> Connection {
    Connection {
        id: 1,
        name: "tls_matrix".to_string(),
        db_type: db_type.to_string(),
        host: host.to_string(),
        port,
        user: env("TLS_USER", "schemaic"),
        password: env("TLS_PASSWORD", "schemaic"),
        file: String::new(),
        ssh: SshTunnel::default(),
        tls,
        color: None,
        prominent_color: false,
        read_only: false,
        environment: Environment::None,
        ai_data: None,
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // The crate's own debug logs, which include how many trust anchors were
    // loaded and from where. When a verified connection fails against a server
    // that looks fine, that line is the first thing worth reading.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "schemaic_db=debug".into()),
        )
        .with_target(false)
        .without_time()
        .init();

    let host = env("TLS_HOST", "schemaic-tls.test");
    let ca = ca_setting("TLS_CA", "/etc/schemaic-tls/ca.crt");
    let wrong_ca = ca_setting("TLS_WRONG_CA", &ca.replace("ca.crt", "otherca.crt"));

    println!("A = {}", ca_label(&ca));
    println!("B = {}", ca_label(&wrong_ca));

    for (engine, port) in [
        ("MySQL", port("TLS_MYSQL_PORT", 3306)),
        ("PostgreSQL", port("TLS_PG_PORT", 5432)),
    ] {
        if port == 0 {
            continue;
        }
        println!("\n{engine} @ {host}:{port}");
        println!("  {:<12} {:<34} B", "mode", "A");

        for mode in SslMode::ALL {
            let mut cells = Vec::new();
            for ca_path in [&ca, &wrong_ca] {
                let tls = Tls {
                    mode,
                    ca_path: ca_path.clone(),
                    ..Tls::default()
                };
                let db = Db::connect(&connection(engine, &host, port, tls), None);
                cells.push(match db.ping(Duration::from_secs(5)).await {
                    Ok(()) => "connected".to_string(),
                    Err(e) => format!("refused ({e})"),
                });
            }
            println!("  {:<12} {:<34} {}", mode.label(), cells[0], cells[1]);
        }
    }
}
