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

#[cfg(test)]
mod tests {
    use super::*;

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
}
