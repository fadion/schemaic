# `core/plan.rs`

`plan.rs` — `QueryPlan::from_result` parses an `EXPLAIN` result into a table + heuristic
warnings (full scan / filesort / temp table), and carries the fetch's `truncated` — a plan
of a table partitioned by day runs two rows a partition and can reach the 10,000-row cap, and
one cut there must not read as whole (`a_capped_plan_carries_its_cut`); `to_prompt_text` for
the AI — the plan modal's hand-off. The MCP server's `explain` reply is its own, capped
`format_plan` (`schemaic-cli`'s `mcp.rs`).
**A warning carries what it found, not a sentence**: `PlanWarning::note` is a `PlanNote`, a
variant per engine's finding with its table, operator, server warning or index columns as
fields, so the plan modal can word it in the interface's language (under `plan_view.rs`).
`PlanWarning::message` — `PlanNote::message`, and `ServerWarning::message` inside it — is the
English, and stays the English for `to_prompt_text`, the MCP server's `explain` and
`schemaic explain`'s notes. Neither enum has a `Display`; the English is asked for by name. A
table the plan names none for is `None` rather than a stand-in word, because the stand-in is
not one word: MySQL's English says *the result*, PostgreSQL's and SQL Server's *a table*.
**SQL Server's plan is not a table but an XML document per statement**, and it is read into
the same tabular `QueryPlan` rather than a second shape the modal would have to learn: a result
whose one column is `SHOWPLAN_COLUMN` (`Microsoft SQL Server 2005 XML Showplan`, the name
`SET SHOWPLAN_XML` and `SET STATISTICS XML` answer under) goes to `showplan`, which walks each
document with `parse_showplan` over quick-xml — a direct dependency of `schemaic-core` for it,
at the 0.41 calamine already puts in the graph (under `import.rs`), because this document too
arrives from the server. Each `<RelOp>` is a row, indented two spaces per depth, as
`Physical (Logical)`, its *Object* `schema.table (index) alias` from the operator's own
`<Object>`, its estimated rows and cost, and — when the plan was measured — *Actual rows* and
*Executions* summed over `RunTimeCountersPerThread`. More than one planned statement gets a
heading row each; a statement with no plan (a `SET`, a `DECLARE`) contributes nothing, and a
batch with none at all is the one row *"No statement in this batch has a plan."* **Statements
nest, so the open ones are a stack**, each placed in the output where it opens: a scalar
function's statements sit inside the calling query's `<StmtSimple>`, after its `<QueryPlan>`,
under `<UDF><Statements>` (a procedure's under `<StoredProc>`), and an `IF`'s branches inside
its `<StmtCond>`, whose `<Condition>` holds the condition's own plan (both captured from 2022,
and the same on 2025). The reader once kept one current statement and began afresh at every
`<StmtSimple>`, so a function's plan replaced the query's — which vanished, its warnings with
it — and an `IF`'s condition was dropped. Now the query comes first, the function's statement
after it headed *in function db.dbo.f*, and the condition before its branch
(`a_function_called_by_a_query_does_not_replace_the_querys_plan`,
`an_if_conditions_plan_is_shown_before_its_branch`, over the server's own documents). **A
statement is whatever sits directly under `<Statements>`**, not a list of the names met so far:
the list was `StmtSimple`/`StmtCond`, and a cursor is a `<StmtCursor>` (its plan under
`<CursorPlan><Operation><QueryPlan>`), so its operators went to whatever statement was still
open — the query calling a function that opened one, drawn as the query's own at a thousand
times its cost; a procedure's `EXECUTE PROC` line, ahead of the procedure's first `SELECT`; or,
for a cursor declared in the batch, nowhere, which read as "no plan". And an operator, warning or
missing index joins a statement only **inside one of that statement's own `<QueryPlan>`s**, so a
plan the reader does not place is dropped rather than drawn into the plan around it. The four
shapes — a function's cursor, a procedure's, a batch's, and a procedure calling a procedure and
a function, whose headings pin that `</StoredProc>` closes the one it opened — are documents
captured whole from 2022 and 2025 under `core/testdata/showplan/`, and
`a_cursors_plan_is_read_as_its_own_statement` checks the live server still writes them so.
**A document
that does not read as a plan is shown verbatim under the reason** rather than as an empty table
that would claim there was no plan (`an_unreadable_showplan_is_shown_verbatim`).
**What of the table Schemaic wrote is said by `QueryPlan::wording`**, a `PlanWording`, so the
plan modal can put those words in the interface's language without the cells changing: `AsRead`
for every other engine's `EXPLAIN`, the server's throughout; `Showplan { measured, headings }`,
whose headers are `showplan_columns(measured)` and whose `headings` pair each heading row's
index with its `StatementHeading` — the number, the `StatementType` as SQL Server spells it, and
the `PlanModule` it runs in, a procedure or a function, named or not; `Unread(UnreadPlan)`, the
reason in row 0 under the one header `PLAN_COLUMN`, the documents verbatim after it; and
`NoPlan`, `NO_PLAN` under the same header. The cells stay the English they were —
`StatementHeading::text` and `UnreadPlan::text` write them — because `to_prompt_text`, the MCP
server's `explain` and `schemaic explain` read them, and all three are English. That is why the
module is a value: `MsStatement` used to splice *in function x* into its `kind`, a sentence no
translation could take apart again; and `parse_showplan` answers an `UnreadPlan` for a document
it cannot read. `a_plans_wording_says_what_its_cells_hold` holds each heading's row to its
`text`, every row beginning *Statement* to a heading, and each unread reason to row 0.
The heuristics are the other engines' in T-SQL's names — `Table Scan` and `Clustered Index Scan` are
`FullScan`, `Sort` is `Filesort`, any `…Spool` is `TempTable` — and two kinds exist for
this engine alone, since no other plan carries them: `MissingIndex`, from the plan's
`<MissingIndexes>` with the impact the server expected (`MissingIndex::note`, the impact
printed as a whole percentage), and `ServerWarning`, the plan's own `<Warnings>` (no join
predicate, a column with no statistics, a plan-affecting conversion…), except that a spill to
`tempdb` maps to `TempTable`. `showplan_warning` answers each element as a
`(PlanWarningKind, ServerWarning)` — an element it does not know is `ServerWarning::Other`
under the server's name — kept on its operator's `MsOp::warnings`, or, written on the statement
as a whole, as a `PlanNote::MsStatement` in `MsStatement::advice` beside the missing indexes.
`supports_plan` answers yes for every
engine now and is kept an exhaustive `match`, so an engine added later has to answer it rather
than inherit the yes (`a_sql_server_showplan_becomes_one_row_per_operator`,
`a_sql_server_plan_warns_from_its_operators_and_its_own_advice`,
`a_measured_sql_server_plan_adds_the_actual_counts`).
