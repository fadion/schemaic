//! Tracing setup, and the log file the shipped app writes to.
//!
//! **The installed app used to log nowhere at all**, and it cost a real
//! diagnosis. Release builds are GUI-subsystem on Windows
//! (`windows_subsystem = "windows"`, see `main.rs`), so the default stdout
//! writer hands every line to a console that does not exist. When the first
//! auto-update failure happened in the field — the chip flashing "Updating…"
//! and then vanishing, which is the deliberately-silent `UpdateState::Failed` —
//! the error string existed for a few microseconds and was then dropped on the
//! floor. Nothing on the machine recorded why, and the only reason the failure
//! was diagnosable at all is that Velopack keeps a log of its own for the
//! separate `Update.exe` process.
//!
//! So a file writer, always, in the config directory beside `tabs.json` and the
//! rest — one place to point someone at. Debug builds keep the console as well.
//!
//! The other half of the fix is the filter: Velopack's in-process half logs
//! through the `log` crate, which `tracing-subscriber` already bridges into
//! `tracing`, but a filter of `schemaic=info` discarded all of it because those
//! records carry a `velopack` target. [`DEFAULT_FILTER`] admits both.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::writer::MakeWriterExt;

/// Default targets when `RUST_LOG` says nothing.
///
/// `velopack` is here deliberately and is not optional: the updater's errors are
/// the ones that most need reading back, and they arrive on that target.
const DEFAULT_FILTER: &str = "schemaic=info,velopack=info";

/// Rotate once the log passes this. One generation is kept (`schemaic.log.1`),
/// so the worst case on disk is twice this.
const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;

/// Whether a log of `len` bytes should be rotated before being appended to.
///
/// Checked once per launch rather than per write: a session that writes 4 MB of
/// log is already pathological, and a size check on every line would put a
/// `stat` in the path of every trace call.
fn should_rotate(len: u64) -> bool {
    len >= MAX_LOG_BYTES
}

/// `%APPDATA%/schemaic/schemaic.log`, or the platform equivalent.
pub fn log_path() -> Option<PathBuf> {
    Some(schemaic_core::persist::config_dir()?.join("schemaic.log"))
}

/// Install the tracing subscriber: a log file always, plus stdout in debug.
///
/// Failing to open the log file is not fatal — it degrades to the old
/// stdout-only behaviour rather than refusing to start, since a read-only or
/// missing config directory is a reason to lose logs, not the app.
pub fn init() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));

    let Some(file) = open_log() else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
        tracing::warn!("no config directory — logging to stdout only");
        return;
    };

    let writer = FileWriter(Arc::new(Mutex::new(file)));
    // ANSI off: the escape codes are noise in a file, and this writer is the one
    // that always exists. Debug builds tee to stdout and lose colour with it,
    // which is a fair trade for the console and the file agreeing.
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(writer.and(std::io::stdout))
        .init();
}

/// Open the log for appending, rotating it first if it has grown past the cap.
fn open_log() -> Option<File> {
    let path = log_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    if std::fs::metadata(&path).is_ok_and(|m| should_rotate(m.len())) {
        // Replaces any previous generation. Best-effort: if the rename fails
        // (the file is held open by another instance, say) appending to the
        // oversized log beats not logging.
        let _ = std::fs::rename(&path, path.with_extension("log.1"));
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
}

/// A `MakeWriter` over one shared append handle.
///
/// Hand-rolled rather than pulling in `tracing-appender`: the whole requirement
/// is "append to one file", and the rotation this needs is a size check at
/// startup rather than the time-based scheme that crate exists for.
#[derive(Clone)]
struct FileWriter(Arc<Mutex<File>>);

/// One borrow of the shared handle, held for the length of a single event.
struct FileHandle(Arc<Mutex<File>>);

impl<'a> MakeWriter<'a> for FileWriter {
    type Writer = FileHandle;

    fn make_writer(&'a self) -> Self::Writer {
        FileHandle(self.0.clone())
    }
}

impl Write for FileHandle {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.0.lock() {
            Ok(mut f) => f.write(buf),
            // A poisoned lock means some other thread panicked mid-write. Losing
            // the line is better than panicking again from inside the logger.
            Err(_) => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.0.lock() {
            Ok(mut f) => f.flush(),
            Err(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_log_is_not_rotated() {
        assert!(!should_rotate(0));
        assert!(!should_rotate(1));
    }

    #[test]
    fn a_log_under_the_cap_is_not_rotated() {
        assert!(!should_rotate(MAX_LOG_BYTES - 1));
    }

    #[test]
    fn a_log_at_or_over_the_cap_is_rotated() {
        assert!(should_rotate(MAX_LOG_BYTES));
        assert!(should_rotate(MAX_LOG_BYTES * 3));
    }

    /// The updater's records arrive on a `velopack` target, and the filter that
    /// shipped before this module (`schemaic=info`) dropped every one of them —
    /// which is half of why the first field failure was undiagnosable.
    #[test]
    fn the_default_filter_admits_velopack() {
        assert!(DEFAULT_FILTER.contains("velopack"));
        assert!(DEFAULT_FILTER.contains("schemaic"));
    }
}
