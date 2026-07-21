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
    // Decode UTF-16LE, then split on lines, trimming NULs/whitespace.
    let u16s: Vec<u16> = out
        .stdout
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
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
