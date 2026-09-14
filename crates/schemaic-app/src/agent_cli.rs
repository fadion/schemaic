//! Locating and interrogating the agent CLI: auto-detection per harness, a
//! minimal `PATH`/`PATHEXT` `which`, resolving a user-provided override to a
//! real executable, and the one `--help` probe that decides what may be passed
//! to it. Shared by the AI-session spawn (`ai::start_ai_session`) and the AI
//! panel's reachability check. (The Windows-`PATHEXT` handling is the H12
//! subtlety: `Command::new` alone won't append `.cmd`/`.exe`.)
//!
//! Std-only and free of app state, with **one exception that is not pure**:
//! [`probe`] spawns `<bin> --help` and memoises what it reads in a process-wide
//! cache. It lives here because it answers a question about the binary this
//! module's whole job is to find, and the decisions it feeds — which flags are
//! safe to pass, and whether the session may start at all — are pure and
//! unit-tested in `schemaic-ai`.
//!
//! **One probe, three answers.** A harness is asked about its sealing flags, its
//! constraint grade and its config isolation together, because they come from
//! one `--help` and spawning that three times to answer three questions is three
//! times the ~140 ms nobody has.

use schemaic_ai::CliSeal;
use schemaic_ai::harness::{Constraint, Harness, codex_isolates_config, constraint_from_help};

/// What one `--help` told us about a binary.
///
/// Not a `CliSeal` with extra fields: `CliSeal` is *Claude's* flag set and is
/// meaningless for the others, whereas [`Probe::constraint`] is the question
/// every harness answers and the one that decides whether a session starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Probe {
    /// Claude's three sealing flags. `CliSeal::NONE` for every other harness —
    /// they have no such flags and nothing reads it for them.
    pub(crate) seal: CliSeal,
    /// How well this binary's built-in tools can be shut off. [`Constraint::Unknown`]
    /// means **do not spawn**.
    pub(crate) constraint: Constraint,
    /// Codex's `--ignore-user-config`, when it was seen. Always false elsewhere.
    pub(crate) isolate_config: bool,
}

/// Auto-detect a harness's binary: its `$SCHEMAIC_*_BIN` override, then the
/// places its own installer puts it, then a `PATH` search. `None` when it cannot
/// be found anywhere, so the UI can honestly report a failed auto-detect instead
/// of a phantom binary.
pub(crate) fn detect_bin(h: Harness) -> Option<String> {
    for var in env_vars(h) {
        if let Ok(p) = std::env::var(var)
            && !p.trim().is_empty()
        {
            return Some(p);
        }
    }
    for cand in known_locations(h) {
        if cand.is_file() {
            return Some(cand.to_string_lossy().into_owned());
        }
    }
    which_on_path(h.bin())
}

/// Environment overrides for a harness, most specific first.
///
/// `SCHEMAIC_CLAUDE_BIN` is listed for Claude because it shipped before any
/// other harness existed and someone's launcher may still set it.
fn env_vars(h: Harness) -> &'static [&'static str] {
    match h {
        Harness::Claude => &["SCHEMAIC_CLAUDE_BIN"],
        Harness::Codex => &["SCHEMAIC_CODEX_BIN"],
        Harness::Antigravity => &["SCHEMAIC_ANTIGRAVITY_BIN"],
        Harness::OpenCode => &["SCHEMAIC_OPENCODE_BIN"],
    }
}

/// Where each installer puts the binary, for the case that matters most: it is
/// installed and working, but the directory is not on the `PATH` this process
/// inherited.
///
/// That is not hypothetical — it is what both non-Claude harnesses looked like
/// on the machine this was written against, where `codex` and `agy` ran fine in
/// the user's own shell and neither was on the `PATH` a child process saw. With
/// only a `PATH` search, the panel would have reported them as not installed.
fn known_locations(h: Harness) -> Vec<std::path::PathBuf> {
    let exe = |name: &str| {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    };
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    let local = std::env::var_os("LOCALAPPDATA");
    let mut out = Vec::new();
    match h {
        Harness::Claude => {
            if let Some(home) = home {
                out.push(
                    std::path::PathBuf::from(home)
                        .join(".local")
                        .join("bin")
                        .join(exe("claude")),
                );
            }
        }
        Harness::Codex => {
            if let Some(local) = local {
                out.push(
                    std::path::PathBuf::from(local)
                        .join("Programs")
                        .join("OpenAI")
                        .join("Codex")
                        .join("bin")
                        .join(exe("codex")),
                );
            }
            if let Some(home) = home {
                out.push(
                    std::path::PathBuf::from(home)
                        .join(".codex")
                        .join("bin")
                        .join(exe("codex")),
                );
            }
        }
        Harness::Antigravity => {
            if let Some(local) = local {
                out.push(
                    std::path::PathBuf::from(local)
                        .join("agy")
                        .join("bin")
                        .join(exe("agy")),
                );
            }
            if let Some(home) = home {
                out.push(
                    std::path::PathBuf::from(home)
                        .join(".local")
                        .join("bin")
                        .join(exe("agy")),
                );
            }
        }
        // **npm-installed, which makes the `PATH` entry the wrong file to
        // spawn.** A global `npm i -g opencode-ai` puts a `opencode.cmd` shim on
        // `PATH` beside the real `opencode.exe` it calls, and `which_on_path`
        // honours `PATHEXT` — so the fallback finds the `.cmd`, which needs a
        // shell to run and is not what `Command::new` starts. The real binary is
        // listed first so it wins before that ever happens.
        Harness::OpenCode => {
            if let Some(dir) = std::env::var_os("APPDATA") {
                out.push(
                    std::path::PathBuf::from(dir)
                        .join("npm")
                        .join("node_modules")
                        .join("opencode-ai")
                        .join("bin")
                        .join(exe("opencode")),
                );
            }
            if let Some(home) = home {
                let home = std::path::PathBuf::from(home);
                // The install script's own location, then npm's Unix prefix.
                out.push(home.join(".opencode").join("bin").join(exe("opencode")));
                out.push(home.join(".local").join("bin").join(exe("opencode")));
                out.push(
                    home.join(".npm-global")
                        .join("lib")
                        .join("node_modules")
                        .join("opencode-ai")
                        .join("bin")
                        .join(exe("opencode")),
                );
            }
        }
    }
    out
}

/// Executable extensions to try on Windows (so an npm-installed `claude.cmd` is
/// found, not just `claude.exe`). Empty elsewhere.
pub(crate) fn pathext() -> Vec<String> {
    if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".to_string())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_string())
            .collect()
    } else {
        Vec::new()
    }
}

/// Which file name in one `PATH` directory is the executable for `name` — the
/// per-directory half of [`which_on_path`], made pure so it can be tested.
///
/// `present` is what that directory actually holds (only the candidates matter);
/// `exts` is [`pathext`], empty off Windows. The answer is the file name to
/// spawn, or `None` if this directory has none.
///
/// **On Windows the bare name is not an executable and must not win.** `npm i
/// -g` writes *three* shims per binary: `<name>` — a `#!/bin/sh` script for Git
/// Bash — plus `<name>.cmd` and `<name>.ps1`. Taking the bare file first
/// resolved an npm-installed harness to the `sh` script, which
/// `Command::spawn` answers with **os error 193, "%1 is not a valid Win32
/// application"**. `harness_reachable` still said connected — it only asks
/// whether the file exists — while `probe`'s `--help` errored into
/// `Constraint::Unknown` and `inline_gate` then refused *every* generation
/// with "could not confirm … so the assistant is disabled". The whole AI
/// feature dead, and the two surfaces giving contradictory reasons.
///
/// `cmd.exe` and `where.exe` consider only `PATHEXT` extensions, so this does
/// too: the bare name is taken only when its own extension is already one of
/// them (`foo.exe` asked for by full name), and `PATHEXT`'s order is the
/// preference order — which is what puts `.CMD` ahead of a `.ps1` nobody can
/// spawn directly.
///
/// **With one departure from `where.exe`: a batch shim goes last.** Rust runs a
/// `.bat`/`.cmd` through `cmd.exe` and refuses any argument it cannot escape for
/// it, and every prompt this app builds is multi-line by construction
/// (`harness::prefixed_prompt` joins with `\n\n`), so a harness resolved to a
/// `.cmd` answers `InvalidInput: batch file arguments are invalid` for *every*
/// generation — Ctrl+K, Optimize, Fix with AI and each chat turn, reported as a
/// batch-file problem rather than a prompt one. A default `PATHEXT` lists `.EXE`
/// before `.CMD` and hid this; the variable is editable and installers edit it.
/// The shim is still taken when it is the only candidate, which is npm's layout
/// — and that case is not silent: `schemaic_ai::batch_shim_reason` refuses the
/// spawn with the cause and the lever, rather than letting the OS report a
/// batch-file problem about a batch file that is fine. Preferring here and
/// refusing there are the two halves of one answer, and neither is sufficient
/// alone: this cannot conjure an executable npm did not install.
///
/// Off Windows `exts` is empty and the bare name is the only candidate, which
/// is correct there: the executable bit is the test, not the name.
pub(crate) fn pick_executable<'a>(
    name: &str,
    present: &[&'a str],
    exts: &[String],
) -> Option<&'a str> {
    let has = |n: &str| present.iter().copied().find(|p| p.eq_ignore_ascii_case(n));
    if exts.is_empty() {
        return has(name);
    }
    // Through the shared predicate, because the spelling this replaced —
    // `name[name.len() - e.len()..]` behind a length guard — indexes a `&str` at
    // a byte count that may be inside a character, and `PATHEXT`'s entries are
    // 3 and 4 bytes. Typing `naïve` into Settings → AI → CLI path panicked the
    // UI thread on the keystroke, since the field is validated per value.
    let already_extended = exts
        .iter()
        .any(|e| schemaic_core::launch::ends_with_ignore_ascii_case(name, e));
    if already_extended && let Some(hit) = has(name) {
        return Some(hit);
    }
    // Two passes over `PATHEXT` in its own order: everything that is not a batch
    // shim, then the shims. See the doc above for why the second pass exists.
    let is_batch = |e: &str| e.eq_ignore_ascii_case(".BAT") || e.eq_ignore_ascii_case(".CMD");
    exts.iter()
        .filter(|e| !is_batch(e))
        .find_map(|e| has(&format!("{name}{e}")))
        .or_else(|| {
            exts.iter()
                .filter(|e| is_batch(e))
                .find_map(|e| has(&format!("{name}{e}")))
        })
}

/// Minimal `which`: locate `name` on `PATH`, honoring `PATHEXT` on Windows.
///
/// **A relative `PATH` entry is skipped.** `std::env::split_paths` yields an
/// *empty* `PathBuf` for the trailing `;` most Windows `PATH`s carry, so
/// `dir.join(name)` became a **cwd-relative** path: `is_file()` answered
/// against the process working directory and the relative string was what came
/// back — becoming both `probe`'s cache key and the `Command::new` program.
/// `Command::new` does not search the cwd, so `harness_reachable` reported a
/// name that cannot be spawned, and the child's `cwd: session_cwd()` moves the
/// directory out from under it anyway.
pub(crate) fn which_on_path(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    let exts = pathext();
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() || dir.is_relative() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| !t.is_dir()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        if let Some(hit) = pick_executable(name, &refs, &exts) {
            return Some(dir.join(hit).to_string_lossy().into_owned());
        }
    }
    None
}

/// Resolve a user-provided CLI override to an existing binary. Handles a concrete
/// file path, a bare command name (searched on `PATH`), and — on Windows — a path
/// missing its executable extension (`C:\tools\claude` → `claude.exe`). `None`
/// means nothing exists there.
pub(crate) fn resolve_override(t: &str) -> Option<String> {
    if std::path::Path::new(t).is_file() {
        return Some(t.to_string());
    }
    // No path separators → treat as a command name on PATH.
    if !t.contains('/') && !t.contains('\\') {
        return which_on_path(t);
    }
    // A path missing its Windows extension.
    if cfg!(windows) && std::path::Path::new(t).extension().is_none() {
        for ext in pathext() {
            let cand = format!("{t}{ext}");
            if std::path::Path::new(&cand).is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// Is this harness reachable for this settings value? Empty = auto-detect must
/// succeed; otherwise the manual override must resolve to a real binary. Drives
/// the UI's "not connected" state and the disabled message box.
pub(crate) fn harness_reachable(h: Harness, cli_path: &str) -> bool {
    let t = cli_path.trim();
    if t.is_empty() {
        detect_bin(h).is_some()
    } else {
        resolve_override(t).is_some()
    }
}

/// Resolve the harness binary for launching: a non-empty user override (AI
/// settings) wins, otherwise auto-detect, otherwise the bare command name as a
/// last-ditch spawn attempt (which then fails with the usual "not installed"
/// error).
///
/// The override is resolved the same way [`harness_reachable`] validates it
/// (`resolve_override`: PATH + Windows `PATHEXT`), so a bare `claude` or an
/// extension-less `C:\tools\claude` that settings reports as reachable actually
/// spawns — `Command::new` alone won't append `.cmd`/`.exe` (review H12).
pub(crate) fn harness_bin(h: Harness, override_path: &str) -> String {
    let t = override_path.trim();
    if !t.is_empty() {
        return resolve_override(t).unwrap_or_else(|| t.to_string());
    }
    detect_bin(h).unwrap_or_else(|| h.bin().to_string())
}

/// What one `--help` says about the binary at `bin`, for the harness we believe
/// it is.
///
/// **Cached per (harness, resolved path)**, because the answer cannot change
/// while that file does not and the probe costs ~140 ms — enough to be felt if
/// it ran on the UI thread before every AI action instead of once.
/// [`warm_probe_cache`] fills it off-thread at startup so the first AI action
/// usually finds it already there. Keyed by harness as well as path because the
/// same binary answers different questions for different harnesses, and because
/// switching harness must not read the previous one's answer.
///
/// **A probe that cannot run at all is not the same non-answer for every
/// harness.** Claude gets [`CliSeal::ALL`] — every flag passed — because an
/// unknown flag there kills the spawn loudly and the dangerous direction is
/// assuming a flag is absent. The *constraint* goes the other way and lands on
/// [`Constraint::Unknown`], which refuses the spawn: for a non-Claude harness
/// there is no safe "pass everything" fallback, since `--sandbox` on a binary
/// that never heard of it is a dead session rather than a degraded one. Both
/// directions are the conservative one for their own question; `schemaic-ai`
/// states why at each.
///
/// **The one stale case is an upgrade in place**, where the path does not change
/// but the binary behind it grows the flags: the cached answer stays until
/// Schemaic restarts, so a session spawned in between is sealed the old way.
/// What covers it for Claude is `DISALLOWED_TOOLS`, passed at every seal level.
pub(crate) fn probe(h: Harness, bin: &str) -> Probe {
    let cache = probe_cache();
    let key = (h, bin.to_string());
    if let Ok(c) = cache.lock()
        && let Some(p) = c.get(&key)
    {
        return *p;
    }

    // **A spawn that never ran is not an answer, so it is neither cached nor
    // treated as one.** Every other outcome below is a property of a file that
    // will not change while the app runs, which is what makes caching it sound.
    // A failed spawn is not: an antivirus lock, an in-place upgrade of the CLI,
    // or a moment of resource exhaustion during the startup `warm_probe_cache`
    // all produce it, and it lands on `Constraint::Unknown` — which *refuses*
    // the session outright. Memoised, one such moment disabled the assistant for
    // the rest of the run, reporting "could not confirm … so the assistant is
    // disabled" with no retry path anywhere. Returning early leaves the next
    // attempt free to ask again.
    let Some(o) = help_command(h, bin).output().ok() else {
        return Probe {
            seal: CliSeal::ALL,
            constraint: Constraint::Unknown,
            isolate_config: false,
        };
    };
    // **Both streams, folded.** `--help` goes to stdout on most CLIs and to
    // *stderr* on `agy`, which exits 0 either way; reading only stdout would
    // have found an empty help there and refused a working binary.
    let stdout = String::from_utf8_lossy(&o.stdout);
    let stderr = String::from_utf8_lossy(&o.stderr);
    let both = format!("{stdout}{stderr}");
    let probe = Probe {
        seal: schemaic_ai::seal_from_probe(o.status.success(), &stdout, &stderr),
        // A `--help` that exited non-zero printed a diagnostic, not help, and a
        // constraint must not be read out of one.
        constraint: if o.status.success() {
            constraint_from_help(h, &both)
        } else {
            Constraint::Unknown
        },
        // Asked as a capability rather than `h == Harness::Codex`: that shape
        // compiles cleanly while sorting a fifth harness onto whichever side it
        // happens to land, and this one lands on `isolate_config: false` — the
        // *unsafe* side, where the child reads the user's own configuration.
        isolate_config: h.isolates_config_by_flag()
            && o.status.success()
            && codex_isolates_config(&both),
    };
    if let Ok(mut c) = cache.lock() {
        c.insert(key, probe);
    }
    probe
}

/// The `--help` invocation [`probe`] runs, built rather than spawned so the
/// **argv** has a test.
///
/// `Harness::help_args`, never a literal `--help`: Codex keeps
/// `--ignore-user-config` off its top-level page, so asking the wrong one
/// silently dropped that harness's whole config isolation and every session
/// loaded the user's own `~/.codex/config.toml` — their MCP servers and their
/// hooks. The test written for that regression asserted `help_args()`'s
/// *return*, which is the answer and not the question: reverting this line to
/// `.arg("--help")` left the whole workspace green.
///
/// It is still one function away from being compile-forced — nothing stops a
/// future edit spawning its own `Command` — so the value of the extraction is
/// that the revert now has to be made in two places, and the second is here,
/// where the comment is.
fn help_command(h: Harness, bin: &str) -> std::process::Command {
    let mut c = std::process::Command::new(bin);
    c.args(h.help_args());
    c
}

/// The `(harness, resolved path) -> Probe` memo, shared by [`probe`] and
/// [`probe_cached`].
fn probe_cache() -> &'static std::sync::Mutex<std::collections::HashMap<(Harness, String), Probe>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<(Harness, String), Probe>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// The cached answer for `(h, bin)`, or `None` if nobody has probed it yet.
///
/// **For a caller that must not block**, which is the settings modal: its notice
/// memo runs on the Floem UI thread, and calling [`probe`] there pays a
/// synchronous `--help` on every cold key. The harness dropdown produces a cold
/// key by construction — switching harness *clears* `cli_path`, so the memo and
/// the warming thread were started in the same update pass and both missed. The
/// modal shows nothing until [`warm_probe_cache`] has an answer, and
/// the app bumps a signal when [`warm_probe_cache`] finishes, which is what
/// tells the memo to look again.
pub(crate) fn probe_cached(h: Harness, bin: &str) -> Option<Probe> {
    let cache = probe_cache();
    let c = cache.lock().ok()?;
    c.get(&(h, bin.to_string())).copied()
}

/// Fill [`probe`]'s cache for `(h, bin)` on a background thread.
///
/// Nothing waits on it: a miss simply pays the probe where it would have anyway.
///
/// **`bin` must be what the spawn will resolve** — `harness_bin(h, &ai_cli_path)`,
/// not `detect_bin(h)`. The cache is keyed by the binary's resolved path, and an
/// AI CLI override makes those two different keys, so warming the auto-detected
/// one warmed an entry nothing ever read and left all four AI entry points
/// paying a blocking `--help` on the UI thread. Its caller re-warms from an
/// effect on the setting for the same reason — and now on the harness too.
///
/// `done` is called on the UI thread once the entry is filled, so a view that
/// showed nothing on the miss can ask again. Build it with
/// `floem::ext_event::create_ext_action`: this runs on a plain `std` thread and
/// must not touch a signal directly.
pub(crate) fn warm_probe_cache(h: Harness, bin: String, done: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        let _ = probe(h, &bin);
        done();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The argv the probe actually runs**, which is what the regression was.
    /// Codex keeps `--ignore-user-config` off its top-level `--help`, so
    /// probing the wrong page reported no config isolation on every real
    /// machine and every session loaded the user's own `~/.codex/config.toml`.
    /// The test written for it lives in `schemaic-ai` and asserts
    /// `help_args()`'s *return* — the answer, not the question — so reverting
    /// the caller to `.arg("--help")` left the workspace green. This module's
    /// tests never reached `probe` at all.
    #[test]
    fn the_probe_asks_each_harness_for_the_page_that_answers_both_questions() {
        let args = |h: Harness| -> Vec<String> {
            help_command(h, "zz-bin")
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(args(Harness::Codex), vec!["exec", "--help"]);
        for h in Harness::ALL {
            if h == Harness::Codex {
                continue;
            }
            assert_eq!(args(h), vec!["--help"], "{h:?}");
        }
        // The binary is the one it was handed, not a re-resolved one.
        assert_eq!(
            help_command(Harness::Claude, "zz-bin").get_program(),
            std::ffi::OsStr::new("zz-bin")
        );
    }

    #[test]
    fn each_harness_looks_for_its_own_binary_name() {
        // `agy`, not `antigravity` — the label and the executable differ, and a
        // detector keyed on the label finds nothing.
        assert_eq!(Harness::Antigravity.bin(), "agy");
        assert_eq!(Harness::Claude.bin(), "claude");
        assert_eq!(Harness::Codex.bin(), "codex");
    }

    #[test]
    fn every_harness_has_its_own_env_override_and_claude_keeps_the_old_one() {
        assert_eq!(env_vars(Harness::Claude), &["SCHEMAIC_CLAUDE_BIN"]);
        let mut seen = std::collections::HashSet::new();
        for h in Harness::ALL {
            for v in env_vars(h) {
                assert!(seen.insert(*v), "{v} is claimed by two harnesses");
                assert!(v.starts_with("SCHEMAIC_") && v.ends_with("_BIN"), "{v}");
            }
        }
    }

    #[test]
    fn the_harnesses_that_install_off_path_have_somewhere_to_look() {
        // Codex and Antigravity were both installed and working while absent
        // from the `PATH` a child process saw; a `PATH`-only search reports
        // those as not installed.
        for h in [Harness::Codex, Harness::Antigravity] {
            assert!(
                !known_locations(h).is_empty(),
                "{h:?} has no known install location"
            );
        }
    }

    #[test]
    fn a_known_location_ends_in_the_harnesss_own_executable() {
        for h in Harness::ALL {
            for p in known_locations(h) {
                let name = p.file_name().expect("a file name").to_string_lossy();
                assert!(
                    name.starts_with(h.bin()),
                    "{h:?} looks for {name}, not {}",
                    h.bin()
                );
                if cfg!(windows) {
                    assert!(name.ends_with(".exe"), "{name} needs an extension");
                }
            }
        }
    }

    #[test]
    fn an_override_beats_detection_and_a_missing_one_still_yields_a_command() {
        // Nothing resolves for a nonsense path, but the spawn still gets a name
        // to fail on with the usual "not installed" message.
        let out = harness_bin(Harness::Codex, "   ");
        assert!(!out.is_empty());
        assert_eq!(
            harness_bin(Harness::Claude, "/nonexistent/zz-not-here"),
            "/nonexistent/zz-not-here",
            "an unresolvable override is passed through, not silently replaced"
        );
    }

    /// **What replaced the three tests that stood here.** They pinned the
    /// Claude-only inline rule — that the override path and the model id were
    /// *withheld* from every other harness — and that rule is gone: the one-shot
    /// generators build their own harness's argv now, so the override belongs to
    /// whichever CLI is selected, exactly as it does for the chat panel. The
    /// argv itself is pinned in `schemaic_ai::harness::inline_tests`.
    #[test]
    fn the_inline_paths_use_the_selected_harnesss_own_binary() {
        let path = "/nonexistent/zz-cli";
        for h in Harness::ALL {
            assert_eq!(
                harness_bin(h, path),
                path,
                "{h:?}: the override was not honoured for the selected harness"
            );
        }
    }

    #[test]
    fn an_empty_override_asks_detection_and_a_set_one_does_not() {
        // `harness_reachable` with a path that cannot exist is false regardless
        // of what is installed, so this does not depend on the machine.
        assert!(!harness_reachable(
            Harness::Claude,
            "/nonexistent/zz-not-here"
        ));
    }
}

#[cfg(test)]
mod pick_executable_tests {
    use super::pick_executable;

    /// The real `PATHEXT` of a Windows 11 install, in its real order.
    fn win() -> Vec<String> {
        ".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC"
            .split(';')
            .map(str::to_string)
            .collect()
    }

    /// **npm's three shims, and only one of them can be spawned.**
    ///
    /// `npm i -g` writes `<name>` (a `#!/bin/sh` script for Git Bash),
    /// `<name>.cmd` and `<name>.ps1`. Taking the bare file resolved an
    /// npm-installed harness to the `sh` script, which `spawn` answers with os
    /// error 193 — and `harness_reachable` reported it connected while every
    /// generation was refused for a reason the user could not act on.
    /// **A non-ASCII CLI path is a question, not a crash.** The extension test
    /// sliced the name at a fixed byte count behind a length guard, so a name
    /// whose last 3 or 4 bytes straddle a character panicked — on the floem UI
    /// thread, per keystroke, because Settings → AI validates the field on every
    /// value rather than on commit. Eight ASCII tests and a hand check with
    /// `café` (which is safe: its multi-byte character is not in the last four
    /// bytes) did not see it.
    #[test]
    fn a_non_ascii_name_is_resolved_rather_than_panicked_on() {
        for name in [
            "na\u{ef}ve",
            "\u{65e5}\u{672c}",
            "\u{514b}\u{52b3}\u{5fb7}",
            "\u{e9}\u{e9}",
        ] {
            assert_eq!(pick_executable(name, &[], &win()), None);
        }
        // And one that is really there still resolves, extension and all.
        assert_eq!(
            pick_executable("na\u{ef}ve", &["na\u{ef}ve.EXE"], &win()),
            Some("na\u{ef}ve.EXE")
        );
    }

    #[test]
    fn an_npm_shim_directory_resolves_to_the_cmd_not_the_sh_script() {
        let present = ["opencode", "opencode.cmd", "opencode.ps1"];
        assert_eq!(
            pick_executable("opencode", &present, &win()),
            Some("opencode.cmd")
        );
    }

    /// `PATHEXT`'s order is the preference order, which is what `cmd.exe` and
    /// `where.exe` use — so a real `.exe` beside a `.cmd` wins.
    #[test]
    fn pathext_order_decides_between_two_real_candidates() {
        let present = ["claude.cmd", "claude.exe"];
        assert_eq!(
            pick_executable("claude", &present, &win()),
            Some("claude.exe")
        );
    }

    /// **A batch shim is the last resort, whatever `PATHEXT` says.**
    ///
    /// Rust runs a `.bat`/`.cmd` through `cmd.exe` and refuses any argument it
    /// cannot escape for it — and every prompt this app builds is multi-line by
    /// construction, so a harness resolved to a `.cmd` answers
    /// `InvalidInput: batch file arguments are invalid` for *every* generation.
    /// `PATHEXT` order gave the right answer on a default install by luck
    /// (`.EXE` precedes `.CMD` there); a machine whose `PATHEXT` is ordered
    /// otherwise — they are editable, and installers do edit them — resolved to
    /// the shim with a real `.exe` sitting beside it.
    #[test]
    fn a_real_executable_beats_a_batch_shim_whatever_pathext_prefers() {
        let cmd_first: Vec<String> = ".CMD;.BAT;.COM;.EXE"
            .split(';')
            .map(str::to_string)
            .collect();
        let present = ["claude.cmd", "claude.exe"];
        assert_eq!(
            pick_executable("claude", &present, &cmd_first),
            Some("claude.exe"),
            "a batch shim won because PATHEXT happened to list it first"
        );
        // The shim is still the answer when it is the only one — npm's layout,
        // which `an_npm_shim_directory_resolves_to_the_cmd_not_the_sh_script`
        // pins from the other side.
        assert_eq!(
            pick_executable("claude", &["claude.cmd"], &cmd_first),
            Some("claude.cmd")
        );
        // And among batch shims, PATHEXT order still decides.
        assert_eq!(
            pick_executable("claude", &["claude.bat", "claude.cmd"], &cmd_first),
            Some("claude.cmd")
        );
    }

    /// A name that already carries an executable extension is taken as it
    /// stands — this is the `resolve_override` shape, a user typing the full
    /// file name.
    #[test]
    fn a_name_that_is_already_executable_is_taken_verbatim() {
        let present = ["claude.exe"];
        assert_eq!(
            pick_executable("claude.exe", &present, &win()),
            Some("claude.exe")
        );
        // Case-insensitively, as the filesystem is.
        let present = ["Claude.EXE"];
        assert_eq!(
            pick_executable("claude.exe", &present, &win()),
            Some("Claude.EXE")
        );
    }

    /// **A name whose extension is not in `PATHEXT` is not executable.** A
    /// `.ps1` cannot be spawned directly and a bare `sh` script cannot be
    /// spawned at all, so neither may be the answer just because it is there.
    #[test]
    fn a_non_pathext_extension_is_not_an_executable() {
        assert_eq!(pick_executable("run.ps1", &["run.ps1"], &win()), None);
        assert_eq!(pick_executable("zzq", &["zzq"], &win()), None);
        assert_eq!(pick_executable("zzq", &["zzq", "zzq.ps1"], &win()), None);
    }

    /// Nothing at all in the directory, and a directory holding only other
    /// programs.
    #[test]
    fn an_empty_or_unrelated_directory_answers_nothing() {
        assert_eq!(pick_executable("claude", &[], &win()), None);
        assert_eq!(
            pick_executable("claude", &["node.exe", "npm.cmd"], &win()),
            None
        );
    }

    /// **Off Windows the bare name is the whole rule.** `pathext` is empty
    /// there, and the executable bit is the test rather than the name — so the
    /// Windows-only refusal above must not reach a Linux or macOS install.
    #[test]
    fn without_pathext_the_bare_name_is_the_answer() {
        let none: Vec<String> = Vec::new();
        assert_eq!(
            pick_executable("claude", &["claude"], &none),
            Some("claude")
        );
        assert_eq!(pick_executable("claude", &["claude.cmd"], &none), None);
    }
}
