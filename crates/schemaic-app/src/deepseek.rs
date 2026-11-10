//! The `DSH_HOME` a DeepSeek Harness run uses, and the `--patch` overlay inside
//! it that seals the run, signs it in, picks its model and gives it our server.
//!
//! # The seal is the overlay; the isolation is this directory
//!
//! A dsh run is a composition of plugins, and its built-in tools are plugins
//! like any other. The overlay (`schemaic_ai::harness::dsh_overlay_yaml`)
//! disables every one of them, which was measured to leave the model with no
//! tool but Schemaic's. What a `--patch` cannot do is stop the layers *before*
//! it: the user's own profile patch and home patch, under their `~/.dsh`, apply
//! first and could insert a tool plugin the overlay has never named. Pointing
//! `DSH_HOME` at a directory Schemaic owns means those layers do not exist for
//! the run — OpenCode's `XDG_CONFIG_HOME` and Copilot's `COPILOT_HOME` again.
//!
//! **The sign-in is read where it lives, never copied.** Headless ignores the
//! desktop app's sign-in by default; the overlay turns the credential store back
//! on and points it at the user's own `.credentials.yaml` by path. The account
//! grant has no expiry or refresh flow (its plugin's README), so a second
//! reader cannot rotate it from under the desktop app — and measured, a run
//! signed in that way left the file untouched.
//!
//! # Reused within an instance, because the session lives in it
//!
//! A turn resumes the previous one with `--session-id`, which adopts a session
//! from the log dsh keeps under `DSH_HOME`'s `sessions/`. So the root is per
//! instance (`crate::opencode::instance_root`), as Copilot's is, and the overlay
//! in it is rewritten per session: it carries the endpoint file's *path*, which
//! differs per connection, and that file is removed when the session ends.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use schemaic_ai::harness::{DshOverlay, DshProvider};

/// The overlay's file name inside the home: named by `--patch`, so which file
/// seals a run is a question the argv answers.
const OVERLAY_FILE: &str = "schemaic-overlay.yml";

/// Where dsh keeps its credential store, relative to its home.
const CREDENTIALS_FILE: &str = ".credentials.yaml";

/// The user's own dsh home: `DSH_HOME` when set, else `<home>/.dsh`
/// (`USERPROFILE` on Windows, `HOME` elsewhere). Pure over the three values so
/// the precedence has a test.
fn user_dsh_home(
    dsh_home: Option<OsString>,
    userprofile: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    let set = |v: Option<OsString>| v.filter(|v| !v.to_string_lossy().trim().is_empty());
    if let Some(d) = set(dsh_home) {
        return Some(PathBuf::from(d));
    }
    let home = match cfg!(windows) {
        true => set(userprofile).or_else(|| set(home)),
        false => set(home),
    }?;
    Some(PathBuf::from(home).join(".dsh"))
}

/// Which route signs the run in: an API key when the user exports one — the
/// route dsh itself reads first — else the account the desktop app signs in
/// with.
fn provider(api_key: Option<OsString>) -> DshProvider {
    match api_key.filter(|k| !k.to_string_lossy().trim().is_empty()) {
        Some(_) => DshProvider::ApiKey,
        None => DshProvider::Account,
    }
}

/// Remove the homes of instances that are gone. See `crate::opencode::sweep`.
pub(crate) fn sweep() {
    crate::opencode::sweep_instances(&["deepseek", "deepseek-inline"]);
}

/// A home for one kind of dsh run, with its overlay written.
pub(crate) struct DshHome {
    root: PathBuf,
    overlay: PathBuf,
}

impl DshHome {
    /// The home for a chat session, our server in its overlay when there is an
    /// endpoint file. `None` when it could not be written, and the caller
    /// **refuses the session** — the overlay is the seal.
    pub(crate) fn write(
        exe: &str,
        endpoint_file: Option<&str>,
        model: &str,
        effort: &str,
    ) -> Option<Self> {
        Self::write_at("deepseek", exe, endpoint_file, model, effort)
    }

    /// The home for a one-shot generation: sealed, signed in, and **no**
    /// server. A root of its own, for Copilot's reason — a session's overlay
    /// and a one-shot's are never one file whichever wrote it last.
    pub(crate) fn write_inline(model: &str, effort: &str) -> Option<Self> {
        Self::write_at("deepseek-inline", "", None, model, effort)
    }

    fn write_at(
        kind: &str,
        exe: &str,
        endpoint_file: Option<&str>,
        model: &str,
        effort: &str,
    ) -> Option<Self> {
        let root = crate::opencode::instance_root(kind)?;
        // No user home at all still writes a path — one in this root, which
        // holds no store — so an API-key user is unaffected and an account
        // user meets dsh's own "no credential" error rather than a refusal.
        let credentials = user_dsh_home(
            std::env::var_os("DSH_HOME"),
            std::env::var_os("USERPROFILE"),
            std::env::var_os("HOME"),
        )
        .unwrap_or_else(|| root.clone())
        .join(CREDENTIALS_FILE);
        let overlay = root.join(OVERLAY_FILE);
        let yaml = schemaic_ai::harness::dsh_overlay_yaml(&DshOverlay {
            exe,
            endpoint_file,
            credentials: &credentials.to_string_lossy(),
            provider: provider(std::env::var_os("DEEPSEEK_API_KEY")),
            model,
            effort,
        });
        // Plainly written: it names files by path and holds no secret.
        std::fs::write(&overlay, yaml).ok()?;
        Some(Self { root, overlay })
    }

    /// The file `--patch` names.
    pub(crate) fn overlay(&self) -> &Path {
        &self.overlay
    }

    /// The environment a dsh child needs: its private home, the read-only
    /// permission preset (there is no shell for it to govern, but it is what
    /// was measured), and telemetry off — by default it uploads a session only
    /// after in-app feedback a headless run cannot give, and `DISABLED`
    /// "constructs no transport" at all.
    ///
    /// `OsString` for `CopilotHome::env`'s reason: a root that is not valid
    /// UTF-8 must not become U+FFFD and point the CLI somewhere else.
    pub(crate) fn env(&self) -> Vec<(OsString, OsString)> {
        vec![
            (
                OsString::from("DSH_HOME"),
                self.root.clone().into_os_string(),
            ),
            (
                OsString::from("DSH_PERMISSION_MODE"),
                OsString::from("read-only"),
            ),
            (
                OsString::from("DSH_TELEMETRY_MODE"),
                OsString::from("DISABLED"),
            ),
        ]
    }

    /// Variables cleared from every harness child, each because it reopens
    /// something the overlay closed:
    ///
    /// - `DSH_TOOLS_MODE` switches the tool layer to "PTC mode", whose one tool
    ///   is `run_code` — a code runner the plugin denylist never sees, since it
    ///   is a mode of the `tools` row and not a plugin of its own.
    /// - `DSH_TELEMETRY_OTLP_URL` redirects the telemetry exporter; with the
    ///   mode `DISABLED` nothing is built to use it, and a shell profile's
    ///   value has no business in a SQL assistant's environment regardless.
    ///
    /// Not cleared: `DEEPSEEK_API_KEY`, which is how a user signs in by key —
    /// clearing it would log the run out rather than isolate it.
    pub(crate) fn env_remove() -> &'static [&'static str] {
        &["DSH_TOOLS_MODE", "DSH_TELEMETRY_OTLP_URL"]
    }
}

/// The arguments that print the composed profile a run with `overlay` would
/// mount — the input `schemaic_ai::harness::dsh_unsealed_rows` checks. Boots
/// nothing and calls no model (measured: 0.2 s).
pub(crate) fn dump_args(overlay: &Path) -> Vec<String> {
    vec![
        "--profile".into(),
        "headless".into(),
        "--patch".into(),
        overlay.to_string_lossy().into_owned(),
        "--dump-config".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(s: &str) -> Option<OsString> {
        Some(OsString::from(s))
    }

    /// dsh's own rule: `DSH_HOME` first, else the platform home's `.dsh`.
    #[test]
    fn the_sign_in_is_read_from_where_the_user_s_dsh_keeps_it() {
        assert_eq!(
            user_dsh_home(os("D:/dsh"), os("C:/Users/u"), os("/home/u")),
            Some(PathBuf::from("D:/dsh"))
        );
        let platform = match cfg!(windows) {
            true => PathBuf::from("C:/Users/u").join(".dsh"),
            false => PathBuf::from("/home/u").join(".dsh"),
        };
        assert_eq!(
            user_dsh_home(os("  "), os("C:/Users/u"), os("/home/u")),
            Some(platform)
        );
        assert_eq!(user_dsh_home(None, None, None), None);
    }

    #[test]
    fn an_exported_key_signs_in_by_key_and_none_by_account() {
        assert_eq!(provider(os("sk-test")), DshProvider::ApiKey);
        assert_eq!(provider(os("  ")), DshProvider::Account);
        assert_eq!(provider(None), DshProvider::Account);
    }

    fn home() -> DshHome {
        DshHome {
            root: PathBuf::from("/tmp/schemaic/deepseek/pid-1"),
            overlay: PathBuf::from("/tmp/schemaic/deepseek/pid-1").join(OVERLAY_FILE),
        }
    }

    /// The home is the lever, so the child's `DSH_HOME` is ours, and nothing
    /// the child is told to set is also on the list it has cleared — a
    /// variable on both would be cleared after being set, or the other way
    /// round, depending on an order nobody chose.
    #[test]
    fn the_home_is_the_lever_and_is_never_also_cleared() {
        let env = home().env();
        assert!(
            env.iter().any(|(k, v)| k == "DSH_HOME"
                && Path::new(v) == Path::new("/tmp/schemaic/deepseek/pid-1"))
        );
        for (k, _) in &env {
            assert!(
                !DshHome::env_remove().contains(&k.to_string_lossy().as_ref()),
                "{k:?}"
            );
        }
        assert!(
            env.iter()
                .any(|(k, v)| k == "DSH_TELEMETRY_MODE" && v == "DISABLED")
        );
    }

    /// PTC mode is a code runner the plugin denylist cannot see.
    #[test]
    fn the_variable_that_turns_on_a_code_runner_is_cleared() {
        assert!(DshHome::env_remove().contains(&"DSH_TOOLS_MODE"));
        assert!(!DshHome::env_remove().contains(&"DEEPSEEK_API_KEY"));
    }

    #[test]
    fn the_dump_is_of_the_same_overlay_the_run_is_given() {
        let h = home();
        let a = dump_args(h.overlay());
        assert_eq!(a.last().map(String::as_str), Some("--dump-config"));
        let i = a.iter().position(|x| x == "--patch").expect("--patch");
        assert_eq!(PathBuf::from(&a[i + 1]), h.overlay().to_path_buf());
    }
}
