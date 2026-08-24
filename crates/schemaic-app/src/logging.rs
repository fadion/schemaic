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
//!
//! A panic went the same way for the same reason, and worse: the default hook
//! writes the payload to *stderr*, which on a GUI-subsystem release build is
//! nowhere at all, so the one class of failure that kills the process left no
//! trace of itself. [`install_panic_hook`] routes it through the writer above
//! instead — payload, thread, source location and a forced backtrace — and
//! still calls the hook it replaced, so a debug build keeps its console
//! message.

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

/// Route panics through the log file, then on to the hook we replaced.
///
/// Call once, after [`init`] — before it, the report would be formatted and then
/// dropped by a subscriber that does not exist yet.
///
/// Chained rather than replacing outright: the default hook is what prints to
/// stderr, which is still worth having in a debug build (and on the terminal
/// launch of a release build on Linux). This adds a destination, it does not
/// take one away.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // `force_capture`, not `capture`: the latter is governed by
        // `RUST_BACKTRACE`, which nobody sets on a machine that just crashed a
        // GUI app. The cost only lands on a process that is already dying.
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed").to_string();
        let location = info.location().map(|l| l.to_string());
        tracing::error!(
            "{}",
            panic_report(
                &name,
                &payload_text(info.payload()),
                location.as_deref(),
                &backtrace,
            )
        );
        previous(info);
    }));
}

/// The panic message, as a string, from the `Any` the hook is handed.
///
/// `panic!("…")` with no arguments boxes a `&'static str` and the formatting
/// form boxes a `String`; anything else came from `panic_any` and has no text
/// to show, so it is named rather than silently rendered as an empty message.
fn payload_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// Format one panic into the block that lands in the log.
///
/// Split out from the hook because the hook itself cannot be called under test
/// — installing it is process-global and a panic inside a test harness is not
/// the panic we want to observe — while the thing worth guarding is that the
/// report actually carries all four pieces. A report missing the location or
/// the backtrace is the same undiagnosable failure this module exists to end.
fn panic_report(thread: &str, payload: &str, location: Option<&str>, backtrace: &str) -> String {
    format!(
        "panic in thread '{thread}' at {}: {payload}\nbacktrace:\n{}",
        location.unwrap_or("<unknown location>"),
        backtrace.trim_end(),
    )
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

    /// All four pieces, or the report is the undiagnosable crash again: the
    /// payload says what, the location says where in the source, the thread
    /// says which of the app's many workers, the backtrace says how it got
    /// there.
    #[test]
    fn a_panic_report_carries_payload_location_thread_and_backtrace() {
        let report = panic_report(
            "grid-write",
            "index out of bounds: the len is 0 but the index is 3",
            Some("crates/schemaic-ui/src/grid.rs:412:9"),
            "   0: schemaic::foo\n   1: schemaic::bar\n",
        );
        assert!(report.contains("grid-write"), "{report}");
        assert!(report.contains("index out of bounds"), "{report}");
        assert!(report.contains("grid.rs:412:9"), "{report}");
        assert!(report.contains("schemaic::bar"), "{report}");
    }

    /// `PanicHookInfo::location` is an `Option`, and a report that renders
    /// `None` as nothing reads as though the panic had no source at all.
    #[test]
    fn a_panic_report_without_a_location_says_so() {
        let report = panic_report("main", "boom", None, "");
        assert!(report.contains("<unknown location>"), "{report}");
        assert!(report.contains("boom"), "{report}");
    }

    #[test]
    fn a_str_payload_is_read_as_its_message() {
        assert_eq!(payload_text(&"boom"), "boom");
    }

    /// `panic!("{x}")` boxes a `String`, not a `&str` — downcasting only to
    /// `&str` would lose every formatted panic, which is most of them.
    #[test]
    fn a_string_payload_is_read_as_its_message() {
        assert_eq!(payload_text(&String::from("boom 3")), "boom 3");
    }

    /// `panic_any(42)` has no message. Naming that beats an empty line that
    /// reads like a lost payload.
    #[test]
    fn a_non_string_payload_is_named_rather_than_blank() {
        let text = payload_text(&42_u32);
        assert!(!text.is_empty());
        assert!(text.contains("non-string"), "{text}");
    }

    /// Collects everything a subscriber writes, so a test can read it back.
    #[derive(Clone)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("capture lock").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Capture {
        type Writer = Capture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// The seam, not the pieces: `panic_report` being well-formed proves
    /// nothing if the hook never reaches a subscriber, and that composition —
    /// hook fires → `tracing::error!` → writer — is the whole feature. Panics
    /// go through a global hook and `catch_unwind`, so this drives the real
    /// path rather than calling the formatter directly.
    ///
    /// **The hook is process-global, and this test puts it back.** Tests in a
    /// crate share one process, so a replacement left standing is one every
    /// later panic goes through — including a genuine failure in another test,
    /// which would then print nothing to stderr and report as an
    /// undiagnosable blank. That is precisely the failure this module exists to
    /// end, so leaving it behind here would be the module's own bug in its own
    /// test.
    ///
    /// **Restored before the assertions, not in a `Drop` guard.**
    /// `std::panic::set_hook` panics if called from a panicking thread, so a
    /// guard restoring during an assertion's unwind would be a panic inside a
    /// panic and abort the whole run. Asserting after the restore gets the same
    /// guarantee with none of that: the only code between take and restore is
    /// the `catch_unwind` this test is about.
    /// **Both hook tests hold this while they run.** The panic hook is one
    /// process-global slot and each test swaps it out and back, so libtest
    /// running them on two threads interleaves two `take_hook`/`set_hook` pairs:
    /// the second `take` captures the first's replacement and its `set` restores
    /// that over the first's — which either fails the pin below or, worse, leaves
    /// the *noop* hook installed for the rest of the run, the undiagnosable blank
    /// this module exists to end.
    ///
    /// A plain `Mutex` and not a re-entrant one, which is why the body below is a
    /// helper: the pin calls it directly, already holding the lock.
    static HOOK_SLOT: Mutex<()> = Mutex::new(());

    /// Take the lock, tolerating a poisoning — a test that panicked while
    /// holding it has already restored the hook (that is what it asserts), and
    /// refusing to run afterwards would turn one failure into two.
    fn hook_slot() -> std::sync::MutexGuard<'static, ()> {
        HOOK_SLOT.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn an_installed_hook_writes_the_panic_through_tracing() {
        let _slot = hook_slot();
        hook_writes_the_panic_through_tracing();
    }

    /// The body, callable while the lock is already held.
    fn hook_writes_the_panic_through_tracing() {
        // The hook we chain onto is the default one, which prints to stderr and
        // would litter the test output with a crash that is on purpose.
        let real_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        install_panic_hook();

        let sink = Capture(Arc::new(Mutex::new(Vec::new())));
        // `set_default`, not `init`: thread-local, so a parallel test's logging
        // is untouched. The hook runs on the panicking thread — this one — so
        // the thread-local dispatcher is the one it finds.
        let subscriber = tracing_subscriber::fmt()
            .with_writer(sink.clone())
            .with_ansi(false)
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);

        let outcome = std::panic::catch_unwind(|| panic!("grid write exploded"));
        drop(guard);

        let text = String::from_utf8(sink.0.lock().expect("capture lock").clone())
            .expect("subscriber output is utf-8");

        // Everything the process shares is back the way it was found *before*
        // anything can fail.
        std::panic::set_hook(real_hook);

        assert!(outcome.is_err(), "the test's own panic should have unwound");
        assert!(text.contains("grid write exploded"), "{text}");
        assert!(text.contains("panic in thread"), "{text}");
        assert!(text.contains("logging.rs:"), "no source location: {text}");
    }

    /// **The pin on the paragraph above.** A hook left replaced is invisible
    /// until some *other* test fails, at which point it reports as a blank, so
    /// nothing about the failure points back here.
    ///
    /// It works by identity rather than by behaviour, which is the only thing
    /// `set_hook`/`take_hook` expose: install a hook this test can recognise,
    /// run the one above, and check the recognisable one is what the process
    /// still holds. Ordering is not left to libtest — the body above is called
    /// directly, in this thread, under the same [`HOOK_SLOT`] lock that keeps
    /// libtest's own copy of it from running beside us.
    #[test]
    fn the_hook_test_puts_the_process_back_as_it_found_it() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static MARKER_RAN: AtomicBool = AtomicBool::new(false);

        let _slot = hook_slot();
        let real_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {
            MARKER_RAN.store(true, Ordering::SeqCst);
        }));

        hook_writes_the_panic_through_tracing();

        // The marker has to be the hook the process still holds. Firing a panic
        // is the only way to ask, so fire one and swallow it.
        let _ = std::panic::catch_unwind(|| panic!("probe"));
        let restored = MARKER_RAN.load(Ordering::SeqCst);
        std::panic::set_hook(real_hook);

        assert!(
            restored,
            "the hook test replaced the process-global panic hook and left it replaced"
        );
    }
}
