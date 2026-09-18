//! Which query tab is active, given that tabs belong to connections.
//!
//! A tab carries its own connection, and the tab strip shows only the active
//! connection's tabs — so every "which tab now?" question (switching
//! connections, closing a tab, cycling with Next/Previous) has to be answered
//! within one connection rather than across the whole list. These are those
//! rules, pure and tested; the UI passes `(tab id, connection id)` pairs in
//! display order and applies the answer.

/// One tab, reduced to what selection cares about.
pub type TabRef = (usize, u64);

/// The ids on `conn`, in display order — what the strip renders.
pub fn visible(tabs: &[TabRef], conn: u64) -> Vec<usize> {
    tabs.iter()
        .filter(|(_, c)| *c == conn)
        .map(|(id, _)| *id)
        .collect()
}

/// The tab to activate when `conn` becomes the active connection.
///
/// Prefers `remembered` — where the user last was on this connection — so
/// switching away and back doesn't dump them on tab 1. Falls back to the first
/// tab, and `None` when the connection has none at all (the caller then opens a
/// fresh one; there is no empty-editor state).
pub fn pick_active(tabs: &[TabRef], conn: u64, remembered: Option<usize>) -> Option<usize> {
    let ids = visible(tabs, conn);
    match remembered {
        Some(id) if ids.contains(&id) => Some(id),
        _ => ids.first().copied(),
    }
}

/// The tab to activate after closing `closing`: the one after it on the same
/// connection, else the one before. `None` when it was that connection's last
/// tab.
///
/// Scoped deliberately — the neighbour in the *full* list can belong to another
/// connection, which would silently switch what the user is looking at.
pub fn neighbor(tabs: &[TabRef], closing: usize) -> Option<usize> {
    let conn = tabs
        .iter()
        .find(|(id, _)| *id == closing)
        .map(|(_, c)| *c)?;
    let ids = visible(tabs, conn);
    let at = ids.iter().position(|id| *id == closing)?;
    ids.get(at + 1)
        .or_else(|| at.checked_sub(1).and_then(|p| ids.get(p)))
        .copied()
}

/// Would closing `id` leave its connection with no tabs?
///
/// This is the "keep at least one tab" test, and it is deliberately **per
/// connection**: the strip shows one connection at a time, so emptying that
/// connection leaves the user staring at nothing however many tabs other
/// connections hold. An unknown `id` answers `true` — there's nothing to close,
/// and the caller's keep-one path bails harmlessly.
pub fn closing_would_empty(tabs: &[TabRef], id: usize) -> bool {
    match tabs.iter().find(|(t, _)| *t == id).map(|(_, c)| *c) {
        Some(conn) => visible(tabs, conn).len() <= 1,
        None => true,
    }
}

/// Step `step` tabs from `current` within `conn`, wrapping at both ends.
///
/// A `current` that isn't on this connection (or no current at all) starts from
/// the first tab, so cycling can't land on something the strip isn't showing.
pub fn cycle(tabs: &[TabRef], conn: u64, current: Option<usize>, step: isize) -> Option<usize> {
    let ids = visible(tabs, conn);
    if ids.is_empty() {
        return None;
    }
    let at = current
        .and_then(|c| ids.iter().position(|id| *id == c))
        .unwrap_or(0) as isize;
    let n = ids.len() as isize;
    let next = ((at + step) % n + n) % n;
    ids.get(next as usize).copied()
}

/// The `n`th (0-based) tab of `conn`, as the strip shows them.
///
/// Ctrl+1..9 means "the nth chip", and the chips are the *visible* tabs — so
/// counting into the flat list picks a different tab whenever one belonging to
/// another connection precedes it.
pub fn nth(tabs: &[TabRef], conn: u64, n: usize) -> Option<usize> {
    visible(tabs, conn).get(n).copied()
}

/// One tab as the **closing** rules see it: id, connection, and whether it is
/// pinned. Wider than [`TabRef`] because a pinned tab is visible and selectable
/// but not closable.
pub type ClosableRef = (usize, u64, bool);

/// Can `id` be closed at all?
///
/// False for a pinned tab, and false for an unknown one — there is nothing there
/// to close, and both callers want the same answer for it.
///
/// This exists because the answer is needed **before** anything is asked, not
/// only before the close happens. The app's close path guards a close with two
/// questions — unsaved `.sql` edits, and an open transaction — and the
/// transaction one is not a question but an action: answering it commits or rolls
/// back. The pinned test used to sit only at the far end, in the app's
/// `close_tab_now`, so Ctrl+W on a pinned tab holding a transaction prompted,
/// took the commit, and *then* declined to close: a transaction settled for a
/// close that could never have happened, with no way back.
///
/// [`all_to_close`] is the set form of the same rule, and
/// `all_to_close_is_every_closable_tab` holds the two to it.
pub fn can_close(tabs: &[ClosableRef], id: usize) -> bool {
    tabs.iter().any(|(i, _, pinned)| *i == id && !*pinned)
}

/// The tabs "Close all tabs" would close on `conn`: its unpinned ones.
pub fn all_to_close(tabs: &[ClosableRef], conn: u64) -> Vec<usize> {
    tabs.iter()
        .filter(|(_, c, pinned)| *c == conn && !*pinned)
        .map(|(id, _, _)| *id)
        .collect()
}

/// The tabs "Close other tabs" would close: [`all_to_close`]'s set, less `keep`.
///
/// The **same expression the menu entry has to dim on**, which is why it is
/// here. The action returned early on an empty set — no dialog, no message —
/// while the entry directly above it (`Reopen last tab`) *is* dimmed for exactly
/// this reason, so on the app's opening state (one tab) the two rows behaved
/// differently for the same kind of reason.
///
/// `keep` is offered on a pinned tab too: a pinned tab is already the one that
/// survives everything, so "close the others" is exactly as meaningful there.
pub fn others_to_close(tabs: &[ClosableRef], conn: u64, keep: usize) -> Vec<usize> {
    all_to_close(tabs, conn)
        .into_iter()
        .filter(|id| *id != keep)
        .collect()
}

/// Does picking `picked` in the database selector actually **move** the tab?
///
/// Three ways the answer is no, and the app asked only the first:
///
/// - `picked` is not one of the databases the selector lists, so there is
///   nothing to bind to;
/// - the tab is already on `picked`, on the connection the pick is made
///   against — the selector *offers* this, because it renders the current row
///   accented rather than disabled, unlike every other already-in-that-state
///   entry in the app.
///
/// **Re-picking the row you are on is not free.** The rebind cancels the tab's
/// in-flight query (a 30-second `SELECT` goes `Cancelled` with nothing saying
/// why, and the database has not changed), it raises the
/// Commit/Rollback/Cancel prompt on a Manual tab with an uncommitted `INSERT` —
/// where answering Rollback discards the user's transaction for a move that is
/// not one — and either answer then drops and re-opens the pinned connection.
/// The sibling action that also settles a transaction and re-pins a session,
/// `set_tx_mode`, opens with exactly this refusal.
///
/// **The connection is half the question.** A tab keeps the connection it was
/// opened on, and the rebind writes `conn_id` as well as `database`, so a tab
/// naming `world` on another connection still has somewhere to move to.
pub fn rebind_needed(
    tab: Option<(u64, Option<&str>)>,
    active_conn: u64,
    picked: &str,
    known: &[String],
) -> bool {
    if !known.iter().any(|n| n == picked) {
        return false;
    }
    tab != Some((active_conn, Some(picked)))
}

/// The database a question about "the current tab" should be answered with:
/// the focused tab's, but **only when that tab is on `active_conn`** —
/// otherwise `fallback`, which the caller has already scoped to the active
/// connection.
///
/// Switching connections does not move the focused tab, and a tab keeps the
/// connection it was opened on, so the focused tab routinely names a database
/// that exists somewhere else. Handing that name to the active connection's
/// `Db` is how the MCP endpoint ended up asking MariaDB for `chinook`.
///
/// **Here, and not in each caller, because the callers are not all harmless.**
/// Four ask this question — the AI's turn context, the terminal's DB-CLI
/// button, the AI proposal card, and the chat code block's Insert/Run — and the
/// third pairs the answer with
/// `edit_ctx`'s *active* connection and stamps that `conn_id` into the plan
/// `run_ddl` executes: getting it wrong runs an `ALTER` on prod against a
/// proposal written about dev. That one had the rule spelled out inline,
/// expression for expression, with no test, because `schemaic-ui` cannot depend
/// on `schemaic-app` — which made it a misplaced function rather than an
/// unavoidable duplicate.
pub fn scoped_database(
    tab: Option<(u64, Option<String>)>,
    active_conn: u64,
    fallback: Option<&str>,
) -> Option<String> {
    match tab_scope(tab, active_conn) {
        TabScope::Bound(database) => Some(database),
        TabScope::NoDatabase | TabScope::OtherConnection => fallback.map(str::to_string),
    }
}

/// Why [`scoped_database`] answered as it did — the two different `None`s, kept
/// apart.
///
/// **Because one caller has to tell them apart and cannot.** `scoped_database`
/// collapses "the focused tab is on another connection" and "the focused tab is
/// on this one and has no database" into a single `None`, which is right for
/// its own question — both mean "do not use the focused tab's database" — and
/// wrong for a caller whose next act is to *explain the refusal to the user*.
/// The chat code block's Insert/Run raised "This chat is about a tab on a
/// different connection… switch to that tab" for a tab on the same connection
/// whose databases are all hidden, or whose schema had not finished loading
/// when it was opened: the first sentence false, and the remedy naming the tab
/// the user was already on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabScope {
    /// The focused tab is on the active connection and names a database.
    Bound(String),
    /// The focused tab is on the active connection and has no database bound —
    /// `schema::tab_target` answered `None` when it was opened.
    NoDatabase,
    /// The focused tab belongs to another connection, or there is no focused
    /// tab. A new tab would open on the connection selected in the tree, which
    /// is not the one this question is about.
    OtherConnection,
}

/// Which of [`TabScope`]'s three states the focused tab is in.
pub fn tab_scope(tab: Option<(u64, Option<String>)>, active_conn: u64) -> TabScope {
    match tab {
        Some((conn_id, _)) if conn_id != active_conn => TabScope::OtherConnection,
        Some((_, Some(database))) => TabScope::Bound(database),
        Some((_, None)) => TabScope::NoDatabase,
        None => TabScope::OtherConnection,
    }
}

/// Where a tab enters the strip so that the pinned block stays contiguous at
/// the left edge: the number of leading pinned tabs.
///
/// **Leading**, not total — a pinned tab sitting to the right of an unpinned one
/// does not extend the block, because the block is what the left edge already
/// holds and the whole point is to put the newcomer at its far side.
///
/// `pinned` is the strip **as it will be at the moment of insertion**, which for
/// a re-pin means after the tab has been taken out. Correct both ways round: a
/// newly pinned tab lands just after the existing pinned ones, and a newly
/// unpinned one lands in the first unpinned slot.
///
/// This was spelled out twice in `app_view` — `take_while(…).count()` in the
/// pin toggle and again in the duplicate, with the second layering a clamp on
/// top. Two assemblies that happen to agree are two rules nobody is comparing,
/// and neither could be tested where it stood.
pub fn pinned_boundary(pinned: &[bool]) -> usize {
    pinned.iter().take_while(|p| **p).count()
}

/// Where a duplicate of the tab at `source` goes: immediately after it, but
/// never inside the pinned block.
///
/// A duplicate is always unpinned, so duplicating a pinned tab cannot put it at
/// `source + 1` — that slot is inside the block, and the invariant
/// [`pinned_boundary`] exists for would break. `None` means the source is no
/// longer in the strip, which puts the duplicate at the end.
pub fn duplicate_slot(pinned: &[bool], source: Option<usize>) -> usize {
    source
        .map(|i| i + 1)
        .unwrap_or(pinned.len())
        .max(pinned_boundary(pinned))
}

/// Whether the reopen-closed-tab ring holds anything for `conn`.
///
/// The ring spans connections and reopening does not, so the menu entry has to
/// ask the same per-connection question the action applies rather than "is the
/// ring empty" — otherwise it offers a click that does nothing whenever the
/// last closed tab was on another connection.
pub fn has_reopenable(closed: &[u64], conn: u64) -> bool {
    closed.contains(&conn)
}

/// Whether a freshly-built tab may replace the active one **in place** rather
/// than opening beside it — the "app opened on an empty Query 1" case.
///
/// All four have to hold, and the fourth is the one that is not about
/// emptiness: a tab bound to a `.sql` file is not a blank slate even when the
/// file is empty, because reusing it drops the binding silently and the next
/// Ctrl+S goes somewhere else.
pub fn is_blank_slate(pinned: bool, query: &str, results_untouched: bool, has_path: bool) -> bool {
    !pinned && query.trim().is_empty() && results_untouched && !has_path
}

/// Does a tab being **closed** carry anything the reopen ring should keep — a
/// query, a table source, a name the user gave it, or a file binding?
///
/// The write side of the same ring [`has_reopenable`] reads, and deliberately
/// **not** `!is_blank_slate`: that one asks whether a tab may be *reused in
/// place*, over a different set of four terms. It weighs `pinned` (a pinned tab
/// is never closed by this path anyway, so it says nothing here) and
/// `results_untouched` (a scrolled result is not work worth restoring — the
/// reopened tab re-runs the query), while this one has to weigh `source` and
/// `name`, which that question does not ask about at all. Two predicates
/// spelled as one negation of the other would have to answer for six terms
/// between them, and each would carry two it does not mean.
///
/// A file-backed tab is worth restoring even when the file is empty, for the
/// reason `is_blank_slate`'s fourth term gives: the binding to the path is the
/// thing being lost.
pub fn worth_remembering(query: &str, has_source: bool, has_name: bool, has_path: bool) -> bool {
    !query.trim().is_empty() || has_source || has_name || has_path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dbs() -> Vec<String> {
        ["world", "classicmodels"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn picking_a_different_database_rebinds() {
        let tab = Some((7, Some("world")));
        assert!(rebind_needed(tab, 7, "classicmodels", &dbs()));
    }

    /// The one the selector invites: the current row is accented, not disabled.
    #[test]
    fn re_picking_the_database_the_tab_is_already_on_does_nothing() {
        let tab = Some((7, Some("world")));
        assert!(!rebind_needed(tab, 7, "world", &dbs()));
    }

    /// A tab that names the same database on a *different* connection still has
    /// somewhere to go — the rebind writes `conn_id` as well as `database`.
    #[test]
    fn the_same_name_on_another_connection_is_still_a_move() {
        let tab = Some((9, Some("world")));
        assert!(rebind_needed(tab, 7, "world", &dbs()));
    }

    /// A tab bound to no database yet, and a tab that no longer exists.
    #[test]
    fn a_tab_with_no_database_binds() {
        assert!(rebind_needed(Some((7, None)), 7, "world", &dbs()));
        assert!(rebind_needed(None, 7, "world", &dbs()));
    }

    /// The existence check the app already had, kept here so there is one
    /// answer rather than two.
    #[test]
    fn a_database_the_selector_does_not_list_binds_to_nothing() {
        assert!(!rebind_needed(
            Some((7, Some("world"))),
            7,
            "chinook",
            &dbs()
        ));
        // Including when it is the tab's own — a stale name after the list
        // reloaded. There is still nothing to move to.
        assert!(!rebind_needed(
            Some((7, Some("chinook"))),
            7,
            "chinook",
            &dbs()
        ));
        assert!(!rebind_needed(Some((7, Some("world"))), 7, "world", &[]));
    }

    #[test]
    fn scoped_database_takes_the_tab_database_on_the_active_connection() {
        let tab = Some((7, Some("classicmodels".to_string())));
        assert_eq!(
            scoped_database(tab, 7, Some("world")),
            Some("classicmodels".to_string())
        );
    }

    #[test]
    fn scoped_database_ignores_a_tab_from_another_connection() {
        // A tab keeps its own connection, so the active tab can name a database
        // that doesn't exist on the connection the AI is bound to — handing that
        // name over produced `Unknown database 'chinook'` against MariaDB. The
        // active connection's own default stands in.
        let tab = Some((9, Some("chinook".to_string())));
        assert_eq!(
            scoped_database(tab, 7, Some("classicmodels")),
            Some("classicmodels".to_string())
        );
        // …and with no default to fall back on, nothing rather than the wrong
        // connection's database. **Two callers turn this `None` into a
        // refusal**: the AI proposal card, which would otherwise run an `ALTER`
        // on the wrong server, and the chat code block's Insert/Run, which
        // would otherwise open a tab on the *active* connection carrying the
        // focused tab's database name — a dev/prod pair almost always has the
        // same database name on both, so a `DELETE … WHERE status = 'draft'`
        // written about dev ran on prod with nothing on screen naming the
        // server.
        assert_eq!(
            scoped_database(Some((9, Some("chinook".into()))), 7, None),
            None
        );
    }

    #[test]
    fn scoped_database_falls_back_for_no_tab_and_a_server_level_tab() {
        assert_eq!(
            scoped_database(None, 7, Some("world")),
            Some("world".to_string())
        );
        // A tab on this connection but with no database (server-level) also
        // takes the default, as it did before the connection guard.
        assert_eq!(
            scoped_database(Some((7, None)), 7, Some("world")),
            Some("world".to_string())
        );
        assert_eq!(scoped_database(None, 7, None), None);
    }

    /// **The two `None`s `scoped_database` folds together, kept apart.**
    ///
    /// Its `None` means "do not use the focused tab's database", which has two
    /// causes: the tab is on another connection, or it is on this one and has
    /// no database bound. A caller whose next act is to *explain the refusal*
    /// needs the difference, and the chat code block's Insert/Run raised "This
    /// chat is about a tab on a different connection… switch to that tab" for
    /// the second — false, and pointing at the tab the user was already on.
    #[test]
    fn tab_scope_tells_the_two_refusals_apart() {
        assert_eq!(
            tab_scope(Some((7, Some("classicmodels".into()))), 7),
            TabScope::Bound("classicmodels".to_string())
        );
        // On this connection, no database: hidden databases, or a schema that
        // had not loaded when the tab was opened.
        assert_eq!(tab_scope(Some((7, None)), 7), TabScope::NoDatabase);
        // Another connection — with or without a database of its own, and the
        // no-tab case, which a new tab would also open on the tree's selection.
        assert_eq!(
            tab_scope(Some((9, Some("chinook".into()))), 7),
            TabScope::OtherConnection
        );
        assert_eq!(tab_scope(Some((9, None)), 7), TabScope::OtherConnection);
        assert_eq!(tab_scope(None, 7), TabScope::OtherConnection);
    }

    /// `scoped_database` is now a reading of [`tab_scope`], so the two cannot
    /// drift: every state either answers with the tab's database or falls back.
    #[test]
    fn scoped_database_still_answers_exactly_what_the_scope_says() {
        for tab in [
            None,
            Some((7, None)),
            Some((7, Some("classicmodels".to_string()))),
            Some((9, None)),
            Some((9, Some("chinook".to_string()))),
        ] {
            let want = match tab_scope(tab.clone(), 7) {
                TabScope::Bound(db) => Some(db),
                _ => Some("world".to_string()),
            };
            assert_eq!(
                scoped_database(tab.clone(), 7, Some("world")),
                want,
                "{tab:?}"
            );
        }
    }

    /// Tabs interleaved across two connections, as the flat list really is.
    fn mixed() -> Vec<TabRef> {
        vec![(1, 10), (2, 20), (3, 10), (4, 20), (5, 10)]
    }

    #[test]
    fn visible_keeps_one_connections_tabs_in_order() {
        assert_eq!(visible(&mixed(), 10), [1, 3, 5]);
        assert_eq!(visible(&mixed(), 20), [2, 4]);
        assert!(visible(&mixed(), 99).is_empty());
    }

    #[test]
    fn pick_active_prefers_where_the_user_left_off() {
        assert_eq!(pick_active(&mixed(), 10, Some(3)), Some(3));
    }

    #[test]
    fn pick_active_falls_back_when_the_remembered_tab_is_gone_or_foreign() {
        // Closed since.
        assert_eq!(pick_active(&mixed(), 10, Some(99)), Some(1));
        // Remembered against a different connection — must not leak across.
        assert_eq!(pick_active(&mixed(), 10, Some(2)), Some(1));
        // Nothing remembered.
        assert_eq!(pick_active(&mixed(), 20, None), Some(2));
    }

    #[test]
    fn pick_active_is_none_for_a_connection_with_no_tabs() {
        assert_eq!(pick_active(&mixed(), 99, None), None);
        assert_eq!(pick_active(&[], 10, Some(1)), None);
    }

    #[test]
    fn neighbor_takes_the_next_tab_on_the_same_connection() {
        // 1 → 3, skipping tab 2, which belongs to connection 20.
        assert_eq!(neighbor(&mixed(), 1), Some(3));
        assert_eq!(neighbor(&mixed(), 3), Some(5));
    }

    #[test]
    fn neighbor_falls_back_to_the_previous_tab_at_the_end() {
        assert_eq!(neighbor(&mixed(), 5), Some(3));
        assert_eq!(neighbor(&mixed(), 4), Some(2));
    }

    #[test]
    fn neighbor_is_none_for_a_connections_last_tab_or_an_unknown_id() {
        assert_eq!(neighbor(&[(1, 10), (2, 20)], 1), None);
        assert_eq!(neighbor(&mixed(), 99), None);
    }

    #[test]
    fn closing_would_empty_is_per_connection_not_global() {
        // Tab 2 is connection 20's… one of two, so closing it leaves tab 4.
        assert!(!closing_would_empty(&mixed(), 2));
        // Strip connection 20 down to a single tab: closing it empties that
        // connection's strip even though three other tabs exist elsewhere.
        let tabs = vec![(1, 10), (2, 20), (3, 10), (5, 10)];
        assert!(closing_would_empty(&tabs, 2));
    }

    #[test]
    fn closing_would_empty_for_the_only_tab_anywhere() {
        assert!(closing_would_empty(&[(1, 10)], 1));
    }

    #[test]
    fn closing_would_empty_for_an_unknown_tab() {
        // Nothing to close — the caller's keep-one path bails harmlessly.
        assert!(closing_would_empty(&mixed(), 99));
    }

    #[test]
    fn cycle_wraps_within_the_connection() {
        assert_eq!(cycle(&mixed(), 10, Some(1), 1), Some(3));
        assert_eq!(cycle(&mixed(), 10, Some(5), 1), Some(1)); // wraps forward
        assert_eq!(cycle(&mixed(), 10, Some(1), -1), Some(5)); // wraps backward
        assert_eq!(cycle(&mixed(), 20, Some(2), 1), Some(4));
    }

    #[test]
    fn cycle_starts_from_the_first_tab_when_current_is_not_on_this_connection() {
        // Current belongs to connection 20 while cycling connection 10.
        assert_eq!(cycle(&mixed(), 10, Some(2), 1), Some(3));
        assert_eq!(cycle(&mixed(), 10, None, 1), Some(3));
    }

    #[test]
    fn cycle_is_none_without_visible_tabs() {
        assert_eq!(cycle(&mixed(), 99, Some(1), 1), None);
        assert_eq!(cycle(&[], 10, None, 1), None);
    }

    #[test]
    fn a_single_tab_cycles_to_itself() {
        assert_eq!(cycle(&[(1, 10)], 10, Some(1), 1), Some(1));
        assert_eq!(cycle(&[(1, 10)], 10, Some(1), -1), Some(1));
    }

    /// Ctrl+1..9 counts chips, not entries in the flat list. On `mixed()`,
    /// connection 10 shows tabs 1, 3, 5 — so the 2nd chip is tab 3, even though
    /// tab 2 sits between them in the underlying vector.
    #[test]
    fn nth_counts_visible_tabs_not_the_flat_list() {
        let t = mixed();
        assert_eq!(nth(&t, 10, 0), Some(1));
        assert_eq!(nth(&t, 10, 1), Some(3));
        assert_eq!(nth(&t, 10, 2), Some(5));
        assert_eq!(nth(&t, 20, 0), Some(2));
        assert_eq!(nth(&t, 20, 1), Some(4));
    }

    #[test]
    fn nth_past_the_end_or_on_an_empty_connection_is_none() {
        let t = mixed();
        assert_eq!(nth(&t, 10, 3), None, "only three tabs on this connection");
        assert_eq!(nth(&t, 99, 0), None);
        assert_eq!(nth(&[], 10, 0), None);
    }

    /// `(id, conn, pinned)` across two connections, with one pinned on each.
    fn closable() -> Vec<ClosableRef> {
        vec![
            (1, 10, false),
            (2, 20, false),
            (3, 10, true),
            (4, 20, false),
            (5, 10, false),
        ]
    }

    /// The regression this predicate exists for: a pinned tab must answer "no"
    /// *before* the app asks anything about closing it, because one of those
    /// questions settles a transaction.
    #[test]
    fn a_pinned_tab_cannot_be_closed() {
        assert!(can_close(&closable(), 1));
        assert!(!can_close(&closable(), 3), "3 is pinned");
        // An unknown id has nothing to close, so it is not closable either — the
        // caller must not prompt about a tab that isn't there.
        assert!(!can_close(&closable(), 99));
        assert!(!can_close(&[], 1));
    }

    /// One rule, two shapes: whatever `can_close` says about a tab one at a time
    /// is what `all_to_close` collects for its connection. The bug this guards is
    /// the two drifting — the set form is what dims the menu, the single form is
    /// what gates the prompts, and a tab the menu offers but the gate refuses
    /// (or the reverse) is a click that does nothing.
    #[test]
    fn all_to_close_is_every_closable_tab() {
        let tabs = closable();
        for conn in [10, 20, 99] {
            let expected: Vec<usize> = tabs
                .iter()
                .filter(|(id, c, _)| *c == conn && can_close(&tabs, *id))
                .map(|(id, _, _)| *id)
                .collect();
            assert_eq!(all_to_close(&tabs, conn), expected, "conn {conn}");
        }
    }

    #[test]
    fn closing_covers_one_connections_unpinned_tabs() {
        assert_eq!(all_to_close(&closable(), 10), vec![1, 5], "3 is pinned");
        assert_eq!(all_to_close(&closable(), 20), vec![2, 4]);
        assert_eq!(all_to_close(&closable(), 99), Vec::<usize>::new());
    }

    #[test]
    fn closing_the_others_keeps_the_one_the_menu_was_opened_on() {
        assert_eq!(others_to_close(&closable(), 10, 1), vec![5]);
        assert_eq!(others_to_close(&closable(), 10, 5), vec![1]);
    }

    /// **The state the app opens in.** One tab and nothing else to close, so the
    /// entry has to be dimmed — it used to return before the confirm, with no
    /// dialog and no message, one row below a `Reopen last tab` that *is* dimmed
    /// for the same kind of reason.
    #[test]
    fn a_lone_tab_has_no_others_to_close() {
        assert!(others_to_close(&[(1, 10, false)], 10, 1).is_empty());
        // And with every other tab pinned, which is the same answer by another
        // route.
        assert!(others_to_close(&[(1, 10, false), (2, 10, true)], 10, 1).is_empty());
    }

    /// Offered on a pinned tab too: a pinned tab is already the one that
    /// survives everything, so "close the others" is exactly as meaningful.
    #[test]
    fn a_pinned_tab_may_still_be_the_one_kept() {
        assert_eq!(others_to_close(&closable(), 10, 3), vec![1, 5]);
    }

    #[test]
    fn the_boundary_of_a_strip_with_no_pins_is_its_left_edge() {
        assert_eq!(pinned_boundary(&[]), 0);
        assert_eq!(pinned_boundary(&[false, false, false]), 0);
    }

    #[test]
    fn the_boundary_of_an_all_pinned_strip_is_its_right_edge() {
        assert_eq!(pinned_boundary(&[true, true, true]), 3);
    }

    /// **Leading, not total.** A pinned tab to the right of an unpinned one is
    /// a strip that is already out of order; counting it would move the
    /// boundary past unpinned tabs and make the disorder permanent.
    #[test]
    fn a_pin_past_the_block_does_not_extend_it() {
        assert_eq!(pinned_boundary(&[true, true, false, true]), 2);
        assert_eq!(pinned_boundary(&[false, true]), 0);
    }

    #[test]
    fn a_duplicate_of_an_unpinned_tab_lands_just_after_it() {
        assert_eq!(duplicate_slot(&[true, false, false], Some(1)), 2);
        assert_eq!(duplicate_slot(&[false, false, false], Some(0)), 1);
    }

    /// The seam the two helpers meet at, and the one a test of either alone
    /// would miss: a duplicate is always unpinned, so `source + 1` inside the
    /// pinned block is the one answer that must not survive.
    #[test]
    fn a_duplicate_of_a_pinned_tab_clears_the_pinned_block() {
        // Three pinned; duplicating the first would otherwise land at 1.
        assert_eq!(duplicate_slot(&[true, true, true, false], Some(0)), 3);
        assert_eq!(duplicate_slot(&[true, true, true, false], Some(1)), 3);
        // The last pinned tab's "just after" already is the boundary.
        assert_eq!(duplicate_slot(&[true, true, true, false], Some(2)), 3);
    }

    /// A source that is no longer in the strip puts the duplicate at the end,
    /// which is past the boundary on any strip.
    #[test]
    fn a_duplicate_with_no_source_goes_to_the_end() {
        assert_eq!(duplicate_slot(&[true, false, false], None), 3);
        assert_eq!(duplicate_slot(&[true, true], None), 2);
        assert_eq!(duplicate_slot(&[], None), 0);
    }

    #[test]
    fn the_ring_is_asked_per_connection_not_whether_it_is_empty() {
        assert!(has_reopenable(&[10, 11], 10));
        assert!(has_reopenable(&[11, 10], 10));
        // The failure this replaces: a non-empty ring holding only another
        // connection's tabs offered a click that did nothing.
        assert!(!has_reopenable(&[11, 12], 10));
        assert!(!has_reopenable(&[], 10));
    }

    #[test]
    fn a_fresh_empty_tab_is_a_blank_slate() {
        assert!(is_blank_slate(false, "", true, false));
        assert!(is_blank_slate(false, "   \n\t ", true, false));
    }

    /// Each of the four on its own is enough to refuse, so each is asserted on
    /// its own — a reuse that ignores any one of them destroys work.
    #[test]
    fn any_one_reason_is_enough_to_refuse_reuse() {
        assert!(!is_blank_slate(true, "", true, false), "pinned");
        assert!(!is_blank_slate(false, "SELECT 1", true, false), "has query");
        assert!(!is_blank_slate(false, "", false, false), "has results");
        // The one that is not about emptiness: reusing a file-bound tab drops
        // the binding, and the next Ctrl+S goes somewhere else.
        assert!(!is_blank_slate(false, "", true, true), "bound to a file");
    }

    /// The ring's write side. A tab with none of the four is a blank the user
    /// opened and closed again; putting it in the ring pushes a real one out.
    #[test]
    fn a_tab_holding_nothing_is_not_worth_reopening() {
        assert!(!worth_remembering("", false, false, false));
        assert!(!worth_remembering("  \n\t ", false, false, false));
    }

    /// Any one of the four is enough on its own, and each is asserted alone —
    /// an `&&` written where an `||` was meant loses three of them silently.
    #[test]
    fn any_one_reason_is_enough_to_remember_a_closed_tab() {
        assert!(worth_remembering("SELECT 1", false, false, false), "query");
        // An empty query with a table behind it: the table tab, which is the
        // most-reopened kind there is and carries no text at all.
        assert!(worth_remembering("", true, false, false), "table source");
        assert!(worth_remembering("", false, true, false), "a given name");
        // Empty file, and still worth it: the binding is what is lost.
        assert!(worth_remembering("", false, false, true), "a file binding");
    }

    /// **Not the negation of `is_blank_slate`**, and this is where they part:
    /// a table tab with no text is *not* a blank slate to reuse and *is* worth
    /// remembering — agreement — but a tab whose results were merely scrolled
    /// is refused reuse while holding nothing to restore.
    #[test]
    fn the_reuse_question_and_the_reopen_question_are_not_the_same_one() {
        // A scrolled-but-empty tab: not reusable, not worth the ring either.
        assert!(!is_blank_slate(false, "", false, false), "not reusable");
        assert!(
            !worth_remembering("", false, false, false),
            "and nothing to keep — `!is_blank_slate` would have kept it"
        );
        // A pinned empty tab is the same shape from the other side.
        assert!(!is_blank_slate(true, "", true, false));
        assert!(!worth_remembering("", false, false, false));
    }
}
