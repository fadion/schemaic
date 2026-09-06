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
//!   startup and removes any Schemaic registration and rules it finds *that no
//!   live instance has claimed*, because a permission the user does not remember
//!   granting is exactly what must not outlive the process that needed it.
//! - **The settings file is merged, never rewritten.** It is the user's, it holds
//!   their `trustedWorkspaces`, and Antigravity rewrites it itself. The surgery
//!   is pure and tested in `schemaic_ai::harness`; this module only does the IO.
//!
//! **Two Schemaic instances, and how the claim tells them apart.** The state is
//! global to the machine but the sweep runs per process, so [`sweep`] used to
//! remove a *live* instance's registration and rules the moment a second
//! Schemaic launched — leaving that session holding database tools which had
//! silently stopped existing. [`AgyRegistration::install`] now writes a marker
//! naming the process that owns them, and the sweep defers only while that exact
//! process is still running.
//!
//! **The pid alone would not do it**, which is why the marker carries a start
//! time as well: pids are reissued, so a crashed instance's marker eventually
//! names some unrelated live process, and a sweep trusting the number would
//! decline to clean up for the rest of that pid's life — turning a transient
//! crash into the permanent standing grant this whole module exists to prevent.
//! An unreadable or absent marker is treated as no claim at all, in the same
//! direction and for the same reason.
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

/// Which process claimed Antigravity's global state, as the marker records it.
///
/// **The start time is not decoration.** A pid alone cannot tell a live claim
/// from a dead one: the operating system hands pids out again, so a marker left
/// by a crashed instance eventually names some unrelated process that is very
/// much running, and a sweep that trusted the pid would then decline to clean up
/// for the rest of that pid's life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Owner {
    pid: u32,
    started: u64,
}

/// Render a marker. One line, two fields — this file is written by one process
/// and read by another, so the format is the interface between them.
fn marker_text(o: Owner) -> String {
    format!("{} {}\n", o.pid, o.started)
}

/// Read a marker back, or `None` for anything that is not one.
///
/// A truncated or garbled marker answers `None`, which [`may_sweep`] treats as
/// *no claim* — the same direction as no file at all. The alternative, refusing
/// to sweep on an unreadable marker, would leave a standing permission grant in
/// the user's config with nothing able to withdraw it.
fn parse_marker(s: &str) -> Option<Owner> {
    let mut it = s.split_whitespace();
    let pid = it.next()?.parse().ok()?;
    let started = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some(Owner { pid, started })
}

/// May the startup sweep remove the registration and the allow-rules?
///
/// `live` is the start time of the process the marker names, if that pid is
/// running at all.
fn may_sweep(marker: Option<Owner>, live: Option<u64>, me: u32) -> bool {
    let Some(o) = marker else {
        return true;
    };
    // Our own pid, at startup: this process has only just begun, so it cannot be
    // the instance in the middle of the session that wrote this.
    if o.pid == me {
        return true;
    }
    // A claim stands only while the process that made it is the one still on
    // that pid. Anything else — gone, or replaced — is a crash's leftovers.
    live != Some(o.started)
}

/// Where the claim is recorded. Beside the app's own state rather than in the
/// shared temp directory: it is a claim on *this* machine's Antigravity config,
/// and world-writable is the wrong permission for something a sweep obeys.
fn marker_path() -> Option<PathBuf> {
    Some(schemaic_core::persist::private_dir("agy")?.join("registration-owner"))
}

/// A process's start time, if that pid is running at all.
///
/// The impure half of [`may_sweep`], kept to one line of answer so the decision
/// itself stays testable without a process to look at.
fn process_start(pid: u32) -> Option<u64> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let p = Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[p]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(p).map(|pr| pr.start_time())
}

/// Claim the global state for this process, so another instance's startup sweep
/// leaves it alone.
fn claim() {
    let me = std::process::id();
    let Some(started) = process_start(me) else {
        // Without a start time the claim could not be told from a pid-reuse, and
        // a claim that cannot expire is worse than none: it would strand the
        // allow-rules the sweep exists to withdraw.
        return;
    };
    let Some(path) = marker_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, marker_text(Owner { pid: me, started }));
}

/// Drop this process's claim.
fn release() {
    if let Some(path) = marker_path() {
        let _ = std::fs::remove_file(path);
    }
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
            // And say who owns them, so a second Schemaic's startup sweep does
            // not withdraw this session's grant while it is still using it.
            claim();
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
        // The claim goes last, and only after the state it claimed is gone: a
        // release that ran first would open a window in which another instance's
        // sweep could race this one's own removal.
        release();
    }
}

/// Remove any Schemaic registration and allow-rules left behind by a session
/// that never got to run its [`Drop`] — a crash, a kill, a power loss.
///
/// Called once at startup, and only when `agy` is actually present: shelling out
/// to a binary the user does not have, on every launch, to clean up state that
/// cannot exist, is a cost paid by everyone for a case that applies to nobody.
pub(crate) fn sweep(bin: Option<&str>) {
    // **Not while another Schemaic is using it.** This runs at every launch, and
    // the state it cleans is global to the machine rather than to a process, so
    // a second window opening was enough to withdraw the first's registration
    // and allow-rules — leaving that session holding database tools that had
    // silently stopped existing. A claim written by `install` says who owns it;
    // only a claim whose owner is gone is a crash's leftovers.
    let owner = marker_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .as_deref()
        .and_then(parse_marker);
    if !may_sweep(
        owner,
        owner.and_then(|o| process_start(o.pid)),
        std::process::id(),
    ) {
        return;
    }
    release();
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

    const ME: u32 = 4242;

    /// The bug this marker exists for: the sweep runs at **every** launch and
    /// removed the registration unconditionally, so starting a second Schemaic
    /// while the first had a live AI session pulled that session's server and
    /// allow-rules out from under it. The first instance then held a session
    /// whose database tools had silently stopped existing.
    #[test]
    fn a_second_instance_does_not_sweep_a_live_instances_registration() {
        let owner = Owner {
            pid: 1234,
            started: 900,
        };
        assert!(
            !may_sweep(Some(owner), Some(900), ME),
            "swept a registration whose owner is still running"
        );
    }

    /// …and the case the sweep exists for is untouched: an owner that is gone.
    #[test]
    fn a_crashed_instances_registration_is_still_swept() {
        let owner = Owner {
            pid: 1234,
            started: 900,
        };
        // The pid is not running at all.
        assert!(may_sweep(Some(owner), None, ME));
        // The pid is running, but it is not the process that wrote the marker —
        // the operating system handed that number to something else. Trusting
        // the pid alone would decline to clean up for the rest of its life.
        assert!(may_sweep(Some(owner), Some(901), ME));
    }

    /// No claim, no reason to defer — including a marker too damaged to read,
    /// which must not be able to strand a permission grant in the user's config.
    #[test]
    fn an_absent_or_unreadable_marker_is_not_a_claim() {
        assert!(may_sweep(None, None, ME));
        for junk in [
            "",
            "   ",
            "not-a-pid 900",
            "1234",
            "1234 900 extra",
            "1234 x",
        ] {
            assert_eq!(parse_marker(junk), None, "{junk:?} parsed as a marker");
        }
    }

    /// A marker naming *us* at startup is our own leftover — this process has
    /// only just begun, so it cannot be in the middle of a session it owns.
    #[test]
    fn our_own_stale_marker_does_not_stop_us() {
        let mine = Owner {
            pid: ME,
            started: 900,
        };
        assert!(may_sweep(Some(mine), Some(900), ME));
    }

    #[test]
    fn a_marker_round_trips() {
        let o = Owner {
            pid: 31337,
            started: 1_700_000_000,
        };
        assert_eq!(parse_marker(&marker_text(o)), Some(o));
    }
}
