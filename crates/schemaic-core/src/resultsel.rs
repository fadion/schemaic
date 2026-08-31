//! Which result panel is shown, and which ones a run is allowed to replace.
//!
//! The results strip is a tab strip: one panel per statement, always visible, and
//! a panel the user has **pinned** survives the next run instead of being
//! replaced by it. That is the whole feature — a result you can keep, so a
//! before/after comparison doesn't mean a second tab and a second execution of a
//! query whose "before" may no longer exist.
//!
//! These are the rules, pure and tested; the UI passes `(panel id, pinned)` in
//! display order and applies the answer. They are deliberately the same rules
//! [`crate::tabsel`] gives the *query* strip — pinned panels sit at the front in
//! pin order, a pinned panel cannot be closed, "close all" means "close all the
//! unpinned ones" — because the two strips sit one above the other and a user who
//! has learned one has learned the other.

/// One panel, reduced to what the strip's rules care about: its id, and whether
/// it is pinned.
pub type PanelRef = (u64, bool);

/// The strip after a run: the pinned panels, in their existing order, then the
/// run's fresh ones.
///
/// **A run replaces the unpinned panels and nothing else.** That is the one
/// sentence the feature is: unpinned panels are what the last run happened to
/// leave behind, pinned ones are what the user asked to keep.
pub fn after_run(panels: &[PanelRef], fresh: &[u64]) -> Vec<u64> {
    panels
        .iter()
        .filter(|(_, pinned)| *pinned)
        .map(|(id, _)| *id)
        .chain(fresh.iter().copied())
        .collect()
}

/// Which panel to show once a run lands: its **first fresh** one.
///
/// Not the panel that was active before it, even when that one survives: a run
/// is a question the user just asked, and leaving the strip on a pinned snapshot
/// of an older answer reads as a query that did nothing. Falls back to the first
/// surviving panel for a run that produced no statements at all, which is not a
/// state the app can reach today and is still not a reason to answer `None` and
/// leave the strip pointing at nothing.
pub fn active_after_run(panels: &[PanelRef], fresh: &[u64]) -> Option<u64> {
    fresh
        .first()
        .copied()
        .or_else(|| after_run(panels, fresh).first().copied())
}

/// The strip after `id`'s pinned flag is set to `pinned`: the panel moves to the
/// boundary of the pinned block.
///
/// **The pinned block stays contiguous at the front**, which is what lets every
/// other rule here be a filter rather than a sort. Pinning moves the panel to the
/// end of that block, unpinning to the first unpinned slot — the same move in
/// both directions, and the same one the query strip makes.
pub fn pin_order(panels: &[PanelRef], id: u64, pinned: bool) -> Vec<u64> {
    let mut rest: Vec<PanelRef> = panels.iter().copied().filter(|(i, _)| *i != id).collect();
    if !panels.iter().any(|(i, _)| *i == id) {
        return rest.into_iter().map(|(i, _)| i).collect();
    }
    let boundary = rest.iter().take_while(|(_, p)| *p).count();
    rest.insert(boundary, (id, pinned));
    rest.into_iter().map(|(i, _)| i).collect()
}

/// Can `id` be closed at all?
///
/// False for a pinned panel, and false for an unknown one — [`crate::tabsel::can_close`]'s
/// rule, and it exists here for the same reason: the menu has to dim the entry
/// *before* the close is attempted, so the test cannot live only at the far end
/// of the close path.
pub fn can_close(panels: &[PanelRef], id: u64) -> bool {
    panels.iter().any(|(i, pinned)| *i == id && !*pinned)
}

/// What "Close all" closes: every unpinned panel.
pub fn all_to_close(panels: &[PanelRef]) -> Vec<u64> {
    panels
        .iter()
        .filter(|(_, pinned)| !*pinned)
        .map(|(id, _)| *id)
        .collect()
}

/// What "Close others" closes: [`all_to_close`]'s set, less `keep`.
///
/// Offered on a pinned panel too — a pinned panel is already the one that
/// survives everything, so "close the others" is exactly as meaningful there.
pub fn others_to_close(panels: &[PanelRef], keep: u64) -> Vec<u64> {
    all_to_close(panels)
        .into_iter()
        .filter(|id| *id != keep)
        .collect()
}

/// Which panel is shown once `removed` are gone.
///
/// The active one when it survives — closing some *other* panel must not move
/// what the user is looking at. Otherwise the nearest survivor to its right,
/// else to its left, which is where the eye already is. `None` only when nothing
/// survives at all.
///
/// One function for all three closes (one, others, all) so they cannot disagree
/// about where the strip lands.
pub fn active_after_removal(panels: &[PanelRef], removed: &[u64], active: u64) -> Option<u64> {
    let gone = |id: u64| removed.contains(&id);
    if !gone(active) && panels.iter().any(|(i, _)| *i == active) {
        return Some(active);
    }
    let at = panels.iter().position(|(i, _)| *i == active);
    let survivors = |range: &mut dyn Iterator<Item = &PanelRef>| -> Option<u64> {
        range.map(|(i, _)| *i).find(|id| !gone(*id))
    };
    match at {
        Some(at) => survivors(&mut panels[at + 1..].iter())
            .or_else(|| survivors(&mut panels[..at].iter().rev())),
        // An active id the strip doesn't hold: fall back to the first survivor
        // rather than to nothing, which is the same answer the `at + 1` walk
        // gives from the front.
        None => survivors(&mut panels.iter()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `[pinned A][B][C]`, the shape every rule below is stated against.
    fn strip() -> Vec<PanelRef> {
        vec![(1, true), (2, false), (3, false)]
    }

    // ── What a run keeps ──────────────────────────────────────────────────────

    #[test]
    fn a_run_replaces_the_unpinned_panels_and_keeps_the_pinned_ones() {
        assert_eq!(after_run(&strip(), &[7, 8]), vec![1, 7, 8]);
    }

    #[test]
    fn a_run_over_a_strip_with_no_pins_keeps_nothing() {
        let panels = [(2, false), (3, false)];
        assert_eq!(after_run(&panels, &[7]), vec![7]);
    }

    #[test]
    fn a_run_over_an_empty_strip_is_just_its_own_panels() {
        assert_eq!(after_run(&[], &[7, 8]), vec![7, 8]);
    }

    /// Pins keep their **relative** order, and all of them survive — the strip is
    /// not capped at one kept result.
    #[test]
    fn every_pin_survives_a_run_in_its_own_order() {
        let panels = [(5, true), (1, true), (2, false)];
        assert_eq!(after_run(&panels, &[9]), vec![5, 1, 9]);
    }

    #[test]
    fn a_run_shows_its_own_first_result_not_the_pin_that_survived() {
        assert_eq!(active_after_run(&strip(), &[7, 8]), Some(7));
    }

    #[test]
    fn a_run_with_no_statements_falls_back_to_the_first_survivor() {
        assert_eq!(active_after_run(&strip(), &[]), Some(1));
        assert_eq!(active_after_run(&[], &[]), None);
    }

    // ── Pinning ───────────────────────────────────────────────────────────────

    #[test]
    fn pinning_moves_a_panel_to_the_end_of_the_pinned_block() {
        assert_eq!(pin_order(&strip(), 3, true), vec![1, 3, 2]);
    }

    #[test]
    fn unpinning_moves_a_panel_to_the_first_unpinned_slot() {
        let panels = [(1, true), (5, true), (2, false)];
        assert_eq!(pin_order(&panels, 1, false), vec![5, 1, 2]);
    }

    /// The invariant every other rule here leans on: after any pin or unpin, the
    /// pinned panels are still a contiguous block at the front.
    #[test]
    fn the_pinned_block_stays_contiguous_through_any_toggle() {
        let panels = strip();
        for (id, to) in [(1, false), (2, true), (3, true), (2, false)] {
            let order = pin_order(&panels, id, to);
            let flags: Vec<bool> = order
                .iter()
                .map(|i| {
                    if *i == id {
                        to
                    } else {
                        panels.iter().find(|(p, _)| p == i).unwrap().1
                    }
                })
                .collect();
            let pins = flags.iter().take_while(|p| **p).count();
            assert!(
                flags[pins..].iter().all(|p| !*p),
                "toggling {id} to {to} split the pinned block: {flags:?}"
            );
        }
    }

    #[test]
    fn pinning_an_unknown_panel_leaves_the_strip_alone() {
        assert_eq!(pin_order(&strip(), 99, true), vec![1, 2, 3]);
    }

    // ── Closing ───────────────────────────────────────────────────────────────

    #[test]
    fn a_pinned_panel_cannot_be_closed() {
        assert!(!can_close(&strip(), 1));
        assert!(can_close(&strip(), 2));
    }

    #[test]
    fn an_unknown_panel_cannot_be_closed_either() {
        assert!(!can_close(&strip(), 99));
    }

    #[test]
    fn close_all_spares_the_pins() {
        assert_eq!(all_to_close(&strip()), vec![2, 3]);
    }

    /// The set form and the single test are the same rule — a panel `can_close`
    /// says no to must never appear in what "Close all" would close.
    #[test]
    fn all_to_close_is_every_closable_panel() {
        let panels = strip();
        for (id, _) in &panels {
            assert_eq!(
                can_close(&panels, *id),
                all_to_close(&panels).contains(id),
                "the two rules disagree about panel {id}"
            );
        }
    }

    #[test]
    fn close_others_spares_the_pins_and_the_kept_one() {
        assert_eq!(others_to_close(&strip(), 2), vec![3]);
    }

    #[test]
    fn close_others_from_a_pinned_panel_closes_every_unpinned_one() {
        assert_eq!(others_to_close(&strip(), 1), vec![2, 3]);
    }

    // ── Where the strip lands ─────────────────────────────────────────────────

    #[test]
    fn closing_another_panel_does_not_move_the_shown_one() {
        assert_eq!(active_after_removal(&strip(), &[3], 2), Some(2));
    }

    #[test]
    fn closing_the_shown_panel_lands_on_the_one_to_its_right() {
        assert_eq!(active_after_removal(&strip(), &[2], 2), Some(3));
    }

    #[test]
    fn closing_the_last_panel_lands_on_the_one_to_its_left() {
        assert_eq!(active_after_removal(&strip(), &[3], 3), Some(2));
    }

    /// Close-all from an unpinned panel: every unpinned one goes at once, so the
    /// walk right has to skip the ones that are also leaving and land on the pin.
    #[test]
    fn close_all_lands_on_the_pin_that_survived() {
        assert_eq!(active_after_removal(&strip(), &[2, 3], 2), Some(1));
    }

    #[test]
    fn closing_everything_leaves_nothing_shown() {
        let panels = [(2, false), (3, false)];
        assert_eq!(active_after_removal(&panels, &[2, 3], 2), None);
    }

    #[test]
    fn a_stale_active_id_falls_back_to_the_first_survivor() {
        assert_eq!(active_after_removal(&strip(), &[2], 99), Some(1));
    }
}
