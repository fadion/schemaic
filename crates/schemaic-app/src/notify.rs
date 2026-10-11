//! The desktop notification for a run that ended behind another window, when
//! the user has asked for one (Settings → *Notify when a run finishes in the
//! background*). What it says is `history::finished_note`; when it is shown is
//! `history::wants_attention`, the same question the taskbar flash asks.
//!
//! **Each platform needs the app's identity to put a name on it**, and each
//! takes it differently:
//! - Windows shows a toast only for an AppUserModelID it can resolve. An
//!   unpackaged app's is registered under
//!   `HKCU\Software\Classes\AppUserModelId\<id>` — a display name and an icon
//!   path — which is all a toast needs; without it the toast is dropped, or
//!   shown as PowerShell's. The id is `core::APP_ID`. The *process's* id is
//!   left alone: setting it would move the window to another taskbar group
//!   than the installed shortcut's.
//! - macOS attributes it to a bundle id, which is `APP_ID` too.
//! - Linux names the desktop entry, `APP_ID.desktop`.
//!
//! Shown on a thread of its own: on Linux it is a blocking D-Bus call, and a
//! notification daemon that never answers must not freeze the window.

use schemaic_core::APP_ID;

/// Show `title` / `body`, best effort: a failure is logged and nothing else —
/// the taskbar has been asked for the window's attention either way.
pub(crate) fn show(title: String, body: String) {
    std::thread::spawn(move || {
        #[cfg(windows)]
        if let Err(e) = win::register_app_id() {
            tracing::warn!("notification identity not registered: {e}");
        }
        #[cfg(target_os = "macos")]
        let _ = notify_rust::set_application(APP_ID);

        let mut n = notify_rust::Notification::new();
        n.summary(&title).body(&body).appname("Schemaic");
        #[cfg(windows)]
        n.app_id(APP_ID);
        #[cfg(all(unix, not(target_os = "macos")))]
        n.icon(APP_ID)
            .hint(notify_rust::Hint::DesktopEntry(APP_ID.to_string()));
        if let Err(e) = n.show() {
            tracing::warn!("desktop notification failed: {e}");
        }
    });
}

#[cfg(windows)]
mod win {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegSetValueExW,
    };

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// `HKCU\Software\Classes\AppUserModelId\<APP_ID>`: `DisplayName`, and
    /// `IconUri` naming the app's icon written beside its settings. Rewritten
    /// on every notification rather than once: two values, and a key someone
    /// deleted comes back without anyone having to notice it went.
    pub(super) fn register_app_id() -> Result<(), String> {
        let icon = icon_file();
        let subkey = wide(&format!(
            "Software\\Classes\\AppUserModelId\\{}",
            super::APP_ID
        ));
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: every pointer is to a live local; the class and security
        // attributes are documented as optional.
        let rc = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        if rc != 0 {
            return Err(std::io::Error::from_raw_os_error(rc as i32).to_string());
        }
        let set = |name: &str, value: &str| {
            let (name, data) = (wide(name), wide(value));
            // SAFETY: `key` is open; `data` is a NUL-terminated UTF-16 buffer
            // of the stated byte length.
            let rc = unsafe {
                RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            };
            if rc == 0 {
                Ok(())
            } else {
                Err(std::io::Error::from_raw_os_error(rc as i32).to_string())
            }
        };
        let mut result = set("DisplayName", "Schemaic");
        if let (Ok(()), Some(icon)) = (&result, icon) {
            result = set("IconUri", &icon.to_string_lossy());
        }
        // SAFETY: opened above, closed once.
        unsafe { RegCloseKey(key) };
        result
    }

    /// The bundled icon as a file a toast can name, written into the config
    /// directory when it is missing or differs from the one this build
    /// carries — so a new icon reaches the toast with the release that ships
    /// it. `None` leaves the toast with Windows' generic icon, which is all a
    /// failed write costs.
    fn icon_file() -> Option<std::path::PathBuf> {
        const ICON: &[u8] = include_bytes!("../../../assets/icon.png");
        let path = schemaic_core::persist::config_dir()?.join("notification-icon.png");
        if std::fs::read(&path).ok().as_deref() != Some(ICON) {
            std::fs::write(&path, ICON).ok()?;
        }
        Some(path)
    }
}
