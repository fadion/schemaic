//! Server activity — who else is connected to this server, what they are
//! running, and who is blocking whom. Pure over an already-fetched snapshot: no
//! DB, no UI, no engine.
//!
//! The panel polls the server's own activity view (`SHOW PROCESSLIST` +
//! `information_schema.INNODB_TRX` on MySQL/MariaDB, `pg_stat_activity` +
//! `pg_blocking_pids` on PostgreSQL) and shows the result. The *fetch* lives in
//! `schemaic-db`, one query set per engine, because the catalogue names differ.
//! Everything downstream of the fetch is here, because it doesn't: a
//! [`SessionInfo`] means the same thing whichever engine produced it, and the
//! decisions the panel makes about it — how the rows are ordered, which one is
//! worth a banner, what the counts line says, what a kill will actually do — are
//! the parts that are easy to get quietly wrong and impossible to test through a
//! live server.
//!
//! **The blocking graph is stored one way and read both.** The engines answer
//! *"who is this session waiting for"* ([`SessionInfo::blocked_by`]) and nothing
//! answers the reverse, so [`prepare`] derives [`SessionInfo::blocks`] from the
//! edges rather than each display site re-scanning the list for them.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::intel::SqlDialect;
use crate::text::plural;
use crate::text_ops::contains_ignore_ascii_case;

/// Does `dialect` have server sessions to show at all?
///
/// MySQL/MariaDB publish `information_schema.PROCESSLIST`, PostgreSQL publishes
/// `pg_stat_activity`. **SQLite has neither, and this is a statement about SQLite
/// rather than unfinished work**: it is a library linked into this process, not a
/// server — there is no other session to see, no connection but ours to name, and
/// nothing to kill. Its concurrency story is a file lock between *processes*,
/// which no catalogue of the database exposes.
///
/// So a SQLite connection gets the panel's one-line explanation instead of an
/// empty list, and its footer toggle is inert — an empty list would read as
/// "quiet server" when the truth is "wrong question".
pub fn supports_activity(dialect: SqlDialect) -> bool {
    !matches!(dialect, SqlDialect::Sqlite)
}

/// Can a session be cancelled or terminated from here?
///
/// Both server engines offer *both* kinds — MySQL `KILL QUERY` / `KILL`,
/// PostgreSQL `pg_cancel_backend` / `pg_terminate_backend` — so this is exactly
/// [`supports_activity`] and is *computed* from it rather than spelling out a
/// second `!= Sqlite`. If a fourth engine ever lists sessions it cannot kill,
/// this is where that splits, and every call site already asks the right
/// question.
pub fn supports_kill(dialect: SqlDialect) -> bool {
    supports_activity(dialect)
}

/// The most sessions the panel will hold. A pooled application server can sit at
/// four figures of connections, and every one of them would become a live view in
/// a list that re-renders on every poll.
///
/// The fetch asks for `MAX_SESSIONS + 1` so [`prepare`] can tell a full list from
/// a truncated one and *say so* — the same rule the live monitor's log follows:
/// a silently shortened list looks complete and isn't.
///
/// **That `+ 1` is also why [`prepare`] answers "was it cut" and not "by how
/// many".** A count derived from a list the fetch already capped can only ever be
/// `1`, whatever the server's real total is, and a panel reading "500 sessions ·
/// 1 more not shown" in front of four thousand is a worse answer than one that
/// declines to put a number on it.
pub const MAX_SESSIONS: usize = 500;

/// How many rows the panel builds views for at once, out of the [`MAX_SESSIONS`]
/// it holds.
///
/// The snapshot is replaced wholesale on every poll, so every row in it is a view
/// torn down and rebuilt two to thirty seconds apart — text shaping included, for
/// a statement preview and an account name each. Holding five hundred sessions
/// costs nothing until they are *drawn*, and nobody reads a five-hundred-row list
/// by scrolling it: [`prepare`] has already sorted the ones worth looking at to
/// the front, and the search box reaches the rest.
///
/// Rows past this are reported, never silently dropped — see [`render_slice`].
pub const RENDER_CAP: usize = 150;

/// The poll intervals the panel offers, in seconds. `0` is off (refresh by hand).
pub const POLL_INTERVALS: [u64; 5] = [0, 2, 5, 10, 30];

/// The interval a fresh install polls at. Fast enough that the list feels live,
/// slow enough that watching it is not itself a load worth noticing.
pub const DEFAULT_POLL_SECS: u64 = 5;

/// Menu label for a poll interval.
pub fn interval_label(secs: u64) -> String {
    if secs == 0 {
        "Off".to_string()
    } else {
        format!("{secs}s")
    }
}

/// Clamp a persisted interval to one the picker can actually show, so a
/// hand-edited or future-version config can't leave the menu with nothing
/// highlighted (or the timer spinning at 1ms).
pub fn sane_interval(secs: u64) -> u64 {
    if POLL_INTERVALS.contains(&secs) {
        secs
    } else {
        DEFAULT_POLL_SECS
    }
}

/// One connection's chosen poll interval, keyed by `conn_id` — the same shape the
/// colour and favourite stores use, and persisted in `ui_state.json` beside the
/// panel layout it belongs to.
///
/// **Per connection and not one global preference**, because the setting is not a
/// taste — it is how hard you are willing to lean on a particular server. Two
/// seconds is right on a laptop MariaDB and is a thing you would not do to a
/// production replica, and a single number means switching to that replica
/// silently keeps whatever the last server was set to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntervalRule {
    pub conn_id: u64,
    pub secs: u64,
}

/// The interval to poll `conn_id` at — its own if it has one, otherwise
/// [`DEFAULT_POLL_SECS`].
///
/// Clamped on the way out through [`sane_interval`], so the file is read
/// defensively at the one place that reads it rather than trusted at every use.
pub fn interval_for(rules: &[IntervalRule], conn_id: u64) -> u64 {
    rules
        .iter()
        .find(|r| r.conn_id == conn_id)
        .map(|r| sane_interval(r.secs))
        .unwrap_or(DEFAULT_POLL_SECS)
}

/// Record `conn_id`'s interval, replacing any previous choice.
///
/// A choice equal to the default is still **stored**, not dropped as redundant: if
/// [`DEFAULT_POLL_SECS`] ever changes, a connection someone deliberately set to
/// the old default would silently move with it.
pub fn set_interval(rules: &mut Vec<IntervalRule>, conn_id: u64, secs: u64) {
    let secs = sane_interval(secs);
    match rules.iter_mut().find(|r| r.conn_id == conn_id) {
        Some(r) => r.secs = secs,
        None => rules.push(IntervalRule { conn_id, secs }),
    }
}

/// Forget `conn_id`'s interval — the connection was deleted, and nothing keyed to
/// it should outlive it (ids are reused).
pub fn clear_conn(rules: &mut Vec<IntervalRule>, conn_id: u64) {
    rules.retain(|r| r.conn_id != conn_id);
}

/// What a session is doing, normalized across engines.
///
/// **The variant order is the attention order**, and [`rank`] is derived from it:
/// a session waiting on a lock is the one you opened the panel for, and an idle
/// connection is the one you scroll past. Reordering these variants reorders the
/// panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SessionState {
    /// Waiting on a lock another session holds. MySQL: an `INNODB_TRX` row in
    /// `LOCK WAIT`. PostgreSQL: `pg_blocking_pids()` came back non-empty.
    Blocked,
    /// Inside an open transaction with no statement running — the state that
    /// holds locks indefinitely while looking harmless. MySQL: a `Sleep` thread
    /// with a live `INNODB_TRX` row. PostgreSQL: `idle in transaction` (and its
    /// `(aborted)` variant).
    IdleInTx,
    /// Executing a statement right now.
    Running,
    /// Connected, no transaction, nothing running.
    Idle,
}

impl SessionState {
    /// The word shown on the row, and the one the search box matches.
    pub fn label(self) -> &'static str {
        match self {
            SessionState::Blocked => "Blocked",
            SessionState::IdleInTx => "Idle in txn",
            SessionState::Running => "Running",
            SessionState::Idle => "Idle",
        }
    }

    /// Is this session inside a transaction it hasn't ended? Both of these hold
    /// locks; the panel counts them together in the summary line.
    pub fn in_transaction(self) -> bool {
        matches!(self, SessionState::Blocked | SessionState::IdleInTx)
    }
}

/// One server session, as the panel understands it. Engine-neutral by
/// construction — see the module docs.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionInfo {
    /// MySQL thread id / PostgreSQL backend pid. Signed because PostgreSQL's
    /// `pid` is a signed `int4`, and what the kill statement quotes back has to
    /// be exactly what the server said.
    pub id: i64,
    /// The account the session authenticated as, without the host part.
    pub user: String,
    /// Where it connected from, when the server publishes it (`None` for a local
    /// socket, which neither engine gives an address).
    pub client: Option<String>,
    /// The database it is attached to, if any.
    pub database: Option<String>,
    pub state: SessionState,
    /// The statement the server reports, if it reports one. `None` for an idle
    /// session, and for a server that withholds it (a non-superuser sees only
    /// their own queries in `pg_stat_activity`).
    pub sql: Option<String>,
    /// How long the session has been in this state, in seconds, as the server
    /// measured it.
    pub seconds: f64,
    /// Sessions this one is waiting for. The engines answer this direction; see
    /// [`prepare`] for the other.
    pub blocked_by: Vec<i64>,
    /// Sessions waiting on this one. **Derived** — [`prepare`] fills it, the
    /// fetch never does.
    pub blocks: Vec<i64>,
}

impl SessionInfo {
    /// `user@host`, or whichever half the server gave. Never empty — an
    /// unattributable session still needs something on the row.
    pub fn who(&self) -> String {
        let host = self.client.as_deref().unwrap_or("").trim();
        match (self.user.trim(), host) {
            ("", "") => "—".to_string(),
            ("", h) => h.to_string(),
            (u, "") => u.to_string(),
            (u, h) => format!("{u}@{h}"),
        }
    }

    /// The statement this session is **running now**, as opposed to the last one
    /// it ran.
    ///
    /// The two engines answer the panel's own question differently:
    /// `PROCESSLIST.INFO` is NULL for a `Sleep` thread, while
    /// `pg_stat_activity.query` keeps the last statement for an `idle` backend
    /// indefinitely. So a wall of PostgreSQL pool connections each drew a real,
    /// plausible statement in the same monospace block a running one gets, with
    /// the grey state word underneath as the only thing saying otherwise — on
    /// the panel whose one decision is *what to kill*.
    ///
    /// Decided here rather than per surface, and rather than by dropping the
    /// column at the reader: `sql` is still wanted for *Copy statement*, and a
    /// second surface that draws a statement should not have to rediscover this.
    pub fn running_sql(&self) -> Option<&str> {
        (self.state != SessionState::Idle).then_some(self.sql.as_deref())?
    }

    /// The one-line explanation under the row's state, or `None` when the
    /// session's state already says everything.
    ///
    /// Waiting is reported before blocking: a session doing both is a link in the
    /// middle of a chain, and what it is waiting for is what has to be resolved
    /// first.
    pub fn note(&self) -> Option<String> {
        if !self.blocked_by.is_empty() {
            let ids = join_ids(&self.blocked_by);
            return Some(format!("waiting on a lock held by {ids}"));
        }
        if !self.blocks.is_empty() {
            let n = self.blocks.len();
            return Some(format!("blocks {n} {}", plural(n, "session", "sessions")));
        }
        None
    }
}

/// Ids as `1148` / `1148, 1150` — the note's tail, so a chain reads as one
/// sentence instead of a list.
fn join_ids(ids: &[i64]) -> String {
    ids.iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Attention rank — the panel's primary sort key. Lower sorts first.
///
/// The state's own order carries most of it (see [`SessionState`]); the one thing
/// it can't express is that a session *blocking* someone belongs near the top
/// whatever it is doing, because it is the one the user is about to kill. A
/// long-running `SELECT` nobody is waiting for is not that.
pub fn rank(s: &SessionInfo) -> u8 {
    match s.state {
        SessionState::Blocked => 0,
        _ if !s.blocks.is_empty() => 1,
        SessionState::IdleInTx => 2,
        SessionState::Running => 3,
        SessionState::Idle => 4,
    }
}

/// Turn a freshly-fetched list into the one the panel renders: derive the
/// `blocks` edges, order it, and cut it to [`MAX_SESSIONS`].
///
/// Returns **whether the cut dropped anything** — not how much, because it
/// cannot know: the fetch asks for `MAX_SESSIONS + 1` rows precisely so this can
/// notice, which means a server holding four thousand sessions and one holding
/// five hundred and one arrive here identical (see [`MAX_SESSIONS`]). A `bool`
/// is the honest shape of that answer, and one nothing downstream can render as
/// a figure.
///
/// The cut happens **after** the sort, so what survives is the interesting end of
/// the list rather than whatever order the server happened to answer in.
///
/// **Both edge lists are deduplicated**, and that is not tidiness. MySQL answers
/// the blocking graph out of `performance_schema.data_lock_waits` (or
/// `INNODB_LOCK_WAITS`), which is one row per *lock* pair, not per transaction
/// pair: a holder sitting on both a record lock and a gap lock the waiter needs
/// reports the same `(waiter, holder)` edge twice. Left alone that reads as
/// "waiting on a lock held by 1148, 1148" on the waiter and inflates the holder's
/// "blocks N sessions" by the number of locks rather than the number of sessions.
/// PostgreSQL's `pg_blocking_pids` already returns distinct pids, so doing it
/// here is also what keeps the two engines from disagreeing.
pub fn prepare(sessions: &mut Vec<SessionInfo>) -> bool {
    // One pass over the edges, so a list of N sessions with M waits costs N+M
    // rather than N².
    let mut waiters: HashMap<i64, Vec<i64>> = HashMap::new();
    for s in sessions.iter() {
        for holder in &s.blocked_by {
            waiters.entry(*holder).or_default().push(s.id);
        }
    }
    for s in sessions.iter_mut() {
        s.blocks = waiters.remove(&s.id).unwrap_or_default();
        s.blocks.sort_unstable();
        s.blocks.dedup();
        s.blocked_by.sort_unstable();
        s.blocked_by.dedup();
    }
    // Rank, then longest-standing first (the worst wait, the slowest query), then
    // id so the order is total and a redraw can't shuffle two equal rows.
    sessions.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| {
                b.seconds
                    .partial_cmp(&a.seconds)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.id.cmp(&b.id))
    });
    let truncated = sessions.len() > MAX_SESSIONS;
    if truncated {
        sessions.truncate(MAX_SESSIONS);
    }
    truncated
}

/// The rows the panel draws, and how many matched but were left undrawn.
///
/// A cheap cut over an already-ordered list: [`prepare`] has put the sessions
/// worth attention at the front, so the first [`RENDER_CAP`] of whatever survived
/// the search box are the ones a person is going to read. The remainder is
/// *counted* — this one is an exact figure, because it is measured against the
/// list actually in hand rather than against a server total nobody fetched.
///
/// Returned as a slice so the caller builds views straight out of the snapshot
/// instead of cloning the rows it is about to render.
///
/// Generic over `T` so the caller can hand over a `Vec<&SessionInfo>` — the
/// search filter's natural shape, and the one that doesn't copy four hundred
/// rows this function is about to drop.
pub fn render_slice<T>(rows: &[T]) -> (&[T], usize) {
    let shown = rows.len().min(RENDER_CAP);
    (&rows[..shown], rows.len() - shown)
}

/// The counts line above the list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActivitySummary {
    pub total: usize,
    pub running: usize,
    pub blocked: usize,
    pub idle_in_tx: usize,
}

impl ActivitySummary {
    /// `18 sessions` — the only part of the line that is always shown, because
    /// zero of it is itself the answer.
    ///
    /// `truncated` is [`prepare`]'s verdict, and it turns the figure into
    /// `500+ sessions`. Without it the line states the *cap* as though it were
    /// the server's count: a box holding four thousand connections read
    /// "500 sessions", which is not a rounding error but a different fact. The
    /// `+` is the whole of what this side of the wire can honestly say, since
    /// the fetch stopped counting at `MAX_SESSIONS + 1`.
    pub fn total_label(&self, truncated: bool) -> String {
        let plus = if truncated { "+" } else { "" };
        format!(
            "{}{plus} {}",
            self.total,
            // A capped list is always plural, whatever `total` happens to be.
            plural(self.total, "session", "sessions")
        )
    }
}

/// Count the states for the summary line. A blocked session is *not* also
/// counted as idle-in-transaction even though it holds one open: the two figures
/// sit side by side and would double-count the same row.
pub fn summarize(sessions: &[SessionInfo]) -> ActivitySummary {
    let mut out = ActivitySummary {
        total: sessions.len(),
        ..Default::default()
    };
    for s in sessions {
        match s.state {
            SessionState::Blocked => out.blocked += 1,
            SessionState::IdleInTx => out.idle_in_tx += 1,
            SessionState::Running => out.running += 1,
            SessionState::Idle => {}
        }
    }
    out
}

/// Should the panel be polling right now?
///
/// Every tick is a full connect + authenticate + three `information_schema`
/// statements against the server being watched, forever, at an interval as short
/// as two seconds — so "nobody can see this" is the whole question, and it has
/// three answers, not two.
///
/// `fits` is the one that was missing. Narrowing the window past the responsive
/// breakpoint gives the right column zero width: the panel is invisible, its
/// footer toggle is inert (`set_right` returns early below the breakpoint) and
/// the palette's toggle is behind the same guard — so the poll could be neither
/// seen nor stopped from inside the app, and went on opening a connection every
/// couple of seconds for nobody.
pub fn should_poll(panel_is_activity: bool, fits: bool, focused: bool) -> bool {
    panel_is_activity && fits && focused
}

/// Does this session match the panel's search box? Matches the id, the account,
/// the client address, the database, the state word and the statement — the
/// panel shows all six, so all six are searchable.
///
/// **The statement is matched whitespace-collapsed**, because that is how the
/// row draws it (`history::preview`). `PROCESSLIST.INFO` keeps whatever newlines
/// the client typed, so a session running a three-line `SELECT * FROM orders`
/// reads as one line on screen and was filtered *out* by the phrase visibly on
/// it. `history::matches_query` solved exactly this, one module over, and this
/// one was written without that half — so it borrows the same collapser rather
/// than growing a second.
pub fn matches_query(s: &SessionInfo, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    contains_ignore_ascii_case(&s.id.to_string(), q)
        || contains_ignore_ascii_case(&s.who(), q)
        || contains_ignore_ascii_case(s.state.label(), q)
        || s.database
            .as_deref()
            .is_some_and(|d| contains_ignore_ascii_case(d, q))
        || s.sql
            .as_deref()
            .is_some_and(|sql| contains_ignore_ascii_case(&crate::history::full_preview(sql), q))
}

/// The lock wait worth putting a banner on, if there is one.
///
/// The list already shows every blocked session; the banner exists because the
/// *interesting* one can be twenty rows down a list that reorders under you. It
/// picks the longest-waiting blocked session — [`prepare`] has already sorted it
/// to the front, but this doesn't assume that, since a filtered list is what the
/// panel usually holds.
///
/// Returns the waiter and the session it is waiting for, when that holder is in
/// the snapshot. A holder the fetch didn't see (killed between the two queries, or
/// hidden by privileges) yields `None`: a banner offering to kill a session that
/// isn't there is worse than no banner.
pub fn lock_wait(sessions: &[SessionInfo]) -> Option<(&SessionInfo, &SessionInfo)> {
    let waiter = sessions
        .iter()
        .filter(|s| !s.blocked_by.is_empty())
        .max_by(|a, b| {
            a.seconds
                .partial_cmp(&b.seconds)
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
    // **The longest-standing holder, not the lowest id.** `prepare` sorts
    // `blocked_by` ascending, so `first()` was "numerically smallest pid", which
    // carries no meaning — and a wait with several holders is ordinary, since
    // more than one session can hold a conflicting lock on a row. Choosing the
    // same way the waiter is chosen at least makes the banner's action the one
    // most likely to matter; how many others there are is `lock_wait_text`'s
    // job to say.
    let holder = waiter
        .blocked_by
        .iter()
        .filter_map(|id| sessions.iter().find(|s| s.id == *id))
        .max_by(|a, b| {
            a.seconds
                .partial_cmp(&b.seconds)
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
    Some((waiter, holder))
}

/// The banner's sentence: who is waiting, on whom, and for how long the holder
/// has been sitting on it.
///
/// The holder's *state* is the point — "idle in transaction for 4m 12s" is a
/// stuck client, "running for 4m 12s" is a slow statement, and the two want
/// different reactions from the person reading it.
///
/// **It says when there are others.** A wait with three holders is ordinary, and
/// naming one of them as *the* holder invites a kill that leaves the waiter
/// blocked by the other two, with the banner immediately re-rendering the next
/// one and nothing having said more existed.
pub fn lock_wait_text(waiter: &SessionInfo, holder: &SessionInfo) -> String {
    let holder_doing = match holder.state {
        SessionState::IdleInTx => "idle in transaction",
        SessionState::Blocked => "itself blocked",
        SessionState::Running => "running",
        SessionState::Idle => "idle",
    };
    let others = waiter.blocked_by.len().saturating_sub(1);
    let and_others = if others == 0 {
        String::new()
    } else {
        format!(
            " {others} other {} also hold{} a lock it needs.",
            plural(others, "session", "sessions"),
            if others == 1 { "s" } else { "" }
        )
    };
    format!(
        "Session {} ({}) has been waiting {} for a lock held by session {} ({}), {} for {}.{}",
        waiter.id,
        waiter.who(),
        format_age(waiter.seconds),
        holder.id,
        holder.who(),
        holder_doing,
        format_age(holder.seconds),
        and_others,
    )
}

/// Normalize a MySQL/MariaDB `PROCESSLIST` row to a [`SessionState`].
///
/// `command` is the thread's current command word; `trx_state` is the matching
/// `information_schema.INNODB_TRX.trx_state`, or `None` when the thread has no
/// open InnoDB transaction (or the account can't read that view).
///
/// **`Sleep` alone does not mean idle**, and that distinction is the reason this
/// takes two arguments. A connection sitting in `Sleep` with a live `INNODB_TRX`
/// row is a client that ran `BEGIN`, did some work, and then went away holding
/// every row it touched — the single most common cause of the lock waits this
/// panel is for. Without the transaction view the same thread is indistinguishable
/// from an idle pool connection, which is exactly how it stays invisible in
/// `SHOW PROCESSLIST`.
pub fn mysql_state(command: &str, trx_state: Option<&str>) -> SessionState {
    let trx = trx_state.map(str::trim);
    if trx.is_some_and(|s| s.eq_ignore_ascii_case("LOCK WAIT")) {
        return SessionState::Blocked;
    }
    if command.trim().eq_ignore_ascii_case("Sleep") {
        return match trx {
            Some(_) => SessionState::IdleInTx,
            None => SessionState::Idle,
        };
    }
    SessionState::Running
}

/// Normalize a PostgreSQL `pg_stat_activity` row to a [`SessionState`].
///
/// `state` is the view's `state` column and `blocked` is whether
/// `pg_blocking_pids()` came back non-empty — PostgreSQL reports a lock wait as
/// an `active` backend with a `Lock` wait event rather than a state of its own,
/// so the blocking graph is what promotes it.
///
/// `has_query` only matters for the states this doesn't recognise. A backend the
/// account isn't privileged to inspect arrives with `state` NULL, and guessing is
/// unavoidable there: with a statement visible it is doing something, without one
/// it is the far more common idle pool connection. Guessing *Running* for every
/// such row would put a wall of them above the sessions that matter.
pub fn pg_state(state: Option<&str>, has_query: bool, blocked: bool) -> SessionState {
    if blocked {
        return SessionState::Blocked;
    }
    match state.map(str::trim) {
        // Covers `idle in transaction` and `idle in transaction (aborted)`.
        Some(s) if s.starts_with("idle in transaction") => SessionState::IdleInTx,
        Some("idle") => SessionState::Idle,
        Some("active") => SessionState::Running,
        _ if has_query => SessionState::Running,
        _ => SessionState::Idle,
    }
}

/// The address half of a client identity, with the ephemeral port dropped.
///
/// MySQL reports `HOST` as `10.0.4.12:54388` — the port is a different number on
/// every reconnect, so keeping it makes the same client look like a new one on
/// every row and pushes the address itself out of a narrow panel. PostgreSQL's
/// `client_addr` has no port to begin with, which is the shape this normalizes to.
///
/// Only a trailing `:<port>` is removed, and only when the tail really is a port
/// number: a bare IPv6 address is colon-dense and must survive intact.
pub fn strip_port(host: &str) -> &str {
    let h = host.trim();
    match h.rsplit_once(':') {
        // `[::1]:3306` — bracketed, so the split is unambiguous.
        Some((head, tail)) if head.ends_with(']') && tail.parse::<u16>().is_ok() => head,
        // A bare IPv6 address has more than one colon and no brackets; there is
        // nothing to distinguish its last group from a port, so leave it alone.
        Some(_) if h.matches(':').count() > 1 => h,
        Some((head, tail)) if tail.parse::<u16>().is_ok() => head,
        _ => h,
    }
}

/// What PostgreSQL puts in `query` for a backend the connecting role may not
/// inspect. It is a *message*, not a statement, and rendering it as one — or
/// letting it stand in for "this backend is doing something" — is how a masked
/// row would claim to be `Running` with a query nobody can act on.
pub const PG_MASKED_QUERY: &str = "<insufficient privilege>";

/// One `information_schema.PROCESSLIST` row, in the order the MySQL backend
/// selects the columns.
///
/// **The fold below is a decision, not an I/O wrapper**, which is why the row
/// shape lives here: it chooses which of three result sets wins for a thread,
/// whether a host becomes an address or nothing, whether an empty `DB` is a
/// database, whether a whitespace-only `INFO` is a statement, and that a
/// negative `TIME` clamps before it becomes a sort key. None of that was
/// reachable from a test while it sat inside an `async fn` holding a connection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MyProcessRow {
    pub id: i64,
    pub user: String,
    pub host: String,
    pub database: Option<String>,
    pub command: String,
    pub seconds: i64,
    pub info: Option<String>,
}

/// Fold MySQL's three activity result sets into [`SessionInfo`]s.
///
/// `trx` is `INNODB_TRX` keyed by thread, `waits` the raw `(waiter, holder)`
/// edges — duplicates and all, because both lock views are keyed by *lock* and
/// one holder sitting on a record lock and a gap lock the waiter needs reports
/// the same pair twice. [`prepare`] deduplicates, so the rule is unit-tested in
/// one place and PostgreSQL (whose `pg_blocking_pids` is already distinct)
/// cannot drift from it.
pub fn from_mysql_rows(
    rows: &[MyProcessRow],
    trx: &std::collections::HashMap<i64, String>,
    waits: &[(i64, i64)],
) -> Vec<SessionInfo> {
    let mut blocked_by: std::collections::HashMap<i64, Vec<i64>> = std::collections::HashMap::new();
    for (waiter, blocker) in waits {
        blocked_by.entry(*waiter).or_default().push(*blocker);
    }
    rows.iter()
        .map(|r| {
            let client = strip_port(&r.host);
            SessionInfo {
                id: r.id,
                user: r.user.clone(),
                client: (!client.is_empty()).then(|| client.to_string()),
                database: r.database.clone().filter(|d| !d.is_empty()),
                state: mysql_state(&r.command, trx.get(&r.id).map(String::as_str)),
                sql: r
                    .info
                    .as_ref()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                // `TIME` is signed and goes negative when the system clock steps
                // back under a live thread; `format_age` floors it, but the sort
                // key must not hoist such a row to the top of its group.
                seconds: r.seconds.max(0) as f64,
                blocked_by: blocked_by.remove(&r.id).unwrap_or_default(),
                blocks: Vec::new(),
            }
        })
        .collect()
}

/// One `pg_stat_activity` row, every column as the text `simple_query` returns:
/// `(pid, usename, client_host, datname, state, query, age, blockers)`.
pub type PgActivityRow = [Option<String>; 8];

/// Fold PostgreSQL's activity rows into [`SessionInfo`]s.
///
/// **A row the role may not inspect is kept, and says so by saying nothing.**
/// PostgreSQL masks `usename`, `datname`, `state`, `client_addr` and friends to
/// NULL for such a backend and puts [`PG_MASKED_QUERY`] in `query` — while `pid`
/// and `pg_blocking_pids(pid)` stay real, so the row is still a killable session
/// and still a usable node in the blocking graph. Dropping the sentinel is what
/// keeps it from being classified `Running` and from being drawn as a statement.
///
/// A row whose **pid** won't parse is dropped, not admitted under a made-up id:
/// a session `0` renders a full row with a live "Kill session" under it, and
/// other rows' `blocked_by` edges would point at it. An unreadable *age* has a
/// safe default — it only decides where the row sorts.
pub fn from_pg_rows(rows: &[PgActivityRow]) -> Vec<SessionInfo> {
    let text = |r: &PgActivityRow, i: usize| r[i].clone().filter(|s| !s.is_empty());
    rows.iter()
        .filter_map(|r| {
            let query = text(r, 5)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s != PG_MASKED_QUERY);
            let blocked_by = parse_pid_array(r[7].as_deref().unwrap_or(""));
            let id: i64 = r[0].as_deref()?.trim().parse().ok()?;
            Some(SessionInfo {
                id,
                user: text(r, 1).unwrap_or_default(),
                client: text(r, 2),
                database: text(r, 3),
                state: pg_state(
                    text(r, 4).as_deref(),
                    query.is_some(),
                    !blocked_by.is_empty(),
                ),
                sql: query,
                seconds: text(r, 6).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                blocked_by,
                blocks: Vec::new(),
            })
        })
        .collect()
}

/// Read a PostgreSQL `int[]` as rendered by `::text` — `{}`, `{1234}`,
/// `{1234,5678}`. `simple_query` hands back every column as text, so the array
/// arrives as its literal and there is no typed accessor to lean on.
///
/// Anything unparseable is skipped rather than defaulting to a pid: a `0` in the
/// blocking graph would draw an edge to a session that does not exist and put a
/// "Kill 0" button under it.
pub fn parse_pid_array(s: &str) -> Vec<i64> {
    s.trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .split(',')
        .filter_map(|p| p.trim().parse::<i64>().ok())
        .collect()
}

/// Which of the two things "kill" can mean.
///
/// Both engines offer both, and the difference is the whole reason this is an
/// enum and not a bool: cancelling a statement leaves the session and its
/// transaction alive (the client sees a query error and can retry), while
/// terminating rolls the transaction back and drops the connection under a client
/// that may not be written to survive it. Offering only the second would make
/// "unstick this query" cost more than it has to; offering only the first would
/// leave an idle-in-transaction holder — which has no statement to cancel —
/// unkillable, and that is the case the panel exists for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KillKind {
    /// Cancel the running statement, keep the session.
    Query,
    /// Terminate the whole session; any open transaction rolls back.
    Session,
}

impl KillKind {
    /// The menu/button label.
    pub fn label(self) -> &'static str {
        match self {
            KillKind::Query => "Cancel query",
            KillKind::Session => "Kill session",
        }
    }

    /// Is this offered for `s`? Cancelling needs a statement **executing** — on
    /// a session that isn't running one the server accepts the request and does
    /// nothing, which is a button that lies.
    ///
    /// Keyed on the state being one where a statement is in flight, not on it
    /// merely not being `Idle`. PostgreSQL keeps the *last* statement in `query`
    /// for an `idle in transaction` backend — the panel's headline case, a
    /// client that ran `BEGIN` and went away holding locks — so the row offered
    /// **Cancel query** and its confirm promised an abort `pg_cancel_backend`
    /// discards while the backend is reading a command. MySQL escaped it by
    /// luck: `PROCESSLIST.INFO` is NULL for a `Sleep` thread, so `sql` was
    /// already `None` there.
    ///
    /// `Blocked` counts as executing — it is waiting *inside* a statement, and
    /// cancelling it is the whole point of the panel.
    pub fn applies_to(self, s: &SessionInfo) -> bool {
        match self {
            KillKind::Query => {
                s.sql.is_some() && matches!(s.state, SessionState::Running | SessionState::Blocked)
            }
            KillKind::Session => true,
        }
    }
}

/// The confirm modal's title and body for a kill.
///
/// Both kinds ask. Killing someone else's work is exactly the class the shared
/// confirm exists for, and the two consequences differ enough that the sentence
/// has to name which one is about to happen.
///
/// **Dialect-aware for one clause, and it is the clause that decides whether the
/// user's work survives.** MySQL's `KILL QUERY` stops the statement and leaves
/// the transaction usable — the client can retry and carry on. PostgreSQL's
/// `pg_cancel_backend` leaves the transaction *open but aborted*: every
/// subsequent statement on that connection fails with "current transaction is
/// aborted" until someone rolls back, so the uncommitted work is gone exactly as
/// it would be under a terminate. Telling a PostgreSQL user their transaction
/// "stays open" is true and useless; what they need to know is that it is no
/// longer usable.
pub fn kill_confirm(kind: KillKind, s: &SessionInfo, dialect: SqlDialect) -> (String, String) {
    let who = s.who();
    let where_ = match s.database.as_deref() {
        Some(db) if !db.is_empty() => format!(" on {db}"),
        _ => String::new(),
    };
    match kind {
        KillKind::Query => (
            format!("Cancel query on session {}", s.id),
            format!(
                "Stop the statement {who} is running{where_}? \
                 The session stays connected and the client sees the query fail. {}",
                cancel_transaction_note(dialect)
            ),
        ),
        KillKind::Session => (
            format!("Kill session {}", s.id),
            format!(
                "Disconnect {who}{where_}? \
                 Any transaction it has open is rolled back and its client sees the \
                 connection drop. This can't be undone."
            ),
        ),
    }
}

/// What a cancelled statement leaves behind, per engine — the second half of
/// [`kill_confirm`]'s cancel sentence.
///
/// Asked as a **capability** would be, off the dialect rather than off a guess:
/// SQLite never reaches here (it has no sessions to cancel, see
/// [`supports_activity`]) but still has to answer something, and the MySQL
/// wording is the safe one to give it — it promises less.
fn cancel_transaction_note(dialect: SqlDialect) -> &'static str {
    match dialect {
        // The transaction survives intact; the client can retry the statement.
        SqlDialect::MySql | SqlDialect::Sqlite => "Any open transaction stays open.",
        // Open, but poisoned: `ERROR 25P02` on everything until a rollback.
        SqlDialect::Postgres => {
            "Any open transaction is left aborted — the client has to roll it back, \
             so its uncommitted work is lost either way."
        }
    }
}

/// How long a session has been in its state, at the precision the coarsest
/// engine actually has: MySQL's `PROCESSLIST.TIME` is whole seconds, so a tenth
/// of a second here would be a digit invented for half the connections in the
/// app. Past a minute the unit steps up rather than the count running on.
///
/// Separate from [`crate::history::format_duration`], which formats a *query's*
/// millisecond runtime and has no arm past minutes — a connection open since
/// yesterday would render there as `1873m 04s`.
pub fn format_age(secs: f64) -> String {
    let secs = if secs.is_finite() && secs > 0.0 {
        secs
    } else {
        0.0
    };
    if secs < 60.0 {
        format!("{}s", secs as u64)
    } else if secs < 3600.0 {
        let t = secs as u64;
        format!("{}m {:02}s", t / 60, t % 60)
    } else if secs < 86_400.0 {
        let t = secs as u64;
        format!("{}h {:02}m", t / 3600, (t % 3600) / 60)
    } else {
        let t = secs as u64;
        format!("{}d {:02}h", t / 86_400, (t % 86_400) / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sess(id: i64, state: SessionState, seconds: f64) -> SessionInfo {
        SessionInfo {
            id,
            user: "app".to_string(),
            client: Some("10.0.0.1".to_string()),
            database: Some("employees".to_string()),
            state,
            sql: Some("SELECT 1".to_string()),
            seconds,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
        }
    }

    #[test]
    fn sqlite_has_no_sessions_and_nothing_to_kill() {
        assert!(!supports_activity(SqlDialect::Sqlite));
        assert!(!supports_kill(SqlDialect::Sqlite));
        for d in [SqlDialect::MySql, SqlDialect::Postgres] {
            assert!(supports_activity(d), "{d:?} publishes sessions");
            assert!(supports_kill(d), "{d:?} can kill one");
        }
    }

    #[test]
    fn who_uses_whichever_halves_the_server_gave() {
        let mut s = sess(1, SessionState::Idle, 0.0);
        assert_eq!(s.who(), "app@10.0.0.1");
        s.client = None;
        assert_eq!(s.who(), "app");
        s.user = String::new();
        s.client = Some("10.0.0.1".to_string());
        assert_eq!(s.who(), "10.0.0.1");
        s.client = Some("   ".to_string());
        assert_eq!(s.who(), "—", "blank is not an address");
    }

    #[test]
    fn prepare_derives_the_reverse_edges() {
        let mut v = vec![
            {
                let mut s = sess(1, SessionState::Blocked, 5.0);
                s.blocked_by = vec![3];
                s
            },
            {
                let mut s = sess(2, SessionState::Blocked, 1.0);
                s.blocked_by = vec![3];
                s
            },
            sess(3, SessionState::IdleInTx, 60.0),
        ];
        assert!(!prepare(&mut v));
        let holder = v.iter().find(|s| s.id == 3).expect("holder kept");
        assert_eq!(holder.blocks, vec![1, 2], "both waiters, sorted");
        assert_eq!(holder.note().as_deref(), Some("blocks 2 sessions"));
        let waiter = v.iter().find(|s| s.id == 1).expect("waiter kept");
        assert!(waiter.blocks.is_empty());
        assert_eq!(
            waiter.note().as_deref(),
            Some("waiting on a lock held by 3"),
            "waiting is reported before blocking"
        );
    }

    #[test]
    fn a_blocker_outranks_its_own_state() {
        let mut running_blocker = sess(9, SessionState::Running, 1.0);
        running_blocker.blocks = vec![1];
        assert_eq!(rank(&running_blocker), 1);
        assert!(
            rank(&running_blocker) < rank(&sess(8, SessionState::IdleInTx, 999.0)),
            "the session someone is waiting on comes before a quiet open transaction"
        );
        assert!(rank(&sess(7, SessionState::Blocked, 0.0)) < rank(&running_blocker));
        assert!(
            rank(&sess(6, SessionState::Running, 0.0)) < rank(&sess(5, SessionState::Idle, 0.0))
        );
    }

    #[test]
    fn prepare_orders_by_rank_then_longest_then_id() {
        let mut v = vec![
            sess(10, SessionState::Idle, 900.0),
            sess(11, SessionState::Running, 5.0),
            sess(12, SessionState::Running, 50.0),
            {
                let mut s = sess(13, SessionState::Blocked, 2.0);
                s.blocked_by = vec![14];
                s
            },
            sess(14, SessionState::IdleInTx, 300.0),
        ];
        prepare(&mut v);
        let ids: Vec<i64> = v.iter().map(|s| s.id).collect();
        assert_eq!(
            ids,
            vec![13, 14, 12, 11, 10],
            "blocked, then the blocker, then the slowest query, then the rest"
        );
    }

    #[test]
    fn prepare_breaks_a_duration_tie_by_id() {
        let mut v = vec![
            sess(30, SessionState::Running, 4.0),
            sess(20, SessionState::Running, 4.0),
        ];
        prepare(&mut v);
        assert_eq!(v.iter().map(|s| s.id).collect::<Vec<_>>(), vec![20, 30]);
    }

    #[test]
    fn prepare_cuts_to_the_cap_and_says_it_cut() {
        let mut v: Vec<SessionInfo> = (0..MAX_SESSIONS as i64 + 3)
            .map(|i| sess(i, SessionState::Idle, i as f64))
            .collect();
        assert!(prepare(&mut v));
        assert_eq!(v.len(), MAX_SESSIONS);
        // The cut is after the sort, so the longest-standing survive.
        assert_eq!(v[0].id, MAX_SESSIONS as i64 + 2);
    }

    /// The fetch asks for `MAX_SESSIONS + 1`, so the only two lists that ever
    /// reach `prepare` truncated are these — and a count taken from either would
    /// say `1` about a server holding any number at all. The answer has to be a
    /// fact, not a figure.
    #[test]
    fn the_cut_is_a_fact_and_not_a_figure() {
        let cut = |extra: i64| {
            let mut v: Vec<SessionInfo> = (0..MAX_SESSIONS as i64 + extra)
                .map(|i| sess(i, SessionState::Idle, 0.0))
                .collect();
            prepare(&mut v)
        };
        // What a 501-row and a 4000-row server both look like by the time the
        // fetch's own `LIMIT` has been applied.
        assert!(cut(1));
        assert!(cut(3500), "still just 'yes, there were more'");
    }

    #[test]
    fn prepare_reports_nothing_dropped_when_it_fits_exactly() {
        let mut v: Vec<SessionInfo> = (0..MAX_SESSIONS as i64)
            .map(|i| sess(i, SessionState::Idle, 0.0))
            .collect();
        assert!(!prepare(&mut v), "a full list is not a truncated one");
        assert_eq!(v.len(), MAX_SESSIONS);
    }

    /// MySQL's lock-wait views are keyed by *lock*, not by transaction: one
    /// holder sitting on a record lock and a gap lock the waiter needs reports
    /// the same edge twice. Undeduplicated it reads "waiting on a lock held by
    /// 3, 3" and turns one waiter into "blocks 2 sessions".
    #[test]
    fn duplicate_lock_rows_are_one_edge() {
        let mut v = vec![
            {
                let mut s = sess(1, SessionState::Blocked, 5.0);
                s.blocked_by = vec![3, 3, 3];
                s
            },
            {
                let mut s = sess(2, SessionState::Blocked, 4.0);
                s.blocked_by = vec![3, 3];
                s
            },
            sess(3, SessionState::IdleInTx, 60.0),
        ];
        prepare(&mut v);
        let waiter = v.iter().find(|s| s.id == 1).expect("waiter kept");
        assert_eq!(waiter.blocked_by, vec![3]);
        assert_eq!(
            waiter.note().as_deref(),
            Some("waiting on a lock held by 3"),
            "one holder, named once"
        );
        let holder = v.iter().find(|s| s.id == 3).expect("holder kept");
        assert_eq!(holder.blocks, vec![1, 2], "two waiters, not five lock rows");
        assert_eq!(holder.note().as_deref(), Some("blocks 2 sessions"));
    }

    /// Two *different* holders are still two edges — dedup must not collapse a
    /// real chain into a single name.
    #[test]
    fn distinct_holders_survive_the_dedup() {
        let mut v = vec![{
            let mut s = sess(1, SessionState::Blocked, 5.0);
            s.blocked_by = vec![7, 3, 7, 3, 9];
            s
        }];
        prepare(&mut v);
        assert_eq!(v[0].blocked_by, vec![3, 7, 9]);
        assert_eq!(
            v[0].note().as_deref(),
            Some("waiting on a lock held by 3, 7, 9")
        );
    }

    #[test]
    fn prepare_is_fine_with_nothing() {
        let mut v: Vec<SessionInfo> = Vec::new();
        assert!(!prepare(&mut v));
        assert_eq!(summarize(&v), ActivitySummary::default());
        assert_eq!(summarize(&v).total_label(false), "0 sessions");
        assert!(lock_wait(&v).is_none());
    }

    #[test]
    fn the_render_slice_draws_the_front_and_counts_the_rest() {
        let short: Vec<SessionInfo> = (0..3).map(|i| sess(i, SessionState::Idle, 0.0)).collect();
        let (rows, rest) = render_slice(&short);
        assert_eq!((rows.len(), rest), (3, 0), "nothing to leave out");

        let long: Vec<SessionInfo> = (0..RENDER_CAP as i64 + 40)
            .map(|i| sess(i, SessionState::Idle, 0.0))
            .collect();
        let (rows, rest) = render_slice(&long);
        assert_eq!((rows.len(), rest), (RENDER_CAP, 40));
        // The front of an already-ordered list, not an arbitrary page.
        assert_eq!(rows[0].id, 0);
        assert_eq!(rows[RENDER_CAP - 1].id, RENDER_CAP as i64 - 1);

        // Exactly full is not over-full.
        let exact: Vec<SessionInfo> = (0..RENDER_CAP as i64)
            .map(|i| sess(i, SessionState::Idle, 0.0))
            .collect();
        assert_eq!(render_slice(&exact).1, 0);
        assert_eq!(render_slice::<SessionInfo>(&[]).1, 0);
    }

    #[test]
    fn a_dangling_blocked_by_edge_survives_prepare() {
        // The holder isn't in the snapshot (killed between the two queries, or
        // invisible to this account). The waiter must still render.
        let mut v = vec![{
            let mut s = sess(1, SessionState::Blocked, 5.0);
            s.blocked_by = vec![999];
            s
        }];
        assert!(!prepare(&mut v));
        assert_eq!(
            v[0].note().as_deref(),
            Some("waiting on a lock held by 999")
        );
        assert!(
            lock_wait(&v).is_none(),
            "no banner for a holder we can't show"
        );
    }

    #[test]
    fn summarize_counts_each_row_once() {
        let v = vec![
            sess(1, SessionState::Blocked, 0.0),
            sess(2, SessionState::IdleInTx, 0.0),
            sess(3, SessionState::IdleInTx, 0.0),
            sess(4, SessionState::Running, 0.0),
            sess(5, SessionState::Idle, 0.0),
        ];
        let s = summarize(&v);
        assert_eq!(
            s,
            ActivitySummary {
                total: 5,
                running: 1,
                blocked: 1,
                idle_in_tx: 2,
            }
        );
        assert_eq!(s.total_label(false), "5 sessions");
        assert_eq!(summarize(&v[..1]).total_label(false), "1 session");
    }

    #[test]
    fn blocked_is_in_a_transaction_but_is_not_counted_as_idle_in_one() {
        assert!(SessionState::Blocked.in_transaction());
        assert!(SessionState::IdleInTx.in_transaction());
        assert!(!SessionState::Running.in_transaction());
        assert!(!SessionState::Idle.in_transaction());
        let v = vec![sess(1, SessionState::Blocked, 0.0)];
        assert_eq!(summarize(&v).idle_in_tx, 0);
    }

    #[test]
    fn search_reaches_every_field_the_row_shows() {
        let s = sess(1148, SessionState::IdleInTx, 0.0);
        for q in [
            "1148",
            "app@10",
            "10.0.0.1",
            "employees",
            "idle in txn",
            "select",
        ] {
            assert!(matches_query(&s, q), "{q:?} should match");
        }
        assert!(
            matches_query(&s, "   "),
            "an empty filter matches everything"
        );
        assert!(!matches_query(&s, "nothing-like-this"));
    }

    #[test]
    fn lock_wait_picks_the_longest_wait_and_names_the_holder() {
        let mut v = vec![
            {
                let mut s = sess(1, SessionState::Blocked, 3.0);
                s.blocked_by = vec![3];
                s
            },
            {
                let mut s = sess(2, SessionState::Blocked, 90.0);
                s.blocked_by = vec![3];
                s
            },
            sess(3, SessionState::IdleInTx, 252.0),
        ];
        prepare(&mut v);
        let (waiter, holder) = lock_wait(&v).expect("a wait to report");
        assert_eq!((waiter.id, holder.id), (2, 3));
        let text = lock_wait_text(waiter, holder);
        assert!(text.contains("Session 2"), "{text}");
        assert!(text.contains("1m 30s"), "{text}");
        assert!(text.contains("session 3"), "{text}");
        assert!(
            text.contains("idle in transaction for 4m 12s"),
            "the holder's state is the point: {text}"
        );
    }

    /// **A wait with several holders is ordinary**, and naming one of them as
    /// *the* holder invited a kill that left the waiter blocked by the other
    /// two, with the banner immediately re-rendering the next-lowest id. The
    /// holder is chosen the way the waiter is — longest-standing — and the rest
    /// are counted out loud.
    #[test]
    fn a_wait_with_several_holders_says_so() {
        let mut v = vec![
            sess(1, SessionState::IdleInTx, 10.0),
            sess(2, SessionState::IdleInTx, 400.0),
            sess(3, SessionState::IdleInTx, 90.0),
            sess(9, SessionState::Blocked, 130.0),
        ];
        v[3].blocked_by = vec![3, 1, 2];
        prepare(&mut v);
        let (waiter, holder) = lock_wait(&v).expect("a wait to report");
        assert_eq!(waiter.id, 9);
        assert_eq!(
            holder.id, 2,
            "the longest-standing holder, not the lowest id"
        );
        let text = lock_wait_text(waiter, holder);
        assert!(text.contains("2 other sessions"), "{text}");

        // One holder says nothing extra. Rebuilt rather than mutated: `prepare`
        // reorders the vector, so the indices above no longer mean what they did.
        let mut v = vec![
            sess(2, SessionState::IdleInTx, 400.0),
            sess(9, SessionState::Blocked, 130.0),
        ];
        v[1].blocked_by = vec![2];
        prepare(&mut v);
        let (w, h) = lock_wait(&v).expect("a wait to report");
        let text = lock_wait_text(w, h);
        assert!(!text.contains("other"), "{text}");
    }

    #[test]
    fn no_wait_no_banner() {
        let mut v = vec![
            sess(1, SessionState::Running, 3.0),
            sess(2, SessionState::IdleInTx, 90.0),
        ];
        prepare(&mut v);
        assert!(lock_wait(&v).is_none());
    }

    /// **The search reads what the row draws.** `PROCESSLIST.INFO` keeps the
    /// client's original newlines and the row renders `history::preview`, which
    /// collapses them — so typing the phrase that is visibly on screen filtered
    /// the one row containing it *out*. `history::matches_query` solved this one
    /// module over; this one was written without that half.
    #[test]
    fn the_search_reads_the_statement_the_way_the_row_draws_it() {
        let mut s = sess(1, SessionState::Running, 1.0);
        s.sql = Some("SELECT *\nFROM orders\nWHERE id = 7".to_string());
        assert!(matches_query(&s, "select * from"));
        assert!(matches_query(&s, "FROM ORDERS WHERE"));
        // And a term that really isn't there still isn't.
        assert!(!matches_query(&s, "delete from"));
    }

    /// The two engines answer the panel's own question differently:
    /// `PROCESSLIST.INFO` is NULL for a `Sleep` thread, while
    /// `pg_stat_activity.query` keeps the last statement of an `idle` backend
    /// indefinitely — so a wall of PostgreSQL pool connections each drew a real,
    /// plausible statement on the panel whose one decision is what to kill.
    #[test]
    fn an_idle_session_is_not_running_the_statement_it_last_ran() {
        let mut idle = sess(1, SessionState::Idle, 400.0);
        idle.sql = Some("SELECT 1234 AS probe_marker".to_string());
        assert_eq!(idle.running_sql(), None);
        // Still available for *Copy statement*, which is why the field stays.
        assert!(idle.sql.is_some());

        for state in [
            SessionState::Running,
            SessionState::Blocked,
            SessionState::IdleInTx,
        ] {
            let mut s = sess(2, state, 1.0);
            s.sql = Some("UPDATE orders SET total = 1".to_string());
            assert!(s.running_sql().is_some(), "{state:?}");
        }
    }

    /// The poll is a full connect + authenticate + three `information_schema`
    /// statements, as often as every two seconds. `fits` is the answer that was
    /// missing: below the responsive breakpoint the right column is 0px wide and
    /// its toggle is inert, so the poll could be neither seen nor stopped.
    #[test]
    fn the_poll_stops_when_the_panel_cannot_be_seen() {
        assert!(should_poll(true, true, true));
        assert!(!should_poll(false, true, true));
        assert!(!should_poll(true, false, true));
        assert!(!should_poll(true, true, false));
    }

    // ── The folds (the seam between a server's answer and `SessionInfo`) ──

    fn my_row(id: i64, command: &str) -> MyProcessRow {
        MyProcessRow {
            id,
            user: "app".to_string(),
            host: "10.0.4.12:54388".to_string(),
            database: Some("shop".to_string()),
            command: command.to_string(),
            seconds: 12,
            info: Some("SELECT 1".to_string()),
        }
    }

    /// Every decision the MySQL fold makes, in one place — none of it was
    /// reachable from a test while it sat inside an `async fn` holding a
    /// connection, which is where the first three findings of this pass lived.
    #[test]
    fn the_mysql_fold_decides_each_field_from_the_three_result_sets() {
        let trx: std::collections::HashMap<i64, String> =
            [(2, "RUNNING".to_string()), (3, "LOCK WAIT".to_string())]
                .into_iter()
                .collect();
        let mut rows = vec![
            my_row(1, "Sleep"),
            my_row(2, "Sleep"),
            my_row(3, "Query"),
            my_row(4, "Query"),
        ];
        // A `Sleep` thread's `INFO` is NULL on this engine.
        rows[0].info = None;
        rows[1].info = None;
        // Socket connections report a bare `localhost`; an empty host is no
        // address at all; an empty `DB` is not a database; a whitespace-only
        // `INFO` is not a statement; a negative `TIME` clamps before it sorts.
        rows[2].host = "localhost".to_string();
        rows[2].database = Some(String::new());
        rows[3].host = String::new();
        rows[3].info = Some("   ".to_string());
        rows[3].seconds = -5;

        // The same edge twice — both lock views are keyed by lock, not by pair.
        let out = from_mysql_rows(&rows, &trx, &[(3, 2), (3, 2)]);
        assert_eq!(out.len(), 4);

        // A plain `Sleep` with no transaction is idle; with one it is the
        // holder the panel exists to surface.
        assert_eq!(out[0].state, SessionState::Idle);
        assert_eq!(out[1].state, SessionState::IdleInTx);
        assert_eq!(out[2].state, SessionState::Blocked);
        assert_eq!(out[3].state, SessionState::Running);

        assert_eq!(out[0].client.as_deref(), Some("10.0.4.12"));
        assert_eq!(out[2].client.as_deref(), Some("localhost"));
        assert_eq!(out[3].client, None);
        assert_eq!(out[2].database, None);
        assert_eq!(out[3].sql, None);
        assert_eq!(out[3].seconds, 0.0);
        // Duplicates survive here; `prepare` is the one place they are dropped.
        assert_eq!(out[2].blocked_by, vec![2, 2]);
    }

    fn pg_row(pid: &str) -> PgActivityRow {
        [
            Some(pid.to_string()),
            Some("app".to_string()),
            Some("10.0.0.4".to_string()),
            Some("shop".to_string()),
            Some("active".to_string()),
            Some("SELECT 1".to_string()),
            Some("12.5".to_string()),
            Some("{}".to_string()),
        ]
    }

    /// **A masked backend is kept, and does not claim to be running.**
    /// PostgreSQL nulls `usename`/`datname`/`state`/`client_addr` for a backend
    /// the role may not inspect and puts `<insufficient privilege>` in `query`,
    /// while `pid` and `pg_blocking_pids(pid)` stay real — so the row is still a
    /// killable session and a usable node in the graph, but its "statement" is a
    /// message and must not be drawn as one.
    #[test]
    fn the_pg_fold_keeps_a_masked_backend_without_inventing_anything() {
        let mut masked = pg_row("41207");
        masked[1] = None;
        masked[2] = None;
        masked[3] = None;
        masked[4] = None;
        masked[5] = Some(PG_MASKED_QUERY.to_string());
        masked[6] = None;
        masked[7] = Some("{4102}".to_string());

        let mut unparseable = pg_row("not-a-pid");
        unparseable[7] = Some("{}".to_string());

        let out = from_pg_rows(&[pg_row("100"), masked, unparseable]);
        // The row with no readable identity is dropped; a session `0` would
        // render a full row with a live "Kill session" under it.
        assert_eq!(out.len(), 2);

        assert_eq!(out[0].state, SessionState::Running);
        assert_eq!(out[0].seconds, 12.5);

        let m = &out[1];
        assert_eq!(m.id, 41207);
        assert_eq!(m.sql, None, "the sentinel is not a statement");
        assert_eq!(m.user, "");
        assert_eq!(m.client, None);
        assert_eq!(m.database, None);
        assert_eq!(m.blocked_by, vec![4102], "the graph is not masked");
        // Blocked because the graph says so — not `Running` off a sentinel.
        assert_eq!(m.state, SessionState::Blocked);
        // …and with no blocker it would be the far more common idle backend.
        let mut alone = pg_row("41207");
        alone[4] = None;
        alone[5] = Some(PG_MASKED_QUERY.to_string());
        assert_eq!(from_pg_rows(&[alone])[0].state, SessionState::Idle);
    }

    #[test]
    fn parse_pid_array_reads_what_pg_blocking_pids_renders() {
        assert_eq!(parse_pid_array("{}"), Vec::<i64>::new());
        assert_eq!(parse_pid_array("{1234}"), vec![1234]);
        assert_eq!(parse_pid_array("{1234,5678}"), vec![1234, 5678]);
        assert_eq!(parse_pid_array(" {1234, 5678} "), vec![1234, 5678]);
        // A NULL column, or anything else unexpected, contributes no edges rather
        // than a pid of 0.
        assert_eq!(parse_pid_array(""), Vec::<i64>::new());
        assert_eq!(parse_pid_array("{NULL}"), Vec::<i64>::new());
        assert_eq!(parse_pid_array("{7,oops}"), vec![7]);
    }

    #[test]
    fn cancel_is_only_offered_where_there_is_a_statement() {
        let running = sess(1, SessionState::Running, 0.0);
        assert!(KillKind::Query.applies_to(&running));
        assert!(KillKind::Session.applies_to(&running));

        let mut idle = sess(2, SessionState::Idle, 0.0);
        idle.sql = None;
        assert!(!KillKind::Query.applies_to(&idle), "nothing to cancel");
        assert!(
            KillKind::Session.applies_to(&idle),
            "an idle session can always be dropped"
        );

        // The case the panel exists for: an idle-in-transaction holder has no
        // statement running but must stay killable.
        //
        // **And must not be offered a cancel.** PostgreSQL keeps the *last*
        // statement in `query` for such a backend, so `sql` is `Some` and the
        // row offered "Cancel query" over a confirm promising an abort — which
        // `pg_cancel_backend` discards while the backend is reading a command.
        // MySQL escaped this by luck: `PROCESSLIST.INFO` is NULL for a `Sleep`
        // thread, so `sql` was already `None` there.
        let mut holder = sess(3, SessionState::IdleInTx, 300.0);
        holder.sql = Some("UPDATE salaries SET salary = 0".to_string());
        assert!(KillKind::Session.applies_to(&holder));
        assert!(!KillKind::Query.applies_to(&holder));

        // A blocked session *is* executing — it is waiting inside a statement,
        // and cancelling it is the whole point of the panel.
        let mut blocked = sess(5, SessionState::Blocked, 12.0);
        blocked.sql = Some("UPDATE orders SET total = 1".to_string());
        assert!(KillKind::Query.applies_to(&blocked));

        // A server that still reports the last statement of an idle session must
        // not turn that into a cancel button.
        let mut stale = sess(4, SessionState::Idle, 51.0);
        stale.sql = Some("SELECT 1".to_string());
        assert!(!KillKind::Query.applies_to(&stale));
    }

    #[test]
    fn kill_confirm_names_the_consequence_of_the_kind_chosen() {
        let s = sess(1148, SessionState::IdleInTx, 300.0);
        let (title, body) = kill_confirm(KillKind::Session, &s, SqlDialect::MySql);
        assert_eq!(title, "Kill session 1148");
        assert!(body.contains("app@10.0.0.1"), "{body}");
        assert!(body.contains("on employees"), "{body}");
        assert!(body.contains("rolled back"), "{body}");

        let (title, body) = kill_confirm(KillKind::Query, &s, SqlDialect::MySql);
        assert_eq!(title, "Cancel query on session 1148");
        assert!(body.contains("stays connected"), "{body}");
        assert!(!body.contains("rolled back"), "cancelling does not: {body}");
    }

    /// `pg_cancel_backend` leaves the transaction open but **aborted** — every
    /// later statement fails until a rollback, so the uncommitted work is gone
    /// exactly as under a terminate. Promising a PostgreSQL user their
    /// transaction survives a cancel is the one sentence here that can cost them
    /// something.
    #[test]
    fn cancelling_says_what_it_costs_on_each_engine() {
        let s = sess(1148, SessionState::Running, 30.0);

        let (_, my) = kill_confirm(KillKind::Query, &s, SqlDialect::MySql);
        assert!(my.contains("stays open"), "{my}");
        assert!(
            !my.contains("aborted"),
            "MySQL's transaction survives: {my}"
        );

        let (_, pg) = kill_confirm(KillKind::Query, &s, SqlDialect::Postgres);
        assert!(pg.contains("aborted"), "{pg}");
        assert!(pg.contains("roll it back"), "{pg}");
        assert!(
            !pg.contains("stays open"),
            "open-but-poisoned is not 'stays open': {pg}"
        );

        // Terminating means the same thing on both, so its sentence doesn't move.
        let (_, a) = kill_confirm(KillKind::Session, &s, SqlDialect::MySql);
        let (_, b) = kill_confirm(KillKind::Session, &s, SqlDialect::Postgres);
        assert_eq!(a, b);
    }

    #[test]
    fn kill_confirm_omits_a_database_the_session_has_none_of() {
        let mut s = sess(7, SessionState::Idle, 0.0);
        s.database = None;
        let (_, body) = kill_confirm(KillKind::Session, &s, SqlDialect::MySql);
        assert!(!body.contains(" on "), "{body}");
        s.database = Some(String::new());
        let (_, body) = kill_confirm(KillKind::Session, &s, SqlDialect::MySql);
        assert!(
            !body.contains(" on "),
            "an empty name is not a database: {body}"
        );
    }

    #[test]
    fn mysql_sleep_with_an_open_transaction_is_not_idle() {
        assert_eq!(mysql_state("Sleep", None), SessionState::Idle);
        assert_eq!(
            mysql_state("Sleep", Some("RUNNING")),
            SessionState::IdleInTx,
            "the whole reason the transaction view is joined in"
        );
        assert_eq!(mysql_state("Query", None), SessionState::Running);
        assert_eq!(
            mysql_state("Execute", Some("RUNNING")),
            SessionState::Running
        );
        assert_eq!(
            mysql_state("Query", Some("LOCK WAIT")),
            SessionState::Blocked
        );
        assert_eq!(
            mysql_state("Sleep", Some("lock wait")),
            SessionState::Blocked,
            "the state word is the server's, so match it loosely"
        );
        assert_eq!(mysql_state(" sleep ", None), SessionState::Idle);
    }

    #[test]
    fn pg_states_map_including_the_aborted_transaction() {
        assert_eq!(pg_state(Some("active"), true, false), SessionState::Running);
        assert_eq!(pg_state(Some("idle"), false, false), SessionState::Idle);
        assert_eq!(
            pg_state(Some("idle in transaction"), true, false),
            SessionState::IdleInTx
        );
        assert_eq!(
            pg_state(Some("idle in transaction (aborted)"), true, false),
            SessionState::IdleInTx
        );
        assert_eq!(
            pg_state(Some("active"), true, true),
            SessionState::Blocked,
            "PostgreSQL reports a lock wait as an active backend; the graph promotes it"
        );
        assert_eq!(
            pg_state(Some("idle"), false, true),
            SessionState::Blocked,
            "the blocking graph wins over any state"
        );
    }

    #[test]
    fn an_unreadable_pg_backend_falls_back_on_whether_it_has_a_statement() {
        assert_eq!(pg_state(None, false, false), SessionState::Idle);
        assert_eq!(pg_state(None, true, false), SessionState::Running);
        assert_eq!(
            pg_state(Some("fastpath function call"), true, false),
            SessionState::Running
        );
        assert_eq!(pg_state(Some("disabled"), false, false), SessionState::Idle);
    }

    #[test]
    fn strip_port_drops_a_port_and_keeps_an_address() {
        assert_eq!(strip_port("10.0.4.12:54388"), "10.0.4.12");
        assert_eq!(strip_port("localhost:58168"), "localhost");
        assert_eq!(strip_port("127.0.0.1"), "127.0.0.1");
        assert_eq!(strip_port("  db.internal:3306 "), "db.internal");
        assert_eq!(strip_port("[::1]:3306"), "[::1]");
        assert_eq!(
            strip_port("::1"),
            "::1",
            "a bare IPv6 address keeps every group"
        );
        assert_eq!(
            strip_port("fe80::1"),
            "fe80::1",
            "the last group is not a port just because it parses as one"
        );
        assert_eq!(strip_port("host:notaport"), "host:notaport");
        assert_eq!(strip_port(""), "");
    }

    #[test]
    fn format_age_scales_from_seconds_to_days() {
        assert_eq!(format_age(0.0), "0s");
        assert_eq!(format_age(12.44), "12s");
        assert_eq!(format_age(59.9), "59s");
        assert_eq!(format_age(60.0), "1m 00s");
        assert_eq!(format_age(252.0), "4m 12s");
        assert_eq!(format_age(3599.0), "59m 59s");
        assert_eq!(format_age(3600.0), "1h 00m");
        assert_eq!(format_age(86_399.0), "23h 59m");
        assert_eq!(format_age(86_400.0), "1d 00h");
        assert_eq!(format_age(200_000.0), "2d 07h");
    }

    #[test]
    fn format_age_refuses_to_render_a_nonsense_clock() {
        // Clock skew between the server's `now()` and a row's start timestamp can
        // hand us a negative age; NaN comes from a missing timestamp parsed loose.
        assert_eq!(format_age(-5.0), "0s");
        assert_eq!(format_age(f64::NAN), "0s");
        assert_eq!(format_age(f64::INFINITY), "0s");
    }

    #[test]
    fn an_interval_is_remembered_per_connection() {
        let mut rules: Vec<IntervalRule> = Vec::new();
        assert_eq!(
            interval_for(&rules, 1),
            DEFAULT_POLL_SECS,
            "a connection nobody has set polls at the default"
        );

        set_interval(&mut rules, 1, 2);
        set_interval(&mut rules, 2, 0);
        assert_eq!(interval_for(&rules, 1), 2);
        assert_eq!(
            interval_for(&rules, 2),
            0,
            "off is a choice, not an absence"
        );
        assert_eq!(interval_for(&rules, 3), DEFAULT_POLL_SECS, "untouched");

        // Re-choosing replaces rather than appending — two rules for one
        // connection would make the answer depend on scan order.
        set_interval(&mut rules, 1, 30);
        assert_eq!(interval_for(&rules, 1), 30);
        assert_eq!(rules.len(), 2);

        // The default is stored like any other choice.
        set_interval(&mut rules, 4, DEFAULT_POLL_SECS);
        assert!(rules.iter().any(|r| r.conn_id == 4));

        clear_conn(&mut rules, 1);
        assert_eq!(interval_for(&rules, 1), DEFAULT_POLL_SECS);
        assert_eq!(interval_for(&rules, 2), 0, "a neighbour is untouched");
    }

    #[test]
    fn a_hand_edited_interval_is_clamped_on_both_sides() {
        // On the way in…
        let mut rules = Vec::new();
        set_interval(&mut rules, 1, 7);
        assert_eq!(rules[0].secs, DEFAULT_POLL_SECS);
        // …and on the way out, for a file this build never wrote.
        let rogue = vec![IntervalRule {
            conn_id: 9,
            secs: 1,
        }];
        assert_eq!(interval_for(&rogue, 9), DEFAULT_POLL_SECS);
    }

    #[test]
    fn the_interval_picker_covers_every_persisted_value() {
        assert_eq!(interval_label(0), "Off");
        assert_eq!(interval_label(30), "30s");
        assert!(POLL_INTERVALS.contains(&DEFAULT_POLL_SECS));
        for secs in POLL_INTERVALS {
            assert_eq!(sane_interval(secs), secs);
        }
        assert_eq!(sane_interval(7), DEFAULT_POLL_SECS, "not on the menu");
        assert_eq!(sane_interval(u64::MAX), DEFAULT_POLL_SECS);
    }
}
