//! Settings → General → Command line → Install / Remove, and the Windows
//! uninstall hook: the side-effect half of [`schemaic_core::cli_install`],
//! which owns every decision and its tests.
//!
//! This module gathers the [`Probe`] and performs the [`Plan`] or the
//! [`Removal`] — a registry edit plus a `WM_SETTINGCHANGE` broadcast on
//! Windows, a symlink on macOS and Linux. The Settings row runs it on a worker
//! thread: the broadcast waits on every top-level window, and a slow one should
//! stall a thread nobody is looking at rather than the UI.

use schemaic_core::cli_install::{self, Os, Plan, Probe, Removal};
use std::path::Path;

/// Install the `schemaic` command, or say why not. The `Ok` text names the path
/// that was resolved, for the Settings row to show.
pub(crate) fn install() -> Result<String, String> {
    let probe = probe()?;
    match cli_install::plan(&probe)? {
        Plan::Already { dir } => Ok(cli_install::report_already(&dir)),
        Plan::AddUserPath { dir } => add_user_path(&dir),
        Plan::Link { link, target } => link_command(&link, &target, &probe),
    }
}

/// Undo [`install`] — only ever what Install could have written.
pub(crate) fn remove() -> Result<String, String> {
    let probe = probe()?;
    match cli_install::removal(&probe)? {
        Removal::Package { dir } => Ok(cli_install::report_package(&dir)),
        Removal::UserPath { dir } => remove_user_path(&dir),
        Removal::Unlink { link, target } => unlink_command(&link, &target),
    }
}

/// Would Remove do anything — is the command ours to take away? The Settings
/// row shows Remove only when it is. A read that fails answers no: the worst
/// case is a missing button, never a Remove that reports it found nothing.
pub(crate) fn installed() -> bool {
    use cli_install::Found;
    let Ok(probe) = probe() else {
        return false;
    };
    let Ok(removal) = cli_install::removal(&probe) else {
        return false;
    };
    let found = match &removal {
        Removal::Package { .. } => Found::Nothing,
        Removal::UserPath { .. } => read_user_path().map_or(Found::Nothing, Found::UserPath),
        Removal::Unlink { link, .. } => read_link_state(link).map_or(Found::Nothing, Found::Link),
    };
    #[cfg(windows)]
    let lookup = env_lookup;
    #[cfg(not(windows))]
    let lookup = |_: &str| None;
    cli_install::removable(&removal, &found, lookup)
}

#[cfg(windows)]
fn read_user_path() -> Option<String> {
    win::UserPath::open(win::Access::Read)
        .ok()
        .map(|p| p.raw.clone())
}

#[cfg(not(windows))]
fn read_user_path() -> Option<String> {
    None
}

#[cfg(unix)]
fn read_link_state(link: &Path) -> Option<cli_install::Existing> {
    existing_at(link).ok()
}

#[cfg(not(unix))]
fn read_link_state(_link: &Path) -> Option<cli_install::Existing> {
    None
}

/// Velopack's `--veloapp-uninstall` hook: take this copy's folder back off the
/// user `PATH` before the uninstaller deletes it. Windows only — Velopack has no
/// uninstaller anywhere else, so nothing of ours runs when a macOS app or an
/// AppImage is thrown away. No UI and a 30-second budget, so the outcome is
/// only logged; a copy that never ran Install finds nothing and changes
/// nothing.
#[cfg(windows)]
pub(crate) fn on_uninstall() {
    match remove() {
        Ok(msg) => tracing::info!("uninstall: {msg}"),
        Err(why) => tracing::warn!("uninstall: could not take the CLI off PATH: {why}"),
    }
}

fn probe() -> Result<Probe, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("Can't tell where Schemaic is running from: {e}"))?;
    let console_shim = exe
        .parent()
        .is_some_and(|dir| dir.join(cli_install::WINDOWS_SHIM).is_file());
    Ok(Probe {
        os: Os::current(),
        // An empty `APPIMAGE` is no image at all.
        appimage: std::env::var_os("APPIMAGE")
            .filter(|v| !v.is_empty())
            .map(Into::into),
        home: std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(Into::into),
        path_var: std::env::var_os("PATH")
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_default(),
        user_path: user_path_now(),
        console_shim,
        exe,
    })
}

/// The registry's user `PATH` as it stands now, expanded — what the planner
/// prefers over this process's own `PATH`, which was fixed at launch.
#[cfg(windows)]
fn user_path_now() -> Option<String> {
    read_user_path().map(|raw| cli_install::expand_env(&raw, env_lookup))
}

#[cfg(not(windows))]
fn user_path_now() -> Option<String> {
    None
}

#[cfg(windows)]
fn env_lookup(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

#[cfg(not(windows))]
fn add_user_path(_dir: &Path) -> Result<String, String> {
    Err("A user PATH entry is a Windows install; this platform links instead.".to_string())
}

#[cfg(not(windows))]
fn remove_user_path(_dir: &Path) -> Result<String, String> {
    Err("A user PATH entry is a Windows install; this platform links instead.".to_string())
}

/// Append `dir` to `HKCU\Environment\Path`, then tell running programs.
#[cfg(windows)]
fn add_user_path(dir: &Path) -> Result<String, String> {
    let path = win::UserPath::open(win::Access::Write)?;
    let Some(updated) = cli_install::user_path_update(&path.raw, dir, env_lookup) else {
        // Already in the registry — written between the planner's read and
        // this one, or the planner could not read it and fell back to our own
        // PATH.
        return Ok(cli_install::report_added(dir));
    };
    path.write(&updated)?;
    win::broadcast_environment_change();
    tracing::info!(dir = %dir.display(), "added the CLI's folder to the user PATH");
    Ok(cli_install::report_added(dir))
}

/// Take every entry naming `dir` out of `HKCU\Environment\Path`, then tell
/// running programs.
#[cfg(windows)]
fn remove_user_path(dir: &Path) -> Result<String, String> {
    let path = win::UserPath::open(win::Access::Write)?;
    let Some(updated) = cli_install::user_path_remove(&path.raw, dir, env_lookup) else {
        return Ok(cli_install::report_path_absent(dir));
    };
    path.write(&updated)?;
    win::broadcast_environment_change();
    tracing::info!(dir = %dir.display(), "removed the CLI's folder from the user PATH");
    Ok(cli_install::report_removed(dir))
}

#[cfg(windows)]
mod win {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ,
        REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW,
        RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn os_error(rc: u32) -> std::io::Error {
        std::io::Error::from_raw_os_error(rc as i32)
    }

    /// `HKCU\Environment\Path`, read as written: the **raw** value in its own
    /// registry type. A `REG_EXPAND_SZ` must stay one or every
    /// `%USERPROFILE%\…` entry in it stops resolving, and an unexpanded entry
    /// must be written back unexpanded — which is why this never goes near
    /// `ExpandEnvironmentStringsW` or `setx`. A value that isn't text, or isn't
    /// valid UTF-16, is refused rather than written back lossily.
    pub(super) struct UserPath {
        key: HKEY,
        ty: REG_VALUE_TYPE,
        pub(super) raw: String,
    }

    impl Drop for UserPath {
        fn drop(&mut self) {
            // SAFETY: `self.key` is a key `open` opened, and only this drop
            // closes it.
            unsafe { RegCloseKey(self.key) };
        }
    }

    /// How [`UserPath::open`] opens the key. Only Install and Remove write; the
    /// Settings row's "is it installed?" read runs at every launch, and a read
    /// has no business asking for write access or creating the key.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(super) enum Access {
        Read,
        Write,
    }

    impl UserPath {
        pub(super) fn open(access: Access) -> Result<UserPath, String> {
            let subkey = wide("Environment");
            let mut key: HKEY = std::ptr::null_mut();
            // SAFETY: every pointer is to a live local; the class and security
            // attributes are documented as optional.
            let rc = unsafe {
                match access {
                    Access::Write => RegCreateKeyExW(
                        HKEY_CURRENT_USER,
                        subkey.as_ptr(),
                        0,
                        std::ptr::null(),
                        REG_OPTION_NON_VOLATILE,
                        KEY_QUERY_VALUE | KEY_SET_VALUE,
                        std::ptr::null(),
                        &mut key,
                        std::ptr::null_mut(),
                    ),
                    Access::Read => RegOpenKeyExW(
                        HKEY_CURRENT_USER,
                        subkey.as_ptr(),
                        0,
                        KEY_QUERY_VALUE,
                        &mut key,
                    ),
                }
            };
            if rc != 0 {
                return Err(format!(
                    "Couldn't open your user environment in the registry: {}",
                    os_error(rc)
                ));
            }
            // Owned from here, so every early return below closes it.
            let mut path = UserPath {
                key,
                ty: REG_EXPAND_SZ,
                raw: String::new(),
            };

            let name = wide("Path");
            let mut ty = 0u32;
            let mut bytes = 0u32;
            // SAFETY: a null data pointer asks only for the size and type.
            let rc = unsafe {
                RegQueryValueExW(
                    path.key,
                    name.as_ptr(),
                    std::ptr::null(),
                    &mut ty,
                    std::ptr::null_mut(),
                    &mut bytes,
                )
            };
            if rc == ERROR_FILE_NOT_FOUND {
                // No user PATH yet: created as REG_EXPAND_SZ, the type Windows
                // itself gives it.
                return Ok(path);
            }
            if rc != 0 {
                return Err(format!("Couldn't read your user PATH: {}", os_error(rc)));
            }
            if ty != REG_SZ && ty != REG_EXPAND_SZ {
                return Err(
                    "Your user PATH isn't stored as text in the registry, so it was left alone."
                        .to_string(),
                );
            }
            // Rounded up to whole u16s, plus room for a terminator the stored
            // value may lack.
            let mut buf = vec![0u16; (bytes as usize).div_ceil(2) + 1];
            let mut len = (buf.len() * 2) as u32;
            // SAFETY: `buf` holds `len` bytes.
            let rc = unsafe {
                RegQueryValueExW(
                    path.key,
                    name.as_ptr(),
                    std::ptr::null(),
                    &mut ty,
                    buf.as_mut_ptr().cast(),
                    &mut len,
                )
            };
            if rc != 0 {
                return Err(format!("Couldn't read your user PATH: {}", os_error(rc)));
            }
            buf.truncate(len as usize / 2);
            while buf.last() == Some(&0) {
                buf.pop();
            }
            path.raw = std::ffi::OsString::from_wide(&buf)
                .into_string()
                .map_err(|_| {
                    "Your user PATH contains characters that can't be read back safely, so it \
                     was left alone."
                        .to_string()
                })?;
            path.ty = ty;
            Ok(path)
        }

        /// Write `value` back in the type it was read in.
        pub(super) fn write(&self, value: &str) -> Result<(), String> {
            let name = wide("Path");
            let data = wide(value);
            // SAFETY: `data` is a NUL-terminated UTF-16 buffer of the stated
            // byte length.
            let rc = unsafe {
                RegSetValueExW(
                    self.key,
                    name.as_ptr(),
                    0,
                    self.ty,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            };
            if rc != 0 {
                return Err(format!("Couldn't write your user PATH: {}", os_error(rc)));
            }
            Ok(())
        }
    }

    /// Tell Explorer (and anything else listening) that the environment changed,
    /// so a terminal started from it next gets the new `PATH`. Best-effort: the
    /// value is written either way, and a window that never answers only means
    /// that program needs a restart.
    pub(super) fn broadcast_environment_change() {
        let area = wide("Environment");
        let mut result = 0usize;
        // SAFETY: `area` is a NUL-terminated UTF-16 string that outlives the
        // call, which is synchronous.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                area.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                5000,
                &mut result,
            );
        }
    }
}

#[cfg(not(unix))]
fn link_command(_link: &Path, _target: &Path, _probe: &Probe) -> Result<String, String> {
    Err("Linking the command is a macOS and Linux install.".to_string())
}

#[cfg(not(unix))]
fn unlink_command(_link: &Path, _target: &Path) -> Result<String, String> {
    Err("Linking the command is a macOS and Linux install.".to_string())
}

/// What is at `link` right now, in the shape `cli_install` decides on.
#[cfg(unix)]
fn existing_at(link: &Path) -> Result<cli_install::Existing, String> {
    use cli_install::Existing;
    match std::fs::symlink_metadata(link) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Existing::Nothing),
        Err(e) => Err(format!("Couldn't look at {}: {e}", link.display())),
        Ok(meta) if meta.file_type().is_symlink() => Ok(Existing::Link {
            to: std::fs::read_link(link)
                .map_err(|e| format!("Couldn't read the link at {}: {e}", link.display()))?,
            // `exists` follows the link.
            live: link.exists(),
        }),
        Ok(_) => Ok(Existing::Other),
    }
}

/// Make `link` a symlink to `target`, creating `~/.local/bin` if needed.
#[cfg(unix)]
fn link_command(link: &Path, target: &Path, probe: &Probe) -> Result<String, String> {
    use cli_install::LinkStep;
    match cli_install::link_step(&existing_at(link)?, link, target) {
        LinkStep::Keep => {}
        LinkStep::Refuse(why) => return Err(why),
        step => {
            if let Some(dir) = link.parent() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
            }
            if step == LinkStep::Replace {
                std::fs::remove_file(link)
                    .map_err(|e| format!("Couldn't remove the old {}: {e}", link.display()))?;
            }
            std::os::unix::fs::symlink(target, link)
                .map_err(|e| format!("Couldn't create {}: {e}", link.display()))?;
            tracing::info!(link = %link.display(), target = %target.display(), "linked the CLI");
        }
    }
    Ok(cli_install::report_linked(
        link,
        target,
        probe.os,
        &probe.path_var,
    ))
}

/// Remove `link` if it is ours. `remove_file` on a symlink removes the link,
/// never what it points at.
#[cfg(unix)]
fn unlink_command(link: &Path, target: &Path) -> Result<String, String> {
    use cli_install::UnlinkStep;
    match cli_install::unlink_step(&existing_at(link)?, link, target) {
        UnlinkStep::Absent => Ok(cli_install::report_link_absent(link)),
        UnlinkStep::Refuse(why) => Err(why),
        UnlinkStep::Remove => {
            std::fs::remove_file(link)
                .map_err(|e| format!("Couldn't remove {}: {e}", link.display()))?;
            tracing::info!(link = %link.display(), "unlinked the CLI");
            Ok(cli_install::report_unlinked(link))
        }
    }
}
