//! Parse a MySQL/MariaDB `EXPLAIN` result set into a displayable query plan plus
//! heuristic warnings (full table scans, filesort, temporary tables).
//!
//! Pure + unit-tested: the app runs `EXPLAIN` / `EXPLAIN ANALYZE` and hands the
//! raw [`ResultSet`] here; the UI renders the resulting [`QueryPlan`] as a table,
//! highlights the flagged rows, and ships [`QueryPlan::to_prompt_text`] to the AI
//! panel for the "Ask AI" optimization button.
//!
//! The plan is kept as the server's own tabular output (column headers + string
//! cells, verbatim) rather than a bespoke tree, so it renders uniformly whether
//! the server returned classic `EXPLAIN` columns (`id`/`type`/`key`/`Extra`/…) or
//! the single-column tree text of `EXPLAIN ANALYZE`. Warnings only fire on the
//! classic columns; the tree-text form simply yields none.

use crate::model::ResultSet;

/// A parsed EXPLAIN plan: the raw tabular output plus heuristic warnings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueryPlan {
    /// Column headers, exactly as the server named them.
    pub columns: Vec<String>,
    /// Row cells (as displayed text), aligned to `columns`.
    pub rows: Vec<Vec<String>>,
    /// Heuristic performance flags, each tied to a row index.
    pub warnings: Vec<PlanWarning>,
}

/// The kind of a heuristic plan warning (drives the icon/colour in the UI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanWarningKind {
    /// `type = ALL` — a full table scan with no usable index.
    FullScan,
    /// `Extra` contains `Using filesort` — an on-the-fly sort.
    Filesort,
    /// `Extra` contains `Using temporary` — an intermediate temp table.
    TempTable,
}

/// One heuristic warning about a plan row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanWarning {
    /// Index into [`QueryPlan::rows`] this warning refers to.
    pub row: usize,
    pub kind: PlanWarningKind,
    /// Human-readable, one-line explanation shown in the modal.
    pub message: String,
}

impl QueryPlan {
    /// Build a plan from an `EXPLAIN` result set: capture the tabular output and
    /// scan it for the common performance smells.
    pub fn from_result(rs: &ResultSet) -> QueryPlan {
        let columns: Vec<String> = rs.columns.iter().map(|c| c.name.clone()).collect();
        let ncols = rs.col_count();
        let rows: Vec<Vec<String>> = (0..rs.row_count())
            .map(|r| {
                (0..ncols)
                    .map(|c| {
                        rs.cell(r, c)
                            .map(|cell| cell.display().to_string())
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .collect();
        let warnings = analyze(&columns, &rows);
        QueryPlan {
            columns,
            rows,
            warnings,
        }
    }

    /// Render the plan (and any warnings) as compact text for the AI prompt — a
    /// pipe-delimited table Claude can read, followed by the flagged rows.
    pub fn to_prompt_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.columns.join(" | "));
        out.push('\n');
        for row in &self.rows {
            out.push_str(&row.join(" | "));
            out.push('\n');
        }
        if !self.warnings.is_empty() {
            out.push_str("\nPotential issues:\n");
            for w in &self.warnings {
                out.push_str("- ");
                out.push_str(&w.message);
                out.push('\n');
            }
        }
        out
    }
}

/// Case-insensitive column lookup (EXPLAIN column names vary a little across
/// server versions — `Extra` vs `extra`, etc.).
fn col_idx(columns: &[String], name: &str) -> Option<usize> {
    columns.iter().position(|c| c.eq_ignore_ascii_case(name))
}

/// A missing / empty / `NULL` cell reads as "not present".
fn is_blank(v: &str) -> bool {
    v.is_empty() || v.eq_ignore_ascii_case("NULL")
}

fn analyze(columns: &[String], rows: &[Vec<String>]) -> Vec<PlanWarning> {
    // Postgres `EXPLAIN` returns a single "QUERY PLAN" text column (one node/detail
    // line per row) rather than MySQL's named columns — scan the text for smells.
    if columns.len() == 1 && columns[0].eq_ignore_ascii_case("QUERY PLAN") {
        return analyze_pg(rows);
    }
    let type_i = col_idx(columns, "type");
    let table_i = col_idx(columns, "table");
    let key_i = col_idx(columns, "key");
    let extra_i = col_idx(columns, "Extra");

    let cell = |row: &[String], i: Option<usize>| -> String {
        i.and_then(|i| row.get(i)).cloned().unwrap_or_default()
    };
    // A readable table label for the message (`<derived2>` etc. pass through).
    let label = |row: &[String]| -> String {
        let t = cell(row, table_i);
        if t.is_empty() {
            "the result".to_string()
        } else {
            format!("`{t}`")
        }
    };

    let mut out = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        let access = cell(row, type_i);
        let extra = cell(row, extra_i);

        // Full table scan: access type ALL with no index chosen.
        if access.eq_ignore_ascii_case("ALL") {
            let no_key = is_blank(&cell(row, key_i));
            let detail = if no_key {
                "no index used"
            } else {
                "index not used for lookup"
            };
            out.push(PlanWarning {
                row: r,
                kind: PlanWarningKind::FullScan,
                message: format!("Full table scan on {} ({detail}, type = ALL)", label(row)),
            });
        }
        // Extra-column smells (case-insensitive substring — MySQL packs several
        // notes into one `Extra` cell separated by `; `).
        let extra_l = extra.to_ascii_lowercase();
        if extra_l.contains("using filesort") {
            out.push(PlanWarning {
                row: r,
                kind: PlanWarningKind::Filesort,
                message: format!("Filesort on {} (Extra: Using filesort)", label(row)),
            });
        }
        if extra_l.contains("using temporary") {
            out.push(PlanWarning {
                row: r,
                kind: PlanWarningKind::TempTable,
                message: format!(
                    "Temporary table for {} (Extra: Using temporary)",
                    label(row)
                ),
            });
        }
    }
    out
}

/// Heuristics for a Postgres text plan: each row is one plan line. Flags
/// sequential scans (no usable index) and on-the-fly sort nodes. Detail lines
/// (`Sort Key:`, `Filter:`, …) are skipped so only the plan *nodes* warn.
fn analyze_pg(rows: &[Vec<String>]) -> Vec<PlanWarning> {
    let mut out = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        let line = row.first().map(|s| s.trim()).unwrap_or("");
        // Strip the tree marker so a child node ("->  Seq Scan …") matches too.
        let node = line.trim_start_matches("->").trim();
        let lower = node.to_ascii_lowercase();
        if lower.starts_with("seq scan") {
            // Table name follows "on ".
            let tbl = node
                .split(" on ")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("");
            let label = if tbl.is_empty() {
                "a table".to_string()
            } else {
                format!("`{tbl}`")
            };
            out.push(PlanWarning {
                row: r,
                kind: PlanWarningKind::FullScan,
                message: format!("Sequential scan on {label} (no index used)"),
            });
        } else if lower.starts_with("sort")
            && !lower.starts_with("sort key")
            && !lower.starts_with("sort method")
        {
            out.push(PlanWarning {
                row: r,
                kind: PlanWarningKind::Filesort,
                message: "Sort node — the query sorts rows on the fly".to_string(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, ResultSet, Value};

    /// Build a fake classic-EXPLAIN result set from headers + string rows.
    fn rs(headers: &[&str], rows: &[&[&str]]) -> ResultSet {
        ResultSet::from_rows(
            headers
                .iter()
                .map(|h| Column {
                    name: h.to_string(),
                    type_name: "VARCHAR".to_string(),
                    origin: None,
                })
                .collect(),
            rows.iter()
                .map(|r| r.iter().map(|c| Value::Str(c.to_string())).collect())
                .collect(),
        )
    }

    const HEADERS: &[&str] = &[
        "id",
        "select_type",
        "table",
        "type",
        "possible_keys",
        "key",
        "key_len",
        "ref",
        "rows",
        "Extra",
    ];

    #[test]
    fn flags_full_scan_filesort_and_temp() {
        let rs = rs(
            HEADERS,
            &[&[
                "1",
                "SIMPLE",
                "film",
                "ALL",
                "NULL",
                "NULL",
                "NULL",
                "NULL",
                "1000",
                "Using where; Using temporary; Using filesort",
            ]],
        );
        let plan = QueryPlan::from_result(&rs);
        let kinds: Vec<_> = plan.warnings.iter().map(|w| w.kind).collect();
        assert!(kinds.contains(&PlanWarningKind::FullScan));
        assert!(kinds.contains(&PlanWarningKind::Filesort));
        assert!(kinds.contains(&PlanWarningKind::TempTable));
        assert_eq!(plan.warnings.len(), 3);
        // Every warning points at the single row and names the table.
        assert!(plan.warnings.iter().all(|w| w.row == 0));
        assert!(plan.warnings[0].message.contains("`film`"));
    }

    #[test]
    fn clean_plan_has_no_warnings() {
        let rs = rs(
            HEADERS,
            &[&[
                "1", "SIMPLE", "film", "const", "PRIMARY", "PRIMARY", "2", "const", "1", "",
            ]],
        );
        let plan = QueryPlan::from_result(&rs);
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    #[test]
    fn full_scan_with_a_chosen_key_is_still_flagged_but_worded_differently() {
        let rs = rs(
            HEADERS,
            &[&[
                "1", "SIMPLE", "t", "ALL", "idx", "idx", "4", "NULL", "50", "",
            ]],
        );
        let plan = QueryPlan::from_result(&rs);
        assert_eq!(plan.warnings.len(), 1);
        assert_eq!(plan.warnings[0].kind, PlanWarningKind::FullScan);
        assert!(plan.warnings[0].message.contains("index not used"));
    }

    #[test]
    fn case_insensitive_column_names() {
        // Lower-case `extra` / `type` headers still parse.
        let rs = rs(
            &["id", "table", "type", "key", "extra"],
            &[&["1", "orders", "all", "NULL", "Using filesort"]],
        );
        let plan = QueryPlan::from_result(&rs);
        let kinds: Vec<_> = plan.warnings.iter().map(|w| w.kind).collect();
        assert!(kinds.contains(&PlanWarningKind::FullScan));
        assert!(kinds.contains(&PlanWarningKind::Filesort));
    }

    #[test]
    fn tree_text_analyze_output_yields_no_warnings() {
        // EXPLAIN ANALYZE (MySQL) returns a single "EXPLAIN" column of tree text.
        let rs = rs(
            &["EXPLAIN"],
            &[&["-> Table scan on film  (cost=1.2 rows=1000) (actual time=0.1..0.5 rows=1000)"]],
        );
        let plan = QueryPlan::from_result(&rs);
        assert!(plan.warnings.is_empty());
        assert_eq!(plan.columns, vec!["EXPLAIN".to_string()]);
    }

    #[test]
    fn postgres_text_plan_flags_seq_scan_and_sort() {
        // A Postgres `EXPLAIN` plan: single "QUERY PLAN" column, one line per row.
        let rs = rs(
            &["QUERY PLAN"],
            &[
                &["Sort  (cost=1.2..1.3 rows=239 width=52)"],
                &["  Sort Key: population DESC"],
                &["  ->  Seq Scan on country  (cost=0.00..7.39 rows=239 width=52)"],
            ],
        );
        let plan = QueryPlan::from_result(&rs);
        let kinds: Vec<_> = plan.warnings.iter().map(|w| w.kind).collect();
        assert!(kinds.contains(&PlanWarningKind::FullScan));
        assert!(kinds.contains(&PlanWarningKind::Filesort));
        // "Sort Key:" is a detail line, not a node → must NOT double-flag.
        assert_eq!(plan.warnings.len(), 2);
        // The Seq Scan warning names the table.
        assert!(
            plan.warnings
                .iter()
                .any(|w| w.message.contains("`country`"))
        );
    }

    #[test]
    fn postgres_clean_index_plan_has_no_warnings() {
        let rs = rs(
            &["QUERY PLAN"],
            &[&["Index Scan using country_pkey on country  (cost=0.15..8.17 rows=1 width=52)"]],
        );
        let plan = QueryPlan::from_result(&rs);
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    #[test]
    fn prompt_text_has_header_rows_and_warnings() {
        let rs = rs(
            HEADERS,
            &[&[
                "1", "SIMPLE", "film", "ALL", "NULL", "NULL", "NULL", "NULL", "1000", "",
            ]],
        );
        let plan = QueryPlan::from_result(&rs);
        let txt = plan.to_prompt_text();
        assert!(txt.contains("select_type | table"));
        assert!(txt.contains("SIMPLE | film | ALL"));
        assert!(txt.contains("Potential issues:"));
        assert!(txt.contains("Full table scan"));
    }
}
