# `ui/plan_view.rs`

`plan_view.rs` — Query Plan modal (`EXPLAIN`/`EXPLAIN ANALYZE` table + warnings + "Ask AI"),
via `TabsActions::run_plan` → `Db::explain`. On a read-only connection the plain plan runs on a
read-only session too, so on MySQL/MariaDB the plan of a write is refused, in
`mysql::read_only_plan_refusal`'s sentence, rather than drawn (`schemaic-db`'s entry). SQL Server's plan needed no change here: it
arrives as the same tabular `QueryPlan` (under `core::plan`), and the two warning kinds only
it produces, `MissingIndex` and `ServerWarning`, take the warning colour through the `_ =>`
arm that keeps `theme::error` for `FullScan` alone.
**Its words are the catalogue's** (`plan-*`) — the title (through
`modal_title_borderless`), the Analyze toggle's hint and the line that replaces the toggle for
a write (`plan-analyze-unavailable`, which keeps the word *Analyze*), *Explaining* under the
loading dots, *Potential issues* and *Ask AI*. The toggle's own **Analyze** label stays a
literal — it is `EXPLAIN ANALYZE`'s keyword, which AGENTS.md's spelling rule and *The string
catalogue*'s translation conventions both name — as do the server's plan columns and rows and the prompt
`ask_ai` sends the AI panel, English as all AI text is. **SQL Server's table is the
catalogue's as far as Schemaic wrote it**: `plan_table` measures and draws what the private
`shown_table` hands it — the plan's headers and cells as read where `QueryPlan::wording` is
`AsRead`, and otherwise the headers from `plan-ms-operation` … `plan-ms-executions` or
`plan-ms-plan`, each heading row's first cell from `statement_heading_text` (`plan-ms-statement`,
and a whole message each for a named and an unnamed procedure or function rather than a module
spliced in), and row 0's notice from `unread_plan_text` (`plan-ms-unread`, its reason a
`plan-ms-unread-*`) or `plan-ms-no-plan`. What stays as written is SQL Server's: the statement's
kind, the module's name, the operators and objects, and the XML reader's own error as an unread
reason. The plan's cells are not rewritten, since `ask_ai` sends them — the translation is a
copy made for drawing. `english_shows_a_plans_table_as_core_wrote_it` holds the English equal to
core's cells for every heading shape, estimated and measured, every unread reason, the batch
with no plan, and an `AsRead` table left untouched.
**Each warning is worded here**, from its `PlanNote` by `plan_note_text` and a server warning
inside one by `server_warning_text` — `plan-detail-*`, `plan-my-*`, `plan-pg-*`, `plan-ms-*`,
`plan-server-*` and `plan-missing-index` — with a message of its own for a table the plan does
not name (`-result` for MySQL's three, `-unnamed` for the PostgreSQL and SQL Server scans)
rather than a word spliced in, and the missing index's four shapes, with and without included
columns and an impact, four whole messages; the key and included columns are joined with
`", "` in code. What stays as written inside them is the plan's: operator, wait-type and table
names, the server's conversion issue and expression, and the quoted `Extra:`/`type =` text,
which names MySQL's plan columns. `english_says_every_plan_note_as_core_does` holds each line
equal to `PlanNote::message` — every engine's note with a table and without, every server
warning on an operator and on the statement, and the missing index in all four shapes.
