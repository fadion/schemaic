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
//! silently stopped existing. [`AgyRegistration::install`] writes a [`Claim`]
//! naming the session that owns them, and both removers defer to it.
//!
//! **Both removers, which is the part that was missing.** A claim that governs
//! one and not the other governs nothing: the sweep consulted the marker while
//! `Drop`, eighteen lines away, performed the same three removals and consulted
//! nothing — so a second window's session *ending* disarmed a live one just as
//! surely as its starting used to. [`may_release`] is now the same question
//! [`crate::liveness::may_sweep`] asks, and neither remover has one of its own.
//!
//! **Three fields, and each closes a different way of getting this wrong.** The
//! pid says who; the start time survives pid reuse, so a crashed instance's
//! marker cannot name some unrelated live process and freeze the sweep for the
//! rest of that pid's life (that reasoning lives in [`crate::liveness`]); and
//! the nonce separates two sessions of *one* process, which a respawn produces
//! and which the first two fields cannot tell apart. An unreadable or absent
//! marker is treated as no claim at all, in the same direction and for the same
//! reason.
//!
//! **The claim goes first and is dropped last.** It used to be written *after*
//! the registration and the grant, leaving a window in which machine-global
//! state existed that no marker accounted for; it is written atomically, because
//! its reader is another process and a torn `fs::write` read as no claim at all.
//!
//! **Known limit: the grant is user-global while it stands, and cannot be
//! narrowed.** `agy mcp add` writes into the user's own MCP config and
//! `permissions.allow` is an allow-list rather than a prompt-list, so for as
//! long as a Schemaic AI session is open, *any* `agy` run by this user — in any
//! directory, started by anything — finds the `schemaic` server registered and
//! its database tools pre-approved, with no prompt. `cd ~/work/some-repo && agy
//! -p "explain this build failure"` can therefore reach the database through a
//! `README` or `AGENTS.md` the user did not write.
//!
//! No lever was found that scopes either half to one process: the registration
//! is per user by construction, and a per-session server *name* would not help,
//! since an unrelated run inherits whatever name is registered. What is left is
//! to keep the window as narrow as it can be — the grant is installed when a
//! session starts and withdrawn when it ends, and covers only the tools that
//! connection's access level offers, so a schema-only connection never grants
//! `run_query` — and to say so rather than let it be assumed shut.
//! `Constraint::notice` tells the user, in the panel, that this harness's MCP
//! surface is not Schemaic's to restrict.
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
    SettingsEdit, antigravity_allow_rules, antigravity_settings_with_rules,
    antigravity_settings_without_rules,
};
use schemaic_core::persist;
use std::path::PathBuf;

/// The MCP server name registered with `agy`. Also the `<server>/` half of every
/// allow-rule, so the two halves cannot drift apart.
///
/// **It is one constant now, and the claim that it always was is what made this
/// worth fixing.** `agy mcp add` was given a private `SERVER` in this crate
/// while `antigravity_allow_rules` emitted the literal `mcp(schemaic/…)` in
/// `schemaic-ai`, which cannot see it — two independent spellings of one name,
/// documented as one. Renaming either half auto-*denies* every tool call, and
/// per this module's header a denied Antigravity turn still reports
/// `"status":"SUCCESS"` with an empty response, so the failure would have been
/// a silent one.
pub(crate) const SERVER: &str = schemaic_ai::harness::MCP_SERVER;

/// Where Antigravity keeps the permissions file.
///
/// The `.gemini` in the path is not a leftover: `agy` is Google's, and it keeps
/// its own settings under the same home-directory root the Gemini CLI used.
///
/// Observed rather than documented, so it is overridable: `$SCHEMAIC_AGY_SETTINGS`
/// exists for the case where that CLI moves it and this constant is wrong — the
/// failure is then "no database tools" rather than "wrote into the wrong file".
fn settings_path() -> Option<PathBuf> {
    settings_path_from(
        std::env::var("SCHEMAIC_AGY_SETTINGS").ok().as_deref(),
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .as_deref(),
    )
}

/// The rule behind [`settings_path`], with the environment as arguments.
///
/// **Pure so the test does not have to mutate the process environment.** It
/// used to, with `unsafe { std::env::set_var }`, in a test binary whose siblings
/// read the environment on other threads (`script.rs` and `conn_sources.rs` both
/// call `env::temp_dir()`) — documented UB, and on glibc a real
/// use-after-free when `setenv` reallocates `environ` under a concurrent
/// `getenv`. It also *removed* a documented user-facing override rather than
/// restoring it, so a developer running the suite lost their own setting.
fn settings_path_from(
    override_var: Option<&str>,
    home: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    if let Some(p) = override_var
        && !p.trim().is_empty()
    {
        return Some(PathBuf::from(p));
    }
    Some(
        PathBuf::from(home?)
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
/// **The answer is used, not discarded.** All three callers ignored it, so
/// `is_installed()` could report `true` with no rules granted, and a withdraw
/// that failed or was declined — an unwritable file, an unparseable one — left a
/// standing `run_query` auto-approval in the user's configuration permanently
/// and silently. A grant that did not take must reach the session; a withdraw
/// that did not take must reach the log.
///
/// `true` means the file now says what it was asked to say — which includes the
/// [`SettingsEdit::Unchanged`] case, where it already did and **nothing is
/// written**. That is not a shortcut: see [`SettingsEdit`] for what a needless
/// rewrite costs the user's file.
///
/// The write is [`persist::write_file_atomic`], whose doc opens by naming the
/// failure it exists for: `fs::write` truncates before it writes, so a full
/// disk, a dropped network share or a crash between the two leaves the file
/// empty. This is another vendor's configuration in a directory Schemaic does
/// not own and cannot regenerate — the highest-blast-radius write in the app.
fn edit_settings(create: bool, edit: impl Fn(&str) -> Option<SettingsEdit>) -> bool {
    let Some(path) = settings_path() else {
        return false;
    };
    let Some(current) = settings_to_edit(create, std::fs::read_to_string(&path).ok()) else {
        return false;
    };
    match edit(&current) {
        // Declined: the document is not a JSON object, and losing the user's
        // file is worse than losing this session's database tools.
        None => false,
        Some(SettingsEdit::Unchanged) => true,
        Some(SettingsEdit::Write(next)) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            persist::write_file_atomic(&path, next.as_bytes()).is_ok()
        }
    }
}

/// Which *session* holds Antigravity's global state, as the marker records it.
///
/// The [`Owner`] half answers "is the holder still running", and the reasoning
/// for carrying a start time beside the pid lives with it in
/// [`crate::liveness`]. The `nonce` answers the question a pid cannot: **two
/// sessions in the same process.** Changing a setting respawns the AI session,
/// and the new session's `install` runs concurrently with the old session's
/// `Drop` on a different worker with no ordering between them — same pid, same
/// start time, so an owner alone cannot tell them apart. When the teardown
/// finished last it removed the registration, the rules and the marker the new
/// session had just installed, and that session then ran with every database
/// tool refused while `is_installed()` said `true`.
///
/// With a nonce the ordering stops mattering, which is the only fix available:
/// whoever claims last owns the state, and a `Drop` whose nonce is no longer the
/// one on disk removes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Claim {
    owner: crate::liveness::Owner,
    nonce: u64,
}

/// Render a claim. One line, three fields — this file is written by one process
/// and read by another, so the format is the interface between them.
fn claim_text(c: Claim) -> String {
    format!("{} {} {}\n", c.owner.pid, c.owner.started, c.nonce)
}

/// Read a claim back, or `None` for anything that is not one.
///
/// A truncated or garbled marker answers `None`, which [`liveness::may_sweep`]
/// treats as *no claim* — the same direction as no file at all. The alternative,
/// refusing to sweep on an unreadable marker, would leave a standing permission
/// grant in the user's config with nothing able to withdraw it.
///
/// [`liveness::may_sweep`]: crate::liveness::may_sweep
fn parse_claim(s: &str) -> Option<Claim> {
    let mut it = s.split_whitespace();
    let pid = it.next()?.parse().ok()?;
    let started = it.next()?.parse().ok()?;
    let nonce = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some(Claim {
        owner: crate::liveness::Owner { pid, started },
        nonce,
    })
}

/// May *this* registration's teardown remove the global state?
///
/// **The asymmetry this closes.** The sweep read the marker, resolved the
/// owner's liveness and deferred; `Drop` performed the same three removals
/// eighteen lines away and consulted nothing at all — so a second window's
/// session ending silently disarmed a live one. A claim that governs one remover
/// and not the other governs nothing.
///
/// Three ways to answer no, and each is a bug that happened:
/// - `installed == false` — never `agy mcp remove` a server this session did not
///   add, which would tear down a *working* registration belonging to another
///   instance.
/// - `mine == None` — this session never managed to claim, so it is not holding
///   anything to give back.
/// - `on_disk != mine` — somebody claimed after us. On a respawn that somebody
///   is the *next session in this very process*, which is why the comparison is
///   the whole [`Claim`] and not its [`Owner`].
fn may_release(mine: Option<Claim>, on_disk: Option<Claim>, installed: bool) -> bool {
    installed && mine.is_some() && on_disk == mine
}

/// Where the claim is recorded. Beside the app's own state rather than in the
/// shared temp directory: it is a claim on *this* machine's Antigravity config,
/// and world-writable is the wrong permission for something a sweep obeys.
fn marker_path() -> Option<PathBuf> {
    Some(persist::private_dir("agy")?.join("registration-owner"))
}

/// The claim recorded on disk right now, if there is a readable one.
fn claim_on_disk() -> Option<Claim> {
    let text = std::fs::read_to_string(marker_path()?).ok()?;
    parse_claim(&text)
}

/// A nonce for one registration. Only ever compared for equality with itself, so
/// a process-local counter mixed with the clock is enough to keep two sessions
/// of one process apart.
fn next_nonce() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    n.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(now)
}

/// Claim the global state for this session, **before** installing it.
///
/// **The claim goes first, and that is the opposite of the release.** It used to
/// be written after `agy mcp add` and after the settings grant, so from the
/// instant the registration landed until the claim returned, machine-global
/// state existed that no marker accounted for — and a second launch's sweep in
/// that window removed a live session's grant. A claim protecting state it was
/// written after protects nothing; the release is last for the mirror reason,
/// and only there because by then the state is already gone.
///
/// Written through [`persist::write_file_atomic`] because the reader is another
/// process: `fs::write` truncates first, so a sweep landing between the truncate
/// and the write read `""` or a prefix, [`parse_claim`] answered `None` for
/// both, and a torn write by a *live* instance was treated as no claim at all.
///
/// `None` when the claim could not be made — no start time (it could not then be
/// told from a pid-reuse, and a claim that cannot expire is worse than none), no
/// private directory, or the write failed. The caller must not install without
/// one.
fn claim() -> Option<Claim> {
    let c = Claim {
        owner: crate::liveness::me()?,
        nonce: next_nonce(),
    };
    let path = marker_path()?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    persist::write_file_atomic(&path, claim_text(c).as_bytes()).ok()?;
    Some(c)
}

/// Drop a claim, but only if it is still the one on disk.
fn release(mine: Option<Claim>) {
    if mine.is_none() || claim_on_disk() != mine {
        return;
    }
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
    /// This session's claim, or `None` if it never made one.
    mine: Option<Claim>,
    /// Whether the registration **and** its allow-rules actually took. A failed
    /// install must not have its `Drop` remove a server this session never
    /// added, and a session whose rules were declined has no database tools
    /// however well `agy mcp add` went.
    installed: bool,
}

impl AgyRegistration {
    /// Register the server and grant exactly `allowed`.
    ///
    /// `allowed` is the connection's own tool list, so a schema-only connection
    /// never grants `run_query` — the same rule the Codex overrides and Claude's
    /// `--allowedTools` follow.
    ///
    /// **Order: claim, register, grant.** See [`claim`] for why it goes first
    /// and [`Claim`] for what the nonce buys on a respawn.
    pub(crate) fn install(bin: &str, exe: &str, endpoint_file: &str, allowed: &[&str]) -> Self {
        let rules = antigravity_allow_rules(allowed);
        let mine = claim();
        let mut reg = Self {
            bin: bin.to_string(),
            rules,
            mine,
            installed: false,
        };
        if mine.is_none() {
            // Without a claim there is nothing to stop another instance's sweep
            // withdrawing this grant mid-session, and nothing to tell this
            // session's own teardown whether the state is still its own. The
            // session runs without database tools instead, which it says.
            tracing::warn!("could not claim Antigravity's configuration for this session");
            return reg;
        }
        // `--` first: the command's own arguments start with `-`, and `agy mcp
        // add` rejects a flag placed after the server name.
        let registered = agy_mcp(
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
        if !registered {
            release(reg.mine.take());
            return reg;
        }
        let r = reg.rules.clone();
        // Granting: a fresh Antigravity install has no settings file yet, and
        // the rules are what its tools need to run at all.
        //
        // **The answer decides whether this session has tools.** It used to be
        // discarded, so an unparseable or unwritable `settings.json` produced a
        // registered server whose every call that CLI refuses, reported as a
        // working session.
        let granted = edit_settings(true, move |cur| antigravity_settings_with_rules(cur, &r));
        if !granted {
            tracing::warn!(
                "Antigravity's settings file could not be granted the Schemaic tool rules; \
                 withdrawing the registration rather than leaving a server whose calls are refused"
            );
            agy_mcp(bin, &["remove", SERVER]);
            release(reg.mine.take());
            return reg;
        }
        reg.installed = true;
        reg
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
        // **The same ownership question the sweep asks**, and it used to ask
        // nothing at all. See [`may_release`].
        if !may_release(self.mine, claim_on_disk(), self.installed) {
            return;
        }
        let rules = std::mem::take(&mut self.rules);
        // Withdrawing: only ever from a file that is there. This arm is reached
        // after a successful `install`, so it will be — but the flag is the
        // rule, not the reachability.
        let withdrawn = edit_settings(false, move |cur| {
            antigravity_settings_without_rules(cur, &rules)
        });
        if !withdrawn {
            // The end state the module doc names as the bad one — rules without
            // a registration — and it is permanent: the next launch's sweep
            // fails on the same file for the same reason. Said out loud rather
            // than discovered.
            tracing::error!(
                "could not withdraw Schemaic's tool rules from Antigravity's settings file; \
                 they are still granted. Remove the `mcp(schemaic/…)` entries from \
                 `permissions.allow` by hand if the file is not going to become writable."
            );
        }
        agy_mcp(&self.bin, &["remove", SERVER]);
        // The claim goes last, and only after the state it claimed is gone: a
        // release that ran first would open a window in which another instance's
        // sweep could race this one's own removal.
        release(self.mine.take());
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
    let held = claim_on_disk();
    if !crate::liveness::may_sweep(
        held.map(|c| c.owner),
        held.and_then(|c| crate::liveness::process_start(c.owner.pid)),
    ) {
        return;
    }
    // Whatever was on disk is a dead instance's, so it is ours to drop — and
    // `release` compares, so passing what we just read is what lets it go.
    release(held);
    // The rules can be removed with no `agy` at all — it is our own file surgery
    // — so that half runs regardless.
    let all: Vec<&str> = crate::ai::AI_TOOLS_WITH_QUERY.to_vec();
    let rules = antigravity_allow_rules(&all);
    // Never creates. This runs on **every** launch, for every user, and most of
    // them have no Antigravity at all — writing `{}` into that CLI's config
    // directory to withdraw rules nobody granted is the whole hazard `create`
    // exists to close.
    if !edit_settings(false, move |cur| {
        antigravity_settings_without_rules(cur, &rules)
    }) {
        // Only ever reached with a file that *is* there and could not be parsed
        // or written — never for the majority of users, who have no Antigravity
        // and whose read simply fails. See the `create` argument.
        tracing::error!(
            "a crashed session's Schemaic tool rules could not be withdrawn from \
             Antigravity's settings file; they are still granted"
        );
    }
    if let Some(bin) = bin {
        agy_mcp(bin, &["remove", SERVER]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::liveness::{Owner, may_sweep};

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
        // the write this test exists to prevent. (It is `Unchanged` now, which
        // is a second guard on the same hazard: nothing was there to remove.)
        assert_eq!(
            antigravity_settings_without_rules("", &rules),
            Some(SettingsEdit::Unchanged)
        );

        // Granting is the other half and still creates: a fresh install has no
        // file, and its tools are refused without the rules.
        let fresh = settings_to_edit(true, None).expect("a document to edit");
        let Some(SettingsEdit::Write(granted)) = antigravity_settings_with_rules(&fresh, &rules)
        else {
            panic!("a fresh install must be granted its rules");
        };
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
        // to make being wrong cost tools rather than the wrong file. Asked as a
        // pure question: the version of this test that answered it by mutating
        // the process environment was documented UB in a parallel test binary
        // whose siblings read it.
        let home = std::ffi::OsString::from("/home/u");
        assert_eq!(
            settings_path_from(Some("/tmp/zz-agy.json"), Some(&home)),
            Some(PathBuf::from("/tmp/zz-agy.json"))
        );
        // An override that is present but blank is not an override.
        for blank in ["", "   "] {
            let p = settings_path_from(Some(blank), Some(&home)).expect("the home path");
            assert!(p.ends_with("settings.json"), "{p:?}");
            assert!(p.to_string_lossy().contains("antigravity-cli"), "{p:?}");
        }
        // Without either, there is no path at all — and no file to write.
        assert_eq!(settings_path_from(None, None), None);
    }

    /// **The guard, not the absence of a panic.** This test used to assert only
    /// that dropping a failed registration did not panic — which it does not
    /// with the guard deleted either, because on CI there is no settings file
    /// and `agy_mcp("zz-not-a-binary")` cannot spawn. With the guard gone the
    /// suite would then delete a *live* session's marker on every `cargo test`,
    /// causing the regression the test exists to catch. So the decision is asked
    /// directly.
    #[test]
    fn a_failed_registration_removes_nothing_on_drop() {
        let mine = Claim {
            owner: Owner {
                pid: 1234,
                started: 900,
            },
            nonce: 7,
        };
        // A registration that never took: nothing of ours is out there, and
        // `agy mcp remove` would tear down another instance's working server.
        assert!(!may_release(Some(mine), Some(mine), false));
        // A session that never managed to claim is not holding anything either.
        assert!(!may_release(None, Some(mine), true));
        assert!(!may_release(None, None, true));
        // The successful case, so the guard is not vacuously "never".
        assert!(may_release(Some(mine), Some(mine), true));
    }

    /// **A respawn is two sessions in one process**, so the pid and the start
    /// time are identical and only the nonce separates them. `ai_send` starts
    /// the new session first and drops the old one afterwards, on a different
    /// worker with no ordering — so the teardown regularly runs *after* the new
    /// install, and used to remove the registration, the rules and the marker
    /// the new session had just put in place. That session then had every
    /// database tool refused while `is_installed()` said `true`.
    #[test]
    fn the_previous_sessions_teardown_does_not_disarm_the_one_that_replaced_it() {
        let owner = Owner {
            pid: 1234,
            started: 900,
        };
        let old = Claim { owner, nonce: 1 };
        let new = Claim { owner, nonce: 2 };
        assert!(
            !may_release(Some(old), Some(new), true),
            "the old session's Drop removed the new session's registration"
        );
        // …and the ordinary case is untouched: nobody claimed after us.
        assert!(may_release(Some(old), Some(old), true));
        // An owner comparison alone cannot see this, which is why the nonce is
        // in the marker at all.
        assert_eq!(old.owner, new.owner);
    }

    /// **The registration and the rules must name the same server.** They were
    /// two independent literals — a private `SERVER` here, and
    /// `format!("mcp(schemaic/…)")` in another crate that cannot see it — while
    /// the doc on both claimed one constant. Renaming either half leaves the
    /// registration standing and every rule pointing at a server that is not
    /// there, which Antigravity answers by *denying* the call — and a denied
    /// turn still reports `"status":"SUCCESS"` with an empty response, so the
    /// user would have seen the assistant simply stop using the database.
    ///
    /// Asserted as the composition rather than `SERVER == MCP_SERVER`: the rule
    /// string is what `agy` matches the registration against, so that is what
    /// has to carry the name.
    #[test]
    fn the_allow_rules_name_the_server_that_was_registered() {
        let rules = antigravity_allow_rules(&["mcp__schemaic__list_schema"]);
        assert_eq!(rules, vec![format!("mcp({SERVER}/list_schema)")]);
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

    /// The bug the marker exists for: the sweep runs at **every** launch and
    /// removed the registration unconditionally, so starting a second Schemaic
    /// while the first had a live AI session pulled that session's server and
    /// allow-rules out from under it.
    #[test]
    fn a_second_instance_does_not_sweep_a_live_instances_registration() {
        let c = Claim {
            owner: Owner {
                pid: 1234,
                started: 900,
            },
            nonce: 5,
        };
        assert!(
            !may_sweep(Some(c.owner), Some(900)),
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
        assert!(may_sweep(Some(owner), None));
        // The pid is running, but it is not the process that wrote the marker —
        // the operating system handed that number to something else. Trusting
        // the pid alone would decline to clean up for the rest of its life.
        assert!(may_sweep(Some(owner), Some(901)));
    }

    /// No claim, no reason to defer — including a marker too damaged to read,
    /// which must not be able to strand a permission grant in the user's config.
    #[test]
    fn an_absent_or_unreadable_marker_is_not_a_claim() {
        assert!(may_sweep(None, None));
        for junk in [
            "",
            "   ",
            "not-a-pid 900 1",
            "1234",
            "1234 900",
            "1234 900 1 extra",
            "1234 x 1",
            "1234 900 x",
        ] {
            assert_eq!(parse_claim(junk), None, "{junk:?} parsed as a claim");
        }
    }

    /// **The arm that is deliberately gone.** `may_sweep` used to answer `true`
    /// for any marker naming our own pid, on the stated assumption that a sweep
    /// runs "at startup" and so cannot be the instance mid-session. The sweep is
    /// a *detached* thread with `detect_bin`, a `sysinfo` refresh and an
    /// `agy mcp remove` ahead of it; a user who presses Enter in the AI panel
    /// first has already installed a marker naming this pid, with this pid's
    /// start time. Nothing joined that thread and no flag marked it done.
    #[test]
    fn our_own_live_claim_survives_our_own_startup_sweep() {
        let mine = Owner {
            pid: 4242,
            started: 900,
        };
        assert!(
            !may_sweep(Some(mine), Some(900)),
            "the startup sweep removed the session this very process had just installed"
        );
    }

    #[test]
    fn a_claim_round_trips() {
        let c = Claim {
            owner: Owner {
                pid: 31337,
                started: 1_700_000_000,
            },
            nonce: 99,
        };
        assert_eq!(parse_claim(&claim_text(c)), Some(c));
    }
}
