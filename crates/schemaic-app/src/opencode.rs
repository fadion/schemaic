//! The config directory an OpenCode session is sealed by, and the environment
//! that points the CLI at it.
//!
//! # Why a directory and not a file
//!
//! Every other harness takes its configuration by flag: Claude's
//! `--mcp-config <path>`, Codex's `-c key=value`. OpenCode has no such flag —
//! `--config` does not exist — and the two environment variables that look like
//! the answer are not one. Measured against the installed binary, both
//! `OPENCODE_CONFIG` and `OPENCODE_CONFIG_CONTENT` **merge** with the user's own
//! `~/.config/opencode/opencode.json`: a config naming only our server resolved
//! to ours *and* theirs. They are additive, and additive is not isolation.
//!
//! `XDG_CONFIG_HOME` is. Pointed at a directory of our own, the user's globally
//! registered servers vanish from `opencode debug config` entirely. That is this
//! harness's `--strict-mcp-config`, and it costs nothing of the *user's* —
//! unlike [`crate::antigravity`] there is no permission to put back. The
//! directory is still ours to collect, which is what [`sweep`] does; the
//! difference is that leaving one behind costs disk rather than a standing
//! grant.
//!
//! **Only `XDG_CONFIG_HOME`, and never `XDG_DATA_HOME`.** The two are easy to set
//! together and the second would break the session: OpenCode keeps `auth.json`
//! under its *data* directory, so moving that logs the user out of the CLI they
//! just signed into, and the turn fails on credentials rather than on anything
//! Schemaic did visibly.
//!
//! # Why it is reused rather than per-session, and per-*instance* rather than
//! per-user
//!
//! A config directory OpenCode has not seen before makes it bootstrap a plugin
//! runtime into that directory — a real `npm install`, writing `package.json`,
//! `package-lock.json` and `node_modules/`. Measured on a fresh directory it ran
//! past three minutes and printed **not one line** before it was killed, which is
//! indistinguishable from a hang. Two things answer it, and both are kept because
//! they answer different halves: `--pure` (in `harness::turn_args`) skips the
//! external-plugin install outright, and reusing one directory means whatever
//! bootstrap does happen is paid at most once per running app rather than once
//! per session.
//!
//! **Reuse stops at the instance boundary**, and it did not, which was a bug
//! rather than a trade. The file carries the endpoint-file path and the offered
//! tool names — exactly what differs per connection — and OpenCode is a process
//! *per turn*, so with one root per user a second window's session re-pointed
//! the first window's next question at *its* database, with its credentials and
//! its access level, while the first window's panel, transcript and deltas all
//! named the first connection. [`instance_root`] adds the pid; [`sweep`] takes
//! the dead ones away.
//!
//! Reuse still has a consequence, and it is the reason the database endpoint is
//! not in this file: a reused directory **outlives the session**. Anything
//! written here is still on disk after that session ends. So the config carries
//! only the *path* of the per-session endpoint file —
//! `ai::write_endpoint_file`, created `O_EXCL` with owner-only permissions in a
//! directory of Schemaic's own, and unlinked when the session ends — and the
//! credentials never enter the reused directory at all.

use std::path::{Path, PathBuf};

/// The config root for `kind`, **belonging to this instance and no other**.
///
/// **Two Schemaic windows, one file.** The root was `private_dir(kind)` with no
/// per-instance component, and the file inside it carries exactly the two things
/// that differ per connection: the path of the endpoint file, and the tool names
/// this connection's access level offers. OpenCode is a process *per turn*, so
/// window A's next question read what window B had written — A's assistant
/// talking to B's database, with B's credentials and B's access level, while A's
/// panel, transcript and deltas all named A's connection.
///
/// The pid is the discriminator because that is what a running instance has and
/// a dead one does not; the sweep below collects the roots of instances that are
/// gone. `write_inline`'s "two roots cannot race" argument is unchanged and is
/// now a *third* axis rather than the only one — see that function.
///
/// The reuse the module docs argue for survives: a directory per instance is
/// still a directory the CLI has usually seen before within that instance's
/// life, and the plugin bootstrap `--pure` skips is paid at most once per
/// instance rather than once per turn.
fn instance_root(kind: &str) -> Option<PathBuf> {
    let dir = schemaic_core::persist::private_dir(kind)?.join(instance_tag());
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// This process's name for its own config roots.
fn instance_tag() -> String {
    format!("pid-{}", std::process::id())
}

/// Remove the config roots of instances that are gone.
///
/// Called at startup, beside `antigravity::sweep` and for the same reason: a
/// per-instance directory that nothing collects is a directory per launch,
/// forever. Nothing in here is a secret — the module docs are explicit that the
/// endpoint never enters this tree — so this is tidiness rather than exposure,
/// which is why it removes recursively where `ai`'s session directories do not:
/// every file under a root here was written by Schemaic or by an OpenCode
/// bootstrap into a directory Schemaic made for it.
pub(crate) fn sweep() {
    for kind in ["opencode", "opencode-inline"] {
        let Some(base) = schemaic_core::persist::private_dir(kind) else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(pid) = name
                .strip_prefix("pid-")
                .and_then(|p| p.parse::<u32>().ok())
            else {
                // Including the pre-instance layout: an `opencode/` directory
                // left directly under the base by an older build. It is ours and
                // nothing reads it any more.
                if name == "opencode" {
                    let _ = std::fs::remove_dir_all(e.path());
                }
                continue;
            };
            // **Any live process on that pid, not ours specifically.** A second
            // window's root must survive, and the cost of being wrong the other
            // way — an unrelated process happening to hold a recycled pid — is
            // one directory kept until the next launch, not a session broken.
            if crate::liveness::process_start(pid).is_none() {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

/// The directory OpenCode is told to read its configuration from, and the file
/// inside it that seals the session.
///
/// `XDG_CONFIG_HOME` names the *parent*: the CLI looks for `<it>/opencode/`, so
/// the returned path is that subdirectory's parent and the caller passes it as
/// the variable's value.
pub(crate) struct OpenCodeConfig {
    /// What `XDG_CONFIG_HOME` is set to.
    root: PathBuf,
}

impl OpenCodeConfig {
    /// Write the sealed config and return the directory to point the CLI at.
    ///
    /// `None` when it could not be written, and the caller must then **refuse the
    /// session** rather than spawn without it. That is not the usual direction —
    /// the Codex path degrades to isolation with no tools of its own — but this
    /// file is the only thing that defines the `schemaic` agent, and OpenCode
    /// without it does not fail closed: `--agent schemaic` naming an agent that
    /// does not exist leaves the run on the default `build` agent, which has
    /// every built-in tool including `bash`. A seal that silently becomes a shell
    /// is exactly the failure `Constraint` exists to prevent.
    pub(crate) fn write(exe: &str, endpoint_file: &str, allowed: &[&str]) -> Option<Self> {
        let root = instance_root("opencode")?;
        let dir = root.join("opencode");
        std::fs::create_dir_all(&dir).ok()?;
        let cfg = schemaic_ai::harness::opencode_config_json(exe, endpoint_file, allowed);
        // Plainly written rather than `create_private_new`: it holds no secret
        // (see the module docs) and it is rewritten on every session, so an
        // existing path is the normal case rather than the collision that
        // `O_EXCL` is there to refuse.
        std::fs::write(dir.join("opencode.json"), cfg).ok()?;
        Some(Self { root })
    }

    /// The same, for a one-shot generation: the sealed agent and **no** server.
    ///
    /// **A directory of its own, and that is not tidiness.** Both configs are
    /// the file `<root>/opencode/opencode.json`, so writing one into the other's
    /// root would leave whichever ran last deciding whether a server is
    /// registered — an inline generation could hand the session's server to a
    /// path with nowhere to show a tool call, or a Ctrl+K could quietly strip
    /// the server from a chat session running beside it. Two roots cannot race.
    ///
    /// `None` for the same reason [`OpenCodeConfig::write`] returns it, and the
    /// caller must refuse just as hard: `--agent schemaic` naming an agent that
    /// does not exist runs on `build`, which has every built-in including
    /// `bash`.
    pub(crate) fn write_inline() -> Option<Self> {
        let root = instance_root("opencode-inline")?;
        let dir = root.join("opencode");
        std::fs::create_dir_all(&dir).ok()?;
        let cfg = schemaic_ai::harness::opencode_inline_config_json();
        std::fs::write(dir.join("opencode.json"), cfg).ok()?;
        Some(Self { root })
    }

    /// The environment a turn's child process needs.
    ///
    /// **`XDG_CONFIG_HOME` must be absolute.** A relative value made the CLI try
    /// to create the directory relative to its own working directory and die on
    /// `EEXIST: mkdir '..\\pC\\opencode'` — measured. `private_dir` returns an
    /// absolute path, so this is a property of the source rather than something
    /// enforced here, and it is written down because a future caller passing
    /// something else would get a failure that names neither Schemaic nor the
    /// variable.
    ///
    /// `OPENCODE_DISABLE_PROJECT_CONFIG` closes the second axis: OpenCode also
    /// reads an `opencode.json` or `.opencode/` from its working directory.
    /// `ai::session_cwd` is a private app directory, so there is nothing there to
    /// read today — this keeps that true if the cwd ever moves, the same way
    /// Claude's `--setting-sources` is defence in depth behind owning the cwd.
    /// **`OsString`, not `String`, and that is the third way the seal could
    /// silently open.** This returned `String`s built with `to_string_lossy`, so
    /// a config root that is not valid UTF-8 — a home directory in a legacy
    /// encoding, a Windows path with an unpaired surrogate — pointed
    /// `XDG_CONFIG_HOME` at a directory with the invalid bytes replaced by
    /// U+FFFD: *not* where the config was written. `--agent schemaic` then names
    /// an agent that does not exist, which OpenCode answers by falling back to
    /// its own `build` agent — every built-in tool, `bash` included — while the
    /// panel reports `Sealed`. `write`'s contract is that a failure to seal must
    /// **refuse**, and it is honoured for a failed write and a missing
    /// `private_dir`; a root that cannot survive a `String` round trip was the
    /// one way past it. `Command::env` takes `AsRef<OsStr>`, so the round trip
    /// was imposed only by this signature.
    pub(crate) fn env(&self) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        vec![
            (
                std::ffi::OsString::from("XDG_CONFIG_HOME"),
                self.root.clone().into_os_string(),
            ),
            (
                std::ffi::OsString::from("OPENCODE_DISABLE_PROJECT_CONFIG"),
                std::ffi::OsString::from("1"),
            ),
        ]
    }

    /// Variables that must be **cleared** from the child's inherited
    /// environment, not merely overridden.
    ///
    /// **The seal is otherwise reopened by the very levers this module rejected.**
    /// The docs above record that `OPENCODE_CONFIG` and `OPENCODE_CONFIG_CONTENT`
    /// *merge* rather than replace, which is why `XDG_CONFIG_HOME` is the lever —
    /// but a child inherits the parent's environment, so a user (or a shell
    /// profile, or a launcher) with `OPENCODE_CONFIG` already exported gets that
    /// file merged straight back in. Measured with both set at once: a foreign
    /// server appears alongside ours, inside a session the panel reports as
    /// `Sealed`. Setting our own variables is half the job; removing theirs is
    /// the other half.
    ///
    /// `OPENCODE_CONFIG_DIR` is cleared on the same principle. It did not move
    /// the resolved config when measured on its own, but it is a
    /// config-redirection variable by name and leaving it to be re-measured by
    /// the next person is how the other two got missed.
    pub(crate) fn env_remove() -> &'static [&'static str] {
        &[
            "OPENCODE_CONFIG",
            "OPENCODE_CONFIG_CONTENT",
            "OPENCODE_CONFIG_DIR",
        ]
    }

    /// Where the config was written, for logging and tests.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use schemaic_ai::harness::{MCP_SERVER, OPENCODE_AGENT, opencode_config_json};

    fn parse(allowed: &[&str]) -> serde_json::Value {
        serde_json::from_str(&opencode_config_json(
            r"C:\Program Files\schemaic\schemaic.exe",
            r"C:\tmp\schemaic-mcp-ep-abc.json",
            allowed,
        ))
        .expect("valid JSON")
    }

    #[test]
    fn the_agent_has_every_built_in_tool_switched_off() {
        // The seal itself. `tools` is a denylist by omission — a name absent from
        // the map stays *enabled* — so this asserts the map is non-empty and that
        // nothing in it is left on, rather than spot-checking `bash`.
        let v = parse(&["mcp__schemaic__list_schema"]);
        let tools = v["agent"][OPENCODE_AGENT]["tools"]
            .as_object()
            .expect("tools map");
        assert!(!tools.is_empty());
        for (name, on) in tools {
            assert_eq!(on, &serde_json::Value::Bool(false), "{name} left enabled");
        }
        // The four that would let a SQL assistant reach the machine, named
        // explicitly: a rename upstream drops them from the map silently, and
        // "the map is all false" would still pass with them gone.
        for must in ["bash", "read", "write", "edit"] {
            assert!(tools.contains_key(must), "{must} not disabled");
        }
    }

    #[test]
    fn the_agent_the_config_defines_is_the_one_the_argv_selects() {
        // The composition, not the constant: `turn_args` passes `--agent <name>`
        // and this file defines it. If they ever disagree the run silently falls
        // back to OpenCode's own `build` agent, which has every tool — the seal
        // absent rather than reported missing.
        let v = parse(&[]);
        let spec = schemaic_ai::harness::TurnSpec {
            prompt: "hi".into(),
            ..Default::default()
        };
        let args = schemaic_ai::harness::turn_args(schemaic_ai::harness::Harness::OpenCode, &spec);
        let i = args.iter().position(|a| a == "--agent").expect("--agent");
        let selected = &args[i + 1];
        let defined = v["agent"].get(selected);
        assert!(
            defined.is_some(),
            "argv selects `{selected}`, config defines {:?}",
            v["agent"].as_object().map(|o| o.keys().collect::<Vec<_>>())
        );
        // **Present is not enough, and this is the half that is not vacuous.**
        // Both sides read `OPENCODE_AGENT`, so a rename can never break the
        // lookup above — what it *can* catch is the JSON growing a different
        // shape around the name (an `agents` table, a nested `config` wrapper, a
        // definition that no longer carries the seal). The agent the argv selects
        // has to be the one with the tools switched off, or `--agent` names
        // something that is not sealed.
        let tools = defined
            .and_then(|d| d.get("tools"))
            .and_then(|t| t.as_object())
            .expect("the selected agent carries a tools map");
        assert!(tools.values().all(|v| v == &serde_json::Value::Bool(false)));
        assert!(tools.contains_key("bash"));
    }

    #[test]
    fn the_endpoint_path_is_configured_but_the_endpoint_is_not() {
        // The reused-directory rule: this file outlives the session, so it may
        // carry the path and never the blob behind it.
        let v = parse(&["mcp__schemaic__run_query"]);
        let cmd = v["mcp"][MCP_SERVER]["command"]
            .as_array()
            .expect("command array")
            .iter()
            .map(|s| s.as_str().unwrap_or_default().to_string())
            .collect::<Vec<_>>();
        assert_eq!(cmd[1], "--mcp-serve");
        assert_eq!(cmd[2], "--endpoint-file");
        assert_eq!(cmd[3], r"C:\tmp\schemaic-mcp-ep-abc.json");
        let whole = v.to_string();
        assert!(!whole.contains("password"), "{whole}");
    }

    /// The seal has two halves and only one is `XDG_CONFIG_HOME`. These
    /// variables *merge* into the resolved config (measured), so a user with one
    /// exported would have their own servers inside a session reported as
    /// `Sealed` — inheriting an environment is enough to undo it.
    ///
    /// **The whole set, not a sample.** This checked two of the three, so
    /// `OPENCODE_CONFIG_DIR` could be dropped or misspelled with the suite
    /// green — and the doc on that entry says in as many words that leaving it
    /// unpinned "is how the other two got missed".
    #[test]
    fn every_config_lever_the_module_rejected_is_cleared_from_the_child() {
        assert_eq!(
            super::OpenCodeConfig::env_remove(),
            &[
                "OPENCODE_CONFIG",
                "OPENCODE_CONFIG_CONTENT",
                "OPENCODE_CONFIG_DIR",
            ]
        );
    }

    /// **"Two roots cannot race" was argued in a paragraph and asserted
    /// nowhere**, and the decision is two string literals inlined between a
    /// `create_dir_all` and an `fs::write`. Changing `"opencode-inline"` to
    /// `"opencode"` is a plausible tidy-up — the subdirectory under each is
    /// already `opencode/` — and it would leave a Ctrl+K during a live chat
    /// overwriting that session's config with the server-less inline one, with
    /// the suite green.
    ///
    /// **And a third axis:** two Schemaic *instances* must not share a root
    /// either. The file carries the endpoint-file path and the offered tool
    /// names, which is exactly what differs per connection, and OpenCode is a
    /// process per turn — so a second window's session used to re-point the
    /// first's next question at its own database and access level.
    #[test]
    fn no_two_configs_that_must_differ_can_land_on_one_path() {
        let Some(session) = super::instance_root("opencode") else {
            // No config directory on this machine: nothing to assert about
            // roots that cannot be created. `write` returns `None` there and the
            // caller refuses the session.
            return;
        };
        let inline = super::instance_root("opencode-inline").expect("a sibling root");
        assert_ne!(
            session, inline,
            "a chat session and a one-shot would share one opencode.json"
        );
        // Both really are the file the CLI reads, so the inequality above is
        // about the thing that matters rather than about two unrelated paths.
        assert_eq!(
            session.file_name(),
            inline.file_name(),
            "the instance component must be the same on both, or this test is \
             comparing the wrong halves"
        );
        // The instance component is what a second window differs by, and it is
        // present rather than assumed.
        let tag = super::instance_tag();
        assert!(session.ends_with(&tag), "{session:?}");
        assert!(inline.ends_with(&tag), "{inline:?}");
        assert_ne!(tag, "pid-", "the tag carries no instance identity");
    }

    #[test]
    fn nothing_the_seal_depends_on_is_cleared_by_accident() {
        // The two halves must not fight: anything `env()` sets has to survive
        // `env_remove()`, or the isolation removes its own lever.
        let cfg = super::OpenCodeConfig {
            root: std::path::PathBuf::from("/tmp/x"),
        };
        let cleared = super::OpenCodeConfig::env_remove();
        for (k, _) in cfg.env() {
            let k = k.to_string_lossy().into_owned();
            assert!(
                !cleared.contains(&k.as_str()),
                "{k} is both set and cleared"
            );
        }
        // **The value reaches the child as the bytes on disk**, not as a
        // `to_string_lossy` of them. A root that is not valid UTF-8 used to
        // point `XDG_CONFIG_HOME` at a directory with U+FFFD where the invalid
        // bytes were — somewhere the config was never written — and
        // `--agent schemaic` naming an agent that does not exist leaves the run
        // on OpenCode's `build` agent, which has `bash`, while the panel reports
        // `Sealed`.
        let (_, v) = cfg
            .env()
            .into_iter()
            .find(|(k, _)| k == "XDG_CONFIG_HOME")
            .expect("the seal's lever");
        assert_eq!(std::path::Path::new(&v), cfg.root());
        // And the auth the session needs is untouched: `auth.json` lives under
        // the *data* directory, so clearing a data or home variable would log
        // the user out of their own CLI rather than isolate anything.
        for keep in ["XDG_DATA_HOME", "HOME", "USERPROFILE", "OPENCODE_API_KEY"] {
            assert!(!cleared.contains(&keep), "{keep} must not be cleared");
        }
    }

    #[test]
    fn only_our_server_is_configured() {
        // Assigned, not merged: the `mcp` table names exactly one server, which
        // is what pointing `XDG_CONFIG_HOME` at this directory makes total.
        let v = parse(&[]);
        let servers = v["mcp"].as_object().expect("mcp table");
        assert_eq!(servers.keys().collect::<Vec<_>>(), vec![MCP_SERVER]);
    }

    #[test]
    fn the_offered_tools_are_named_by_their_bare_names() {
        // The description is what tells the model which database tools exist;
        // the allow-list speaks qualified names and OpenCode's server speaks
        // bare ones.
        let v = parse(&["mcp__schemaic__list_schema", "mcp__schemaic__run_query"]);
        let d = v["agent"][OPENCODE_AGENT]["description"]
            .as_str()
            .unwrap_or_default();
        assert!(d.contains("list_schema"), "{d}");
        assert!(d.contains("run_query"), "{d}");
        assert!(!d.contains("mcp__"), "{d}");
    }

    #[test]
    fn a_read_only_connection_never_advertises_run_query() {
        // The access level reaches the config, exactly as it reaches Claude's
        // `--allowedTools` and Codex's per-tool approvals.
        let v = parse(&["mcp__schemaic__list_schema"]);
        let d = v["agent"][OPENCODE_AGENT]["description"]
            .as_str()
            .unwrap_or_default();
        assert!(!d.contains("run_query"), "{d}");
    }
}
