//! Shell discovery + launch config. Cross-platform: Windows (PowerShell / cmd /
//! Git Bash / WSL distros) and Unix (`$SHELL`, `/etc/shells`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// How to launch a shell: a program + its args (env/cwd applied at spawn time).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellConfig {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory; `None` → user home.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Extra environment variables applied at spawn (e.g. `MYSQL_PWD` for the DB
    /// CLI — keeps the password off the command line and out of shell history).
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

/// A named shell the user can pick in settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellProfile {
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl ShellProfile {
    pub fn config(&self) -> ShellConfig {
        ShellConfig {
            program: self.program.clone(),
            args: self.args.clone(),
            cwd: None,
            env: Vec::new(),
        }
    }
}

/// Which shell profile a settings save should write: the picker's row, or —
/// when the **loaded** profile is not among the detected ones — the loaded one,
/// untouched.
///
/// **A saved choice must survive a launch on which it could not be detected.**
/// The picker index is an index into a list built by `detect_shells()`, and
/// that list is short whenever the shell is not reachable at startup: on
/// Windows `wsl.exe -l -q` exiting non-zero yields no WSL rows at all, and on
/// Unix a program that has left `/etc/shells` (or an unreadable `/etc/shells`)
/// leaves a single `/bin/bash`. The index then resolved to `0`, and the
/// appearance-saving effect — which floem runs **immediately on creation** —
/// rewrote `terminal.json` with `detected[0]` before the window was drawn, with
/// no user action. The choice was then gone for good: WSL coming back could not
/// restore it, because nothing remembered it any more. The same index sent the
/// Restart icon into a different shell mid-session.
///
/// The session itself was always right — the terminal spawns from
/// `TerminalSettings::shell` directly — so there was no visible symptom until
/// the next launch, or the first Restart.
///
/// Once the user picks a shell, `selected` names a row of `detected` and that
/// row wins; `loaded` only answers for a profile the list does not contain.
///
/// `loaded` is *what the session is running*, not what was on disk — see
/// [`initial_shell_state`], which is where the two stopped being the same.
pub fn shell_to_persist(
    loaded: Option<&ShellProfile>,
    detected: &[ShellProfile],
    selected: usize,
) -> Option<ShellProfile> {
    if let Some(saved) = loaded
        && !detected
            .iter()
            .any(|d| d.program == saved.program && d.args == saved.args)
    {
        return Some(saved.clone());
    }
    detected.get(selected).cloned()
}

/// Which picker row to start on, and which profile the session is **actually
/// running**, for a launch with `saved` on disk and `running` spawned.
///
/// **The third input `shell_to_persist` needed and never had.** On a first run
/// there is no saved profile, so the session spawns [`default_shell`] — `$SHELL`
/// on Unix — while the picker index fell through to `0`. `detect_shells()` on
/// Unix is `/etc/shells` in file order, whose first line on Debian and Ubuntu is
/// `/bin/sh`. The appearance-saving effect, which floem runs **immediately on
/// creation**, then wrote `/bin/sh` into `terminal.json` before the window was
/// drawn and with no user action. The running session was still zsh, so there
/// was no symptom until the *next* launch opened `/bin/sh`: no history, no
/// prompt, no completion, and nothing saying why. The picker had agreed with the
/// file and not with what was running since the first run.
///
/// This is the same mechanism [`shell_to_persist`]'s doc describes, on the
/// branch that guard does not cover (`loaded == None`). Windows escaped it by
/// coincidence: `default_shell()` and `detect_shells()[0]` are the same profile
/// there, which is why the platform this shipped on never showed it.
///
/// Returns `(row, running)`. `running` is `Some` whenever a profile can be named
/// for what is spawned, so the `loaded` branch above answers even when the
/// running shell is absent from `detected` — a `$SHELL` outside `/etc/shells`.
pub fn initial_shell_state(
    saved: Option<&ShellProfile>,
    running: &ShellConfig,
    detected: &[ShellProfile],
) -> (usize, Option<ShellProfile>) {
    let same =
        |d: &ShellProfile, program: &str, args: &[String]| d.program == program && d.args == args;
    // A saved profile the list holds keeps its row; a saved profile it does not
    // hold keeps row 0 and is protected by `shell_to_persist`'s `loaded` branch,
    // exactly as before.
    if let Some(s) = saved {
        let row = detected
            .iter()
            .position(|d| same(d, &s.program, &s.args))
            .unwrap_or(0);
        return (row, Some(s.clone()));
    }
    let row = detected
        .iter()
        .position(|d| same(d, &running.program, &running.args));
    let profile = match row.and_then(|i| detected.get(i)) {
        // Named after the detected row, so the picker's label and the file agree.
        Some(d) => d.clone(),
        None => ShellProfile {
            name: program_label(&running.program),
            program: running.program.clone(),
            args: running.args.clone(),
        },
    };
    (row.unwrap_or(0), Some(profile))
}

/// A display name for a program with no detected row: its file stem, or the
/// whole string when there isn't one.
fn program_label(program: &str) -> String {
    std::path::Path::new(program)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(program)
        .to_string()
}

fn default_font_size() -> u16 {
    13
}
fn default_cursor_style() -> String {
    "block".to_string()
}
fn default_true() -> bool {
    true
}

/// Persisted terminal preferences.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TerminalSettings {
    /// The chosen shell; `None` → auto-detected default.
    #[serde(default)]
    pub shell: Option<ShellProfile>,
    /// Terminal font size (logical px).
    #[serde(default = "default_font_size")]
    pub font_size: u16,
    /// Copy the selection to the clipboard as soon as a drag-select ends.
    #[serde(default)]
    pub copy_on_select: bool,
    /// Cursor shape key: `block` / `bar` / `underline`.
    #[serde(default = "default_cursor_style")]
    pub cursor_style: String,
    /// Whether the cursor blinks while the terminal is focused.
    #[serde(default = "default_true")]
    pub cursor_blink: bool,
}

// Manual `Default` (not derived) so a missing file defaults font size to 13,
// cursor to a blinking block — not `0` / empty string / `false`.
impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            shell: None,
            font_size: default_font_size(),
            copy_on_select: false,
            cursor_style: default_cursor_style(),
            cursor_blink: true,
        }
    }
}

/// Resolve `program` against `PATH` (honoring `PATHEXT` on Windows).
pub fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let direct = dir.join(program);
        if direct.is_file() {
            return Some(direct);
        }
        #[cfg(windows)]
        {
            let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into());
            for ext in exts.split(';') {
                let cand = dir.join(format!("{program}{ext}"));
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    None
}

/// The default shell to open on first run.
pub fn default_shell() -> ShellConfig {
    #[cfg(windows)]
    {
        if which("pwsh.exe").is_some() {
            return ShellConfig {
                program: "pwsh.exe".into(),
                args: vec!["-NoLogo".into()],
                cwd: None,
                env: Vec::new(),
            };
        }
        ShellConfig {
            program: "powershell.exe".into(),
            args: vec!["-NoLogo".into()],
            cwd: None,
            env: Vec::new(),
        }
    }
    #[cfg(not(windows))]
    {
        let program = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        ShellConfig {
            program,
            args: vec![],
            cwd: None,
            env: Vec::new(),
        }
    }
}

/// All shells we can find on this machine, for the settings picker.
pub fn detect_shells() -> Vec<ShellProfile> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        if which("pwsh.exe").is_some() {
            out.push(ShellProfile {
                name: "PowerShell 7".into(),
                program: "pwsh.exe".into(),
                args: vec!["-NoLogo".into()],
            });
        }
        out.push(ShellProfile {
            name: "Windows PowerShell".into(),
            program: "powershell.exe".into(),
            args: vec!["-NoLogo".into()],
        });
        out.push(ShellProfile {
            name: "Command Prompt".into(),
            program: "cmd.exe".into(),
            args: vec![],
        });
        if which("bash.exe").is_some() {
            out.push(ShellProfile {
                name: "Git Bash".into(),
                program: "bash.exe".into(),
                args: vec!["-i".into(), "-l".into()],
            });
        }
        for distro in wsl_distros() {
            out.push(ShellProfile {
                name: format!("WSL · {distro}"),
                program: "wsl.exe".into(),
                args: vec!["-d".into(), distro],
            });
        }
    }
    #[cfg(not(windows))]
    {
        let mut seen = std::collections::HashSet::new();
        if let Ok(shells) = std::fs::read_to_string("/etc/shells") {
            for line in shells.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let name = std::path::Path::new(line)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(line)
                    .to_string();
                if seen.insert(line.to_string()) {
                    out.push(ShellProfile {
                        name,
                        program: line.to_string(),
                        args: vec![],
                    });
                }
            }
        }
        if out.is_empty() {
            out.push(ShellProfile {
                name: "bash".into(),
                program: "/bin/bash".into(),
                args: vec![],
            });
        }
    }
    out
}

/// List installed WSL distributions (`wsl.exe -l -q`). The output is UTF-16LE.
#[cfg(windows)]
fn wsl_distros() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(out) = std::process::Command::new("wsl.exe")
        .args(["-l", "-q"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    parse_wsl_list(&out.stdout)
}

/// Decode `wsl.exe -l -q`'s UTF-16LE output into distro names: one per line,
/// each trimmed of NULs/whitespace, blanks dropped. Pure so it's unit-testable
/// without spawning `wsl.exe`. A trailing odd byte (incomplete code unit) lands
/// in `as_chunks`' remainder and is dropped.
#[cfg(windows)]
fn parse_wsl_list(stdout: &[u8]) -> Vec<String> {
    let u16s: Vec<u16> = stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16_lossy(&u16s)
        .lines()
        .map(|l| {
            l.trim_matches(|c: char| c == '\0' || c.is_whitespace())
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .collect()
}

#[cfg(test)]
mod persist_tests {
    use super::{ShellProfile, initial_shell_state, shell_to_persist};
    use crate::ShellConfig;

    fn p(name: &str, program: &str) -> ShellProfile {
        ShellProfile {
            name: name.to_string(),
            program: program.to_string(),
            args: Vec::new(),
        }
    }

    /// **A saved shell survives a launch that could not detect it.**
    ///
    /// The saved profile was found by matching `(program, args)` against the
    /// detected list, and a miss resolved to index `0` — so on a launch where
    /// `wsl.exe -l -q` exits non-zero (or a Unix shell has left
    /// `/etc/shells`), the appearance-saving effect, which floem runs
    /// immediately on creation, rewrote `terminal.json` with `detected[0]`
    /// before the window was drawn and with no user action. The choice was
    /// gone for good: the shell coming back could not restore it, because
    /// nothing remembered it any more.
    #[test]
    fn a_saved_shell_the_launch_could_not_detect_is_kept() {
        let wsl = ShellProfile {
            name: "WSL · Ubuntu".into(),
            program: "wsl.exe".into(),
            args: vec!["-d".into(), "Ubuntu".into()],
        };
        let detected = [
            p("PowerShell 7", "pwsh.exe"),
            p("Command Prompt", "cmd.exe"),
        ];

        assert_eq!(
            shell_to_persist(Some(&wsl), &detected, 0),
            Some(wsl.clone()),
            "the index fell back to 0 and PowerShell overwrote the user's choice"
        );
        // …and on an empty detection too, which is what an unreadable
        // `/etc/shells` and a failed `wsl -l` both look like.
        assert_eq!(shell_to_persist(Some(&wsl), &[], 0), Some(wsl.clone()));
    }

    /// A profile the list **does** contain is the picker's business: the row
    /// wins, which is what makes picking a different shell take effect.
    #[test]
    fn a_detected_shell_follows_the_picker() {
        let ps = p("PowerShell 7", "pwsh.exe");
        let cmd = p("Command Prompt", "cmd.exe");
        let detected = [ps.clone(), cmd.clone()];

        assert_eq!(shell_to_persist(Some(&ps), &detected, 1), Some(cmd.clone()));
        assert_eq!(shell_to_persist(Some(&cmd), &detected, 0), Some(ps.clone()));
        // Nothing loaded yet: the row, as before.
        assert_eq!(shell_to_persist(None, &detected, 1), Some(cmd));
        // A row that is not there answers nothing rather than panicking.
        assert_eq!(shell_to_persist(None, &detected, 9), None);
        assert_eq!(shell_to_persist(None, &[], 0), None);
    }

    /// **A first run persists the shell it is actually running.**
    ///
    /// The composition, not the pure function: `shell_to_persist(None, …, 0)`
    /// was doing exactly what it was told, and what it was told was wrong.
    /// `init_selected` fell through to `0` when nothing was saved, and on
    /// Debian/Ubuntu `/etc/shells[0]` is `/bin/sh` while the session spawns
    /// `$SHELL`. The save effect runs immediately on creation, so `/bin/sh` was
    /// in `terminal.json` before the window was drawn — with the zsh session
    /// still running, so nothing looked wrong until the next launch.
    #[test]
    fn a_first_run_persists_the_shell_it_is_running() {
        let sh = p("sh", "/bin/sh");
        let bash = p("bash", "/bin/bash");
        let zsh = p("zsh", "/usr/bin/zsh");
        let detected = [sh.clone(), bash.clone(), zsh.clone()];
        let running = ShellConfig {
            program: "/usr/bin/zsh".into(),
            args: vec![],
            cwd: None,
            env: Vec::new(),
        };

        // What the old wiring handed the save — `current_shell` seeded from
        // `term_prefs.shell` (None) and the row fallen through to 0. Still true
        // of the pure function, which is the point: it was doing what it was
        // told.
        assert_eq!(shell_to_persist(None, &detected, 0), Some(sh.clone()));

        let (row, current) = initial_shell_state(None, &running, &detected);
        assert_eq!(row, 2, "the picker must start on what is running");
        assert_eq!(current.as_ref(), Some(&zsh));
        // …and the save that follows writes it, which is the whole point.
        assert_eq!(
            shell_to_persist(current.as_ref(), &detected, row),
            Some(zsh.clone())
        );

        // A `$SHELL` that is not in `/etc/shells` at all: the row cannot name
        // it, so the `loaded` branch has to — which is why `current` is `Some`
        // on a first run at all.
        let exotic = ShellConfig {
            program: "/opt/fish/bin/fish".into(),
            args: vec![],
            cwd: None,
            env: Vec::new(),
        };
        let (row, current) = initial_shell_state(None, &exotic, &detected);
        assert_eq!(row, 0);
        let got = shell_to_persist(current.as_ref(), &detected, row).expect("a profile");
        assert_eq!(got.program, "/opt/fish/bin/fish");
        assert_eq!(got.name, "fish", "named from the program for the picker");

        // A saved profile is untouched in both directions — detected or not.
        let (row, current) = initial_shell_state(Some(&bash), &running, &detected);
        assert_eq!((row, current.as_ref()), (1, Some(&bash)));
        let gone = p("Ubuntu", "wsl.exe");
        let (row, current) = initial_shell_state(Some(&gone), &running, &detected);
        assert_eq!((row, current.as_ref()), (0, Some(&gone)));
        assert_eq!(
            shell_to_persist(current.as_ref(), &detected, row),
            Some(gone),
            "a saved profile this launch could not detect still survives"
        );

        // Nothing detected at all: no row to start on, and the running shell is
        // still what gets written.
        let (row, current) = initial_shell_state(None, &running, &[]);
        assert_eq!(row, 0);
        assert_eq!(
            shell_to_persist(current.as_ref(), &[], row),
            Some(zsh),
            "an empty list must not erase the running shell"
        );
    }

    /// **The args are half the identity.** Two WSL distros differ only by
    /// `-d <name>`, so matching on the program alone would call a saved
    /// `Ubuntu` detected because `Debian` is in the list, and the picker's row
    /// would then overwrite it.
    #[test]
    fn two_profiles_of_one_program_are_told_apart_by_their_args() {
        let ubuntu = ShellProfile {
            name: "WSL · Ubuntu".into(),
            program: "wsl.exe".into(),
            args: vec!["-d".into(), "Ubuntu".into()],
        };
        let debian = ShellProfile {
            name: "WSL · Debian".into(),
            program: "wsl.exe".into(),
            args: vec!["-d".into(), "Debian".into()],
        };
        assert_eq!(
            shell_to_persist(Some(&ubuntu), &[debian], 0),
            Some(ubuntu),
            "one distro's presence vouched for another's"
        );
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::parse_wsl_list;

    /// Encode `s` as UTF-16LE bytes (what `wsl.exe -l -q` emits).
    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    #[test]
    fn parses_distro_names_one_per_line() {
        let bytes = utf16le("Ubuntu\r\nDebian\r\n");
        assert_eq!(parse_wsl_list(&bytes), vec!["Ubuntu", "Debian"]);
    }

    #[test]
    fn trims_nuls_and_whitespace_and_drops_blanks() {
        // wsl.exe interleaves NULs and pads with spaces; blank lines dropped.
        let bytes = utf16le("Ubuntu\0 \r\n\r\n  Debian  \r\n");
        assert_eq!(parse_wsl_list(&bytes), vec!["Ubuntu", "Debian"]);
    }

    #[test]
    fn empty_output_is_empty_list() {
        assert!(parse_wsl_list(&[]).is_empty());
        assert!(parse_wsl_list(&utf16le("\r\n \r\n")).is_empty());
    }

    #[test]
    fn trailing_odd_byte_is_ignored() {
        let mut bytes = utf16le("Ubuntu");
        bytes.push(0x00); // incomplete final code unit
        assert_eq!(parse_wsl_list(&bytes), vec!["Ubuntu"]);
    }
}
