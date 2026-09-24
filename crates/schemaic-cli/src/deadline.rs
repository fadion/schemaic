//! One deadline, for every database read the headless front ends make.
//!
//! **Why it is a function rather than two `tokio::select!` blocks.** It was two
//! — one in [`crate::query`], one in [`crate::exec`] — and the second carried a
//! comment saying it made "the same trade for the same reason" as the first,
//! which is a duplicated decision admitting to being one. The trade is subtle
//! enough that it should exist once: the future is **awaited past the cancel**,
//! because the token goes down into the driver and its cancel branch is what
//! issues the server-side `KILL`. Dropping the future instead returns promptly
//! and leaves the statement running on the server with nobody left to stop it.
//!
//! `app/mcp.rs` has its own `with_deadline` over the same property, kept there
//! because it wraps reads (`fetch_schema`, `fetch_databases`) this crate never
//! makes, and because one of them genuinely *must* abandon its future.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

/// How long a cancelled future is waited for before it is abandoned: long
/// enough for the driver's own `KILL`, which [`schemaic_db::CANCEL_TIMEOUT`]
/// bounds, and then a moment more.
pub const UNWIND_GRACE: Duration =
    schemaic_db::CANCEL_TIMEOUT.saturating_add(Duration::from_secs(1));

/// Await `fut` for at most `timeout`; on expiry cancel `token` and wait for the
/// future to finish unwinding — for at most [`UNWIND_GRACE`].
///
/// `None` means the deadline passed. The token must be the one handed to the
/// future, or the cancel reaches nothing.
///
/// **The wait after the cancel is bounded, because not every future listens.**
/// A driver still *connecting* has not reached its `select!` on the token, so a
/// host that drops packets held a `--timeout 2` command for the OS connect
/// timeout — about 21 s in all, measured on both engines. Before the statement has
/// started there is nothing server-side to `KILL`, so abandoning it then is
/// safe; once it has, the driver's cancel branch finishes well inside the grace.
pub async fn with_deadline<F>(
    fut: F,
    token: CancellationToken,
    timeout: Duration,
) -> Option<F::Output>
where
    F: std::future::Future,
{
    tokio::pin!(fut);
    tokio::select! {
        r = &mut fut => Some(r),
        _ = tokio::time::sleep(timeout) => {
            token.cancel();
            let _ = tokio::time::timeout(UNWIND_GRACE, fut).await;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A read that finishes in time comes back whole, and the clock is never
    /// consulted for it.
    #[tokio::test(start_paused = true)]
    async fn a_prompt_future_is_not_deadlined() {
        let token = CancellationToken::new();
        let got = with_deadline(async { 7 }, token.clone(), Duration::from_secs(30)).await;
        assert_eq!(got, Some(7));
        assert!(!token.is_cancelled(), "nothing to cancel");
    }

    /// **The cancel fires, and the future is still awaited afterwards.** If it
    /// were dropped at the deadline instead, the driver's KILL branch would
    /// never run and the statement would outlive the command.
    #[tokio::test(start_paused = true)]
    async fn an_overrunning_future_is_cancelled_and_then_awaited() {
        let token = CancellationToken::new();
        let observer = token.clone();
        let fut = async move {
            // Stands in for the driver: it only finishes once cancelled, which
            // is exactly what makes this assert the *await past the cancel*.
            observer.cancelled().await;
            99
        };
        let got = with_deadline(fut, token.clone(), Duration::from_secs(30)).await;
        assert_eq!(got, None, "the deadline is what the caller hears about");
        assert!(token.is_cancelled(), "the driver must have been told");
    }

    /// **A future that never hears the cancel is abandoned, not awaited
    /// forever.** It stands in for a driver stuck connecting to a host that
    /// drops packets, before it has any token to watch.
    #[tokio::test(start_paused = true)]
    async fn a_future_that_ignores_the_cancel_is_abandoned_after_the_grace() {
        let token = CancellationToken::new();
        let start = tokio::time::Instant::now();
        let got = with_deadline(
            std::future::pending::<()>(),
            token.clone(),
            Duration::from_secs(2),
        )
        .await;
        assert_eq!(got, None);
        assert!(
            start.elapsed() <= Duration::from_secs(2) + UNWIND_GRACE,
            "held for {:?}",
            start.elapsed()
        );
    }

    /// **Every database read in this crate is inside a deadline.**
    ///
    /// `app/mcp.rs` carries this gate over its own source and counts the reads
    /// it expects, so that a needle which stops matching cannot read as a clean
    /// file. Folding `mcp::run_query` onto `query::read_only_query` moved one
    /// read *out* of that file — its count went from seven to six — and the
    /// read landed here, in a crate no gate was watching. This is that gate,
    /// following the read.
    ///
    /// Lexical, like its counterpart: it asserts the call is an argument to
    /// `with_deadline`, with no statement boundary in between.
    #[test]
    fn no_database_read_in_this_crate_is_awaited_without_a_deadline() {
        let sources = [
            ("query.rs", include_str!("query.rs")),
            ("exec.rs", include_str!("exec.rs")),
            ("run.rs", include_str!("run.rs")),
        ];
        let mut checked = 0usize;
        let mut offenders = Vec::new();
        // **Every awaited `db.` call, not one family of them.** The needle was
        // `db.fetch_`, which a switch to `db.run_batch` or a new `db.ping()`
        // would have walked straight past. `engine()` is the one accessor that
        // does no I/O.
        let needle = format!("{}.", "db");
        for (name, body) in sources {
            let mut from = 0usize;
            while let Some(rel) = body[from..].find(&needle) {
                let at = from + rel;
                from = at + 1;
                // A method call on a variable called `db`, not `self.db` or a
                // word ending in `db`.
                if body[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
                {
                    continue;
                }
                let method: String = body[at + needle.len()..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if method.is_empty() || method == "engine" {
                    continue;
                }
                checked += 1;
                let before = &body[..at];
                let wrapped = before.rfind("with_deadline").is_some_and(|w| {
                    !before[w..].contains(';')
                        && !before[w..].contains('{')
                        && !before[w..].contains('}')
                });
                if !wrapped {
                    let line = 1 + before.bytes().filter(|c| *c == b'\n').count();
                    offenders.push(format!("{name}:{line}"));
                }
            }
        }
        assert!(
            checked >= 2,
            "the needle stopped matching: {checked} database reads found, and this \
             crate has at least two — a gate that scans nothing reports success"
        );
        assert!(
            offenders.is_empty(),
            "database reads awaited with no deadline; a statement with no window \
             to close holds the connection until the server gives up:\n{}",
            offenders.join("\n")
        );
    }
}
