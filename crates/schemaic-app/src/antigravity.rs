//! Antigravity's two pieces of global state, owned for the life of one session.
//!
//! Every other harness is configured per invocation: Claude gets a temp
//! `--mcp-config` file, Codex gets `-c` overrides. Antigravity has neither. Its
//! MCP servers live in a user-level config managed by `agy mcp add/remove`, and
//! its tool permissions live in a `settings.json` it shares with itself — so
//! giving it Schemaic's database tools means **writing into the user's own
//! configuration** and taking it back out again.
//!
//! That is worth stating plainly because it is the part that can go wrong
//! quietly:
//!
//! - **Both are installed together and removed together.** A registration
//!   without its allow-rules is a server whose every call is refused; rules
//!   without a registration are a standing grant for a server that is not there.
//!   [`AgyRegistration`] owns both and its `Drop` removes both.
//! - **A crash leaves both behind**, and neither expires. [`sweep`] runs at
//!   startup and removes any Schemaic registration and rules it finds, because a
//!   permission the user does not remember granting is exactly what must not
//!   outlive the process that needed it.
//! - **The settings file is merged, never rewritten.** It is the user's, it holds
//!   their `trustedWorkspaces`, and Antigravity rewrites it itself. The surgery
//!   is pure and tested in `schemaic_ai::harness`; this module only does the IO.
//!
//! **Known limit: two Schemaic instances.** [`sweep`] cannot tell a registration
//! left by a crash from one belonging to a second running instance, so starting
//! a second Schemaic removes the first's rules until that session next starts
//! one. The alternative — leaving them on the chance somebody is using them — is
//! a standing grant nobody remembers making, which is the worse failure.
//!
//! **Known limit: the server name is not ours to reserve.** `agy mcp add` is an
//! upsert and `agy mcp remove` is unconditional, so a user who has registered
//! their *own* MCP server under the name `schemaic` — or one pointing at a
//! different Schemaic build — has it replaced on the first AI turn and deleted
//! by the next [`sweep`]. Telling ours from theirs needs a read of that CLI's
//! registry (`agy mcp list`) whose output has not been measured, and guessing at
//! a format in order to decide whether to delete somebody's configuration is
//! worse than the collision. Recorded rather than guarded, and the name is a
//! single [`SERVER`] constant so a future check has one place to hook.

use schemaic_ai::harness::{
    antigravity_allow_rules, antigravity_settings_with_rules, antigravity_settings_without_rules,
};
use std::path::PathBuf;

/// The MCP server name registered with `agy`. Also the `schemaic/` half of every
/// allow-rule, so the two halves cannot drift apart.
const SERVER: &str = "schemaic";

/// Where Antigravity keeps the permissions file.
///
/// The `.gemini` in the path is not a leftover: `agy` is Google's, and it keeps
/// its own settings under the same home-directory root the Gemini CLI used.
///
/// Observed rather than documented, so it is overridable: `$SCHEMAIC_AGY_SETTINGS`
/// exists for the case where that CLI moves it and this constant is wrong — the
/// failure is then "no database tools" rather than "wrote into the wrong file".
fn settings_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SCHEMAIC_AGY_SETTINGS")
        && !p.trim().is_empty()
    {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(
        PathBuf::from(home)
            .join(".gemini")
            .join("antigravity-cli")
            .join("settings.json"),
    )
}

/// The document to hand `edit`, or `None` to decline — the filesystem modelled
/// at the boundary so the rule itself has a test.
///
/// **`create` is what separates granting a rule from withdrawing one.** A
/// missing file is a fresh install, so adding rules creates it; *removing* them
/// must not. The cleanup half ran unconditionally at every startup, and for the
/// overwhelming majority of users — everyone who has never installed Antigravity
/// — the sequence was: read fails, the pure layer treats empty as an empty
/// document, removing nothing from it yields `{}`, that differs from the empty
/// string, and Schemaic writes `{}` into a directory it creates inside another
/// vendor's config tree. An app that has never run that CLI has no business
/// leaving a file where it keeps its settings.
///
/// `existing` is `None` when the file could not be read, which for this CLI
/// means it is not there.
fn settings_to_edit(create: bool, existing: Option<String>) -> Option<String> {
    match (existing, create) {
        (Some(cur), _) => Some(cur),
        // A fresh install: the pure layer treats empty as an empty document.
        (None, true) => Some(String::new()),
        // Nothing to withdraw from, so nothing to write.
        (None, false) => None,
    }
}

/// Rewrite the settings file through `edit`, which is handed the current text.
///
/// Does nothing when [`settings_to_edit`] declines, or when `edit` does (an
/// unparseable document — see `antigravity_settings_with_rules`). Returns
/// whether it wrote.
fn edit_settings(create: bool, edit: impl Fn(&str) -> Option<String>) -> bool {
    let Some(path) = settings_path() else {
        return false;
    };
    let Some(current) = settings_to_edit(create, std::fs::read_to_string(&path).ok()) else {
        return false;
    };
    let Some(next) = edit(&current) else {
        return false;
    };
    if next == current {
        return true;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&path, next).is_ok()
}

/// Run one `agy mcp …` subcommand, discarding its output.
///
/// Best effort by design: if `agy` is missing or refuses, the session simply has
/// no database tools, which the panel already reports through the tool call that
/// then fails. Failing the whole session over a config write would be worse.
fn agy_mcp(bin: &str, args: &[&str]) -> bool {
    std::process::Command::new(bin)
        .arg("mcp")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Antigravity's global state for one session: the MCP registration and the
/// allow-rules that make it callable. Removed on drop.
pub(crate) struct AgyRegistration {
    bin: String,
    rules: Vec<String>,
    /// Whether the registration actually took. A failed install must not have
    /// its `Drop` remove a server this session never added.
    installed: bool,
}

impl AgyRegistration {
    /// Register the server and grant exactly `allowed`.
    ///
    /// `allowed` is the connection's own tool list, so a schema-only connection
    /// never grants `run_query` — the same rule the Codex overrides and Claude's
    /// `--allowedTools` follow.
    pub(crate) fn install(bin: &str, exe: &str, endpoint_file: &str, allowed: &[&str]) -> Self {
        // `--` first: the command's own arguments start with `-`, and `agy mcp
        // add` rejects a flag placed after the server name.
        let installed = agy_mcp(
            bin,
            &[
                "add",
                SERVER,
                "--",
                exe,
                "--mcp-serve",
                "--endpoint-file",
                endpoint_file,
            ],
        );
        let rules = antigravity_allow_rules(allowed);
        if installed {
            let r = rules.clone();
            // Granting: a fresh Antigravity install has no settings file yet, and
            // the rules are what its tools need to run at all.
            edit_settings(true, move |cur| antigravity_settings_with_rules(cur, &r));
        }
        Self {
            bin: bin.to_string(),
            rules,
            installed,
        }
    }

    /// Did the registration take? `false` means the session runs without
    /// database tools, which is worth saying rather than discovering.
    pub(crate) fn is_installed(&self) -> bool {
        self.installed
    }
}

/// **Blocking, and deliberately left that way.** This is the mirror of
/// [`AgyRegistration::install`], which was moved off the UI thread because a
/// multi-second freeze while a window is in front of someone is the failure that
/// matters. `Drop` runs when the session task ends, on one worker of a
/// multi-threaded runtime, with nobody waiting on that thread — and the
/// alternative, handing the removal to a detached thread, races process exit
/// with the one piece of work that must not be skipped: a standing permission
/// grant left in the user's own config. A stalled worker at session end is the
/// cheaper of the two.
impl Drop for AgyRegistration {
    fn drop(&mut self) {
        if !self.installed {
            return;
        }
        let rules = std::mem::take(&mut self.rules);
        // Withdrawing: only ever from a file that is there. This arm is reached
        // after a successful `install`, so it will be — but the flag is the
        // rule, not the reachability.
        edit_settings(false, move |cur| {
            antigravity_settings_without_rules(cur, &rules)
        });
        agy_mcp(&self.bin, &["remove", SERVER]);
    }
}

/// Remove any Schemaic registration and allow-rules left behind by a session
/// that never got to run its [`Drop`] — a crash, a kill, a power loss.
///
/// Called once at startup, and only when `agy` is actually present: shelling out
/// to a binary the user does not have, on every launch, to clean up state that
/// cannot exist, is a cost paid by everyone for a case that applies to nobody.
pub(crate) fn sweep(bin: Option<&str>) {
    // The rules can be removed with no `agy` at all — it is our own file surgery
    // — so that half runs regardless.
    let all: Vec<&str> = crate::ai::AI_TOOLS_WITH_QUERY.to_vec();
    let rules = antigravity_allow_rules(&all);
    // Never creates. This runs on **every** launch, for every user, and most of
    // them have no Antigravity at all — writing `{}` into that CLI's config
    // directory to withdraw rules nobody granted is the whole hazard `create`
    // exists to close.
    edit_settings(false, move |cur| {
        antigravity_settings_without_rules(cur, &rules)
    });
    if let Some(bin) = bin {
        agy_mcp(bin, &["remove", SERVER]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The sweep must not create the file it is cleaning.** It runs at every
    /// launch for every user, and most have no Antigravity: reading fails, the
    /// pure layer turns the empty string into `{}`, that differs from what was
    /// read, and Schemaic writes a file into another vendor's config directory —
    /// creating the directory on the way. The composition is what bites, so this
    /// checks the same pipeline the caller runs rather than the predicate alone.
    #[test]
    fn withdrawing_rules_from_a_file_that_is_not_there_writes_nothing() {
        let rules = antigravity_allow_rules(&["list_schema"]);
        // The sweep's arguments: no file, and no permission to create one.
        assert_eq!(settings_to_edit(false, None), None);
        // …and had it been allowed to proceed, this is what would have landed —
        // the write this test exists to prevent.
        let would_have_written =
            antigravity_settings_without_rules("", &rules).expect("the pure layer accepts empty");
        assert_eq!(would_have_written.trim(), "{}");

        // Granting is the other half and still creates: a fresh install has no
        // file, and its tools are refused without the rules.
        let fresh = settings_to_edit(true, None).expect("a document to edit");
        let granted = antigravity_settings_with_rules(&fresh, &rules).expect("merged");
        assert!(granted.contains("mcp(schemaic/list_schema)"), "{granted}");

        // An existing document is handed over verbatim either way.
        let doc = r#"{"permissions":{"allow":["mcp(other/thing)"]}}"#.to_string();
        assert_eq!(
            settings_to_edit(false, Some(doc.clone())),
            Some(doc.clone())
        );
        assert_eq!(settings_to_edit(true, Some(doc.clone())), Some(doc));
    }

    #[test]
    fn the_settings_path_is_overridable_for_a_cli_that_moves_it() {
        // Not a documented location — it was observed — so the override exists
        // to make being wrong cost tools rather than the wrong file.
        unsafe { std::env::set_var("SCHEMAIC_AGY_SETTINGS", "/tmp/zz-agy.json") };
        assert_eq!(settings_path(), Some(PathBuf::from("/tmp/zz-agy.json")));
        unsafe { std::env::remove_var("SCHEMAIC_AGY_SETTINGS") };
        // Without it, the observed path under the user's home.
        let p = settings_path().expect("a home directory");
        assert!(p.ends_with("settings.json"), "{p:?}");
        assert!(p.to_string_lossy().contains("antigravity-cli"), "{p:?}");
    }

    #[test]
    fn a_failed_registration_removes_nothing_on_drop() {
        // The `Drop` must not `agy mcp remove` a server this session never
        // added — that would tear down a *working* registration belonging to
        // another instance.
        let reg = AgyRegistration {
            bin: "zz-not-a-binary".to_string(),
            rules: vec!["mcp(schemaic/list_schema)".to_string()],
            installed: false,
        };
        assert!(!reg.is_installed());
        drop(reg); // must not panic, and must not touch anything
    }

    #[test]
    fn the_sweep_targets_every_tool_not_just_the_current_level() {
        // A crashed session may have granted the full set, so the sweep has to
        // clear the full set — clearing only a schema-only connection's two
        // would leave `run_query` granted forever.
        let all: Vec<&str> = crate::ai::AI_TOOLS_WITH_QUERY.to_vec();
        let rules = antigravity_allow_rules(&all);
        assert!(rules.iter().any(|r| r.contains("run_query")), "{rules:?}");
        assert!(rules.len() >= crate::ai::AI_TOOLS_READ_ONLY.len());
    }
}
