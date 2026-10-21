# `app/logging.rs`

`logging.rs` — the tracing subscriber, and the log file the shipped app writes to. **The
installed app used to log nowhere at all.** Release builds are GUI-subsystem on Windows
(`windows_subsystem = "windows"`, in `main.rs`), so `tracing_subscriber`'s default stdout writer
hands every line to a console that does not exist; when the first auto-update failure happened
in the field — the header chip flashing "Updating…" and then vanishing, which is the
deliberately-silent `UpdateState::Failed` — the error string was dropped on the floor and
nothing on the machine recorded why. It was diagnosable at all only because Velopack keeps a
separate log for its out-of-process `Update.exe`. So `init()` installs a **file** writer always,
at `log_path()` — `%APPDATA%/schemaic/schemaic.log`, or the platform equivalent — in the config
directory beside `tabs.json` and the rest of the state, so there is one directory to point a
user at; that is what `core::persist::config_dir` is `pub` for. Debug builds tee to stdout on
top of it (`MakeWriterExt::and`). ANSI is off, because escape codes are noise in a file and the
file is the writer that always exists — a debug console losing its colour is the price of the
two agreeing. **`DEFAULT_FILTER` is `"schemaic=info,velopack=info"`, and the `velopack` half is
load-bearing rather than decoration.** Velopack's *in-process* half (`UpdateManager`) logs
through the `log` crate, which `tracing-subscriber` already bridges into `tracing`, but the old
`schemaic=info` filter discarded every one of those records because they carry a `velopack`
target — that was the other half of why the field failure was undiagnosable
(`the_default_filter_admits_velopack` pins it); `RUST_LOG` still overrides the pair where it is
set. **But `RUST_LOG` cannot ask for a credential.** The filter is built by
`log_directives`/`filter_for`, which take what the environment asked for (or `DEFAULT_FILTER`)
and then append `russh=warn`, `russh_cryptovec=warn` and `russh_util=warn` — `CREDENTIAL_TARGETS`,
a floor the environment cannot lift. Three facts composed into the bug it closes: `RUST_LOG`
*replaces* the default filter rather than adding to it, so a bare `RUST_LOG=trace` takes the
target allowlist away with it; `tracing-subscriber` bridges every dependency's `log` records into
`tracing`; and `russh`'s `session_write_encrypted` traces the packet body **before** the writer
encrypts it. So `RUST_LOG=trace` put the pre-encryption `SSH_MSG_USERAUTH_REQUEST` — the tunnel
account's password, as a decimal byte array — into `schemaic.log`, the file Settings → General
offers an **Open folder** button for, which is to say into the folder whose own doc calls it
"everything anyone would be asked for". **The order is what makes it a floor**: a bare `trace` is
a global directive and a target directive is more specific (`EnvFilter` matches
most-specific-first), while an explicit `RUST_LOG=russh=trace` is an *equal* key that
`DirectiveSet::add` replaces with the later one — this one. Underscores, not hyphens, since a
target is a Rust module path (`russh-cryptovec` reports as `russh_cryptovec`), and `russh` covers
its submodules by prefix. It is russh's surface alone on purpose: `mysql_async`,
`tokio-postgres` and `rusqlite` depend on neither `log` nor `tracing`, so there is nothing of
theirs to cap. A malformed `RUST_LOG` falls back to the whole default filter rather than being
applied directive-by-directive with the bad ones dropped — a filter half the user asked for is
harder to notice than none of it. Rotation is a size check **once per launch**, not per write: `MAX_LOG_BYTES` is 4 MB and
one generation is kept as `schemaic.log.1`, so the worst case on disk is twice that, and a
`stat` stays out of the path of every trace call. Failing to open the log is not fatal — it
degrades to the old stdout-only behaviour rather than refusing to start, since a read-only or
missing config directory is a reason to lose logs, not the app. `FileWriter`/`FileHandle` are a
hand-rolled `MakeWriter` over one `Arc<Mutex<File>>` rather than a dependency on
`tracing-appender`: the whole requirement is "append to one file", and the rotation it needs is
a startup size check rather than the time-based scheme that crate exists for. A poisoned lock
drops the line instead of panicking a second time from inside the logger. **A panic went the
same way, for the same reason, and worse:** Rust's default hook writes the payload to *stderr*,
which on a GUI-subsystem release build is the same console that does not exist — so the one
failure class that kills the process left no trace of itself at all. `install_panic_hook()`,
called from `main` immediately after `init()` (before it, the report would be formatted and
handed to a subscriber that does not exist yet), routes the payload, thread name, source
location and a **forced** backtrace through `tracing::error!` and so into the same file.
`Backtrace::force_capture`, not `capture`: the latter is governed by `RUST_BACKTRACE`, which
nobody has set on the machine that just crashed a GUI app, and the cost only lands on a process
that is already dying. The hook **chains** the one it replaced rather than replacing it
outright, so a debug build (and a terminal launch on Linux) keeps its stderr message — this
adds a destination, it does not take one away. `panic_report` and `payload_text` are split out
as pure functions because the hook itself is process-global and cannot be driven from a test
that is not itself panicking; `payload_text` downcasts to both `&str` and `String` because
`panic!("{x}")` boxes the latter and a `&str`-only downcast would lose most panics.
`an_installed_hook_writes_the_panic_through_tracing` guards the *seam* rather than the pieces —
a thread-local subscriber over a capture buffer, a real `catch_unwind` panic — since a
well-formed `panic_report` proves nothing if the hook never reaches a subscriber.
**That test puts the hook back**, and the reason is this module's own subject: tests share one
process, so a replacement left standing is one every later panic goes through — including a
genuine failure in another test, which then prints nothing to stderr and reports as an
undiagnosable blank. It restores **before its assertions, not in a `Drop` guard**, because
`std::panic::set_hook` panics if called from a panicking thread and a guard restoring during an
assertion's unwind would abort the run.
`the_hook_test_puts_the_process_back_as_it_found_it` is the pin: it installs a hook it can
recognise, calls the test above directly (ordering is not left to libtest), fires a swallowed
panic and checks the recognisable one is what the process still holds.
