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

/// Can the editor show a query plan for `dialect`?
///
/// Every engine can now — SQL Server's plan is an XML document
/// (`SET SHOWPLAN_XML ON`) that [`QueryPlan::from_result`] reads — but the
/// match stays exhaustive, so an engine added later has to answer it rather
/// than inherit a yes. The editor's *Plan* entry asks this and is absent where
/// it is `false`, rather than opening a modal to report a refusal.
pub fn supports_plan(dialect: crate::intel::SqlDialect) -> bool {
    use crate::intel::SqlDialect;
    match dialect {
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::Sqlite | SqlDialect::MsSql => true,
    }
}

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
    /// `Extra` contains `Using temporary` — an intermediate temp table; on SQL
    /// Server, a spool or a spill to `tempdb`.
    TempTable,
    /// An index the optimiser says it would have used — SQL Server names one in
    /// its plan (`MissingIndexes`), with the improvement it expected.
    MissingIndex,
    /// A warning the server wrote into the plan itself — SQL Server's
    /// `<Warnings>`: a join with no predicate, a column with no statistics, a
    /// conversion that stops a seek.
    ServerWarning,
}

/// The one column `SET SHOWPLAN_XML ON` and `SET STATISTICS XML ON` answer
/// under, one XML document per statement — the name that tells
/// [`QueryPlan::from_result`] the rows are SQL Server plans to be read rather
/// than a table to be shown.
pub const SHOWPLAN_COLUMN: &str = "Microsoft SQL Server 2005 XML Showplan";

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
    ///
    /// A result under [`SHOWPLAN_COLUMN`] is SQL Server's instead — one XML
    /// document per statement — and is read by [`showplan`] into the same
    /// shape.
    pub fn from_result(rs: &ResultSet) -> QueryPlan {
        let columns: Vec<String> = rs.columns.iter().map(|c| c.name.clone()).collect();
        if columns.len() == 1 && columns[0] == SHOWPLAN_COLUMN {
            let docs: Vec<String> = (0..rs.row_count())
                .map(|r| {
                    rs.cell(r, 0)
                        .map(|c| c.display().to_string())
                        .unwrap_or_default()
                })
                .collect();
            return showplan(&docs);
        }
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

// ── SQL Server ───────────────────────────────────────────────────────────────

/// One operator (`<RelOp>`) of a SQL Server plan, as the table shows it.
#[derive(Default)]
struct MsOp {
    depth: usize,
    physical: String,
    logical: String,
    /// `schema.table`, for a warning's label.
    table: Option<String>,
    /// `schema.table (index) alias`, for the table's *Object* cell.
    object: Option<String>,
    est_rows: String,
    est_cost: String,
    /// Summed over `RunTimeCountersPerThread` — `None` when not measured.
    actual_rows: Option<u64>,
    executions: Option<u64>,
    /// The operator's own `<Warnings>`, as (kind, text).
    warnings: Vec<(PlanWarningKind, String)>,
}

/// One planned statement (`<StmtSimple>` with a `<QueryPlan>`).
#[derive(Default)]
struct MsStatement {
    kind: String,
    ops: Vec<MsOp>,
    /// Statement-level advice: missing indexes and the plan's own warnings.
    advice: Vec<(PlanWarningKind, String)>,
}

/// SQL Server's plan documents — one per statement, from `SET SHOWPLAN_XML ON`
/// or the measured `SET STATISTICS XML ON` — as one table: a row per operator,
/// indented by its depth in the tree, the object it reads, the optimiser's
/// estimates, and, when the plan was measured, the actual rows and executions
/// summed over the threads that ran it. More than one planned statement gets a
/// heading row each; a statement with no plan (a `SET`, a `DECLARE`) none.
///
/// **The warnings are the server's as well as the heuristics'.** A scan of the
/// whole table (`Table Scan`, `Clustered Index Scan`), a `Sort` and a spool are
/// flagged as the other engines' equivalents are; the plan's own
/// `<MissingIndexes>` and `<Warnings>` — which no other engine's plan carries —
/// are passed on as SQL Server wrote them.
///
/// A document that does not read as a plan is shown verbatim under the reason,
/// rather than as an empty table that would say there was no plan.
fn showplan(docs: &[String]) -> QueryPlan {
    let mut statements = Vec::new();
    for doc in docs {
        match parse_showplan(doc) {
            Ok(mut s) => statements.append(&mut s),
            Err(reason) => {
                let mut rows = vec![vec![format!("Could not read the plan: {reason}")]];
                rows.extend(docs.iter().map(|d| vec![d.clone()]));
                return QueryPlan {
                    columns: vec!["Plan".to_string()],
                    rows,
                    warnings: Vec::new(),
                };
            }
        }
    }
    statements.retain(|s| !s.ops.is_empty());
    if statements.is_empty() {
        return QueryPlan {
            columns: vec!["Plan".to_string()],
            rows: vec![vec!["No statement in this batch has a plan.".to_string()]],
            warnings: Vec::new(),
        };
    }
    let measured = statements
        .iter()
        .flat_map(|s| &s.ops)
        .any(|o| o.actual_rows.is_some());
    let mut columns: Vec<String> = ["Operation", "Object", "Estimated rows", "Estimated cost"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if measured {
        columns.push("Actual rows".to_string());
        columns.push("Executions".to_string());
    }
    let headed = statements.len() > 1;
    let mut rows = Vec::new();
    let mut warnings = Vec::new();
    for (n, stmt) in statements.iter().enumerate() {
        let indent = if headed {
            let mut head = vec![format!("Statement {}: {}", n + 1, stmt.kind)];
            head.resize(columns.len(), String::new());
            rows.push(head);
            1
        } else {
            0
        };
        let first = rows.len();
        for op in &stmt.ops {
            let r = rows.len();
            let name = if op.logical.is_empty() || op.logical == op.physical {
                op.physical.clone()
            } else {
                format!("{} ({})", op.physical, op.logical)
            };
            let mut row = vec![
                format!("{}{name}", "  ".repeat(op.depth + indent)),
                op.object.clone().unwrap_or_default(),
                op.est_rows.clone(),
                op.est_cost.clone(),
            ];
            if measured {
                row.push(op.actual_rows.map(|v| v.to_string()).unwrap_or_default());
                row.push(op.executions.map(|v| v.to_string()).unwrap_or_default());
            }
            rows.push(row);
            let label = match &op.table {
                Some(t) => format!("`{t}`"),
                None => "a table".to_string(),
            };
            match op.physical.as_str() {
                "Table Scan" | "Clustered Index Scan" => warnings.push(PlanWarning {
                    row: r,
                    kind: PlanWarningKind::FullScan,
                    message: format!("Full scan of {label} ({})", op.physical),
                }),
                "Sort" => warnings.push(PlanWarning {
                    row: r,
                    kind: PlanWarningKind::Filesort,
                    message: "Sort — the query sorts rows on the fly".to_string(),
                }),
                p if p.ends_with("Spool") => warnings.push(PlanWarning {
                    row: r,
                    kind: PlanWarningKind::TempTable,
                    message: format!("{p} — an intermediate worktable in tempdb"),
                }),
                _ => {}
            }
            for (kind, text) in &op.warnings {
                warnings.push(PlanWarning {
                    row: r,
                    kind: *kind,
                    message: format!("{name}: {text}"),
                });
            }
        }
        for (kind, text) in &stmt.advice {
            warnings.push(PlanWarning {
                row: first,
                kind: *kind,
                message: text.clone(),
            });
        }
    }
    QueryPlan {
        columns,
        rows,
        warnings,
    }
}

/// A missing index being read out of `<MissingIndexGroup>`.
#[derive(Default)]
struct MissingIndex {
    impact: String,
    table: String,
    usage: String,
    keys: Vec<String>,
    include: Vec<String>,
}

impl MissingIndex {
    fn message(&self) -> String {
        let mut cols = self.keys.join(", ");
        if !self.include.is_empty() {
            cols.push_str("; include ");
            cols.push_str(&self.include.join(", "));
        }
        let impact = self
            .impact
            .parse::<f64>()
            .map(|v| format!(" — the server expected a {v:.0}% improvement"))
            .unwrap_or_default();
        format!("Missing index on `{}` ({cols}){impact}", self.table)
    }
}

/// SQL Server's `[name]` quoting off, `]]` back to `]`.
fn unbracket(s: &str) -> String {
    s.strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .map(|s| s.replace("]]", "]"))
        .unwrap_or_else(|| s.to_string())
}

/// A multi-part name as a plan writes one — `[db].[dbo].[f]` — with each
/// part's brackets off: `db.dbo.f`. A `.` inside brackets stays in its part.
fn unbracket_parts(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '[' if !quoted => quoted = true,
            ']' if quoted && chars.peek() == Some(&']') => {
                chars.next();
                out.push(']');
            }
            ']' if quoted => quoted = false,
            c => out.push(c),
        }
    }
    out
}

/// One statement being read: its index in the output, the `<RelOp>`s open
/// around the reader inside it (indexes into its `ops`), and how many of its
/// own `<QueryPlan>`s are open — an operator is the statement's only inside
/// one of them.
struct OpenStatement {
    at: usize,
    ops: Vec<usize>,
    plans: usize,
}

/// What an element under `<Warnings>` means, in the words the table shows;
/// one this does not know is named as the server named it.
fn showplan_warning(
    name: &str,
    attr: &dyn Fn(&str) -> Option<String>,
) -> (PlanWarningKind, String) {
    let text = match name {
        "NoJoinPredicate" => "no join predicate — every row pairs with every row".to_string(),
        "ColumnsWithNoStatistics" => "a column it reads has no statistics".to_string(),
        "SpillToTempDb" | "HashSpillDetails" | "SortSpillDetails" | "ExchangeSpillDetails" => {
            return (
                PlanWarningKind::TempTable,
                "spilled to tempdb — its memory grant was too small".to_string(),
            );
        }
        "PlanAffectingConvert" => {
            let issue = attr("ConvertIssue").unwrap_or_default();
            let expr = attr("Expression").unwrap_or_default();
            format!("a type conversion affects the plan ({issue}): {expr}")
        }
        "UnmatchedIndexes" => "a filtered index could not be used".to_string(),
        "MemoryGrantWarning" => "its memory grant was misjudged".to_string(),
        "Wait" => {
            let t = attr("WaitType").unwrap_or_default();
            format!("waited on {t}")
        }
        other => format!("the server warns: {other}"),
    };
    (PlanWarningKind::ServerWarning, text)
}

/// Every planned statement one document holds, in document order, or why it
/// is not a plan.
///
/// **Statements nest**, so the open ones are a stack. A scalar function's
/// statements sit *inside* the calling query's `<StmtSimple>`, after its
/// `<QueryPlan>`, under `<UDF><Statements>` (a procedure's under
/// `<StoredProc>`), and an `IF`'s branches inside its `<StmtCond>`, whose own
/// `<Condition>` carries the condition's plan. The reader kept one current
/// statement and began a new one at every `<StmtSimple>`, so a function's plan
/// replaced the query's — which was lost, warnings and all — and an `IF`'s
/// condition, under no `<StmtSimple>`, was dropped. Each statement is placed
/// in the output where it opens, so the query comes before the function it
/// calls, and a statement inside a function or procedure is headed by whose
/// it is.
///
/// **A statement is whatever sits directly under `<Statements>`**, not a list
/// of the element names one has met. The list was `StmtSimple`/`StmtCond`, and
/// a cursor is a `<StmtCursor>` — its plan under `<CursorPlan><Operation>` —
/// so its operators went to whichever statement was still open: the query
/// calling a function that opened it (drawn as the query's own, at a
/// thousand times its cost), the `EXECUTE PROC` line of a procedure that did,
/// or nothing at all for a cursor declared in the batch, which then read as
/// "no plan". And an operator joins a statement only inside one of that
/// statement's own `<QueryPlan>`s, so a plan this reader does not place is
/// dropped rather than added to the plan of the statement around it.
fn parse_showplan(doc: &str) -> Result<Vec<MsStatement>, String> {
    use quick_xml::XmlVersion;
    use quick_xml::events::{BytesStart, Event};

    if !doc.trim_start().starts_with('<') {
        return Err("not an XML document".to_string());
    }
    let attrs = |e: &BytesStart| -> Vec<(String, String)> {
        e.attributes()
            .flatten()
            .map(|a| {
                let key = String::from_utf8_lossy(a.key.local_name().as_ref()).into_owned();
                let value = a
                    .normalized_value(XmlVersion::Implicit1_0)
                    .map(|v| v.into_owned())
                    .unwrap_or_default();
                (key, value)
            })
            .collect()
    };
    let mut reader = quick_xml::Reader::from_str(doc);
    let mut out: Vec<MsStatement> = Vec::new();
    // The statements open around the reader, innermost last.
    let mut stack: Vec<OpenStatement> = Vec::new();
    // The functions and procedures open around it, as a heading names them.
    let mut modules: Vec<String> = Vec::new();
    // The elements open around it, outermost first.
    let mut path: Vec<Vec<u8>> = Vec::new();
    // Element depth inside a `<Warnings>`: 0 outside one, 1 at its children.
    let mut in_warnings = 0usize;
    let mut missing: Option<MissingIndex> = None;
    let mut seen_root = false;
    loop {
        let event = reader.read_event().map_err(|e| e.to_string())?;
        let (e, empty) = match &event {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(e) => {
                path.pop();
                if path.last().is_some_and(|p| p == b"Statements") {
                    stack.pop();
                }
                match e.local_name().as_ref() {
                    b"RelOp" => {
                        if let Some(top) = stack.last_mut() {
                            top.ops.pop();
                        }
                    }
                    b"QueryPlan" => {
                        if let Some(top) = stack.last_mut() {
                            top.plans = top.plans.saturating_sub(1);
                        }
                    }
                    b"UDF" | b"StoredProc" => {
                        modules.pop();
                    }
                    b"MissingIndex" => {
                        let top = stack.last().filter(|t| t.plans > 0);
                        if let (Some(m), Some(top)) = (missing.as_mut(), top) {
                            out[top.at]
                                .advice
                                .push((PlanWarningKind::MissingIndex, m.message()));
                            m.keys.clear();
                            m.include.clear();
                        }
                    }
                    b"MissingIndexGroup" => missing = None,
                    _ => {}
                }
                in_warnings = in_warnings.saturating_sub(1);
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let name = e.local_name();
        let name = std::str::from_utf8(name.as_ref()).unwrap_or("");
        let a = attrs(e);
        let attr = |k: &str| a.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        if !seen_root {
            if name != "ShowPlanXML" {
                return Err(format!("expected ShowPlanXML, found {name}"));
            }
            seen_root = true;
        }
        let statement = path.last().is_some_and(|p| p == b"Statements");
        if !empty {
            path.push(name.as_bytes().to_vec());
        }
        // The innermost open statement, and the operator open innermost in it
        // — while one of the statement's own plans is open, as everything a
        // plan reports about it (operators, warnings, advice) is.
        let top = stack
            .last()
            .filter(|t| t.plans > 0)
            .map(|t| (t.at, t.ops.last().copied()));
        if in_warnings == 1 {
            let warning = showplan_warning(name, &attr);
            if let Some((at, op)) = top {
                let s = &mut out[at];
                match op {
                    Some(i) => s.ops[i].warnings.push(warning),
                    None => s.advice.push(warning),
                }
            }
        }
        match name {
            _ if statement => {
                let mut kind = attr("StatementType").unwrap_or_default();
                if let Some(module) = modules.last() {
                    kind.push_str(&format!(" in {module}"));
                }
                out.push(MsStatement {
                    kind,
                    ..MsStatement::default()
                });
                if !empty {
                    stack.push(OpenStatement {
                        at: out.len() - 1,
                        ops: Vec::new(),
                        plans: 0,
                    });
                }
            }
            "QueryPlan" if !empty => {
                if let Some(t) = stack.last_mut() {
                    t.plans += 1;
                }
            }
            "UDF" | "StoredProc" if !empty => {
                let what = if name == "UDF" {
                    "function"
                } else {
                    "procedure"
                };
                let proc = attr("ProcName").map(|p| unbracket_parts(&p));
                modules.push(match proc {
                    Some(p) => format!("{what} {p}"),
                    None => format!("a {what}"),
                });
            }
            "RelOp" => {
                if let Some(t) = stack.last_mut().filter(|t| t.plans > 0) {
                    let s = &mut out[t.at];
                    s.ops.push(MsOp {
                        depth: t.ops.len(),
                        physical: attr("PhysicalOp").unwrap_or_default(),
                        logical: attr("LogicalOp").unwrap_or_default(),
                        est_rows: attr("EstimateRows").unwrap_or_default(),
                        est_cost: attr("EstimatedTotalSubtreeCost").unwrap_or_default(),
                        ..MsOp::default()
                    });
                    if !empty {
                        t.ops.push(s.ops.len() - 1);
                    }
                }
            }
            "Object" if in_warnings == 0 => {
                if let Some((at, Some(i))) = top
                    && out[at].ops[i].object.is_none()
                {
                    let part = |k: &str| attr(k).map(|v| unbracket(&v)).filter(|v| !v.is_empty());
                    let table = [part("Schema"), part("Table")]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(".");
                    let mut object = table.clone();
                    if let Some(index) = part("Index") {
                        object.push_str(&format!(" ({index})"));
                    }
                    if let Some(alias) = part("Alias") {
                        object.push_str(&format!(" {alias}"));
                    }
                    let op = &mut out[at].ops[i];
                    op.table = (!table.is_empty()).then_some(table);
                    op.object = (!object.is_empty()).then_some(object);
                }
            }
            "RunTimeCountersPerThread" => {
                if let Some((at, Some(i))) = top {
                    let n = |k: &str| attr(k).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
                    let op = &mut out[at].ops[i];
                    op.actual_rows = Some(op.actual_rows.unwrap_or(0) + n("ActualRows"));
                    op.executions = Some(op.executions.unwrap_or(0) + n("ActualExecutions"));
                }
            }
            "MissingIndexGroup" => {
                missing = Some(MissingIndex {
                    impact: attr("Impact").unwrap_or_default(),
                    ..MissingIndex::default()
                })
            }
            "MissingIndex" => {
                if let Some(m) = missing.as_mut() {
                    let part = |k: &str| attr(k).map(|v| unbracket(&v)).unwrap_or_default();
                    m.table = format!("{}.{}", part("Schema"), part("Table"));
                }
            }
            "ColumnGroup" => {
                if let Some(m) = missing.as_mut() {
                    m.usage = attr("Usage").unwrap_or_default();
                }
            }
            "Column" => {
                if let Some(m) = missing.as_mut() {
                    let col = attr("Name").map(|v| unbracket(&v)).unwrap_or_default();
                    if m.usage == "INCLUDE" {
                        m.include.push(col);
                    } else {
                        m.keys.push(col);
                    }
                }
            }
            _ => {}
        }
        if !empty && (in_warnings > 0 || name == "Warnings") {
            in_warnings += 1;
        }
    }
    if !seen_root {
        return Err("an empty document".to_string());
    }
    Ok(out)
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

    /// A `SET SHOWPLAN_XML ON` answer, trimmed from SQL Server 2022's own for a
    /// grouped join over AdventureWorksLT — the element and attribute names
    /// as the server writes them, the noise (`StatementSetOptions`,
    /// `OutputList`, `DefinedValues`, statistics) mostly cut.
    const SHOWPLAN: &str = r#"<ShowPlanXML xmlns="http://schemas.microsoft.com/sqlserver/2004/07/showplan" Version="1.564" Build="16.0.4295.3"><BatchSequence><Batch><Statements><StmtSimple StatementText="SELECT TOP 5 c.CompanyName, COUNT(*) AS n FROM SalesLT.Customer c JOIN SalesLT.SalesOrderHeader h ON h.CustomerID = c.CustomerID GROUP BY c.CompanyName ORDER BY n DESC" StatementId="1" StatementType="SELECT" StatementSubTreeCost="0.0612223" StatementEstRows="1.55988"><QueryPlan CachedPlanSize="40"><MissingIndexes><MissingIndexGroup Impact="39.2932"><MissingIndex Database="[AdventureWorksLT]" Schema="[SalesLT]" Table="[Customer]"><ColumnGroup Usage="INEQUALITY"><Column Name="[CompanyName]" ColumnId="8"></Column></ColumnGroup><ColumnGroup Usage="INCLUDE"><Column Name="[CustomerID]" ColumnId="1"></Column></ColumnGroup></MissingIndex></MissingIndexGroup></MissingIndexes><RelOp NodeId="0" PhysicalOp="Sort" LogicalOp="TopN Sort" EstimateRows="1.55988" EstimatedTotalSubtreeCost="0.0612223"><OutputList><ColumnReference Database="[AdventureWorksLT]" Schema="[SalesLT]" Table="[Customer]" Alias="[c]" Column="CompanyName"></ColumnReference></OutputList><TopSort Distinct="false" Rows="5"><RelOp NodeId="1" PhysicalOp="Stream Aggregate" LogicalOp="Aggregate" EstimateRows="1.55988" EstimatedTotalSubtreeCost="0.0498595"><StreamAggregate><RelOp NodeId="2" PhysicalOp="Nested Loops" LogicalOp="Inner Join" EstimateRows="1.55988" EstimatedTotalSubtreeCost="0.0498593"><Warnings><NoJoinPredicate></NoJoinPredicate></Warnings><NestedLoops Optimized="false"><RelOp NodeId="3" PhysicalOp="Clustered Index Scan" LogicalOp="Clustered Index Scan" EstimateRows="15.5988" EstimatedTotalSubtreeCost="0.0127253"><IndexScan Ordered="true"><Object Database="[AdventureWorksLT]" Schema="[SalesLT]" Table="[Customer]" Index="[PK_Customer_CustomerID]" Alias="[c]" IndexKind="Clustered" Storage="RowStore"></Object></IndexScan></RelOp><RelOp NodeId="4" PhysicalOp="Index Seek" LogicalOp="Index Seek" EstimateRows="0.1" EstimatedTotalSubtreeCost="0.0371308"><IndexScan Ordered="true"><Object Database="[AdventureWorksLT]" Schema="[SalesLT]" Table="[SalesOrderHeader]" Index="[IX_SalesOrderHeader_CustomerID]" Alias="[h]" IndexKind="NonClustered" Storage="RowStore"></Object></IndexScan></RelOp></NestedLoops></RelOp></StreamAggregate></RelOp></TopSort></RelOp></QueryPlan></StmtSimple></Statements></Batch></BatchSequence></ShowPlanXML>"#;

    fn showplan_rs(docs: &[&str]) -> ResultSet {
        let rows: Vec<&[&str]> = docs.iter().map(std::slice::from_ref).collect();
        rs(&[SHOWPLAN_COLUMN], &rows)
    }

    /// **SQL Server's plan is a document, not a table** — one XML cell per
    /// statement under the column `SET SHOWPLAN_XML` names — and it is walked
    /// into the same table every other engine's plan is: one row per operator,
    /// indented by depth, with what it reads and the optimiser's estimates.
    #[test]
    fn a_sql_server_showplan_becomes_one_row_per_operator() {
        let plan = QueryPlan::from_result(&showplan_rs(&[SHOWPLAN]));
        assert_eq!(
            plan.columns,
            ["Operation", "Object", "Estimated rows", "Estimated cost"]
        );
        let ops: Vec<&str> = plan.rows.iter().map(|r| r[0].as_str()).collect();
        assert_eq!(
            ops,
            [
                "Sort (TopN Sort)",
                "  Stream Aggregate (Aggregate)",
                "    Nested Loops (Inner Join)",
                "      Clustered Index Scan",
                "      Index Seek",
            ]
        );
        // The object is the operator's own, not a descendant's or an output
        // column's; the brackets are the server's quoting, not the name.
        assert_eq!(plan.rows[0][1], "");
        assert_eq!(
            plan.rows[3][1],
            "SalesLT.Customer (PK_Customer_CustomerID) c"
        );
        assert_eq!(
            plan.rows[4][1],
            "SalesLT.SalesOrderHeader (IX_SalesOrderHeader_CustomerID) h"
        );
        assert_eq!(
            (plan.rows[4][2].as_str(), plan.rows[4][3].as_str()),
            ("0.1", "0.0371308")
        );
    }

    /// The warnings read what SQL Server itself reports — a missing index, a
    /// join with no predicate — beside the scans and sorts the other engines'
    /// heuristics flag. A seek is not a scan.
    #[test]
    fn a_sql_server_plan_warns_from_its_operators_and_its_own_advice() {
        let plan = QueryPlan::from_result(&showplan_rs(&[SHOWPLAN]));
        let at = |kind| {
            plan.warnings
                .iter()
                .filter(|w| w.kind == kind)
                .map(|w| (w.row, w.message.as_str()))
                .collect::<Vec<_>>()
        };
        assert_eq!(at(PlanWarningKind::FullScan).len(), 1);
        assert_eq!(at(PlanWarningKind::FullScan)[0].0, 3);
        assert!(
            at(PlanWarningKind::FullScan)[0]
                .1
                .contains("`SalesLT.Customer`")
        );
        assert_eq!(at(PlanWarningKind::Filesort)[0].0, 0);
        let missing = at(PlanWarningKind::MissingIndex);
        assert_eq!(missing.len(), 1);
        assert!(
            missing[0].1.contains("`SalesLT.Customer`"),
            "{}",
            missing[0].1
        );
        assert!(missing[0].1.contains("CompanyName"), "{}", missing[0].1);
        assert!(missing[0].1.contains("39%"), "{}", missing[0].1);
        let server = at(PlanWarningKind::ServerWarning);
        assert_eq!(server.len(), 1);
        assert_eq!(server[0].0, 2);
        assert!(server[0].1.contains("no join predicate"), "{}", server[0].1);
        // And the prompt carries the table and the issues, as for any engine.
        let txt = plan.to_prompt_text();
        assert!(txt.contains("Operation | Object"), "{txt}");
        assert!(txt.contains("Missing index"), "{txt}");
    }

    /// `STATISTICS XML` — the measured form — adds what actually happened,
    /// summed over the threads that ran each operator; a statement with no
    /// plan of its own (a `SET`, a `DECLARE`) contributes no rows, and more
    /// than one planned statement is headed by which it is.
    #[test]
    fn a_measured_sql_server_plan_adds_the_actual_counts() {
        let measured = r#"<ShowPlanXML xmlns="http://schemas.microsoft.com/sqlserver/2004/07/showplan"><BatchSequence><Batch><Statements><StmtSimple StatementType="SELECT" StatementText="SELECT * FROM t"><QueryPlan><RelOp NodeId="0" PhysicalOp="Table Scan" LogicalOp="Table Scan" EstimateRows="10" EstimatedTotalSubtreeCost="0.003"><RunTimeInformation><RunTimeCountersPerThread Thread="1" ActualRows="7" ActualExecutions="1"></RunTimeCountersPerThread><RunTimeCountersPerThread Thread="2" ActualRows="5" ActualExecutions="1"></RunTimeCountersPerThread></RunTimeInformation><TableScan><Object Database="[d]" Schema="[dbo]" Table="[t]"></Object></TableScan></RelOp></QueryPlan></StmtSimple></Statements></Batch></BatchSequence></ShowPlanXML>"#;
        let plan = QueryPlan::from_result(&showplan_rs(&[measured]));
        assert_eq!(
            plan.columns,
            [
                "Operation",
                "Object",
                "Estimated rows",
                "Estimated cost",
                "Actual rows",
                "Executions"
            ]
        );
        assert_eq!(plan.rows.len(), 1);
        assert_eq!(plan.rows[0][1], "dbo.t");
        assert_eq!(
            (plan.rows[0][4].as_str(), plan.rows[0][5].as_str()),
            ("12", "2")
        );
        assert_eq!(plan.warnings[0].kind, PlanWarningKind::FullScan);

        let plain = r#"<ShowPlanXML xmlns="x"><BatchSequence><Batch><Statements><StmtSimple StatementType="SET ON/OFF"></StmtSimple></Statements></Batch></BatchSequence></ShowPlanXML>"#;
        let plan = QueryPlan::from_result(&showplan_rs(&[plain, SHOWPLAN, measured]));
        let ops: Vec<&str> = plan.rows.iter().map(|r| r[0].as_str()).collect();
        assert_eq!(ops[0], "Statement 1: SELECT");
        assert_eq!(ops[1], "  Sort (TopN Sort)");
        assert_eq!(ops[6], "Statement 2: SELECT");
        assert_eq!(ops[7], "  Table Scan");
        // Warnings point at the row they are about, headers and all.
        assert!(
            plan.warnings
                .iter()
                .any(|w| w.row == 7 && w.kind == PlanWarningKind::FullScan)
        );
        // Only one document measured, so no row claims an actual it lacks:
        // the estimated statement's actual cells are blank.
        assert_eq!(plan.rows[1].len(), 6);
        assert_eq!(plan.rows[1][4], "");
    }

    /// SQL Server 2022's own estimated plan (captured whole, from a scratch
    /// database) of `SELECT id, dbo.f(v) AS n FROM dbo.t WHERE s = N'a'`,
    /// `dbo.f` a scalar function `WITH INLINE = OFF`: the function's statements
    /// sit **inside** the query's `<StmtSimple>`, after its `<QueryPlan>`,
    /// under `<UDF><Statements>`.
    const SHOWPLAN_UDF: &str = r#"<ShowPlanXML xmlns="http://schemas.microsoft.com/sqlserver/2004/07/showplan" Version="1.564" Build="16.0.4295.3"><BatchSequence><Batch><Statements><StmtSimple StatementText="SELECT id, dbo.f(v) AS n FROM dbo.t WHERE s = N&apos;a&apos;" StatementId="1" StatementCompId="1" StatementType="SELECT" RetrievedFromCache="false" StatementSubTreeCost="0.00328466" StatementEstRows="1" SecurityPolicyApplied="false" StatementOptmLevel="TRIVIAL" QueryHash="0x1789A53103E6CE47" QueryPlanHash="0x663DEBD53ED321D1" CardinalityEstimationModelVersion="160"><StatementSetOptions QUOTED_IDENTIFIER="true" ARITHABORT="false" CONCAT_NULL_YIELDS_NULL="true" ANSI_NULLS="true" ANSI_PADDING="true" ANSI_WARNINGS="true" NUMERIC_ROUNDABORT="false"></StatementSetOptions><QueryPlan NonParallelPlanReason="TSQLUserDefinedFunctionsNotParallelizable" CachedPlanSize="24" CompileTime="373" CompileCPU="342" CompileMemory="136"><MemoryGrantInfo SerialRequiredMemory="0" SerialDesiredMemory="0" GrantedMemory="0" MaxUsedMemory="0"></MemoryGrantInfo><OptimizerHardwareDependentProperties EstimatedAvailableMemoryGrant="54289" EstimatedPagesCached="81433" EstimatedAvailableDegreeOfParallelism="12" MaxCompileMemory="7817112"></OptimizerHardwareDependentProperties><OptimizerStatsUsage><StatisticsInfo LastUpdate="2026-09-30T23:08:09.56" ModificationCount="0" SamplingPercent="100" Statistics="[_WA_Sys_00000003_35BCFE0A]" Table="[t]" Schema="[dbo]" Database="[zz_acct_s6]"></StatisticsInfo></OptimizerStatsUsage><RelOp NodeId="0" PhysicalOp="Compute Scalar" LogicalOp="Compute Scalar" EstimateRows="1" EstimateIO="0" EstimateCPU="1e-07" AvgRowSize="15" EstimatedTotalSubtreeCost="0.00328466" Parallel="0" EstimateRebinds="0" EstimateRewinds="0" EstimatedExecutionMode="Row"><OutputList><ColumnReference Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Column="id"></ColumnReference><ColumnReference Column="Expr1002"></ColumnReference></OutputList><ComputeScalar><DefinedValues><DefinedValue><ColumnReference Column="Expr1002"></ColumnReference><ScalarOperator ScalarString="[zz_acct_s6].[dbo].[f]([zz_acct_s6].[dbo].[t].[v])"><UserDefinedFunction FunctionName="[zz_acct_s6].[dbo].[f]"><ScalarOperator><Identifier><ColumnReference Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Column="v"></ColumnReference></Identifier></ScalarOperator></UserDefinedFunction></ScalarOperator></DefinedValue></DefinedValues><RelOp NodeId="1" PhysicalOp="Clustered Index Scan" LogicalOp="Clustered Index Scan" EstimateRows="1" EstimatedRowsRead="2" EstimateIO="0.003125" EstimateCPU="0.0001592" AvgRowSize="21" EstimatedTotalSubtreeCost="0.0032842" TableCardinality="2" Parallel="0" EstimateRebinds="0" EstimateRewinds="0" EstimatedExecutionMode="Row"><OutputList><ColumnReference Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Column="id"></ColumnReference><ColumnReference Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Column="v"></ColumnReference></OutputList><IndexScan Ordered="0" ForcedIndex="0" ForceScan="0" NoExpandHint="0" Storage="RowStore"><DefinedValues><DefinedValue><ColumnReference Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Column="id"></ColumnReference></DefinedValue><DefinedValue><ColumnReference Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Column="v"></ColumnReference></DefinedValue></DefinedValues><Object Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Index="[PK__t__3213E83F90DFD4A8]" IndexKind="Clustered" Storage="RowStore"></Object><Predicate><ScalarOperator ScalarString="[zz_acct_s6].[dbo].[t].[s]=N&apos;a&apos;"><Compare CompareOp="EQ"><ScalarOperator><Identifier><ColumnReference Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Column="s"></ColumnReference></Identifier></ScalarOperator><ScalarOperator><Const ConstValue="N&apos;a&apos;"></Const></ScalarOperator></Compare></ScalarOperator></Predicate></IndexScan></RelOp></ComputeScalar></RelOp></QueryPlan><UDF ProcName="[zz_acct_s6].[dbo].[f]"><Statements><StmtSimple StatementText="CREATE FUNCTION dbo.f (@x int) RETURNS int WITH INLINE = OFF AS&#xa;BEGIN&#xa;    DECLARE @r int;&#xa;    SELECT @r = COUNT(*) FROM dbo.t WHERE v &gt; @x" StatementId="2" StatementCompId="3" StatementType="SELECT" RetrievedFromCache="false" StatementSubTreeCost="0.00328566" StatementEstRows="1" SecurityPolicyApplied="false" StatementOptmLevel="TRIVIAL" QueryHash="0x22980B521DDE57DA" QueryPlanHash="0x3B6A83E2489AFA48" CardinalityEstimationModelVersion="160"><StatementSetOptions QUOTED_IDENTIFIER="true" ARITHABORT="false" CONCAT_NULL_YIELDS_NULL="true" ANSI_NULLS="true" ANSI_PADDING="true" ANSI_WARNINGS="true" NUMERIC_ROUNDABORT="false"></StatementSetOptions><QueryPlan CachedPlanSize="16" CompileTime="0" CompileCPU="0" CompileMemory="136"><MemoryGrantInfo SerialRequiredMemory="0" SerialDesiredMemory="0" GrantedMemory="0" MaxUsedMemory="0"></MemoryGrantInfo><OptimizerHardwareDependentProperties EstimatedAvailableMemoryGrant="54289" EstimatedPagesCached="81433" EstimatedAvailableDegreeOfParallelism="12" MaxCompileMemory="7817112"></OptimizerHardwareDependentProperties><RelOp NodeId="0" PhysicalOp="Compute Scalar" LogicalOp="Compute Scalar" EstimateRows="1" EstimateIO="0" EstimateCPU="0" AvgRowSize="11" EstimatedTotalSubtreeCost="0.00328566" Parallel="0" EstimateRebinds="0" EstimateRewinds="0" EstimatedExecutionMode="Row"><OutputList><ColumnReference Column="Expr1002"></ColumnReference></OutputList><ComputeScalar><DefinedValues><DefinedValue><ColumnReference Column="Expr1002"></ColumnReference><ScalarOperator ScalarString="CONVERT_IMPLICIT(int,[Expr1003],0)"><Convert DataType="int" Style="0" Implicit="1"><ScalarOperator><Identifier><ColumnReference Column="Expr1003"></ColumnReference></Identifier></ScalarOperator></Convert></ScalarOperator></DefinedValue></DefinedValues><RelOp NodeId="1" PhysicalOp="Stream Aggregate" LogicalOp="Aggregate" EstimateRows="1" EstimateIO="0" EstimateCPU="1.1e-06" AvgRowSize="11" EstimatedTotalSubtreeCost="0.00328566" Parallel="0" EstimateRebinds="0" EstimateRewinds="0" EstimatedExecutionMode="Row"><OutputList><ColumnReference Column="Expr1003"></ColumnReference></OutputList><StreamAggregate><DefinedValues><DefinedValue><ColumnReference Column="Expr1003"></ColumnReference><ScalarOperator ScalarString="Count(*)"><Aggregate AggType="countstar" Distinct="0"></Aggregate></ScalarOperator></DefinedValue></DefinedValues><RelOp NodeId="2" PhysicalOp="Clustered Index Scan" LogicalOp="Clustered Index Scan" EstimateRows="1" EstimatedRowsRead="2" EstimateIO="0.003125" EstimateCPU="0.0001592" AvgRowSize="11" EstimatedTotalSubtreeCost="0.0032842" TableCardinality="2" Parallel="0" EstimateRebinds="0" EstimateRewinds="0" EstimatedExecutionMode="Row"><OutputList></OutputList><IndexScan Ordered="0" ForcedIndex="0" ForceScan="0" NoExpandHint="0" Storage="RowStore"><DefinedValues></DefinedValues><Object Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Index="[PK__t__3213E83F90DFD4A8]" IndexKind="Clustered" Storage="RowStore"></Object><Predicate><ScalarOperator ScalarString="[zz_acct_s6].[dbo].[t].[v]&gt;[@x]"><Compare CompareOp="GT"><ScalarOperator><Identifier><ColumnReference Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Column="v"></ColumnReference></Identifier></ScalarOperator><ScalarOperator><Identifier><ColumnReference Column="@x"></ColumnReference></Identifier></ScalarOperator></Compare></ScalarOperator></Predicate></IndexScan></RelOp></StreamAggregate></RelOp></ComputeScalar></RelOp><ParameterList><ColumnReference Column="@x" ParameterDataType="int" ParameterCompiledValue="NULL"></ColumnReference></ParameterList></QueryPlan></StmtSimple><StmtSimple StatementText=";&#xa;    RETURN @r" StatementId="3" StatementCompId="4" StatementType="RETURN" RetrievedFromCache="false"></StmtSimple></Statements></UDF></StmtSimple></Statements></Batch></BatchSequence></ShowPlanXML>"#;

    /// SQL Server 2022's own estimated plan (captured whole) of `IF EXISTS
    /// (SELECT 1 FROM dbo.t WHERE v = 2) SELECT id FROM dbo.t WHERE s =
    /// N'b'`: the condition's plan is a `<StmtCond>`'s `<Condition>`, and the
    /// branch a `<StmtSimple>` inside its `<Then>`.
    const SHOWPLAN_IF: &str = r#"<ShowPlanXML xmlns="http://schemas.microsoft.com/sqlserver/2004/07/showplan" Version="1.564" Build="16.0.4295.3"><BatchSequence><Batch><Statements><StmtCond StatementText="IF EXISTS (SELECT 1 FROM dbo.t WHERE v = 2)" StatementId="1" StatementCompId="1" StatementType="COND WITH QUERY" RetrievedFromCache="false" StatementSubTreeCost="0.0032906" StatementEstRows="1" SecurityPolicyApplied="false" StatementOptmLevel="FULL" QueryHash="0x2C5778B6680D2163" QueryPlanHash="0x1FB8EF2D628E8B85" StatementOptmEarlyAbortReason="GoodEnoughPlanFound" CardinalityEstimationModelVersion="160"><StatementSetOptions QUOTED_IDENTIFIER="true" ARITHABORT="false" CONCAT_NULL_YIELDS_NULL="true" ANSI_NULLS="true" ANSI_PADDING="true" ANSI_WARNINGS="true" NUMERIC_ROUNDABORT="false"></StatementSetOptions><Condition><QueryPlan CachedPlanSize="16" CompileTime="195" CompileCPU="168" CompileMemory="216"><MemoryGrantInfo SerialRequiredMemory="0" SerialDesiredMemory="0" GrantedMemory="0" MaxUsedMemory="0"></MemoryGrantInfo><OptimizerHardwareDependentProperties EstimatedAvailableMemoryGrant="54289" EstimatedPagesCached="81433" EstimatedAvailableDegreeOfParallelism="12" MaxCompileMemory="7817112"></OptimizerHardwareDependentProperties><OptimizerStatsUsage><StatisticsInfo LastUpdate="2026-09-30T23:08:10.24" ModificationCount="0" SamplingPercent="100" Statistics="[_WA_Sys_00000002_35BCFE0A]" Table="[t]" Schema="[dbo]" Database="[zz_acct_s6]"></StatisticsInfo></OptimizerStatsUsage><RelOp NodeId="0" PhysicalOp="Compute Scalar" LogicalOp="Compute Scalar" EstimateRows="1" EstimateIO="0" EstimateCPU="1e-07" AvgRowSize="11" EstimatedTotalSubtreeCost="0.0032906" Parallel="0" EstimateRebinds="0" EstimateRewinds="0" EstimatedExecutionMode="Row"><OutputList><ColumnReference Column="Expr1003"></ColumnReference></OutputList><ComputeScalar><DefinedValues><DefinedValue><ColumnReference Column="Expr1003"></ColumnReference><ScalarOperator ScalarString="CASE WHEN [Expr1004] THEN (1) ELSE (0) END"><IF><Condition><ScalarOperator><Identifier><ColumnReference Column="Expr1004"></ColumnReference></Identifier></ScalarOperator></Condition><Then><ScalarOperator><Const ConstValue="(1)"></Const></ScalarOperator></Then><Else><ScalarOperator><Const ConstValue="(0)"></Const></ScalarOperator></Else></IF></ScalarOperator></DefinedValue></DefinedValues><RelOp NodeId="1" PhysicalOp="Nested Loops" LogicalOp="Left Semi Join" EstimateRows="1" EstimateIO="0" EstimateCPU="4.18e-06" AvgRowSize="9" EstimatedTotalSubtreeCost="0.0032905" Parallel="0" EstimateRebinds="0" EstimateRewinds="0" EstimatedExecutionMode="Row"><OutputList><ColumnReference Column="Expr1004"></ColumnReference></OutputList><NestedLoops Optimized="0"><DefinedValues><DefinedValue><ColumnReference Column="Expr1004"></ColumnReference></DefinedValue></DefinedValues><ProbeColumn><ColumnReference Column="Expr1004"></ColumnReference></ProbeColumn><RelOp NodeId="2" PhysicalOp="Constant Scan" LogicalOp="Constant Scan" EstimateRows="1" EstimateIO="0" EstimateCPU="1.157e-06" AvgRowSize="9" EstimatedTotalSubtreeCost="1.157e-06" Parallel="0" EstimateRebinds="0" EstimateRewinds="0" EstimatedExecutionMode="Row"><OutputList></OutputList><ConstantScan></ConstantScan></RelOp><RelOp NodeId="3" PhysicalOp="Clustered Index Scan" LogicalOp="Clustered Index Scan" EstimateRows="1" EstimatedRowsRead="2" EstimateIO="0.003125" EstimateCPU="0.0001592" AvgRowSize="11" EstimatedTotalSubtreeCost="0.0032842" TableCardinality="2" Parallel="0" EstimateRebinds="0" EstimateRewinds="0" EstimatedExecutionMode="Row"><OutputList></OutputList><IndexScan Ordered="0" ForcedIndex="0" ForceScan="0" NoExpandHint="0" Storage="RowStore"><DefinedValues></DefinedValues><Object Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Index="[PK__t__3213E83F90DFD4A8]" IndexKind="Clustered" Storage="RowStore"></Object><Predicate><ScalarOperator ScalarString="[zz_acct_s6].[dbo].[t].[v]=(2)"><Compare CompareOp="EQ"><ScalarOperator><Identifier><ColumnReference Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Column="v"></ColumnReference></Identifier></ScalarOperator><ScalarOperator><Const ConstValue="(2)"></Const></ScalarOperator></Compare></ScalarOperator></Predicate></IndexScan></RelOp></NestedLoops></RelOp></ComputeScalar></RelOp></QueryPlan></Condition><Then><Statements><StmtSimple StatementText=" SELECT id FROM dbo.t WHERE s = N&apos;b&apos;" StatementId="2" StatementCompId="2" StatementType="SELECT" RetrievedFromCache="false" StatementSubTreeCost="0.0032842" StatementEstRows="1" SecurityPolicyApplied="false" StatementOptmLevel="TRIVIAL" QueryHash="0xC880E133B782836E" QueryPlanHash="0x4A627152A98B55E4" CardinalityEstimationModelVersion="160" ParameterizedText="(@1 nvarchar(4000))SELECT [id] FROM [dbo].[t] WHERE [s]=@1"><StatementSetOptions QUOTED_IDENTIFIER="true" ARITHABORT="false" CONCAT_NULL_YIELDS_NULL="true" ANSI_NULLS="true" ANSI_PADDING="true" ANSI_WARNINGS="true" NUMERIC_ROUNDABORT="false"></StatementSetOptions><QueryPlan CachedPlanSize="24" CompileTime="0" CompileCPU="0" CompileMemory="112"><MemoryGrantInfo SerialRequiredMemory="0" SerialDesiredMemory="0" GrantedMemory="0" MaxUsedMemory="0"></MemoryGrantInfo><OptimizerHardwareDependentProperties EstimatedAvailableMemoryGrant="54289" EstimatedPagesCached="81433" EstimatedAvailableDegreeOfParallelism="12" MaxCompileMemory="7817112"></OptimizerHardwareDependentProperties><OptimizerStatsUsage><StatisticsInfo LastUpdate="2026-09-30T23:08:09.56" ModificationCount="0" SamplingPercent="100" Statistics="[_WA_Sys_00000003_35BCFE0A]" Table="[t]" Schema="[dbo]" Database="[zz_acct_s6]"></StatisticsInfo></OptimizerStatsUsage><RelOp NodeId="0" PhysicalOp="Clustered Index Scan" LogicalOp="Clustered Index Scan" EstimateRows="1" EstimatedRowsRead="2" EstimateIO="0.003125" EstimateCPU="0.0001592" AvgRowSize="17" EstimatedTotalSubtreeCost="0.0032842" TableCardinality="2" Parallel="0" EstimateRebinds="0" EstimateRewinds="0" EstimatedExecutionMode="Row"><OutputList><ColumnReference Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Column="id"></ColumnReference></OutputList><IndexScan Ordered="0" ForcedIndex="0" ForceScan="0" NoExpandHint="0" Storage="RowStore"><DefinedValues><DefinedValue><ColumnReference Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Column="id"></ColumnReference></DefinedValue></DefinedValues><Object Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Index="[PK__t__3213E83F90DFD4A8]" IndexKind="Clustered" Storage="RowStore"></Object><Predicate><ScalarOperator ScalarString="[zz_acct_s6].[dbo].[t].[s]=[@1]"><Compare CompareOp="EQ"><ScalarOperator><Identifier><ColumnReference Database="[zz_acct_s6]" Schema="[dbo]" Table="[t]" Column="s"></ColumnReference></Identifier></ScalarOperator><ScalarOperator><Identifier><ColumnReference Column="@1"></ColumnReference></Identifier></ScalarOperator></Compare></ScalarOperator></Predicate></IndexScan></RelOp><ParameterList><ColumnReference Column="@1" ParameterDataType="nvarchar(4000)" ParameterCompiledValue="N&apos;b&apos;"></ColumnReference></ParameterList></QueryPlan></StmtSimple></Statements></Then></StmtCond></Statements></Batch></BatchSequence></ShowPlanXML>"#;

    /// **A query calling a scalar function is shown its own plan, first**,
    /// and the function's statement after it under a heading that says whose
    /// it is. The function's `<StmtSimple>` is nested inside the query's, and
    /// the reader started a new statement at every one — so the function's
    /// plan replaced the query's, which vanished, warning and all.
    #[test]
    fn a_function_called_by_a_query_does_not_replace_the_querys_plan() {
        let plan = QueryPlan::from_result(&showplan_rs(&[SHOWPLAN_UDF]));
        let ops: Vec<&str> = plan.rows.iter().map(|r| r[0].as_str()).collect();
        assert_eq!(
            ops,
            [
                "Statement 1: SELECT",
                "  Compute Scalar",
                "    Clustered Index Scan",
                "Statement 2: SELECT in function zz_acct_s6.dbo.f",
                "  Compute Scalar",
                "    Stream Aggregate (Aggregate)",
                "      Clustered Index Scan",
            ]
        );
        // The query's own cost, not the function's 0.00328566.
        assert_eq!(plan.rows[1][3], "0.00328466");
        assert_eq!(plan.rows[2][1], "dbo.t (PK__t__3213E83F90DFD4A8)");
        // Each scan warns on its own row.
        let scans: Vec<usize> = plan
            .warnings
            .iter()
            .filter(|w| w.kind == PlanWarningKind::FullScan)
            .map(|w| w.row)
            .collect();
        assert_eq!(scans, [2, 6]);
    }

    /// **An `IF`'s condition has a plan too**, under `<StmtCond>`'s
    /// `<Condition>` rather than a `<StmtSimple>`, and the reader dropped it:
    /// only the branch's plan was shown.
    #[test]
    fn an_if_conditions_plan_is_shown_before_its_branch() {
        let plan = QueryPlan::from_result(&showplan_rs(&[SHOWPLAN_IF]));
        let ops: Vec<&str> = plan.rows.iter().map(|r| r[0].as_str()).collect();
        assert_eq!(
            ops,
            [
                "Statement 1: COND WITH QUERY",
                "  Compute Scalar",
                "    Nested Loops (Left Semi Join)",
                "      Constant Scan",
                "      Clustered Index Scan",
                "Statement 2: SELECT",
                "  Clustered Index Scan",
            ]
        );
        assert_eq!(plan.rows[1][3], "0.0032906");
    }

    /// Estimated plans captured whole from SQL Server (`testdata/showplan`),
    /// over a scratch database holding `dbo.t (id, v)` (3 rows) and
    /// `dbo.big (id, v)` (2,000):
    ///
    /// - `function_cursor.xml` (2022): `SELECT id, dbo.fc(v) AS n FROM dbo.t`,
    ///   `dbo.fc` a scalar function `WITH INLINE = OFF` that opens
    ///   `DECLARE c CURSOR LOCAL FOR SELECT id FROM dbo.big WHERE v = @x`.
    /// - `procedure_cursor.xml` (2022): `EXEC dbo.pc`, which runs `SELECT …
    ///   WHERE id = 3`, then a cursor over `dbo.big`, then `SELECT … WHERE id
    ///   = 2`.
    /// - `batch_cursor.xml` (2022): `DECLARE c CURSOR FOR SELECT id FROM dbo.t
    ///   WHERE v > 1; OPEN c; FETCH NEXT FROM c; CLOSE c; DEALLOCATE c;`.
    /// - `nested_procedures.xml` (2025): `EXEC dbo.p1`, `p1` running `EXEC
    ///   dbo.p2` and then `SELECT id, dbo.f(v) AS n FROM dbo.t`, `p2` a
    ///   `SELECT` over `dbo.big` and `dbo.f` a scalar function.
    ///
    /// The two versions write these four shapes identically.
    const SHOWPLAN_FUNCTION_CURSOR: &str = include_str!("../testdata/showplan/function_cursor.xml");
    const SHOWPLAN_PROCEDURE_CURSOR: &str =
        include_str!("../testdata/showplan/procedure_cursor.xml");
    const SHOWPLAN_BATCH_CURSOR: &str = include_str!("../testdata/showplan/batch_cursor.xml");
    const SHOWPLAN_NESTED_PROCEDURES: &str =
        include_str!("../testdata/showplan/nested_procedures.xml");

    fn plan_ops(plan: &QueryPlan) -> Vec<&str> {
        plan.rows.iter().map(|r| r[0].as_str()).collect()
    }

    /// **A function's cursor is not the calling query's plan.** SQL Server
    /// writes a cursor as `<StmtCursor>`, which the reader did not open as a
    /// statement, so the cursor's operators — a worktable insert and a full
    /// scan of `dbo.big`, at about 1,300 times the query's cost — were added
    /// to the innermost statement still open: the query, whose own plan had
    /// already closed. They are the function's, under their own heading.
    #[test]
    fn a_cursor_in_a_function_is_its_own_statement_not_the_querys() {
        let plan = QueryPlan::from_result(&showplan_rs(&[SHOWPLAN_FUNCTION_CURSOR]));
        assert_eq!(
            plan_ops(&plan),
            [
                "Statement 1: SELECT",
                "  Compute Scalar",
                "    Clustered Index Scan",
                "Statement 2: DECLARE CURSOR in function zz_plan_s71_ab11.dbo.fc",
                "  Clustered Index Insert (Insert)",
                "    Compute Scalar",
                "      Clustered Index Scan",
            ]
        );
        // The query's own cost and table; the cursor's scan is of `dbo.big`.
        assert_eq!(plan.rows[1][3], "0.0032856");
        assert_eq!(plan.rows[2][1].split(' ').next(), Some("dbo.t"));
        assert_eq!(plan.rows[6][1].split(' ').next(), Some("dbo.big"));
    }

    /// **A procedure's cursor is the procedure's**: it was listed first, under
    /// the `EXECUTE PROC` line that called it and with no "in procedure",
    /// ahead of the procedure's own first `SELECT`.
    #[test]
    fn a_cursor_in_a_procedure_is_its_own_statement_in_order() {
        let plan = QueryPlan::from_result(&showplan_rs(&[SHOWPLAN_PROCEDURE_CURSOR]));
        assert_eq!(
            plan_ops(&plan),
            [
                "Statement 1: SELECT in procedure dbo.pc",
                "  Clustered Index Seek",
                "Statement 2: DECLARE CURSOR in procedure dbo.pc",
                "  Clustered Index Insert (Insert)",
                "    Compute Scalar",
                "      Clustered Index Scan",
                "Statement 3: SELECT in procedure dbo.pc",
                "  Clustered Index Seek",
            ]
        );
    }

    /// **A cursor typed in the editor has a plan**: its `DECLARE CURSOR` is
    /// the batch's one planned statement, and the reader — with no statement
    /// open to put its operators in — dropped them and said there was none.
    #[test]
    fn a_cursor_declared_in_the_batch_has_a_plan() {
        let plan = QueryPlan::from_result(&showplan_rs(&[SHOWPLAN_BATCH_CURSOR]));
        assert_eq!(
            plan_ops(&plan),
            [
                "Clustered Index Insert (Insert)",
                "  Compute Scalar",
                "    Clustered Index Scan",
            ]
        );
        assert!(
            plan.warnings
                .iter()
                .any(|w| w.row == 2 && w.kind == PlanWarningKind::FullScan),
            "{:?}",
            plan.warnings
        );
    }

    /// **A procedure's statements are headed by whose they are, and a nested
    /// call's heading ends with it**: `p2`'s `SELECT` is "in procedure
    /// dbo.p2", and `p1`'s own `SELECT` after the call is `p1`'s again — not
    /// `p2`'s, which it would be if `</StoredProc>` did not close the heading
    /// it opened — with the function that `SELECT` calls after it.
    #[test]
    fn nested_procedures_are_each_headed_by_their_own_name() {
        let plan = QueryPlan::from_result(&showplan_rs(&[SHOWPLAN_NESTED_PROCEDURES]));
        assert_eq!(
            plan_ops(&plan),
            [
                "Statement 1: SELECT in procedure dbo.p2",
                "  Clustered Index Scan",
                "Statement 2: SELECT in procedure dbo.p1",
                "  Compute Scalar",
                "    Clustered Index Scan",
                "Statement 3: SELECT in function zz_plan_s71_ab11.dbo.f",
                "  Compute Scalar",
                "    Stream Aggregate (Aggregate)",
                "      Clustered Index Scan",
            ]
        );
        assert_eq!(plan.rows[1][1].split(' ').next(), Some("dbo.big"));
        assert_eq!(plan.rows[4][1].split(' ').next(), Some("dbo.t"));
    }

    /// A document that is not a plan is shown as the server sent it, with the
    /// reason — never a panic, never an empty table that reads as "no plan".
    #[test]
    fn an_unreadable_showplan_is_shown_verbatim() {
        for bad in ["", "not xml", "<ShowPlanXML><unclosed", "<ShowPlanXML/>"] {
            let plan = QueryPlan::from_result(&showplan_rs(&[bad]));
            assert!(!plan.rows.is_empty(), "{bad:?}");
            assert!(plan.warnings.is_empty());
        }
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
