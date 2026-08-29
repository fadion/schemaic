//! Line-level text diff — pure `&str` → data, no UI.
//!
//! Drives the inline-AI (Ctrl+K) edit preview: [`line_diff`] produces one tagged
//! entry per output row (context / removed / added), and [`inline_plan`]
//! re-addresses those rows as **document line numbers** — which lines the
//! suggestion replaces, and what it puts in their place.
//!
//! That second step is what lets the UI draw the suggestion inside the editor's
//! own line flow instead of in a box over it: the renderer needs to know which
//! of the user's lines to fade and where to hang the new ones, not how to lay
//! out a list. None of this is UI-specific, so it lives here with tests.

/// Whether a diff line is unchanged context, a deletion, or an insertion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffTag {
    Equal,
    Del,
    Ins,
}

/// Above this many LCS cells, skip the O(n·m) DP and emit a whole-middle
/// replace (~2M cells ≈ 16 MB). An LLM Ctrl+K result on a large pasted buffer
/// otherwise allocates the full n·m matrix (10k lines ≈ 800 MB → OOM).
const DIFF_MAX_CELLS: usize = 2_000_000;

/// Line-level LCS diff of `old` vs `new`, one entry per displayed row in order:
/// Equal (context), Del (removed) or Ins (added). O(n·m) time/space over the
/// changed middle only — the common prefix/suffix is stripped first, and buffers
/// bigger than [`DIFF_MAX_CELLS`] fall back to a whole-middle replace.
pub fn line_diff(old: &str, new: &str) -> Vec<(DiffTag, String)> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    // Strip the common prefix/suffix — a targeted edit changes a small middle,
    // so the DP only needs to run over that.
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && a[pre] == b[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < a.len() - pre && suf < b.len() - pre && a[a.len() - 1 - suf] == b[b.len() - 1 - suf]
    {
        suf += 1;
    }
    let am = &a[pre..a.len() - suf];
    let bm = &b[pre..b.len() - suf];
    let (n, m) = (am.len(), bm.len());

    let mut out = Vec::with_capacity(pre + n + m + suf);
    for line in &a[..pre] {
        out.push((DiffTag::Equal, line.to_string()));
    }

    if n.saturating_mul(m) > DIFF_MAX_CELLS {
        // Too big to diff line-by-line: replace the whole middle.
        for line in am {
            out.push((DiffTag::Del, line.to_string()));
        }
        for line in bm {
            out.push((DiffTag::Ins, line.to_string()));
        }
    } else {
        let mut dp = vec![vec![0usize; m + 1]; n + 1];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                dp[i][j] = if am[i] == bm[j] {
                    dp[i + 1][j + 1] + 1
                } else {
                    dp[i + 1][j].max(dp[i][j + 1])
                };
            }
        }
        let (mut i, mut j) = (0usize, 0usize);
        while i < n && j < m {
            if am[i] == bm[j] {
                out.push((DiffTag::Equal, am[i].to_string()));
                i += 1;
                j += 1;
            } else if dp[i + 1][j] >= dp[i][j + 1] {
                out.push((DiffTag::Del, am[i].to_string()));
                i += 1;
            } else {
                out.push((DiffTag::Ins, bm[j].to_string()));
                j += 1;
            }
        }
        while i < n {
            out.push((DiffTag::Del, am[i].to_string()));
            i += 1;
        }
        while j < m {
            out.push((DiffTag::Ins, bm[j].to_string()));
            j += 1;
        }
    }

    for line in &a[a.len() - suf..] {
        out.push((DiffTag::Equal, line.to_string()));
    }
    out
}

/// The 0-based document lines a byte range covers, half-open.
///
/// The companion to [`inline_plan`] for the state *before* there is a plan: the
/// inline-AI request is captured as a byte range, but everything that renders
/// against the editor's surface is keyed on line numbers, so the lines being
/// worked on have to be named the same way the lines being replaced are.
///
/// The last line is the one holding the range's final byte, not the one holding
/// `end` — a range that stops just after a newline covers the line it ended, not
/// the empty one after it.
pub fn line_span(text: &str, start: usize, end: usize) -> std::ops::Range<usize> {
    let count = |upto: usize| {
        text.as_bytes()[..upto.min(text.len())]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
    };
    let first = count(start);
    let last = if end > start { count(end - 1) } else { first };
    first..last + 1
}

/// One contiguous change in an inline-AI suggestion, addressed in **document
/// lines** rather than in diff rows.
///
/// [`line_diff`] answers "what would a preview list look like"; this answers
/// "which lines of the buffer the user is looking at does this touch", which is
/// what rendering the change *in place* needs and a preview list never did.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InlineHunk {
    /// The 0-based document lines this hunk removes, half-open. Empty for a
    /// pure insertion.
    pub del: std::ops::Range<usize>,
    /// The lines this hunk adds, in order. Empty for a pure deletion.
    pub add: Vec<String>,
    /// The 0-based document line the added rows hang off.
    pub anchor: usize,
    /// Render the added rows *before* `anchor` instead of after it. Only ever
    /// true for an insertion at the very top of the buffer, which has no
    /// preceding line to hang off.
    pub before: bool,
}

/// The in-place render plan for an inline-AI (Ctrl+K) suggestion: every change
/// it makes, as document-line coordinates, plus the totals the footer reports.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct InlinePlan {
    pub hunks: Vec<InlineHunk>,
    /// Total added lines across every hunk.
    pub added: usize,
    /// Total removed lines across every hunk.
    pub removed: usize,
}

impl InlinePlan {
    /// No hunks — the suggestion is the buffer the user already has.
    pub fn is_empty(&self) -> bool {
        self.hunks.is_empty()
    }
}

/// What an inline-AI reply would do to the buffer: the text that will actually be
/// spliced into `full[start..end]`, and the whole buffer that results.
///
/// **One function because the two answers have to agree.** The preview diffs the
/// prospective buffer; Accept splices the reply. When those were computed
/// separately, a CRLF buffer plus an LF reply — the ordinary case on Windows,
/// where the clipboard hands you CRLF and neither `clipboard-win` nor floem's
/// editor normalises it — made [`inline_plan`] answer *empty*, because
/// [`str::lines`] strips a trailing `\r`. The footer then read "No changes
/// suggested" while Accept, gated on the state rather than on the plan, converted
/// every line ending in the range. The buffer changed after the UI said it would
/// not.
///
/// So the reply is brought into the buffer's own line ending first, and both
/// callers read the result of the same decision. The buffer's ending is whatever
/// its **first** newline uses; a buffer with none is left alone, since there is
/// nothing to disagree with.
pub fn inline_splice(full: &str, start: usize, end: usize, reply: &str) -> (String, String) {
    let text = if full.find('\n').is_some_and(|i| full[..i].ends_with('\r')) {
        reply.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        reply.to_string()
    };
    let new_full = format!("{}{}{}", &full[..start], text, &full[end..]);
    (text, new_full)
}

/// Turn `old` → `new` into an in-place render plan against `old`'s line numbering.
///
/// `new` is the whole prospective buffer (the caller splices the model's SQL into
/// the trigger range first), so this is [`line_diff`] re-addressed: consecutive
/// non-`Equal` rows group into one hunk, deletions carry the document lines they
/// cover, and the additions hang off the last line the hunk removes — which is
/// what puts the `+` rows *below* the `−` rows they replace.
pub fn inline_plan(old: &str, new: &str) -> InlinePlan {
    let diff = line_diff(old, new);
    let mut plan = InlinePlan::default();
    // `line` tracks the OLD buffer's 0-based line number: `Equal` and `Del` rows
    // consume one, `Ins` rows consume none (they aren't in `old` yet).
    let mut line = 0usize;
    let mut i = 0usize;
    while i < diff.len() {
        if diff[i].0 == DiffTag::Equal {
            line += 1;
            i += 1;
            continue;
        }
        // A run of consecutive changed rows is one hunk, however its deletions
        // and insertions happen to interleave.
        let del_start = line;
        let mut add = Vec::new();
        while i < diff.len() && diff[i].0 != DiffTag::Equal {
            match diff[i].0 {
                DiffTag::Del => line += 1,
                DiffTag::Ins => add.push(diff[i].1.clone()),
                DiffTag::Equal => unreachable!("loop guard excludes Equal"),
            }
            i += 1;
        }
        let del = del_start..line;
        // Additions hang off the last line the hunk removes, so `+` rows sit
        // below the `−` rows they replace. A pure insertion has no such line and
        // falls back to the line above it — or, at the top of the buffer, to
        // rendering before line 0, the one position with nothing to hang off.
        let (anchor, before) = if del.is_empty() {
            match del_start.checked_sub(1) {
                Some(prev) => (prev, false),
                None => (0, true),
            }
        } else {
            (del.end - 1, false)
        };
        plan.removed += del.len();
        plan.added += add.len();
        plan.hunks.push(InlineHunk {
            del,
            add,
            anchor,
            before,
        });
    }
    // The one collision the anchoring can produce, and the renderer resolves a
    // line by `.find(|h| h.anchor == line)` — so a second hunk on the same anchor
    // is drawn nowhere at all, while the footer counts its rows and Accept
    // applies them. A `before` hunk consumes no document line, so line 0's own
    // `Equal` row still advances `line` from 0 to 1, and a pure insertion right
    // after it anchors on `1 - 1 = 0` as well. It is the only way two hunks can
    // share an anchor: every other pair has an `Equal` row between them that
    // moved the counter.
    //
    // Merging their `add`s into one *before* block would draw the trailing
    // addition above the line it belongs under, so line 0 joins the hunk instead:
    // one strike-through and the three lines re-offered in place. Truthful, and a
    // shape the renderer already draws everywhere else.
    if plan.hunks.len() > 1 && plan.hunks[0].before && plan.hunks[1].anchor == 0 {
        let carried = diff
            .iter()
            .find(|(tag, _)| *tag == DiffTag::Equal)
            .map(|(_, text)| text.clone())
            .unwrap_or_default();
        let second = plan.hunks.remove(1);
        let first = &mut plan.hunks[0];
        first.add.push(carried);
        first.add.extend(second.add);
        first.del = 0..1;
        first.before = false;
        plan.added += 1;
        plan.removed += 1;
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_byte_range_names_the_lines_it_covers() {
        let s = "one\ntwo\nthree\nfour";
        // "two\nthree" — starts on line 1, ends inside line 2.
        assert_eq!(line_span(s, 4, 13), 1..3);
        // Entirely within one line.
        assert_eq!(line_span(s, 4, 7), 1..2);
        // An empty range still names the line the caret is on.
        assert_eq!(line_span(s, 5, 5), 1..2);
        assert_eq!(line_span(s, 0, 0), 0..1);
    }

    /// The off-by-one this is written against: a range ending immediately after a
    /// newline covers the line it *ended*, not the empty one that follows.
    #[test]
    fn a_range_ending_on_a_newline_does_not_reach_the_next_line() {
        let s = "one\ntwo\nthree";
        assert_eq!(line_span(s, 0, 4), 0..1, "\"one\\n\" is line 0 alone");
        assert_eq!(line_span(s, 0, 8), 0..2, "\"one\\ntwo\\n\" is lines 0-1");
    }

    #[test]
    fn a_line_span_past_the_end_is_clamped() {
        let s = "one\ntwo";
        assert_eq!(line_span(s, 0, 999), 0..2);
        assert_eq!(line_span("", 0, 0), 0..1);
    }

    /// The design's own example: a statement's two tail lines replaced by three.
    #[test]
    fn inline_plan_maps_a_replacement_to_document_lines() {
        let old = "SELECT * FROM employees\nORDER BY emp_no ASC\nLIMIT 100";
        let new = "SELECT * FROM employees\nWHERE hire_date >= '1990-01-01'\nORDER BY hire_date DESC\nLIMIT 50";
        let plan = inline_plan(old, new);
        assert_eq!(plan.removed, 2);
        assert_eq!(plan.added, 3);
        assert_eq!(plan.hunks.len(), 1, "one contiguous change is one hunk");
        let h = &plan.hunks[0];
        assert_eq!(h.del, 1..3, "document lines 1 and 2 are the removed ones");
        assert_eq!(h.add.len(), 3);
        assert_eq!(h.add[0], "WHERE hire_date >= '1990-01-01'");
        assert_eq!(h.anchor, 2, "additions hang off the LAST removed line");
        assert!(!h.before);
    }

    #[test]
    fn a_pure_insertion_hangs_off_the_line_above() {
        let plan = inline_plan("a\nb", "a\nX\nb");
        assert_eq!(plan.hunks.len(), 1);
        let h = &plan.hunks[0];
        assert!(h.del.is_empty(), "nothing is removed");
        assert_eq!(h.add, vec!["X".to_string()]);
        // Inserted before old line 1, so it renders after old line 0.
        assert_eq!(h.anchor, 0);
        assert!(!h.before);
        assert_eq!((plan.added, plan.removed), (1, 0));
    }

    /// The one case with no preceding line to hang off — the phantom rows have
    /// to render *before* line 0 instead of after some line.
    #[test]
    fn an_insertion_at_the_top_renders_before_line_zero() {
        let plan = inline_plan("a\nb", "X\na\nb");
        assert_eq!(plan.hunks.len(), 1);
        let h = &plan.hunks[0];
        assert!(h.del.is_empty());
        assert_eq!(h.add, vec!["X".to_string()]);
        assert_eq!(h.anchor, 0);
        assert!(h.before, "nothing precedes line 0 to anchor to");
    }

    /// Every hunk that adds rows must be reachable from its anchor line, because
    /// that is the only key the renderer has: `block_at` and `segments` both
    /// resolve a line with `.find(|h| h.anchor == line && !h.add.is_empty())`, so
    /// a second hunk sharing an anchor is drawn *nowhere* — while the footer
    /// counts its rows and Accept applies them. Consent given on half a change.
    #[test]
    fn no_two_hunks_that_add_rows_share_an_anchor() {
        let cases = [
            ("b", "d\nb\nd"),
            (
                "SELECT * FROM employees",
                "-- employees hired since 1990\nSELECT * FROM employees\nWHERE hire_date >= '1990-01-01'",
            ),
            ("a\nb", "X\na\nY\nb"),
            ("a", "X\na\nY\nZ"),
        ];
        for (old, new) in cases {
            let plan = inline_plan(old, new);
            let mut anchors: Vec<usize> = plan
                .hunks
                .iter()
                .filter(|h| !h.add.is_empty())
                .map(|h| h.anchor)
                .collect();
            let before = anchors.len();
            anchors.sort_unstable();
            anchors.dedup();
            assert_eq!(anchors.len(), before, "{old:?} -> {new:?}: {plan:?}");
        }
    }

    /// The merge the collision forces, and the shape it has to take: rendering
    /// both blocks before line 0 would put the trailing addition *above* the line
    /// it belongs under, so line 0 joins the hunk instead — struck through once
    /// and re-offered in place, which is what the screen can actually show.
    #[test]
    fn a_prepend_and_an_append_around_line_zero_become_one_hunk_that_carries_it() {
        let plan = inline_plan(
            "SELECT * FROM employees",
            "-- employees hired since 1990\nSELECT * FROM employees\nWHERE hire_date >= '1990-01-01'",
        );
        assert_eq!(plan.hunks.len(), 1);
        let h = &plan.hunks[0];
        assert_eq!(h.del, 0..1);
        assert!(!h.before);
        assert_eq!(h.anchor, 0);
        assert_eq!(
            h.add,
            vec![
                "-- employees hired since 1990".to_string(),
                "SELECT * FROM employees".to_string(),
                "WHERE hire_date >= '1990-01-01'".to_string(),
            ]
        );
        // The footer reads straight off these, so they have to describe the hunk
        // as merged rather than as the two it came from.
        assert_eq!((plan.added, plan.removed), (3, 1));
    }

    /// The composition the two call sites used to get wrong separately: whatever
    /// the preview says about the plan has to be true of what Accept splices.
    #[test]
    fn a_crlf_buffer_and_an_lf_reply_do_not_disagree_about_whether_anything_changed() {
        let full = "SELECT a\r\nFROM t\r\n";
        // Identical SQL, LF-terminated — what a model hands back.
        let (text, new_full) = inline_splice(full, 0, full.len(), "SELECT a\nFROM t\n");
        assert_eq!(text, full, "the splice must be a no-op, not a re-ending");
        assert!(inline_plan(full, &new_full).is_empty());

        // One real edit: only that line may show as changed, and only that line
        // may be different after the splice.
        let full = "SELECT a\r\nFROM t\r\nWHERE x=1\r\n";
        let (text, new_full) = inline_splice(full, 0, full.len(), "SELECT a\nFROM t\nWHERE x=2\n");
        assert_eq!(text, "SELECT a\r\nFROM t\r\nWHERE x=2\r\n");
        let plan = inline_plan(full, &new_full);
        assert_eq!(plan.hunks.len(), 1);
        assert_eq!(plan.hunks[0].del, 2..3);
    }

    #[test]
    fn an_lf_buffer_leaves_the_reply_exactly_as_the_model_wrote_it() {
        let full = "SELECT a\nFROM t\n";
        let (text, new_full) = inline_splice(full, 0, full.len(), "SELECT b\nFROM t\n");
        assert_eq!(text, "SELECT b\nFROM t\n");
        assert_eq!(new_full, "SELECT b\nFROM t\n");
        // A buffer with no newline at all has no ending to match.
        let (text, new_full) = inline_splice("SELECT a", 0, 8, "SELECT b\nFROM t");
        assert_eq!(text, "SELECT b\nFROM t");
        assert_eq!(new_full, "SELECT b\nFROM t");
    }

    #[test]
    fn a_splice_into_the_middle_keeps_the_text_around_it() {
        let full = "-- head\nSELECT a\n-- tail\n";
        let (text, new_full) = inline_splice(full, 8, 16, "SELECT b");
        assert_eq!(text, "SELECT b");
        assert_eq!(new_full, "-- head\nSELECT b\n-- tail\n");
    }

    /// The same property, swept rather than sampled: every buffer of up to three
    /// lines against every suggestion of up to four, over a two-symbol alphabet.
    /// Deterministic and exhaustive over that space, which is where the collision
    /// lives — it needs only a prepend, one line, and an append.
    #[test]
    fn no_anchor_collision_anywhere_in_a_small_exhaustive_sweep() {
        fn corpus(max_lines: usize) -> Vec<String> {
            let mut out = vec![String::new()];
            let mut level = vec![String::new()];
            for _ in 0..max_lines {
                let mut next = Vec::new();
                for base in &level {
                    for sym in ["a", "b"] {
                        next.push(if base.is_empty() {
                            sym.to_string()
                        } else {
                            format!("{base}\n{sym}")
                        });
                    }
                }
                out.extend(next.iter().cloned());
                level = next;
            }
            out
        }
        for old in corpus(3) {
            for new in corpus(4) {
                let plan = inline_plan(&old, &new);
                let mut anchors: Vec<usize> = plan
                    .hunks
                    .iter()
                    .filter(|h| !h.add.is_empty())
                    .map(|h| h.anchor)
                    .collect();
                let seen = anchors.len();
                anchors.sort_unstable();
                anchors.dedup();
                assert_eq!(anchors.len(), seen, "{old:?} -> {new:?}: {plan:?}");
                // The counts the footer prints have to keep matching the hunks.
                let (add, del) = plan
                    .hunks
                    .iter()
                    .fold((0, 0), |(a, d), h| (a + h.add.len(), d + h.del.len()));
                assert_eq!((plan.added, plan.removed), (add, del), "{old:?} -> {new:?}");
            }
        }
    }

    /// The counterweight: a prepend followed by an insertion further down does
    /// not collide, and must not be merged into one enormous hunk.
    #[test]
    fn a_prepend_and_a_later_insertion_stay_two_hunks() {
        let plan = inline_plan("a\nb\nc", "X\na\nb\nY\nc");
        assert_eq!(plan.hunks.len(), 2);
        assert_eq!((plan.hunks[0].anchor, plan.hunks[0].before), (0, true));
        assert_eq!((plan.hunks[1].anchor, plan.hunks[1].before), (1, false));
        assert_eq!((plan.added, plan.removed), (2, 0));
    }

    #[test]
    fn a_pure_deletion_has_no_added_rows() {
        let plan = inline_plan("a\nb\nc", "a\nc");
        assert_eq!(plan.hunks.len(), 1);
        let h = &plan.hunks[0];
        assert_eq!(h.del, 1..2);
        assert!(h.add.is_empty());
        assert_eq!(h.anchor, 1);
        assert_eq!((plan.added, plan.removed), (0, 1));
    }

    #[test]
    fn identical_text_yields_no_hunks() {
        let plan = inline_plan("a\nb\nc", "a\nb\nc");
        assert!(plan.is_empty());
        assert_eq!((plan.added, plan.removed), (0, 0));
    }

    #[test]
    fn two_separate_changes_are_two_hunks() {
        let plan = inline_plan("a\nb\nc\nd\ne", "a\nB\nc\nD\ne");
        assert_eq!(plan.hunks.len(), 2, "an unchanged line between them splits");
        assert_eq!(plan.hunks[0].del, 1..2);
        assert_eq!(plan.hunks[0].add, vec!["B".to_string()]);
        assert_eq!(plan.hunks[1].del, 3..4);
        assert_eq!(plan.hunks[1].add, vec!["D".to_string()]);
        assert_eq!((plan.added, plan.removed), (2, 2));
    }

    #[test]
    fn an_empty_buffer_gaining_text_is_one_hunk() {
        let plan = inline_plan("", "SELECT 1");
        assert_eq!(plan.added, 1);
        assert_eq!(plan.removed, 0);
        assert_eq!(plan.hunks.len(), 1);
        assert!(plan.hunks[0].before, "there is no line 0 to hang off");
    }

    #[test]
    fn replacing_the_whole_buffer_removes_every_line() {
        let plan = inline_plan("a\nb", "X");
        assert_eq!(plan.hunks.len(), 1);
        assert_eq!(plan.hunks[0].del, 0..2);
        assert_eq!(plan.hunks[0].anchor, 1);
        assert_eq!((plan.added, plan.removed), (1, 2));
    }

    #[test]
    fn diff_marks_changed_middle_only() {
        let d = line_diff("a\nb\nc", "a\nX\nc");
        assert_eq!(
            d,
            vec![
                (DiffTag::Equal, "a".to_string()),
                (DiffTag::Del, "b".to_string()),
                (DiffTag::Ins, "X".to_string()),
                (DiffTag::Equal, "c".to_string()),
            ]
        );
    }

    #[test]
    fn identical_text_is_all_equal() {
        let d = line_diff("a\nb", "a\nb");
        assert!(d.iter().all(|(t, _)| *t == DiffTag::Equal));
    }

    #[test]
    fn pure_insertion_and_deletion() {
        assert_eq!(
            line_diff("a", "a\nb"),
            vec![
                (DiffTag::Equal, "a".to_string()),
                (DiffTag::Ins, "b".to_string())
            ]
        );
        assert_eq!(
            line_diff("a\nb", "a"),
            vec![
                (DiffTag::Equal, "a".to_string()),
                (DiffTag::Del, "b".to_string())
            ]
        );
    }

    /// The old preview list collapsed distant unchanged lines into gap rows. The
    /// in-place render has no such notion — the unchanged lines are simply the
    /// user's own, still on screen — so a change deep in a long buffer must come
    /// back as one hunk pointing at the right document line and nothing else.
    #[test]
    fn a_change_deep_in_a_long_buffer_is_one_hunk_at_that_line() {
        let old = (0..20)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut newv: Vec<String> = (0..20).map(|i| format!("l{i}")).collect();
        newv[10] = "changed".to_string();
        let plan = inline_plan(&old, &newv.join("\n"));
        assert_eq!(plan.hunks.len(), 1);
        assert_eq!(plan.hunks[0].del, 10..11);
        assert_eq!(plan.hunks[0].add, vec!["changed".to_string()]);
        assert_eq!((plan.added, plan.removed), (1, 1));
    }

    #[test]
    fn huge_middle_falls_back_to_whole_replace() {
        // Enough distinct lines that n*m exceeds DIFF_MAX_CELLS (2M): ~1500 each
        // → ~2.25M cells. Every line differs so there is no common prefix/suffix.
        let old = (0..1500)
            .map(|i| format!("a{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let new = (0..1500)
            .map(|i| format!("b{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let d = line_diff(&old, &new);
        // Fallback = all deletions first, then all insertions (no interleaving).
        let dels = d.iter().filter(|(t, _)| *t == DiffTag::Del).count();
        let ins = d.iter().filter(|(t, _)| *t == DiffTag::Ins).count();
        assert_eq!(dels, 1500);
        assert_eq!(ins, 1500);
        let first_ins = d.iter().position(|(t, _)| *t == DiffTag::Ins).unwrap();
        let last_del = d.iter().rposition(|(t, _)| *t == DiffTag::Del).unwrap();
        assert!(
            last_del < first_ins,
            "all Dels precede all Ins in the fallback"
        );
    }

    /// The numbering that matters now is the **old** buffer's, because that is the
    /// one on screen. A preview list numbered deletions by the old file and
    /// insertions by the new; a hunk has no use for the latter — its additions
    /// have no document line yet, which is the point of `anchor`.
    #[test]
    fn hunks_are_numbered_against_the_buffer_on_screen() {
        let plan = inline_plan("a\nb\nc", "a\nX\nc");
        assert_eq!(plan.hunks.len(), 1);
        let h = &plan.hunks[0];
        assert_eq!(h.del, 1..2, "old line 1 (0-based) is what is replaced");
        assert_eq!(h.anchor, 1, "and the addition hangs off that same line");
        assert_eq!(h.add, vec!["X".to_string()]);
    }
}
