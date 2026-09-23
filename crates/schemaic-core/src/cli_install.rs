//! Putting the `schemaic` command on `PATH`, and taking it off again — the
//! decision half of Settings → General → Command line → Install / Remove, and
//! of the Windows uninstall hook.
//!
//! The CLI itself needs nothing installed: it is an argv branch of the app's own
//! binary (plus the `schemaic.com` console shim on Windows). What it lacks is a
//! way to be reached **by name**, and that is a per-platform write to the
//! user's own environment, so it happens only when asked — never at install
//! time — and it reports back the path it resolved.
//!
//! - **Windows** appends the directory holding `schemaic.com` to the *user*
//!   `PATH` (`HKCU\Environment`), keeping the value's type and every entry as
//!   written: an unexpanded `%USERPROFILE%\…` stays unexpanded. That is the
//!   classic `setx PATH "%PATH%;…"` bug, which writes the *expanded, merged*
//!   system-plus-user value back into the user key.
//! - **macOS and a loose Linux binary** link `~/.local/bin/schemaic` to the
//!   running executable. Under an **AppImage** the running executable is inside
//!   a mount that disappears at exit, so the link targets `$APPIMAGE` instead.
//! - **deb/rpm** already install `/usr/bin/schemaic`: the planner finds that
//!   directory on `PATH` and there is nothing to do.
//!
//! [`removal`] is the inverse, and only ever undoes what Install could have
//! written: this copy's folder out of the user `PATH`, or a link that points at
//! this copy (or at nothing). On Windows Velopack's uninstall hook runs it too;
//! nothing of ours runs when a macOS app or an AppImage is thrown away, so
//! there the Remove button is the only undo.
//!
//! Everything here is pure; the registry, the symlink and the broadcast live at
//! the app boundary (`schemaic-app`'s `install_cli`).

use std::path::{Path, PathBuf};

/// The name a terminal types.
pub const COMMAND: &str = "schemaic";

/// The Windows console shim beside `schemaic.exe`. `PATHEXT` lists `.COM`
/// before `.EXE`, so a bare `schemaic` at a prompt resolves to it while
/// shortcuts keep launching the GUI binary.
pub const WINDOWS_SHIM: &str = "schemaic.com";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
}

impl Os {
    /// The platform this binary was built for.
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

/// Everything [`plan`] reads from the machine, gathered at the app boundary.
#[derive(Clone, Debug)]
pub struct Probe {
    pub os: Os,
    /// `std::env::current_exe()`.
    pub exe: PathBuf,
    /// `$APPIMAGE` — set by the AppImage runtime to the image file itself.
    pub appimage: Option<PathBuf>,
    pub home: Option<PathBuf>,
    /// The process's own `PATH`, lossily decoded.
    pub path_var: String,
    /// Windows only: whether [`WINDOWS_SHIM`] sits beside `exe`.
    pub console_shim: bool,
}

/// What Install will do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// The command already resolves from `dir`, which is on `PATH`.
    Already { dir: PathBuf },
    /// Windows: append `dir` to the user `PATH`.
    AddUserPath { dir: PathBuf },
    /// macOS/Linux: make `link` a symlink to `target`.
    Link { link: PathBuf, target: PathBuf },
}

/// Decide what Install does on this machine, or why it can't.
pub fn plan(p: &Probe) -> Result<Plan, String> {
    match p.os {
        Os::Windows => {
            let exe = p.exe.to_string_lossy();
            let dir = parent_of(&exe, Os::Windows)
                .ok_or_else(|| format!("Can't tell which folder {exe} is in."))?;
            if !p.console_shim {
                return Err(format!(
                    "There is no {WINDOWS_SHIM} beside this copy of Schemaic, in {dir}, so there \
                     is no console command to put on PATH. The installed app has one; a \
                     development build doesn't."
                ));
            }
            let dir = PathBuf::from(dir);
            Ok(if path_has_dir(&p.path_var, &dir, Os::Windows) {
                Plan::Already { dir }
            } else {
                Plan::AddUserPath { dir }
            })
        }
        Os::MacOs | Os::Linux => {
            let target = link_target(p);
            if let Some(dir) = on_path_as_command(p, &target) {
                return Ok(Plan::Already { dir });
            }
            Ok(Plan::Link {
                link: link_path(p)?,
                target,
            })
        }
    }
}

/// What a link should point at. Under an AppImage `exe` is inside the image's
/// mount, which is gone the moment the app exits — a link to it, or "already
/// on PATH" because of it, would be false by the next terminal.
fn link_target(p: &Probe) -> PathBuf {
    match (&p.appimage, p.os) {
        (Some(image), Os::Linux) => image.clone(),
        _ => p.exe.clone(),
    }
}

/// The directory `target` already resolves from by name, if it does — a
/// deb/rpm's `/usr/bin/schemaic`.
fn on_path_as_command(p: &Probe, target: &Path) -> Option<PathBuf> {
    let t = target.to_string_lossy();
    let dir = parent_of(&t, p.os)?;
    (file_name(&t, p.os) == COMMAND && path_has_dir(&p.path_var, Path::new(dir), p.os))
        .then(|| PathBuf::from(dir))
}

/// `~/.local/bin/schemaic` — the one link Install writes and Remove removes.
fn link_path(p: &Probe) -> Result<PathBuf, String> {
    let home = p.home.as_ref().ok_or_else(|| {
        "There is no home directory to hold the link, so there is nowhere to look.".to_string()
    })?;
    let home = home.to_string_lossy();
    Ok(PathBuf::from(format!(
        "{}/.local/bin/{COMMAND}",
        home.trim_end_matches('/')
    )))
}

/// Does the symlink at `link`, whose `read_link` is `to`, point at `target`? A
/// relative `to` is resolved lexically against the link's own directory.
fn link_points_at(link: &Path, to: &Path, target: &Path) -> bool {
    let to_s = to.to_string_lossy();
    let resolved = if to_s.starts_with('/') {
        lexical(&to_s)
    } else {
        let link_s = link.to_string_lossy();
        let base = parent_of(&link_s, Os::Linux).unwrap_or("");
        lexical(&format!("{base}/{to_s}"))
    };
    resolved == lexical(&target.to_string_lossy())
}

/// The separators of `os`'s paths. String work rather than `Path`, so the
/// Windows arms answer the same when the tests run on Linux, where a backslash
/// is an ordinary byte of a file name.
fn seps(os: Os) -> &'static [char] {
    match os {
        Os::Windows => &['\\', '/'],
        Os::MacOs | Os::Linux => &['/'],
    }
}

/// Everything before the last separator; the root itself when that is the only
/// one.
fn parent_of(path: &str, os: Os) -> Option<&str> {
    let i = path.rfind(seps(os))?;
    Some(if i == 0 { &path[..1] } else { &path[..i] })
}

fn file_name(path: &str, os: Os) -> &str {
    path.rfind(seps(os)).map_or(path, |i| &path[i + 1..])
}

/// One `PATH` entry in the form two entries are compared in.
fn normalize_entry(entry: &str, os: Os) -> String {
    let mut s = match os {
        Os::Windows => entry
            .trim()
            .trim_matches('"')
            .trim()
            .replace('/', "\\")
            .to_lowercase(),
        Os::MacOs | Os::Linux => entry.to_string(),
    };
    while s.len() > 1 && s.ends_with(seps(os)) {
        s.pop();
    }
    s
}

/// Is `dir` one of the entries of a `PATH`-style value?
///
/// Entries are compared with trailing separators dropped; on Windows also
/// case-insensitively, with `/` read as `\` and surrounding quotes removed. An
/// empty entry never matches — on Unix it means the current directory.
pub fn path_has_dir(path_var: &str, dir: &Path, os: Os) -> bool {
    let want = normalize_entry(&dir.to_string_lossy(), os);
    if want.is_empty() {
        return false;
    }
    let sep = if os == Os::Windows { ';' } else { ':' };
    path_var
        .split(sep)
        .any(|entry| normalize_entry(entry, os) == want)
}

/// Expand `%NAME%` references the way `ExpandEnvironmentStringsW` does: a name
/// `lookup` doesn't know, and a lone `%`, are left as written.
pub fn expand_env(s: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) if j > 0 => {
                let name = &after[..j];
                if let Some(value) = lookup(name) {
                    out.push_str(&value);
                    rest = &after[j + 1..];
                } else {
                    // The closing `%` may open the next reference.
                    out.push('%');
                    out.push_str(name);
                    rest = &after[j..];
                }
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The new *raw* user `PATH` with `dir` appended, or `None` when `raw` already
/// names it (after expanding `%NAME%` through `lookup`).
pub fn user_path_update(
    raw: &str,
    dir: &Path,
    lookup: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    if path_has_dir(&expand_env(raw, &lookup), dir, Os::Windows) {
        return None;
    }
    let dir = dir.to_string_lossy();
    Some(if raw.trim().is_empty() {
        dir.into_owned()
    } else if raw.ends_with(';') {
        format!("{raw}{dir}")
    } else {
        format!("{raw};{dir}")
    })
}

/// What already sits where the link is going.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Existing {
    Nothing,
    /// A symlink, its target as `read_link` gave it, and whether that target
    /// exists.
    Link {
        to: PathBuf,
        live: bool,
    },
    /// A regular file or directory.
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkStep {
    Create,
    /// The link already points at the target.
    Keep,
    /// A dangling link — an AppImage that was moved or deleted — is replaced.
    Replace,
    /// Something that isn't ours is left alone, and this says what.
    Refuse(String),
}

/// Whether to create, keep, replace or refuse `link` → `target`, given what is
/// already there.
pub fn link_step(existing: &Existing, link: &Path, target: &Path) -> LinkStep {
    let link_s = link.to_string_lossy();
    match existing {
        Existing::Nothing => LinkStep::Create,
        Existing::Other => LinkStep::Refuse(format!(
            "{link_s} already exists and isn't a link, so Install left it alone. Move it \
             aside and try again."
        )),
        Existing::Link { to, live } => {
            if link_points_at(link, to, target) {
                LinkStep::Keep
            } else if !live {
                LinkStep::Replace
            } else {
                LinkStep::Refuse(format!(
                    "{link_s} already points at {}. Remove it if that's an old copy of \
                     Schemaic, then try again.",
                    to.display()
                ))
            }
        }
    }
}

/// A `/`-separated path with `.` and `..` resolved without touching the disk.
fn lexical(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    let joined = parts.join("/");
    if path.starts_with('/') {
        format!("/{joined}")
    } else {
        joined
    }
}

/// What Remove will do — the inverse of [`Plan`], and deliberately narrower:
/// it only ever undoes what Install could have written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Removal {
    /// Windows: take `dir` out of the user `PATH`.
    UserPath { dir: PathBuf },
    /// macOS/Linux: remove `link` if it is ours — pointing at `target`, or at
    /// nothing.
    Unlink { link: PathBuf, target: PathBuf },
    /// The command resolves from `dir` because a package put it there, not
    /// Install; removing it is the package manager's job.
    Package { dir: PathBuf },
}

/// Decide what Remove does on this machine, or why it can't.
///
/// Unlike [`plan`] this does not ask for the Windows shim: the entry to remove
/// is this copy's folder whether or not `schemaic.com` survived, and the
/// uninstall hook in particular must clean up without it.
pub fn removal(p: &Probe) -> Result<Removal, String> {
    match p.os {
        Os::Windows => {
            let exe = p.exe.to_string_lossy();
            let dir = parent_of(&exe, Os::Windows)
                .ok_or_else(|| format!("Can't tell which folder {exe} is in."))?;
            Ok(Removal::UserPath {
                dir: PathBuf::from(dir),
            })
        }
        Os::MacOs | Os::Linux => {
            let target = link_target(p);
            if let Some(dir) = on_path_as_command(p, &target) {
                return Ok(Removal::Package { dir });
            }
            Ok(Removal::Unlink {
                link: link_path(p)?,
                target,
            })
        }
    }
}

/// The new *raw* user `PATH` with every entry naming `dir` taken out — after
/// `%NAME%` expansion through `lookup`, so an entry written unexpanded goes
/// too — or `None` when there was none. Every other entry, empty ones and a
/// trailing `;` included, is kept exactly as written.
pub fn user_path_remove(
    raw: &str,
    dir: &Path,
    lookup: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let mut removed = false;
    let kept: Vec<&str> = raw
        .split(';')
        .filter(|entry| {
            let ours = path_has_dir(&expand_env(entry, &lookup), dir, Os::Windows);
            removed |= ours;
            !ours
        })
        .collect();
    removed.then(|| kept.join(";"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnlinkStep {
    /// The link is ours: it points at the target, or at nothing.
    Remove,
    /// Nothing there — nothing to do.
    Absent,
    /// Something that isn't ours is left alone, and this says what.
    Refuse(String),
}

/// Whether Remove may delete `link`, given what is there. A dangling link goes
/// too: it is what a moved or deleted copy leaves behind, and it points at
/// nothing anyone could be relying on.
pub fn unlink_step(existing: &Existing, link: &Path, target: &Path) -> UnlinkStep {
    match existing {
        Existing::Nothing => UnlinkStep::Absent,
        Existing::Link { to, live } if !live || link_points_at(link, to, target) => {
            UnlinkStep::Remove
        }
        Existing::Link { to, .. } => UnlinkStep::Refuse(format!(
            "{} points at {}, not at this copy of Schemaic, so Remove left it alone.",
            link.display(),
            to.display()
        )),
        Existing::Other => UnlinkStep::Refuse(format!(
            "{} isn't a link Schemaic made, so Remove left it alone.",
            link.display()
        )),
    }
}

/// The report once `dir` has left the user `PATH`.
pub fn report_removed(dir: &Path) -> String {
    format!(
        "Removed {} from your user PATH. A terminal that is already open keeps the old PATH.",
        dir.display()
    )
}

/// The report when `dir` was not on the user `PATH` to begin with.
pub fn report_path_absent(dir: &Path) -> String {
    format!(
        "{} isn't on your user PATH, so there was nothing to remove.",
        dir.display()
    )
}

/// The report once `link` is gone.
pub fn report_unlinked(link: &Path) -> String {
    format!("Removed {}.", link.display())
}

/// The report when there was no link to remove.
pub fn report_link_absent(link: &Path) -> String {
    format!(
        "There is no {}, so there was nothing to remove.",
        link.display()
    )
}

/// The report for [`Removal::Package`].
pub fn report_package(dir: &Path) -> String {
    format!(
        "The schemaic command in {} came with the installed package, not from Install — \
         remove the package to remove it.",
        dir.display()
    )
}

/// The report for [`Plan::Already`].
pub fn report_already(dir: &Path) -> String {
    format!(
        "The schemaic command is already on your PATH, in {}.",
        dir.display()
    )
}

/// The report once `dir` is on the user `PATH` — whether Install wrote it or
/// found it already there in the registry but not yet in this process.
pub fn report_added(dir: &Path) -> String {
    format!(
        "Added {} to your user PATH. Open a new terminal to use schemaic — one that is \
         already open keeps the old PATH.",
        dir.display()
    )
}

/// The report once `link` points at `target`. Adds a hint when the link's
/// directory may not be on `PATH`: on macOS always, because an app started from
/// Finder gets launchd's minimal `PATH` and cannot tell; on Linux only when the
/// process's own `PATH` lacks it.
pub fn report_linked(link: &Path, target: &Path, os: Os, path_var: &str) -> String {
    let link_s = link.to_string_lossy();
    let done = format!("Linked {link_s} to {}.", target.display());
    let dir = parent_of(&link_s, os).unwrap_or("");
    match os {
        Os::MacOs => format!(
            "{done} If a new terminal can't find schemaic, add {dir} to your PATH in your \
             shell profile."
        ),
        _ if !path_has_dir(path_var, Path::new(dir), os) => format!(
            "{done} {dir} isn't on your PATH yet — add it in your shell profile, then open a \
             new terminal."
        ),
        _ => done,
    }
}

/// The Settings row's hint for this platform.
pub fn hint(os: Os) -> &'static str {
    match os {
        Os::Windows => "Run Schemaic from a terminal by name. Adds its folder to your user PATH.",
        Os::MacOs | Os::Linux => {
            "Run Schemaic from a terminal by name. Links it into ~/.local/bin."
        }
    }
}

/// The Settings row's lifecycle. Transient — never persisted.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum InstallState {
    #[default]
    Idle,
    Running,
    Removing,
    /// Installed (or already was), and the report saying where.
    Done(String),
    Failed(String),
}

impl InstallState {
    /// Is an Install or a Remove in flight? Either one blocks both buttons:
    /// a Remove racing an Install over the same registry value or link would
    /// leave whichever finished second as the answer, and the row would show
    /// the other.
    pub fn busy(&self) -> bool {
        matches!(self, InstallState::Running | InstallState::Removing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(exe: &str, path_var: &str, shim: bool) -> Probe {
        Probe {
            os: Os::Windows,
            exe: PathBuf::from(exe),
            appimage: None,
            home: Some(PathBuf::from(r"C:\Users\me")),
            path_var: path_var.to_string(),
            console_shim: shim,
        }
    }

    fn unix(os: Os, exe: &str, path_var: &str) -> Probe {
        Probe {
            os,
            exe: PathBuf::from(exe),
            appimage: None,
            home: Some(PathBuf::from("/home/me")),
            path_var: path_var.to_string(),
            console_shim: false,
        }
    }

    const VELOPACK_EXE: &str = r"C:\Users\me\AppData\Local\Schemaic\current\schemaic.exe";
    const VELOPACK_DIR: &str = r"C:\Users\me\AppData\Local\Schemaic\current";

    // ---- plan: Windows ----

    #[test]
    fn windows_adds_the_directory_holding_the_shim() {
        let p = win(VELOPACK_EXE, r"C:\Windows\System32;C:\Windows", true);
        assert_eq!(
            plan(&p),
            Ok(Plan::AddUserPath {
                dir: PathBuf::from(VELOPACK_DIR)
            })
        );
    }

    #[test]
    fn windows_already_on_path_is_nothing_to_do() {
        let path = format!(r"C:\Windows;{}\", VELOPACK_DIR.to_lowercase());
        assert_eq!(
            plan(&win(VELOPACK_EXE, &path, true)),
            Ok(Plan::Already {
                dir: PathBuf::from(VELOPACK_DIR)
            })
        );
    }

    #[test]
    fn windows_without_the_shim_is_refused_and_says_why() {
        // A dev build: `schemaic-cli.exe` sits beside the exe, not `schemaic.com`,
        // so the directory on PATH would give a terminal the GUI.
        let err = plan(&win(r"C:\src\target\debug\schemaic.exe", "", false)).unwrap_err();
        assert!(err.contains(WINDOWS_SHIM), "{err}");
        assert!(err.contains(r"C:\src\target\debug"), "{err}");
    }

    #[test]
    fn windows_ignores_appimage_and_home() {
        let mut p = win(VELOPACK_EXE, "", true);
        p.appimage = Some(PathBuf::from("/x.AppImage"));
        p.home = None;
        assert!(matches!(plan(&p), Ok(Plan::AddUserPath { .. })));
    }

    // ---- plan: macOS / Linux ----

    #[test]
    fn deb_and_rpm_are_already_on_path() {
        let p = unix(
            Os::Linux,
            "/usr/bin/schemaic",
            "/usr/local/bin:/usr/bin:/bin",
        );
        assert_eq!(
            plan(&p),
            Ok(Plan::Already {
                dir: PathBuf::from("/usr/bin")
            })
        );
    }

    #[test]
    fn a_binary_on_path_under_another_name_still_gets_a_link() {
        // Nothing named `schemaic` resolves from /opt/bin just because the
        // running binary lives there.
        let p = unix(Os::Linux, "/opt/bin/schemaic-nightly", "/opt/bin:/usr/bin");
        assert_eq!(
            plan(&p),
            Ok(Plan::Link {
                link: PathBuf::from("/home/me/.local/bin/schemaic"),
                target: PathBuf::from("/opt/bin/schemaic-nightly"),
            })
        );
    }

    #[test]
    fn an_appimage_links_the_image_not_its_mount() {
        let mut p = unix(
            Os::Linux,
            "/tmp/.mount_SchemaXYZ/usr/bin/schemaic",
            "/usr/bin",
        );
        p.appimage = Some(PathBuf::from("/home/me/Apps/Schemaic.AppImage"));
        assert_eq!(
            plan(&p),
            Ok(Plan::Link {
                link: PathBuf::from("/home/me/.local/bin/schemaic"),
                target: PathBuf::from("/home/me/Apps/Schemaic.AppImage"),
            })
        );
    }

    #[test]
    fn an_appimage_mount_on_path_is_not_already_installed() {
        // The mount directory could only be on PATH by accident; it vanishes at
        // exit, so it must not answer "already".
        let mut p = unix(
            Os::Linux,
            "/tmp/.mount_SchemaXYZ/usr/bin/schemaic",
            "/tmp/.mount_SchemaXYZ/usr/bin:/usr/bin",
        );
        p.appimage = Some(PathBuf::from("/home/me/Schemaic.AppImage"));
        assert!(matches!(plan(&p), Ok(Plan::Link { .. })));
    }

    #[test]
    fn macos_links_the_binary_inside_the_bundle() {
        let exe = "/Applications/Schemaic.app/Contents/MacOS/schemaic";
        let p = unix(Os::MacOs, exe, "/usr/bin:/bin");
        assert_eq!(
            plan(&p),
            Ok(Plan::Link {
                link: PathBuf::from("/home/me/.local/bin/schemaic"),
                target: PathBuf::from(exe),
            })
        );
    }

    #[test]
    fn macos_ignores_a_stray_appimage_variable() {
        let exe = "/Applications/Schemaic.app/Contents/MacOS/schemaic";
        let mut p = unix(Os::MacOs, exe, "");
        p.appimage = Some(PathBuf::from("/elsewhere.AppImage"));
        assert!(matches!(plan(&p), Ok(Plan::Link { target, .. }) if target == Path::new(exe)));
    }

    #[test]
    fn no_home_directory_is_refused() {
        let mut p = unix(Os::Linux, "/opt/schemaic/schemaic", "/usr/bin");
        p.home = None;
        assert!(plan(&p).unwrap_err().contains("home"));
    }

    // ---- path_has_dir ----

    #[test]
    fn path_has_dir_matches_an_exact_unix_entry() {
        assert!(path_has_dir(
            "/a:/usr/bin:/b",
            Path::new("/usr/bin"),
            Os::Linux
        ));
    }

    #[test]
    fn path_has_dir_ignores_trailing_slashes() {
        assert!(path_has_dir("/usr/bin/", Path::new("/usr/bin"), Os::Linux));
        assert!(path_has_dir("/usr/bin", Path::new("/usr/bin/"), Os::MacOs));
    }

    #[test]
    fn path_has_dir_is_case_sensitive_on_unix() {
        assert!(!path_has_dir("/Usr/Bin", Path::new("/usr/bin"), Os::Linux));
    }

    #[test]
    fn path_has_dir_does_not_match_a_prefix() {
        assert!(!path_has_dir(
            "/usr/bin2:/usr",
            Path::new("/usr/bin"),
            Os::Linux
        ));
    }

    #[test]
    fn path_has_dir_on_windows_folds_case_slashes_and_quotes() {
        let dir = Path::new(VELOPACK_DIR);
        assert!(path_has_dir(
            r#"C:\x;"c:/users/ME/appdata/local/schemaic/current/""#,
            dir,
            Os::Windows
        ));
    }

    #[test]
    fn path_has_dir_on_windows_splits_on_semicolons_only() {
        // `:` is part of a drive letter there, never a separator.
        assert!(!path_has_dir(r"C:\a:C:\b", Path::new(r"C:\b"), Os::Windows));
        assert!(path_has_dir(r"C:\a;C:\b", Path::new(r"C:\b"), Os::Windows));
    }

    #[test]
    fn path_has_dir_never_matches_an_empty_entry() {
        assert!(!path_has_dir("", Path::new(""), Os::Linux));
        assert!(!path_has_dir("::", Path::new(""), Os::Linux));
        assert!(!path_has_dir(";;", Path::new(""), Os::Windows));
    }

    #[test]
    fn path_has_dir_does_not_reduce_a_root_to_nothing() {
        assert!(path_has_dir("/usr:/", Path::new("/"), Os::Linux));
    }

    // ---- expand_env ----

    fn env(name: &str) -> Option<String> {
        match name.to_ascii_uppercase().as_str() {
            "USERPROFILE" => Some(r"C:\Users\me".into()),
            "LOCALAPPDATA" => Some(r"C:\Users\me\AppData\Local".into()),
            _ => None,
        }
    }

    #[test]
    fn expand_env_replaces_known_names() {
        assert_eq!(
            expand_env(r"%LOCALAPPDATA%\Schemaic\current", env),
            VELOPACK_DIR
        );
    }

    #[test]
    fn expand_env_leaves_unknown_names_and_lone_percents() {
        assert_eq!(expand_env("%NOPE%;50%", env), "%NOPE%;50%");
        assert_eq!(expand_env("%", env), "%");
        assert_eq!(expand_env("a%%b", env), "a%%b");
    }

    #[test]
    fn expand_env_handles_several_and_adjacent() {
        assert_eq!(
            expand_env("%USERPROFILE%%NOPE%%USERPROFILE%", env),
            r"C:\Users\me%NOPE%C:\Users\me"
        );
    }

    #[test]
    fn expand_env_is_identity_without_percents() {
        assert_eq!(expand_env(r"C:\plain;D:\ü", env), r"C:\plain;D:\ü");
    }

    // ---- user_path_update ----

    #[test]
    fn user_path_update_appends_to_the_raw_value() {
        assert_eq!(
            user_path_update(r"%USERPROFILE%\bin", Path::new(VELOPACK_DIR), env),
            Some(format!(r"%USERPROFILE%\bin;{VELOPACK_DIR}"))
        );
    }

    #[test]
    fn user_path_update_on_an_empty_value_is_just_the_dir() {
        assert_eq!(
            user_path_update("", Path::new(VELOPACK_DIR), env),
            Some(VELOPACK_DIR.to_string())
        );
        assert_eq!(
            user_path_update("  ", Path::new(VELOPACK_DIR), env),
            Some(VELOPACK_DIR.to_string())
        );
    }

    #[test]
    fn user_path_update_does_not_double_a_trailing_semicolon() {
        assert_eq!(
            user_path_update(r"C:\a;", Path::new(VELOPACK_DIR), env),
            Some(format!(r"C:\a;{VELOPACK_DIR}"))
        );
    }

    #[test]
    fn user_path_update_sees_an_unexpanded_entry_as_present() {
        assert_eq!(
            user_path_update(
                r"C:\a;%LOCALAPPDATA%\Schemaic\current",
                Path::new(VELOPACK_DIR),
                env
            ),
            None
        );
    }

    // ---- link_step ----

    const LINK: &str = "/home/me/.local/bin/schemaic";
    const TARGET: &str = "/home/me/Apps/Schemaic.AppImage";

    fn step(existing: Existing) -> LinkStep {
        link_step(&existing, Path::new(LINK), Path::new(TARGET))
    }

    #[test]
    fn link_step_creates_where_nothing_is() {
        assert_eq!(step(Existing::Nothing), LinkStep::Create);
    }

    #[test]
    fn link_step_keeps_a_link_already_pointing_at_the_target() {
        let to = PathBuf::from(TARGET);
        assert_eq!(step(Existing::Link { to, live: true }), LinkStep::Keep);
    }

    #[test]
    fn link_step_resolves_a_relative_link_against_its_directory() {
        let to = PathBuf::from("../../Apps/Schemaic.AppImage");
        assert_eq!(step(Existing::Link { to, live: true }), LinkStep::Keep);
    }

    #[test]
    fn link_step_replaces_a_dangling_link() {
        let to = PathBuf::from("/home/me/Downloads/Schemaic.AppImage");
        assert_eq!(step(Existing::Link { to, live: false }), LinkStep::Replace);
    }

    #[test]
    fn link_step_refuses_a_live_link_elsewhere_and_names_it() {
        let to = PathBuf::from("/usr/local/bin/other");
        let LinkStep::Refuse(why) = step(Existing::Link { to, live: true }) else {
            panic!("expected a refusal");
        };
        assert!(
            why.contains(LINK) && why.contains("/usr/local/bin/other"),
            "{why}"
        );
    }

    #[test]
    fn link_step_refuses_a_regular_file() {
        let LinkStep::Refuse(why) = step(Existing::Other) else {
            panic!("expected a refusal");
        };
        assert!(why.contains(LINK), "{why}");
    }

    // ---- reports ----

    #[test]
    fn reports_name_the_path_they_resolved() {
        assert!(report_already(Path::new("/usr/bin")).contains("/usr/bin"));
        assert!(report_added(Path::new(VELOPACK_DIR)).contains(VELOPACK_DIR));
        let r = report_linked(Path::new(LINK), Path::new(TARGET), Os::Linux, "");
        assert!(r.contains(LINK) && r.contains(TARGET), "{r}");
    }

    #[test]
    fn report_added_says_a_new_terminal_is_needed() {
        assert!(report_added(Path::new(VELOPACK_DIR)).contains("new terminal"));
    }

    #[test]
    fn report_linked_hints_when_linux_path_lacks_the_directory() {
        let r = report_linked(Path::new(LINK), Path::new(TARGET), Os::Linux, "/usr/bin");
        assert!(r.contains("isn't on your PATH"), "{r}");
    }

    #[test]
    fn report_linked_is_quiet_when_linux_path_has_the_directory() {
        let path = "/home/me/.local/bin:/usr/bin";
        let r = report_linked(Path::new(LINK), Path::new(TARGET), Os::Linux, path);
        assert!(!r.contains("PATH"), "{r}");
    }

    #[test]
    fn report_linked_always_hints_on_macos() {
        // Finder hands the app launchd's PATH, so the process can't tell.
        let path = "/home/me/.local/bin:/usr/bin";
        let r = report_linked(Path::new(LINK), Path::new(TARGET), Os::MacOs, path);
        assert!(r.contains("PATH"), "{r}");
    }

    // ---- removal ----

    #[test]
    fn windows_removal_names_this_copys_folder() {
        assert_eq!(
            removal(&win(VELOPACK_EXE, "", true)),
            Ok(Removal::UserPath {
                dir: PathBuf::from(VELOPACK_DIR)
            })
        );
    }

    #[test]
    fn windows_removal_does_not_need_the_shim() {
        // The uninstall hook must clean up even if schemaic.com is already gone.
        assert_eq!(
            removal(&win(VELOPACK_EXE, "", false)),
            Ok(Removal::UserPath {
                dir: PathBuf::from(VELOPACK_DIR)
            })
        );
    }

    #[test]
    fn windows_removal_ignores_the_process_path() {
        // What this process inherited says nothing about the registry, which is
        // what Remove edits.
        let p = win(VELOPACK_EXE, VELOPACK_DIR, true);
        assert!(matches!(removal(&p), Ok(Removal::UserPath { .. })));
    }

    #[test]
    fn a_package_install_is_not_removed_by_the_app() {
        let p = unix(Os::Linux, "/usr/bin/schemaic", "/usr/bin:/bin");
        assert_eq!(
            removal(&p),
            Ok(Removal::Package {
                dir: PathBuf::from("/usr/bin")
            })
        );
    }

    #[test]
    fn unix_removal_targets_the_same_link_install_writes() {
        let mut p = unix(Os::Linux, "/tmp/.mount_X/usr/bin/schemaic", "/usr/bin");
        p.appimage = Some(PathBuf::from(TARGET));
        let Ok(Plan::Link { link, target }) = plan(&p) else {
            panic!("expected a link plan");
        };
        assert_eq!(removal(&p), Ok(Removal::Unlink { link, target }));
    }

    #[test]
    fn macos_removal_unlinks() {
        let exe = "/Applications/Schemaic.app/Contents/MacOS/schemaic";
        assert_eq!(
            removal(&unix(Os::MacOs, exe, "")),
            Ok(Removal::Unlink {
                link: PathBuf::from("/home/me/.local/bin/schemaic"),
                target: PathBuf::from(exe),
            })
        );
    }

    #[test]
    fn unix_removal_without_home_is_refused() {
        let mut p = unix(Os::MacOs, "/Applications/S.app/Contents/MacOS/schemaic", "");
        p.home = None;
        assert!(removal(&p).unwrap_err().contains("home"));
    }

    // ---- user_path_remove ----

    #[test]
    fn user_path_remove_takes_out_the_entry_and_keeps_the_rest_raw() {
        let raw = format!(r"%USERPROFILE%\bin;{VELOPACK_DIR};C:\z");
        assert_eq!(
            user_path_remove(&raw, Path::new(VELOPACK_DIR), env),
            Some(r"%USERPROFILE%\bin;C:\z".to_string())
        );
    }

    #[test]
    fn user_path_remove_undoes_user_path_update_exactly() {
        for raw in ["", r"C:\a", r"C:\a;", r"%USERPROFILE%\bin;C:\b"] {
            let dir = Path::new(VELOPACK_DIR);
            let added = user_path_update(raw, dir, env).unwrap();
            let back = user_path_remove(&added, dir, env).unwrap();
            // Every entry the user wrote comes back verbatim; only a trailing
            // `;` — the separator the entry was appended after — may not.
            assert_eq!(
                back.trim_end_matches(';'),
                raw.trim_end_matches(';'),
                "{raw:?} -> {added:?} -> {back:?}"
            );
        }
    }

    #[test]
    fn user_path_remove_matches_case_slashes_and_expansion() {
        let raw =
            r"C:\a;%LOCALAPPDATA%\schemaic\CURRENT\;c:/users/me/appdata/local/schemaic/current";
        assert_eq!(
            user_path_remove(raw, Path::new(VELOPACK_DIR), env),
            Some(r"C:\a".to_string())
        );
    }

    #[test]
    fn user_path_remove_keeps_empty_entries_and_a_trailing_separator() {
        let raw = format!(r"C:\a;;{VELOPACK_DIR};");
        assert_eq!(
            user_path_remove(&raw, Path::new(VELOPACK_DIR), env),
            Some(r"C:\a;;".to_string())
        );
    }

    #[test]
    fn user_path_remove_is_none_when_absent() {
        assert_eq!(user_path_remove("", Path::new(VELOPACK_DIR), env), None);
        let raw = r"C:\a;C:\Users\me\AppData\Local\Schemaic\current-old";
        assert_eq!(user_path_remove(raw, Path::new(VELOPACK_DIR), env), None);
    }

    // ---- unlink_step ----

    fn unstep(existing: Existing) -> UnlinkStep {
        unlink_step(&existing, Path::new(LINK), Path::new(TARGET))
    }

    #[test]
    fn unlink_step_removes_a_link_to_the_target() {
        let to = PathBuf::from(TARGET);
        assert_eq!(
            unstep(Existing::Link { to, live: true }),
            UnlinkStep::Remove
        );
        let to = PathBuf::from("../../Apps/Schemaic.AppImage");
        assert_eq!(
            unstep(Existing::Link { to, live: true }),
            UnlinkStep::Remove
        );
    }

    #[test]
    fn unlink_step_removes_a_dangling_link() {
        let to = PathBuf::from("/gone/Schemaic.AppImage");
        assert_eq!(
            unstep(Existing::Link { to, live: false }),
            UnlinkStep::Remove
        );
    }

    #[test]
    fn unlink_step_is_absent_where_nothing_is() {
        assert_eq!(unstep(Existing::Nothing), UnlinkStep::Absent);
    }

    #[test]
    fn unlink_step_refuses_a_live_link_elsewhere_and_names_it() {
        let to = PathBuf::from("/opt/other/schemaic");
        let UnlinkStep::Refuse(why) = unstep(Existing::Link { to, live: true }) else {
            panic!("expected a refusal");
        };
        assert!(
            why.contains(LINK) && why.contains("/opt/other/schemaic"),
            "{why}"
        );
    }

    #[test]
    fn unlink_step_refuses_a_regular_file() {
        let UnlinkStep::Refuse(why) = unstep(Existing::Other) else {
            panic!("expected a refusal");
        };
        assert!(why.contains(LINK), "{why}");
    }

    #[test]
    fn busy_covers_install_and_remove_and_nothing_else() {
        assert!(InstallState::Running.busy());
        assert!(InstallState::Removing.busy());
        assert!(!InstallState::Idle.busy());
        assert!(!InstallState::Done("x".into()).busy());
        assert!(!InstallState::Failed("x".into()).busy());
    }

    #[test]
    fn removal_reports_name_the_path() {
        assert!(report_removed(Path::new(VELOPACK_DIR)).contains(VELOPACK_DIR));
        assert!(report_path_absent(Path::new(VELOPACK_DIR)).contains(VELOPACK_DIR));
        assert!(report_unlinked(Path::new(LINK)).contains(LINK));
        assert!(report_link_absent(Path::new(LINK)).contains(LINK));
        assert!(report_package(Path::new("/usr/bin")).contains("/usr/bin"));
    }

    #[test]
    fn every_platform_has_a_hint() {
        for os in [Os::Windows, Os::MacOs, Os::Linux] {
            assert!(!hint(os).is_empty());
        }
        assert!(hint(Os::Windows).contains("PATH"));
        assert!(hint(Os::MacOs).contains(".local/bin"));
    }
}
