//! The working directory a Cursor session runs in, the two files in it that
//! restrict and configure the session, the one approval the CLI needs before
//! it will load our server, and the sweep that takes back what the CLI keeps
//! about that directory in the user's own `~/.cursor`.
//!
//! # The restriction is a file, and a missing file fails open
//!
//! Cursor has no flag that empties its tools and no sandbox measured to hold
//! headless. What it does have is a project permissions file,
//! `<cwd>/.cursor/cli.json`, and that is what `harness::cursor_permissions_json`
//! writes: our tools allowed by name, writers, the shell and fetching denied.
//! Measured (2026-09-23 build): those denials held against a model trying hard,
//! a `Task` subagent included, and beat an allow in the user's own global
//! config. **Without the file, a headless turn wrote a file unprompted** — the
//! shell and MCP calls fail closed, writes do not. So [`CursorWorkspace::write`]
//! returning `None` refuses the session, as OpenCode's missing config does, and
//! for the same reason: the grade is only honest because the refusal is there.
//!
//! **Its readers are not closed, and the notice says so.** `Read(**)` is denied
//! and refused the `Read` tool; `Grep` still returned a file's contents.
//!
//! # A directory the CLI finds by being started in it
//!
//! Both files are *project* files, found from the working directory, so the
//! child's working directory is the configuration. It is per instance rather
//! than per session, from `crate::opencode::instance_root`, and reused within
//! the instance: Cursor keys a conversation's saved state by the directory it
//! ran in, so a directory per session would scatter one entry per question
//! across the user's `~/.cursor`, and a directory per *turn* would lose the
//! conversation. The files carry the endpoint file's path, never the endpoint.
//!
//! # One approval, and state in the user's config that has to be taken back
//!
//! A project MCP server loads only once approved — measured, "not loaded (needs
//! approval)" and every call refused until then. [`approve`] runs
//! `cursor-agent mcp enable schemaic` in the workspace, which approves our
//! server and nothing else. The alternative, `--approve-mcps`, approves every
//! server in reach, a user's plugins' among them. The record is keyed by a hash
//! of the config, so it is asked again per session.
//!
//! That record, and the conversations, land in the user's Cursor config
//! directory: `projects/<slug>/` (whose `.workspace-trusted` names the
//! workspace, and which holds the approvals) and `chats/<hash>/<chat>/` (whose
//! `meta.json` names it too). This is Antigravity's position — global state that
//! outlives the process — with one difference that makes the cleanup safe:
//! every entry is **found by its content naming one of Schemaic's own
//! directories**, never by recomputing the CLI's slug or hash, so nothing of
//! the user's own can match. [`sweep`] removes the entries of instances that are
//! gone.
//!
//! # And what cannot be closed
//!
//! The user's own `~/.cursor/mcp.json` is read from a path the CLI hard-codes
//! to the home directory, and its servers need no approval: measured, they are
//! started on every turn. Their tools are listed to the model and refused when
//! called, because no allow rule here names them — unless the user's own
//! settings do. `Harness::isolates_mcp_servers` is false for this harness, and
//! the notice says it.

use std::path::{Path, PathBuf};

use schemaic_ai::harness::MCP_SERVER;

/// The workspace kinds, as `opencode::instance_root` names them.
const KINDS: [&str; 2] = ["cursor", "cursor-inline"];

/// A Cursor working directory, written and ready to start the CLI in.
pub(crate) struct CursorWorkspace {
    root: PathBuf,
    /// Whether this workspace configures our server — and so needs [`approve`].
    has_server: bool,
}

impl CursorWorkspace {
    /// The workspace for a chat session: the permissions file for `allowed`,
    /// and our server when there is an endpoint file to point it at.
    ///
    /// `None` when either file could not be written, and the caller **refuses
    /// the session** — see the module docs for why a missing permissions file
    /// is not a degraded state. With no endpoint, any `mcp.json` an earlier
    /// session left is removed, so the CLI is not handed a server pointing at a
    /// file that no longer exists.
    pub(crate) fn write(
        exe: &str,
        endpoint_file: Option<&str>,
        allowed: &[String],
    ) -> Option<Self> {
        let root = crate::opencode::instance_root(KINDS[0])?;
        let dot = root.join(".cursor");
        std::fs::create_dir_all(&dot).ok()?;
        std::fs::write(
            dot.join("cli.json"),
            schemaic_ai::harness::cursor_permissions_json(allowed),
        )
        .ok()?;
        let mcp = dot.join("mcp.json");
        let has_server = match endpoint_file {
            Some(ep) => {
                std::fs::write(&mcp, schemaic_ai::harness::cursor_mcp_config_json(exe, ep)).ok()?;
                true
            }
            None => {
                if mcp.exists() {
                    std::fs::remove_file(&mcp).ok()?;
                }
                false
            }
        };
        Some(Self { root, has_server })
    }

    /// The workspace for a one-shot: the same denials, nothing allowed, no
    /// server. A root of its own, for OpenCode's reason — the session's
    /// `mcp.json` must not be lying in a one-shot's directory.
    pub(crate) fn write_inline() -> Option<Self> {
        let root = crate::opencode::instance_root(KINDS[1])?;
        let dot = root.join(".cursor");
        std::fs::create_dir_all(&dot).ok()?;
        std::fs::write(
            dot.join("cli.json"),
            schemaic_ai::harness::cursor_permissions_json(&[]),
        )
        .ok()?;
        Some(Self {
            root,
            has_server: false,
        })
    }

    /// The directory to start the CLI in.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Whether [`approve`] is owed before the first turn.
    pub(crate) fn has_server(&self) -> bool {
        self.has_server
    }
}

/// How long `mcp enable` may take. It starts node and writes one file; the
/// measured runs took a few seconds, and a hung one must not hold the session.
const APPROVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Approve our server — and only ours — for `workspace`: `cursor-agent mcp
/// enable schemaic`, run there.
///
/// Blocking; the caller runs it off the async workers. `Err` carries what the
/// CLI said, and the session then runs with no database tools and says so,
/// Antigravity's shape for a registration that failed.
pub(crate) fn approve(launch: &crate::agent_cli::Launch, workspace: &Path) -> Result<(), String> {
    let mut c = launch.std_command();
    c.args(["mcp", "enable", MCP_SERVER])
        .current_dir(workspace)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = c.spawn().map_err(|e| e.to_string())?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > APPROVE_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("`mcp enable` did not finish".to_string());
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    match out.status.success() {
        true => Ok(()),
        false => Err(schemaic_ai::cli_failure_message(
            schemaic_ai::harness::Harness::Cursor,
            out.status.code(),
            &String::from_utf8_lossy(&out.stdout),
            &String::from_utf8_lossy(&out.stderr),
        )),
    }
}

/// Where the CLI keeps its configuration and per-workspace state, by its own
/// rule (read out of its bundle): `CURSOR_CONFIG_DIR`, else
/// `$XDG_CONFIG_HOME/cursor`, else `<home>/.cursor`.
fn cursor_config_dir() -> Option<PathBuf> {
    let set = |k: &str| std::env::var_os(k).filter(|v| !v.to_string_lossy().trim().is_empty());
    if let Some(d) = set("CURSOR_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    if let Some(x) = set("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(x).join("cursor"));
    }
    // Node's `os.homedir()`: `USERPROFILE` on Windows, `HOME` elsewhere.
    let home = match cfg!(windows) {
        true => set("USERPROFILE"),
        false => set("HOME"),
    }?;
    Some(PathBuf::from(home).join(".cursor"))
}

/// The instance pid of a Schemaic Cursor workspace, when `path` is one.
///
/// **This is the whole safety of the sweep**: an entry in the user's Cursor
/// directory is removed only when the path it records is exactly
/// `<one of our bases>/pid-<n>`. Compared case-insensitively on Windows, where
/// the CLI records whatever its process reports as the working directory.
fn workspace_pid(path: &str, bases: &[PathBuf]) -> Option<u32> {
    let p = Path::new(path);
    let pid = p
        .file_name()?
        .to_str()?
        .strip_prefix("pid-")?
        .parse::<u32>()
        .ok()?;
    let parent = p.parent()?;
    let same = |a: &Path, b: &Path| match cfg!(windows) {
        true => a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase(),
        false => a == b,
    };
    bases.iter().any(|b| same(parent, b)).then_some(pid)
}

/// The workspace a Cursor state file records, if it records one.
fn recorded_workspace(file: &Path, field: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    v.get(field)?.as_str().map(str::to_string)
}

/// Remove the workspaces of instances that are gone, and what the CLI keeps
/// about them in the user's Cursor directory. See the module docs.
///
/// Called at startup beside the other sweeps. Each removal is best-effort: a
/// failure leaves an entry for the next launch, never touches anything else.
pub(crate) fn sweep() {
    let bases: Vec<PathBuf> = KINDS
        .iter()
        .filter_map(|k| schemaic_core::persist::private_dir(k))
        .collect();
    let dead = |path: &str| {
        workspace_pid(path, &bases).is_some_and(|pid| crate::liveness::process_start(pid).is_none())
    };
    // **Only walk the user's Cursor directory when there is something of ours
    // to find in it.** Its `chats/` holds one entry per conversation the user
    // has ever had with the CLI, and every one would otherwise be opened and
    // parsed on every Schemaic launch — including for users who have never
    // picked Cursor here. An entry of ours exists only while the workspace it
    // names does: this sweep removes the two together, the state first.
    let any_dead = bases.iter().any(|b| {
        std::fs::read_dir(b)
            .is_ok_and(|entries| entries.flatten().any(|e| dead(&e.path().to_string_lossy())))
    });
    if !any_dead {
        return;
    }
    if let Some(cfg) = cursor_config_dir() {
        // `projects/<slug>/.workspace-trusted` → `workspacePath`.
        if let Ok(entries) = std::fs::read_dir(cfg.join("projects")) {
            for e in entries.flatten() {
                let trusted = e.path().join(".workspace-trusted");
                if recorded_workspace(&trusted, "workspacePath").is_some_and(|w| dead(&w)) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
        // `chats/<hash>/<chat>/meta.json` → `cwd`.
        if let Ok(groups) = std::fs::read_dir(cfg.join("chats")) {
            for g in groups.flatten() {
                let Ok(chats) = std::fs::read_dir(g.path()) else {
                    continue;
                };
                let mut removed = false;
                for c in chats.flatten() {
                    if recorded_workspace(&c.path().join("meta.json"), "cwd")
                        .is_some_and(|w| dead(&w))
                    {
                        removed |= std::fs::remove_dir_all(c.path()).is_ok();
                    }
                }
                // Empty-only, and only where we emptied it.
                if removed {
                    let _ = std::fs::remove_dir(g.path());
                }
            }
        }
    }
    crate::opencode::sweep_instances(&KINDS);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bases() -> Vec<PathBuf> {
        let config = Path::new("/schemaic-fixture-config");
        KINDS
            .iter()
            .map(|k| schemaic_core::persist::private_dir_in(config, k))
            .collect()
    }

    fn inside(kind: usize, leaf: &str) -> String {
        bases()[kind].join(leaf).to_string_lossy().into_owned()
    }

    #[test]
    fn only_our_own_workspaces_are_recognised() {
        assert_eq!(workspace_pid(&inside(0, "pid-42"), &bases()), Some(42));
        assert_eq!(workspace_pid(&inside(1, "pid-7"), &bases()), Some(7));
        // A user's own project, whatever its name.
        assert_eq!(workspace_pid("/home/u/src/pid-42", &bases()), None);
        // Inside ours, but not an instance root.
        assert_eq!(workspace_pid(&inside(0, "pid-42x"), &bases()), None);
        assert_eq!(workspace_pid(&inside(0, "other"), &bases()), None);
        // One level too deep: a user who put a project under our directory is
        // still not ours to delete.
        let deeper = Path::new(&inside(0, "pid-42"))
            .join("pid-43")
            .to_string_lossy()
            .into_owned();
        assert_eq!(workspace_pid(&deeper, &bases()), None);
        // OpenCode's roots share the helper but are not Cursor's to sweep.
        let oc = schemaic_core::persist::private_dir_in(
            Path::new("/schemaic-fixture-config"),
            "opencode",
        )
        .join("pid-42");
        assert_eq!(workspace_pid(&oc.to_string_lossy(), &bases()), None);
    }

    #[test]
    fn a_session_and_a_one_shot_never_share_a_workspace() {
        let config = Path::new("/schemaic-fixture-config");
        let session = crate::opencode::instance_path_in(config, KINDS[0]);
        let inline = crate::opencode::instance_path_in(config, KINDS[1]);
        assert_ne!(session, inline);
    }
}
