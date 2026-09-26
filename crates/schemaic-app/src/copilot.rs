//! The `COPILOT_HOME` a GitHub Copilot session runs under, and the MCP config
//! file inside it that gives the session our server.
//!
//! # The seal is a flag; the isolation is this directory
//!
//! Copilot's built-in tools are shut off on its command line, by
//! `--available-tools` naming only ours (`harness::copilot_available_tools`).
//! What that flag does *not* do is stop the CLI loading the user's own
//! configuration: `~/.copilot/mcp-config.json` (whose servers it **starts**,
//! whether or not their tools are then visible), their skills, their plugins and
//! their hooks — and a hook is a command. `--additional-mcp-config`, the flag
//! that adds our server, says of itself that it "augments config from
//! ~/.copilot/mcp-config.json", so it cannot displace any of that either.
//!
//! `COPILOT_HOME` can. Its help: "override the directory where configuration
//! and state files are stored; defaults to `$HOME/.copilot`". Pointed at a
//! directory Schemaic owns, none of the user's configuration exists for the
//! session. This is OpenCode's `XDG_CONFIG_HOME` again, and it costs nothing of
//! the user's.
//!
//! **Moving a CLI's home can log the user out, and here it would have, off
//! Windows.** Measured on 1.0.88 under Windows with an empty directory, a turn
//! authenticated and answered — the token was in the OS credential store. But
//! `copilot login --help` says where it goes otherwise: *"If a credential store
//! is not found or there is an issue using it, the token will be stored in a
//! plain text config file under ~/.copilot/"* — which is every headless Linux
//! box with no Secret Service. There, an empty home has no token and every turn
//! fails on authentication. So each home is seeded with the user's own
//! `config.json` ([`carry_login`]): the file the CLI marks "managed
//! automatically", holding the login and nothing it reads as configuration —
//! its settings, MCP servers, skills and hooks all live in other files, which
//! stay behind.
//!
//! # Reused within an instance, because the session lives in it
//!
//! Copilot is a process per turn, and a turn resumes the previous one by id —
//! from state the CLI keeps under `COPILOT_HOME` (`session-state/`,
//! `session-store.db`). A home per *session* would work; a home per *turn* would
//! forget every conversation after one question. So the root is per instance,
//! from `crate::opencode::instance_root`, for the reason that function gives: the
//! MCP config carries the endpoint file's path, which differs per connection,
//! and a second window must not re-point the first one's next turn.
//!
//! The endpoint itself is not here, for OpenCode's reason: this directory
//! outlives the session. The config names the per-session endpoint file by
//! path, and that file is removed when the session ends.

use std::path::{Path, PathBuf};

/// The file inside the home that `--additional-mcp-config=@…` names.
///
/// Not `mcp-config.json`, which is the name Copilot reads *by default* from its
/// home: naming ours by flag keeps "which servers does this session have" a
/// question the argv answers, rather than one that depends on which file a
/// build happens to look for.
const MCP_CONFIG_FILE: &str = "schemaic-mcp.json";

/// The file Copilot keeps its login state in — and, with no credential store,
/// the token itself. Its own first line: "User settings belong in
/// settings.json. This file is managed automatically."
const LOGIN_FILE: &str = "config.json";

/// The user's own Copilot home, as their environment names it: `COPILOT_HOME`
/// when set, else `<home>/.copilot` (`USERPROFILE` on Windows, `HOME`
/// elsewhere, as the CLI's own `$HOME/.copilot` default resolves).
///
/// Pure over the three values so the precedence has a test; [`carry_login`]
/// reads them from this process, whose environment is the user's — the
/// child's `COPILOT_HOME` is set on the child alone.
fn user_copilot_home(
    copilot_home: Option<std::ffi::OsString>,
    userprofile: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let set = |v: Option<std::ffi::OsString>| v.filter(|v| !v.to_string_lossy().trim().is_empty());
    if let Some(d) = set(copilot_home) {
        return Some(PathBuf::from(d));
    }
    let home = match cfg!(windows) {
        true => set(userprofile).or_else(|| set(home)),
        false => set(home),
    }?;
    Some(PathBuf::from(home).join(".copilot"))
}

/// Copy the user's login into `root`, so the session is signed in as they are.
///
/// Best-effort: with nothing to copy — a fresh install, a login held only in
/// the credential store or an environment token — the home is still usable,
/// and a missing login surfaces as the CLI's own authentication error on the
/// first turn. The copy lands in a directory `private_dir` made owner-only, and
/// goes with the instance root when the sweep collects it.
fn carry_login(root: &Path) {
    let Some(user) = user_copilot_home(
        std::env::var_os("COPILOT_HOME"),
        std::env::var_os("USERPROFILE"),
        std::env::var_os("HOME"),
    ) else {
        return;
    };
    let from = user.join(LOGIN_FILE);
    let to = root.join(LOGIN_FILE);
    // A user whose own `COPILOT_HOME` *is* this root has nothing to copy.
    if from.is_file() && from != to {
        let _ = std::fs::copy(&from, &to);
    }
}

/// Remove the homes of instances that are gone. See `crate::opencode::sweep`.
pub(crate) fn sweep() {
    crate::opencode::sweep_instances(&["copilot", "copilot-inline"]);
}

/// A home directory for one kind of Copilot run, written and ready to point the
/// CLI at.
pub(crate) struct CopilotHome {
    root: PathBuf,
    /// The MCP config inside it, when this home gives the session a server.
    mcp_config: Option<PathBuf>,
}

impl CopilotHome {
    /// The home for a chat session, with our server configured in it when there
    /// is an endpoint file to point it at.
    ///
    /// `None` when it could not be written, and the caller **refuses the
    /// session** rather than spawning without it — the same direction OpenCode's
    /// config takes, for a reason of this harness's own. The allowlist would
    /// still hide every foreign tool, but without the home the CLI would *start*
    /// every server in the user's `mcp-config.json` and load their hooks, and
    /// neither is something a SQL assistant should do on their behalf.
    ///
    /// **No endpoint file is still the session's home, not the one-shots'.** A
    /// session with no database tools is still a conversation whose state has
    /// to be found again next turn, and `write_inline`'s root exists so the two
    /// never share a directory. The config from an earlier session may still be
    /// lying in this root; it is not read, because the file is named only by
    /// the flag and the flag is not passed without one.
    pub(crate) fn write(exe: &str, endpoint_file: Option<&str>) -> Option<Self> {
        let root = crate::opencode::instance_root("copilot")?;
        carry_login(&root);
        let mcp_config = match endpoint_file {
            Some(ep) => {
                let cfg = root.join(MCP_CONFIG_FILE);
                // Plainly written: it holds no secret, and it is rewritten per
                // session.
                std::fs::write(&cfg, schemaic_ai::harness::copilot_mcp_config_json(exe, ep))
                    .ok()?;
                Some(cfg)
            }
            None => None,
        };
        Some(Self { root, mcp_config })
    }

    /// The home for a one-shot generation: isolated, and **no** server.
    ///
    /// A root of its own, for OpenCode's reason: a session's config and a
    /// one-shot's must not be one file whichever wrote it last. Here there is
    /// no file at all — but a one-shot running in the session's home would
    /// still find the session's `schemaic-mcp.json` lying there, and a later
    /// build that read it by default would hand a server to the one path with
    /// nowhere to show a tool call.
    pub(crate) fn write_inline() -> Option<Self> {
        let root = crate::opencode::instance_root("copilot-inline")?;
        carry_login(&root);
        Some(Self {
            root,
            mcp_config: None,
        })
    }

    /// The config for `--additional-mcp-config`, when there is one.
    pub(crate) fn mcp_config(&self) -> Option<&Path> {
        self.mcp_config.as_deref()
    }

    /// The environment a Copilot child needs.
    ///
    /// `OsString` throughout, for the reason `OpenCodeConfig::env` gives: a root
    /// that is not valid UTF-8 would otherwise point the CLI at a directory with
    /// U+FFFD where the bytes were — the user's own home, in effect, while the
    /// panel reports the session isolated.
    pub(crate) fn env(&self) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        vec![(
            std::ffi::OsString::from("COPILOT_HOME"),
            self.root.clone().into_os_string(),
        )]
    }

    /// Variables that must be **cleared** from a Copilot child's inherited
    /// environment, each because it reopens something the argv closed.
    ///
    /// - `COPILOT_ALLOW_ALL` auto-approves every tool, and its exact value
    ///   `"true"` also "trusts the working directory … which loads that
    ///   directory's skills, plugins, MCP servers, and hooks". The allowlist
    ///   still decides visibility, but approval-for-everything is not a state
    ///   this assistant should inherit from someone's shell profile.
    /// - `COPILOT_ASSISTED_APPROVAL` swaps the approval policy for a model's
    ///   judgement.
    /// - `COPILOT_CUSTOM_INSTRUCTIONS_DIRS` adds instruction directories, which
    ///   `--no-custom-instructions` is passed to keep out.
    ///
    /// Not cleared: the `COPILOT_PROVIDER_*` and token variables. They are how
    /// a user authenticates or brings their own model, and clearing them would
    /// log the session out rather than isolate it.
    pub(crate) fn env_remove() -> &'static [&'static str] {
        &[
            "COPILOT_ALLOW_ALL",
            "COPILOT_ASSISTED_APPROVAL",
            "COPILOT_CUSTOM_INSTRUCTIONS_DIRS",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::CopilotHome;

    fn os(s: &str) -> Option<std::ffi::OsString> {
        Some(std::ffi::OsString::from(s))
    }

    /// Where the login is read from is the CLI's own rule: the user's
    /// `COPILOT_HOME` first — a user who moved their home keeps their token
    /// there — else the platform home's `.copilot`.
    #[test]
    fn the_login_is_read_from_where_the_user_s_copilot_keeps_it() {
        use super::user_copilot_home as at;
        let p = std::path::PathBuf::from;
        assert_eq!(
            at(os("/my/copilot"), os("/w"), os("/h")),
            Some(p("/my/copilot"))
        );
        let fallback = match cfg!(windows) {
            true => p("/w").join(".copilot"),
            false => p("/h").join(".copilot"),
        };
        assert_eq!(at(None, os("/w"), os("/h")), Some(fallback.clone()));
        // A blank variable is not a home.
        assert_eq!(at(os("  "), os("/w"), os("/h")), Some(fallback));
        assert_eq!(at(None, None, None), None);
    }

    /// **Only the login file is carried**, never the files that are the user's
    /// configuration — carrying those would undo the isolation the home is for.
    #[test]
    fn only_the_login_is_carried_into_the_home() {
        assert_eq!(super::LOGIN_FILE, "config.json");
        for config in ["mcp-config.json", "settings.json"] {
            assert_ne!(super::LOGIN_FILE, config);
        }
    }

    fn home() -> CopilotHome {
        CopilotHome {
            root: std::path::PathBuf::from("/tmp/copilot-home"),
            mcp_config: None,
        }
    }

    #[test]
    fn the_home_is_the_lever_and_is_never_also_cleared() {
        let cleared = CopilotHome::env_remove();
        for (k, _) in home().env() {
            let k = k.to_string_lossy().into_owned();
            assert!(
                !cleared.contains(&k.as_str()),
                "{k} is both set and cleared"
            );
        }
        let (_, v) = home()
            .env()
            .into_iter()
            .find(|(k, _)| k == "COPILOT_HOME")
            .expect("the isolation's lever");
        assert_eq!(std::path::Path::new(&v), home().root);
    }

    /// **The whole set**, for the reason `OpenCodeConfig`'s own test gives: a
    /// sample of it lets one entry be dropped with the suite green.
    #[test]
    fn every_variable_that_widens_a_session_is_cleared() {
        assert_eq!(
            CopilotHome::env_remove(),
            &[
                "COPILOT_ALLOW_ALL",
                "COPILOT_ASSISTED_APPROVAL",
                "COPILOT_CUSTOM_INSTRUCTIONS_DIRS",
            ]
        );
    }

    #[test]
    fn authentication_survives_the_isolation() {
        // Clearing any of these logs the session out rather than isolating it.
        let cleared = CopilotHome::env_remove();
        for keep in [
            "COPILOT_GITHUB_TOKEN",
            "GH_TOKEN",
            "GITHUB_TOKEN",
            "COPILOT_PROVIDER_BASE_URL",
            "COPILOT_PROVIDER_API_KEY",
            "HOME",
            "USERPROFILE",
        ] {
            assert!(!cleared.contains(&keep), "{keep} must not be cleared");
        }
    }

    /// A session's home and a one-shot's must not be one directory; see
    /// `write_inline`. Asked of the path, never of `instance_root`, which
    /// creates directories under the real `config_dir()`.
    #[test]
    fn a_session_and_a_one_shot_never_share_a_home() {
        let config = std::path::Path::new("/schemaic-fixture-config");
        let session = crate::opencode::instance_path_in(config, "copilot");
        let inline = crate::opencode::instance_path_in(config, "copilot-inline");
        assert_ne!(session, inline);
        // …nor share one with OpenCode's config roots.
        assert_ne!(
            session,
            crate::opencode::instance_path_in(config, "opencode")
        );
        assert!(session.ends_with(crate::opencode::instance_tag()));
    }
}
