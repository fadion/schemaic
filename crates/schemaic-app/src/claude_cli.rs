//! Locating the `claude` CLI binary: auto-detection, a minimal `PATH`/`PATHEXT`
//! `which`, and resolving a user-provided override to a real executable. Pure
//! (std-only) — no app state. Shared by the AI-session spawn (`ai::start_ai_session`)
//! and the AI panel's reachability check. (The Windows-`PATHEXT` handling is the
//! H12 subtlety: `Command::new` alone won't append `.cmd`/`.exe`.)

// ===== moved from main.rs (claude CLI discovery) =====
/// Auto-detect the `claude` binary: `$SCHEMAIC_CLAUDE_BIN`, then `~/.local/bin`,
/// then a `PATH` search. Returns `None` when it can't be found anywhere, so the
/// UI can honestly report a failed auto-detect instead of a phantom `claude`.
pub(crate) fn detect_claude_bin() -> Option<String> {
    if let Ok(p) = std::env::var("SCHEMAIC_CLAUDE_BIN")
        && !p.trim().is_empty()
    {
        return Some(p);
    }
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let exe = std::path::PathBuf::from(home)
            .join(".local")
            .join("bin")
            .join(if cfg!(windows) {
                "claude.exe"
            } else {
                "claude"
            });
        if exe.exists() {
            return Some(exe.to_string_lossy().into_owned());
        }
    }
    which_on_path("claude")
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

/// Is Claude reachable for this settings value? Empty = auto-detect must succeed;
/// otherwise the manual override must resolve to a real binary. Drives the UI's
/// "Claude not connected" state and the disabled message box.
pub(crate) fn claude_reachable(cli_path: &str) -> bool {
    let t = cli_path.trim();
    if t.is_empty() {
        detect_claude_bin().is_some()
    } else {
        resolve_override(t).is_some()
    }
}

/// Resolve the `claude` binary for launching: a non-empty user override (AI
/// settings) wins, otherwise auto-detect, otherwise bare `claude` as a last-ditch
/// spawn attempt (which then fails with the usual "not installed" error).
///
/// The override is resolved the same way [`claude_reachable`] validates it
/// (`resolve_override`: PATH + Windows `PATHEXT`), so a bare `claude` or an
/// extension-less `C:\tools\claude` that settings reports as reachable actually
/// spawns — `Command::new` alone won't append `.cmd`/`.exe` (review H12).
pub(crate) fn claude_bin(override_path: &str) -> String {
    let t = override_path.trim();
    if !t.is_empty() {
        return resolve_override(t).unwrap_or_else(|| t.to_string());
    }
    detect_claude_bin().unwrap_or_else(|| "claude".to_string())
}

/// What sealing flags the CLI at `bin` accepts, from its own `--help`.
///
/// **Cached per resolved binary path**, because the answer cannot change while
/// that file does not and the probe costs ~140 ms — enough to be felt if it ran
/// on the UI thread before every AI action instead of once.
/// [`warm_seal_cache`] fills it off-thread at startup so the first AI action
/// usually finds it already there.
///
/// A probe that cannot run at all yields [`schemaic_ai::CliSeal::ALL`] (see
/// [`schemaic_ai::seal_from_help`]): a binary that answers nothing is one the
/// spawn is about to fail on anyway, and guessing *absent* there would silently
/// unseal a session on a CLI that was merely busy.
///
/// **The one stale case is an upgrade in place**, where the path does not change
/// but the binary behind it grows the flags: the cached answer stays until
/// Schemaic restarts, so a session spawned in between is sealed the old way.
/// What covers it is `DISALLOWED_TOOLS`, which is passed at every seal level and
/// now names the whole measured built-in set — the two halves of that change
/// exist for each other, and dropping either leaves this window open.
pub(crate) fn claude_seal(bin: &str) -> schemaic_ai::CliSeal {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, schemaic_ai::CliSeal>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(c) = cache.lock()
        && let Some(seal) = c.get(bin)
    {
        return *seal;
    }
    let help = std::process::Command::new(bin)
        .arg("--help")
        .output()
        .ok()
        .map(|o| {
            // `--help` goes to stdout; keep stderr as a fallback for a CLI that
            // prints usage there, so a readable answer isn't mistaken for none.
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            s
        })
        .unwrap_or_default();
    let seal = schemaic_ai::seal_from_help(&help);
    if let Ok(mut c) = cache.lock() {
        c.insert(bin.to_string(), seal);
    }
    seal
}

/// Fill [`claude_seal`]'s cache for `bin` on a background thread.
///
/// Called once at startup with the auto-detected path. Nothing waits on it: a
/// miss simply pays the probe where it would have anyway.
pub(crate) fn warm_seal_cache(bin: String) {
    std::thread::spawn(move || {
        let _ = claude_seal(&bin);
    });
}
