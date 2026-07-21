//! Line-level text diff — pure `&str` → data, no UI.
//!
//! Drives the inline-AI (Ctrl+K) edit preview: [`line_diff`] produces one tagged
//! entry per output row (context / removed / added), and [`build_diff_rows`]
//! hunks that down to changed lines plus a little context, collapsing long
//! unchanged runs into gaps. The UI renders the rows; none of this logic is
//! UI-specific, so it lives here with tests.

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

/// Unchanged lines kept on each side of a change (git-style hunking); longer
/// runs collapse into a single "⋯ N unchanged lines" row.
const DIFF_CONTEXT: usize = 3;

/// A display row: a diff line (with its real document line number) or a gap
/// standing in for `n` collapsed unchanged lines.
pub enum DiffRow {
    Line {
        tag: DiffTag,
        num: usize,
        text: String,
    },
    Gap(usize),
}

/// Hunk a full line diff for display: keep every changed line plus [`DIFF_CONTEXT`]
/// unchanged lines around it, collapse the rest to `Gap`s. Line numbers are real
/// document positions — old-file number for deletions, new-file number for
/// context/insertions — so the collapsed gaps read correctly.
pub fn build_diff_rows(diff: Vec<(DiffTag, String)>) -> Vec<DiffRow> {
    let n = diff.len();
    if n == 0 {
        return Vec::new();
    }
    let (mut old_ln, mut new_ln) = (1usize, 1usize);
    let numbered: Vec<(DiffTag, usize, String)> = diff
        .into_iter()
        .map(|(tag, text)| {
            let num = match tag {
                DiffTag::Equal => {
                    let v = new_ln;
                    old_ln += 1;
                    new_ln += 1;
                    v
                }
                DiffTag::Del => {
                    let v = old_ln;
                    old_ln += 1;
                    v
                }
                DiffTag::Ins => {
                    let v = new_ln;
                    new_ln += 1;
                    v
                }
            };
            (tag, num, text)
        })
        .collect();

    // Keep every changed line and the context window around it.
    let mut keep = vec![false; n];
    for (i, (tag, _, _)) in numbered.iter().enumerate() {
        if *tag != DiffTag::Equal {
            let lo = i.saturating_sub(DIFF_CONTEXT);
            let hi = (i + DIFF_CONTEXT).min(n - 1);
            for slot in keep.iter_mut().take(hi + 1).skip(lo) {
                *slot = true;
            }
        }
    }

    // Emit kept rows; collapse dropped (unchanged) runs into gaps.
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if keep[i] {
            let (tag, num, text) = numbered[i].clone();
            out.push(DiffRow::Line { tag, num, text });
            i += 1;
        } else {
            let start = i;
            while i < n && !keep[i] {
                i += 1;
            }
            out.push(DiffRow::Gap(i - start));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn build_rows_collapses_unchanged_runs_into_gaps() {
        // 20 identical lines, one change in the middle → the far context collapses.
        let old = (0..20)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut newv: Vec<String> = (0..20).map(|i| format!("l{i}")).collect();
        newv[10] = "changed".to_string();
        let rows = build_diff_rows(line_diff(&old, &newv.join("\n")));
        // There is at least one Gap (the unchanged head/tail beyond the context).
        assert!(rows.iter().any(|r| matches!(r, DiffRow::Gap(_))));
        // The changed line (Del old + Ins new) is present.
        assert!(rows.iter().any(
            |r| matches!(r, DiffRow::Line { tag: DiffTag::Ins, text, .. } if text == "changed")
        ));
    }

    #[test]
    fn build_rows_empty_diff_is_empty() {
        assert!(build_diff_rows(Vec::new()).is_empty());
    }
}
