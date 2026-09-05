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

/// Minimal `which`: locate `name` on `PATH`, honoring `PATHEXT` on Windows.
pub(crate) fn which_on_path(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    let exts = pathext();
    for dir in std::env::split_paths(&path) {
        let direct = dir.join(name);
        if direct.is_file() {
            return Some(direct.to_string_lossy().into_owned());
        }
        for ext in &exts {
            let cand = dir.join(format!("{name}{ext}"));
            if cand.is_file() {
                return Some(cand.to_string_lossy().into_owned());
            }
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
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<(Harness, String), Probe>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
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
    let Some(o) = std::process::Command::new(bin).arg("--help").output().ok() else {
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
        isolate_config: h == Harness::Codex && o.status.success() && codex_isolates_config(&both),
    };
    if let Ok(mut c) = cache.lock() {
        c.insert(key, probe);
    }
    probe
}

/// The binary for the **one-shot inline** generations — Ctrl+K, AI Fill, AI
/// Seed — which are Claude-only.
///
/// Those three build their argv with `schemaic_ai::inline_args`, which is
/// Claude's flag set (`-p --append-system-prompt --model` plus the seal). No
/// other harness accepts it, so they always spawn Claude regardless of which
/// harness drives the *chat panel*.
///
/// **The override path is honoured only when Claude is the selected harness.**
/// `ai_cli_path` points at whichever CLI the user chose; handing that path to a
/// Claude spawn would run `codex` with Claude's flags and die on the first
/// unknown option. When another harness is selected, this auto-detects Claude
/// instead and fails with the ordinary "not installed" message if it is absent —
/// which is the truth: that feature needs Claude and it is not there.
pub(crate) fn inline_claude_bin(selected: Harness, override_path: &str) -> String {
    let path = if selected == Harness::Claude {
        override_path
    } else {
        ""
    };
    harness_bin(Harness::Claude, path)
}

/// The model id for those same one-shot generations.
///
/// **The exact counterpart of [`inline_claude_bin`], and it was missing.** That
/// function refuses to hand Claude's argv another harness's *binary*; the model
/// id travelled anyway. `ai_model` follows the selected harness — under Codex
/// the suggestion chips are `gpt-5.4`, `gpt-5.4-codex`, `o3` — so Ctrl+K after
/// picking one spawned `claude … --model gpt-5.4-codex` and died on an unknown
/// model, with the same "check your installation" message covering the same
/// wrong cause.
///
/// Empty when another harness is selected, which `inline_args` omits, so the
/// generation runs on Claude's own default. Guessing a Claude equivalent for the
/// id the user picked would be inventing a mapping between two vendors'
/// catalogues.
pub(crate) fn inline_claude_model(selected: Harness, model: &str) -> String {
    match selected == Harness::Claude {
        true => model.to_string(),
        false => String::new(),
    }
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
pub(crate) fn warm_probe_cache(h: Harness, bin: String) {
    std::thread::spawn(move || {
        let _ = probe(h, &bin);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The trap: `ai_cli_path` follows the *selected* harness, so on Codex it is
    /// a path to `codex`. Passing that to a Claude-flagged spawn runs the wrong
    /// binary with flags it has never heard of.
    #[test]
    fn the_inline_paths_never_spawn_another_harnesss_binary() {
        let codex_path = "/nonexistent/zz-codex";
        assert_ne!(
            inline_claude_bin(Harness::Codex, codex_path),
            codex_path,
            "Codex's binary was handed to a Claude-only argv"
        );
        assert_ne!(
            inline_claude_bin(Harness::Antigravity, codex_path),
            codex_path
        );
        // With Claude selected the override is exactly what it has always been.
        assert_eq!(inline_claude_bin(Harness::Claude, codex_path), codex_path);
    }

    /// The same trap one field over, and it was live: the binary was guarded and
    /// the **model id** was not, so `claude … --model gpt-5.4-codex` was what
    /// Ctrl+K spawned after picking a Codex chip.
    #[test]
    fn the_inline_paths_never_pass_another_harnesss_model_id() {
        // A real id from another harness's own suggestion list, so this fails if
        // the guard is dropped rather than if a placeholder changes.
        for id in Harness::Codex.suggested_models() {
            assert_eq!(inline_claude_model(Harness::Codex, id), "");
        }
        assert_eq!(inline_claude_model(Harness::Antigravity, "anything"), "");
        // Claude's own choice still travels, and empty stays empty — which
        // `inline_args` omits, leaving the CLI its default.
        assert_eq!(inline_claude_model(Harness::Claude, "opus"), "opus");
        assert_eq!(inline_claude_model(Harness::Claude, ""), "");
    }

    /// The seam the pure test cannot see: the binary and the id are two
    /// decisions and the bug was that only one of them was made. Whatever
    /// `inline_claude_bin` decides about the override, `inline_claude_model`
    /// must decide the same way about the model — one selected harness, one
    /// answer.
    #[test]
    fn the_inline_binary_and_the_inline_model_agree_on_who_is_selected() {
        for h in Harness::ALL {
            let honours_path = inline_claude_bin(h, "/nonexistent/zz-cli") == "/nonexistent/zz-cli";
            let honours_model = !inline_claude_model(h, "some-id").is_empty();
            assert_eq!(
                honours_path, honours_model,
                "{h:?}: the override path and the model id disagree about the selected harness"
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
