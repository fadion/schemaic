//! Settings → General → Command line → Install: the side-effect half of
//! [`schemaic_core::cli_install`], which owns every decision and its tests.
//!
//! This module gathers the [`Probe`] and performs the [`Plan`] — a registry
//! write plus a `WM_SETTINGCHANGE` broadcast on Windows, a symlink on macOS and
//! Linux. It runs on a worker thread: the broadcast waits on every top-level
//! window, and a slow one should stall a thread nobody is looking at rather than
//! the UI.

use schemaic_core::cli_install::{self, Os, Plan, Probe};
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
        console_shim,
        exe,
    })
}

#[cfg(not(windows))]
fn add_user_path(_dir: &Path) -> Result<String, String> {
    Err("A user PATH entry is a Windows install; this platform links instead.".to_string())
}

/// Append `dir` to `HKCU\Environment\Path`, then tell running programs.
///
/// Read-modify-write on the **raw** value, in its own registry type: an entry
/// written as `%USERPROFILE%\bin` must come back unexpanded, and a
/// `REG_EXPAND_SZ` must stay one or every such entry stops resolving. A value
/// that isn't valid UTF-16 is refused rather than written back lossily.
#[cfg(windows)]
fn add_user_path(dir: &Path) -> Result<String, String> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ,
        REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegQueryValueExW,
        RegSetValueExW,
    };

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Closes the key on every path out, including the early returns.
    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: `self.0` is a key `RegCreateKeyExW` opened and nothing
            // else closes it.
            unsafe { RegCloseKey(self.0) };
        }
    }

    let subkey = wide("Environment");
    let name = wide("Path");
    let mut raw_key: HKEY = std::ptr::null_mut();
    // SAFETY: every pointer is to a live local; the class and security
    // attributes are documented as optional.
    let rc = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            std::ptr::null(),
            &mut raw_key,
            std::ptr::null_mut(),
        )
    };
    if rc != 0 {
        return Err(format!(
            "Couldn't open your user environment in the registry: {}",
            std::io::Error::from_raw_os_error(rc as i32)
        ));
    }
    let key = Key(raw_key);

    let mut ty = 0u32;
    let mut bytes = 0u32;
    // SAFETY: a null data pointer asks only for the size and type.
    let rc = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            std::ptr::null(),
            &mut ty,
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    let (raw, ty) = if rc == ERROR_FILE_NOT_FOUND {
        (String::new(), REG_EXPAND_SZ)
    } else if rc != 0 {
        return Err(format!(
            "Couldn't read your user PATH: {}",
            std::io::Error::from_raw_os_error(rc as i32)
        ));
    } else {
        if ty != REG_SZ && ty != REG_EXPAND_SZ {
            return Err(
                "Your user PATH isn't stored as text in the registry, so Install left it alone."
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
                key.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut ty,
                buf.as_mut_ptr().cast(),
                &mut len,
            )
        };
        if rc != 0 {
            return Err(format!(
                "Couldn't read your user PATH: {}",
                std::io::Error::from_raw_os_error(rc as i32)
            ));
        }
        buf.truncate(len as usize / 2);
        while buf.last() == Some(&0) {
            buf.pop();
        }
        let raw = std::ffi::OsString::from_wide(&buf)
            .into_string()
            .map_err(|_| {
                "Your user PATH contains characters that can't be read back safely, so \
                 Install left it alone."
                    .to_string()
            })?;
        (raw, ty)
    };

    let lookup = |name: &str| std::env::var(name).ok();
    let Some(updated) = cli_install::user_path_update(&raw, dir, lookup) else {
        // Already in the registry — written after this process started, which
        // is why the planner's look at our own PATH missed it.
        return Ok(cli_install::report_added(dir));
    };
    let data = wide(&updated);
    // SAFETY: `data` is a NUL-terminated UTF-16 buffer of the stated byte length.
    let rc = unsafe {
        RegSetValueExW(
            key.0,
            name.as_ptr(),
            0,
            ty,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
    if rc != 0 {
        return Err(format!(
            "Couldn't write your user PATH: {}",
            std::io::Error::from_raw_os_error(rc as i32)
        ));
    }
    drop(key);
    broadcast_environment_change();
    tracing::info!(dir = %dir.display(), "added the CLI's folder to the user PATH");
    Ok(cli_install::report_added(dir))
}

/// Tell Explorer (and anything else listening) that the environment changed, so
/// a terminal started from it next gets the new `PATH`. Best-effort: the value is
/// written either way, and a window that never answers only means that program
/// needs a restart.
#[cfg(windows)]
fn broadcast_environment_change() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    let area: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut result = 0usize;
    // SAFETY: `area` is a NUL-terminated UTF-16 string that outlives the call,
    // which is synchronous.
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

#[cfg(not(unix))]
fn link_command(_link: &Path, _target: &Path, _probe: &Probe) -> Result<String, String> {
    Err("Linking the command is a macOS and Linux install.".to_string())
}

/// Make `link` a symlink to `target`, creating `~/.local/bin` if needed.
#[cfg(unix)]
fn link_command(link: &Path, target: &Path, probe: &Probe) -> Result<String, String> {
    use cli_install::{Existing, LinkStep};

    let existing = match std::fs::symlink_metadata(link) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Existing::Nothing,
        Err(e) => return Err(format!("Couldn't look at {}: {e}", link.display())),
        Ok(meta) if meta.file_type().is_symlink() => Existing::Link {
            to: std::fs::read_link(link)
                .map_err(|e| format!("Couldn't read the link at {}: {e}", link.display()))?,
            // `exists` follows the link.
            live: link.exists(),
        },
        Ok(_) => Existing::Other,
    };
    match cli_install::link_step(&existing, link, target) {
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
