//! The performance reads as tables — what the MCP server's performance tools
//! answer with, over [`schemaic_core::workload`]'s model.
//!
//! Every reply is a [`ResultSet`] built here, so the tool's text goes through
//! the same capped pipe-table renderer as `run_query`'s rows, with typed
//! numeric cells rather than pre-formatted strings.
//!
//! **What an absent source says is decided here too** ([`unavailable_text`]):
//! the reason, then the one statement that would turn the source on where
//! there is one — for the user to run. The tool never runs it.

use schemaic_core::activity::LockList;
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::{Column, ResultSet, Value};
use schemaic_core::schema::DbSchema;
use schemaic_core::stats::SchemaStats;
use schemaic_core::workload::{
    self as wl, IndexFinding, IndexProblem, MissingIndex, SizeEntry, StatementStat, Unavailable,
};

fn col(name: &str, type_name: &str) -> Column {
    Column {
        name: name.into(),
        type_name: type_name.into(),
        origin: None,
    }
}

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

fn opt_text(s: Option<&str>) -> Value {
    s.map_or(Value::Null, |s| Value::Str(s.to_string()))
}

fn opt_uint(n: Option<u64>) -> Value {
    n.map_or(Value::Null, Value::UInt)
}

/// Milliseconds to three places — enough to tell a 0.2 ms lookup from a
/// 0.02 ms one without printing float noise.
fn ms(f: f64) -> Value {
    Value::Float((f * 1000.0).round() / 1000.0)
}

/// What a missing source says: why, then the statement that would turn it on
/// where one does.
pub fn unavailable_text(u: &Unavailable) -> String {
    match u.enable_sql() {
        Some(sql) => format!("{} To turn it on, the user can run: {sql}", u.reason()),
        None => u.reason(),
    }
}

/// The ranked statements, each text that carries a credential withheld
/// ([`wl::statement_text`]) — what both the MCP `top_statements` tool and
/// `schemaic statements` print, so neither can forget to ask.
pub fn statements_result(v: &[StatementStat], dialect: SqlDialect) -> ResultSet {
    ResultSet::from_rows(
        vec![
            col("total_ms", "DOUBLE"),
            col("mean_ms", "DOUBLE"),
            col("calls", "BIGINT"),
            col("rows", "BIGINT"),
            col("database", "TEXT"),
            col("query", "TEXT"),
        ],
        v.iter()
            .map(|s| {
                vec![
                    ms(s.total_ms),
                    ms(s.mean_ms),
                    Value::UInt(s.calls),
                    opt_uint(s.rows),
                    opt_text(s.database.as_deref()),
                    text(wl::statement_text(&s.query, dialect).to_string()),
                ]
            })
            .collect(),
    )
}

/// The qualified table name a finding is about.
fn table_name(schema: Option<&str>, table: &str) -> String {
    match schema {
        Some(s) => format!("{s}.{table}"),
        None => table.to_string(),
    }
}

/// The index findings, one row each.
pub fn index_health_result(f: &[IndexFinding]) -> ResultSet {
    ResultSet::from_rows(
        vec![
            col("table", "TEXT"),
            col("index", "TEXT"),
            col("problem", "TEXT"),
            col("detail", "TEXT"),
            col("bytes", "BIGINT"),
        ],
        f.iter()
            .map(|f| {
                let (problem, detail) = match &f.problem {
                    IndexProblem::Unused => ("unused", String::new()),
                    IndexProblem::Duplicate { of } => ("duplicate", format!("same as {of}")),
                    IndexProblem::Redundant { by } => ("redundant", format!("prefix of {by}")),
                };
                vec![
                    text(table_name(f.schema.as_deref(), &f.table)),
                    text(f.index.clone()),
                    text(problem),
                    text(detail),
                    opt_uint(f.bytes),
                ]
            })
            .collect(),
    )
}

/// One missing-index suggestion with the DDL that would create it.
pub struct Suggestion {
    pub index: MissingIndex,
    /// `ddl::diff`'s plan as the reader's script
    /// ([`wl::suggestion_ddl`]), or empty when the table could not be found
    /// in the schema read (dropped since the DMV saw it).
    pub ddl: String,
}

/// The suggestions, numbered to match [`suggestion_ddl_text`].
pub fn missing_result(v: &[Suggestion]) -> ResultSet {
    ResultSet::from_rows(
        missing_columns(),
        v.iter()
            .enumerate()
            .map(|(i, s)| missing_row(i, s))
            .collect(),
    )
}

fn missing_columns() -> Vec<Column> {
    vec![
        col("n", "INT"),
        col("table", "TEXT"),
        col("key", "TEXT"),
        col("include", "TEXT"),
        col("uses", "BIGINT"),
        col("impact_pct", "DOUBLE"),
    ]
}

/// Suggestion `i`'s row, numbered from one.
fn missing_row(i: usize, s: &Suggestion) -> Vec<Value> {
    let m = &s.index;
    let key: Vec<&str> = m
        .equality
        .iter()
        .chain(&m.inequality)
        .map(String::as_str)
        .collect();
    vec![
        Value::UInt(i as u64 + 1),
        text(table_name(m.schema.as_deref(), &m.table)),
        text(key.join(", ")),
        text(m.include.join(", ")),
        Value::UInt(m.uses),
        Value::Float((m.avg_impact * 10.0).round() / 10.0),
    ]
}

/// [`missing_result`] with each suggestion's DDL as a last column — what
/// `schemaic indexes --missing` prints. A CLI row is not cut the way a tool
/// reply's cell is, and a `--format json` reader wants the statement on its
/// row rather than in numbered text beside it. Null where the table was not in
/// the schema read.
pub fn missing_with_ddl_result(v: &[Suggestion]) -> ResultSet {
    let mut columns = missing_columns();
    columns.push(col("ddl", "TEXT"));
    ResultSet::from_rows(
        columns,
        v.iter()
            .enumerate()
            .map(|(i, s)| {
                let mut row = missing_row(i, s);
                let ddl = without_closing_go(&s.ddl);
                row.push(if ddl.is_empty() {
                    Value::Null
                } else {
                    text(ddl)
                });
                row
            })
            .collect(),
    )
}

/// `ddl` without trailing whitespace and without the editor script's closing
/// `GO` line — sqlcmd's batch separator, not T-SQL, so a driver handed the
/// cell would send it as a word. Only a last line that is `GO` alone goes.
fn without_closing_go(ddl: &str) -> &str {
    let ddl = ddl.trim_end();
    match ddl.rsplit_once('\n') {
        Some((head, last)) if last.trim().eq_ignore_ascii_case("go") => head.trim_end(),
        _ => ddl,
    }
}

/// Each suggestion's DDL, **whole** — a table cell is cut to a few dozen
/// characters, and a cut `CREATE INDEX` is no DDL at all. Numbered as
/// [`missing_result`] numbers them; a suggestion whose table the schema read
/// no longer holds says so instead.
pub fn suggestion_ddl_text(v: &[Suggestion]) -> String {
    let mut out = String::new();
    for (i, s) in v.iter().enumerate() {
        out.push_str(&format!("{}. ", i + 1));
        if s.ddl.is_empty() {
            out.push_str("(the table is not in the schema read; no DDL)\n");
        } else {
            out.push_str(s.ddl.trim_end());
            out.push('\n');
        }
    }
    out
}

/// The largest tables, with their row estimates.
pub fn table_sizes_result(v: &[SizeEntry]) -> ResultSet {
    ResultSet::from_rows(
        vec![
            col("table", "TEXT"),
            col("bytes", "BIGINT"),
            col("size", "TEXT"),
            col("rows_estimate", "BIGINT"),
        ],
        v.iter()
            .map(|e| {
                vec![
                    text(table_name(e.schema.as_deref(), &e.table)),
                    Value::UInt(e.bytes),
                    text(schemaic_core::stats::format_bytes(e.bytes)),
                    opt_uint(e.rows),
                ]
            })
            .collect(),
    )
}

/// The largest indexes, each with its table.
pub fn index_sizes_result(v: &[SizeEntry]) -> ResultSet {
    ResultSet::from_rows(
        vec![
            col("table", "TEXT"),
            col("index", "TEXT"),
            col("bytes", "BIGINT"),
            col("size", "TEXT"),
        ],
        v.iter()
            .map(|e| {
                vec![
                    text(table_name(e.schema.as_deref(), &e.table)),
                    opt_text(e.index.as_deref()),
                    Value::UInt(e.bytes),
                    text(schemaic_core::stats::format_bytes(e.bytes)),
                ]
            })
            .collect(),
    )
}

/// The locks held and waited for.
pub fn locks_result(list: &LockList) -> ResultSet {
    ResultSet::from_rows(
        vec![
            col("session", "BIGINT"),
            col("state", "TEXT"),
            col("kind", "TEXT"),
            col("mode", "TEXT"),
            col("object", "TEXT"),
            col("blocked_by", "TEXT"),
        ],
        list.locks
            .iter()
            .map(|l| {
                let blocked: Vec<String> = l.blocked_by.iter().map(i64::to_string).collect();
                vec![
                    Value::Int(l.session),
                    text(if l.granted { "holds" } else { "waits" }),
                    text(l.kind.clone()),
                    text(l.mode.clone()),
                    text(l.object.clone()),
                    text(blocked.join(", ")),
                ]
            })
            .collect(),
    )
}

/// Each suggestion with its DDL, emitted against the table as `schema` read
/// it ([`wl::suggestion_ddl`]).
pub fn suggestions(
    schema: &DbSchema,
    dialect: schemaic_core::intel::SqlDialect,
    missing: Vec<MissingIndex>,
) -> Vec<Suggestion> {
    missing
        .into_iter()
        .map(|m| {
            let ddl = schema
                .tables
                .iter()
                .find(|t| t.name == m.table && (m.schema.is_none() || t.schema == m.schema))
                .map(|t| {
                    wl::suggestion_ddl(
                        t,
                        &m,
                        schemaic_core::ddl::Target::new(dialect, schema.flavour),
                    )
                })
                .unwrap_or_default();
            Suggestion { index: m, ddl }
        })
        .collect()
}

/// The caveat every index answer carries: what "unused" can and cannot mean.
pub fn usage_note(stats: &SchemaStats) -> &'static str {
    if wl::usage_counted(stats) {
        "Usage counts run from the server's last restart or statistics reset, so an index used \
         only by a monthly job can look unused. Check before dropping one."
    } else {
        "This server reported no index usage counts (performance_schema off, or not granted), \
         so no index can be called unused — only duplicates and prefixes are shown."
    }
}

/// A line the CLI writes to stderr beside its rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// `schemaic: …` — what the answer is, or why there is none.
    Warn(String),
    /// `hint: …` — what to do about the line before it.
    Hint(String),
}

/// What a `schemaic` performance command prints and how it exits, decided
/// from what the read returned — pure, so the contract "exit 2 when the
/// server cannot answer, exit 0 when it answered empty" is tested rather
/// than written inline after a live `connect`, which is where a wrong exit 0
/// once sat unseen.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// The rows, written to stdout first; `None` when there is no answer.
    pub rows: Option<ResultSet>,
    pub notes: Vec<Note>,
    /// Did the server answer? `false` is exit 2: the answer is not there,
    /// and a script must not read its absence as an empty list.
    pub answered: bool,
}

fn warn(s: impl Into<String>) -> Note {
    Note::Warn(s.into())
}

/// `schemaic statements`: a source that is off is no answer, with the one
/// statement that turns it on (never run); one that is on and has counted
/// nothing yet is an empty answer, with a note.
pub fn statements_outcome(report: &wl::StatementReport, dialect: SqlDialect) -> Outcome {
    match report {
        wl::StatementReport::Ready { statements, since } => {
            let mut notes = Vec::new();
            if statements.is_empty() {
                notes.push(warn(
                    "the statistics source is on but has recorded no statements yet",
                ));
            }
            if let Some(since) = since {
                notes.push(warn(format!("counted since {since}")));
            }
            Outcome {
                rows: Some(statements_result(statements, dialect)),
                notes,
                answered: true,
            }
        }
        wl::StatementReport::Unavailable(u) => {
            let mut notes = vec![warn(u.reason())];
            if let Some(sql) = u.enable_sql() {
                notes.push(Note::Hint(format!("to turn it on, run: {sql}")));
            }
            Outcome {
                rows: None,
                notes,
                answered: false,
            }
        }
    }
}

/// `schemaic sizes`: the largest `limit` tables, or with `of_indexes` the
/// largest indexes. **No answer** where the engine reports no sizes at all
/// ([`wl::reports_sizes`], DuckDB) or, under `of_indexes`, sizes indexes per
/// table only ([`wl::index_sizes_per_table_only`], MySQL — a database of
/// primary keys alone included); an empty answer where the database has no
/// tables, or no indexes, to size.
pub fn sizes_outcome(
    stats: &SchemaStats,
    dialect: SqlDialect,
    database: &str,
    of_indexes: bool,
    limit: usize,
) -> Outcome {
    let none = |notes: Vec<Note>| Outcome {
        rows: None,
        notes,
        answered: false,
    };
    if !wl::reports_sizes(dialect) {
        return none(vec![warn(
            "this engine reports no table or index sizes, only row estimates",
        )]);
    }
    let (tables, indexes) = wl::largest(stats, limit);
    if !of_indexes {
        let notes = if tables.is_empty() {
            vec![warn(format!("{database} reported no table sizes"))]
        } else {
            Vec::new()
        };
        return Outcome {
            rows: Some(table_sizes_result(&tables)),
            notes,
            answered: true,
        };
    }
    if indexes.is_empty() && wl::index_sizes_per_table_only(stats) {
        return none(vec![warn(
            "this engine reports index sizes per table, not per index",
        )]);
    }
    let notes = if indexes.is_empty() {
        vec![warn(format!("{database} has no indexes to size"))]
    } else {
        Vec::new()
    };
    Outcome {
        rows: Some(index_sizes_result(&indexes)),
        notes,
        answered: true,
    }
}

/// `schemaic locks`: the list, with the MCP tool's wording for an empty one
/// and for a server that lists contended locks only, so the two say one
/// thing of one server. (An engine with no server sessions is refused before
/// anything is opened, by `supports_activity`.)
pub fn locks_outcome(list: &LockList) -> Outcome {
    let mut notes = Vec::new();
    match (list.locks.is_empty(), list.contended_only) {
        (true, true) => notes.push(warn(
            "no session is waiting on a lock (this server lists contended locks only)",
        )),
        (true, false) => notes.push(warn("no locks are held or waited for")),
        (false, true) => notes.push(warn("this server lists contended locks only")),
        (false, false) => {}
    }
    if list.truncated {
        notes.push(warn("the list was capped; more locks exist"));
    }
    Outcome {
        rows: Some(locks_result(list)),
        notes,
        answered: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(rs: &ResultSet, r: usize, c: usize) -> String {
        rs.cell(r, c)
            .map(|c| c.display().to_string())
            .unwrap_or_default()
    }

    #[test]
    fn a_missing_source_says_why_and_offers_only_a_one_statement_fix() {
        let t = unavailable_text(&Unavailable::ExtensionNotInstalled);
        assert!(t.contains("not installed"), "{t}");
        assert!(t.ends_with("CREATE EXTENSION pg_stat_statements;"), "{t}");
        let t = unavailable_text(&Unavailable::PerformanceSchemaOff);
        assert!(!t.contains("run:"), "{t}");
    }

    #[test]
    fn statements_lead_with_the_cost_and_end_with_the_text() {
        let rs = statements_result(
            &[StatementStat {
                query: "SELECT * FROM t WHERE id = ?".into(),
                database: Some("app".into()),
                calls: 3,
                total_ms: 1.234_56,
                mean_ms: 0.411_52,
                rows: None,
            }],
            SqlDialect::MySql,
        );
        assert_eq!(rs.columns[0].name, "total_ms");
        assert_eq!(cell(&rs, 0, 0), "1.235");
        assert_eq!(cell(&rs, 0, 2), "3");
        assert!(rs.cell(0, 3).is_some_and(|c| c.is_null()));
        assert_eq!(cell(&rs, 0, 5), "SELECT * FROM t WHERE id = ?");
    }

    // ── The perf commands' exit decisions (S6.2-L6-01) ──

    fn stats_of(tables: Vec<schemaic_core::stats::TableStats>) -> SchemaStats {
        SchemaStats::new(tables)
    }

    fn table_stats(
        name: &str,
        data: Option<u64>,
        index: Option<u64>,
        indexes: Vec<(&str, Option<u64>)>,
    ) -> schemaic_core::stats::TableStats {
        schemaic_core::stats::TableStats {
            table: name.into(),
            data_bytes: data,
            index_bytes: index,
            indexes: indexes
                .into_iter()
                .map(|(n, b)| schemaic_core::stats::IndexStats {
                    name: n.into(),
                    bytes: b,
                    is_primary: n == "PRIMARY",
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_statement_source_that_is_off_is_no_answer_with_the_statement_that_turns_it_on() {
        let o = statements_outcome(
            &wl::StatementReport::Unavailable(Unavailable::ExtensionNotInstalled),
            SqlDialect::Postgres,
        );
        assert!(!o.answered);
        assert!(o.rows.is_none());
        assert!(o.notes.contains(&Note::Hint(
            "to turn it on, run: CREATE EXTENSION pg_stat_statements;".into()
        )));
    }

    #[test]
    fn a_statement_source_with_nothing_counted_is_an_empty_answer() {
        let o = statements_outcome(
            &wl::StatementReport::Ready {
                statements: vec![],
                since: None,
            },
            SqlDialect::Postgres,
        );
        assert!(o.answered);
        assert_eq!(o.rows.map(|r| r.row_count()), Some(0));
        assert_eq!(o.notes.len(), 1);
    }

    /// **S6.2-L1-01**: a MySQL database of primary keys only — `PRIMARY`
    /// listed with no figure, `INDEX_LENGTH` 0 — is the per-table case, exit
    /// 2, not "no indexes to size" and exit 0.
    #[test]
    fn index_sizes_on_a_primary_key_only_mysql_database_are_no_answer() {
        let stats = stats_of(vec![table_stats(
            "t",
            Some(16_384),
            Some(0),
            vec![("PRIMARY", None)],
        )]);
        let o = sizes_outcome(&stats, SqlDialect::MySql, "shop", true, 20);
        assert!(!o.answered, "{o:?}");
        assert!(o.rows.is_none());
        // The tables are still sized.
        let o = sizes_outcome(&stats, SqlDialect::MySql, "shop", false, 20);
        assert!(o.answered);
        assert_eq!(o.rows.map(|r| r.row_count()), Some(1));
    }

    #[test]
    fn index_sizes_on_a_database_with_no_indexes_are_an_empty_answer() {
        let stats = stats_of(vec![table_stats("t", Some(8_192), Some(0), vec![])]);
        let o = sizes_outcome(&stats, SqlDialect::Postgres, "shop", true, 20);
        assert!(o.answered);
        assert_eq!(
            o.notes,
            vec![Note::Warn("shop has no indexes to size".into())]
        );
    }

    /// **R2-L8-04**: DuckDB reports no byte figures, so neither ranking is
    /// an empty answer.
    #[test]
    fn sizes_on_duckdb_are_no_answer_and_say_why() {
        let stats = stats_of(vec![table_stats("t", None, None, vec![])]);
        for of_indexes in [false, true] {
            let o = sizes_outcome(&stats, SqlDialect::DuckDb, "memory", of_indexes, 20);
            assert!(!o.answered, "{of_indexes}");
            assert!(o.rows.is_none());
            assert!(
                matches!(&o.notes[..], [Note::Warn(m)] if m.contains("no table or index sizes"))
            );
        }
    }

    #[test]
    fn an_empty_lock_list_is_an_answer_that_says_which_empty() {
        let o = locks_outcome(&LockList {
            contended_only: true,
            ..Default::default()
        });
        assert!(o.answered);
        assert!(matches!(&o.notes[..], [Note::Warn(m)] if m.contains("contended locks only")));
    }

    /// **`pg_stat_statements` keeps a role's password.** What the MCP tool
    /// and `schemaic statements` print withholds the text and keeps the
    /// figures.
    #[test]
    fn a_statement_carrying_a_password_is_withheld_and_its_figures_kept() {
        let rs = statements_result(
            &[StatementStat {
                query: "ALTER ROLE app PASSWORD 'S3cret!'".into(),
                calls: 1,
                total_ms: 2.0,
                ..Default::default()
            }],
            SqlDialect::Postgres,
        );
        assert!(!cell(&rs, 0, 5).contains("S3cret"), "{}", cell(&rs, 0, 5));
        assert_eq!(cell(&rs, 0, 5), wl::WITHHELD_STATEMENT);
        assert_eq!(cell(&rs, 0, 2), "1");
    }

    #[test]
    fn a_finding_names_its_table_qualified_and_what_covers_it() {
        let rs = index_health_result(&[IndexFinding {
            schema: Some("sales".into()),
            table: "orders".into(),
            index: "ix_c".into(),
            problem: IndexProblem::Redundant { by: "ix_cd".into() },
            bytes: Some(8192),
        }]);
        assert_eq!(cell(&rs, 0, 0), "sales.orders");
        assert_eq!(cell(&rs, 0, 2), "redundant");
        assert_eq!(cell(&rs, 0, 3), "prefix of ix_cd");
        assert_eq!(cell(&rs, 0, 4), "8192");
    }

    #[test]
    fn a_suggestion_for_a_table_the_schema_lacks_has_no_ddl() {
        let schema = DbSchema::default();
        let s = suggestions(
            &schema,
            schemaic_core::intel::SqlDialect::MsSql,
            vec![MissingIndex {
                table: "gone".into(),
                equality: vec!["a".into()],
                ..Default::default()
            }],
        );
        assert!(s[0].ddl.is_empty());
        let rs = missing_result(&s);
        assert_eq!(cell(&rs, 0, 0), "1");
        assert_eq!(cell(&rs, 0, 2), "a");
        assert!(suggestion_ddl_text(&s).starts_with("1. (the table is not in the schema read"));
    }

    #[test]
    fn a_suggestions_ddl_is_given_whole_and_numbered() {
        let s = vec![Suggestion {
            index: MissingIndex::default(),
            ddl: format!("CREATE INDEX [{}] ON [t] ([a]);\n", "x".repeat(200)),
        }];
        let text = suggestion_ddl_text(&s);
        assert!(text.starts_with("1. CREATE INDEX ["), "{text}");
        assert!(text.contains(&"x".repeat(200)), "{text}");
    }

    /// `schemaic indexes --missing` carries each suggestion's DDL in its own
    /// column, whole and without its trailing newline — a `--format json`
    /// reader takes the statement from the row, with no numbered text to
    /// match it against. A table the schema read lacks is a null, not "".
    #[test]
    fn the_cli_suggestion_table_carries_each_ddl_whole() {
        let long = format!("CREATE INDEX [{}] ON [t] ([a]);", "x".repeat(200));
        let s = vec![
            Suggestion {
                index: MissingIndex {
                    table: "t".into(),
                    equality: vec!["a".into()],
                    ..Default::default()
                },
                ddl: format!("{long}\n"),
            },
            Suggestion {
                index: MissingIndex::default(),
                ddl: String::new(),
            },
        ];
        let rs = missing_with_ddl_result(&s);
        let last = rs.columns.len() - 1;
        assert_eq!(rs.columns[last].name, "ddl");
        assert_eq!(cell(&rs, 0, 1), "t");
        assert_eq!(cell(&rs, 0, last), long);
        assert!(matches!(
            rs.cell(1, last).map(|c| c.to_value()),
            Some(Value::Null)
        ));
    }

    /// **The editor script's closing `GO` is not part of the statement.** It
    /// is sqlcmd's batch separator, which a driver handed the cell would send
    /// to the server as a word — seen live on SQL Server 2022, where the cell
    /// read `CREATE INDEX … ;\nGO`. Only a trailing separator line goes: a
    /// `GO` inside a name or between batches is the script's own.
    #[test]
    fn the_cli_ddl_cell_drops_the_scripts_closing_go() {
        let s = |ddl: &str| Suggestion {
            index: MissingIndex::default(),
            ddl: ddl.to_string(),
        };
        let rs = missing_with_ddl_result(&[
            s("CREATE INDEX [IX_a] ON [t] ([a]);\nGO\n"),
            s("CREATE INDEX [IX_GO] ON [t] ([a]);\r\ngo"),
            s("CREATE INDEX [IX_b] ON [t] ([b]);\nGO\nCREATE INDEX [IX_c] ON [t] ([c]);\nGO"),
        ]);
        let last = rs.columns.len() - 1;
        assert_eq!(cell(&rs, 0, last), "CREATE INDEX [IX_a] ON [t] ([a]);");
        assert_eq!(cell(&rs, 1, last), "CREATE INDEX [IX_GO] ON [t] ([a]);");
        assert_eq!(
            cell(&rs, 2, last),
            "CREATE INDEX [IX_b] ON [t] ([b]);\nGO\nCREATE INDEX [IX_c] ON [t] ([c]);"
        );
    }

    #[test]
    fn sizes_carry_bytes_and_a_readable_size() {
        let rs = table_sizes_result(&[SizeEntry {
            schema: None,
            table: "t".into(),
            index: None,
            bytes: 2048,
            rows: Some(10),
        }]);
        assert_eq!(cell(&rs, 0, 1), "2048");
        assert_eq!(cell(&rs, 0, 2), "2 KiB");
        assert_eq!(cell(&rs, 0, 3), "10");
        let rs = index_sizes_result(&[SizeEntry {
            schema: Some("s".into()),
            table: "t".into(),
            index: Some("ix".into()),
            bytes: 1,
            rows: None,
        }]);
        assert_eq!(cell(&rs, 0, 0), "s.t");
        assert_eq!(cell(&rs, 0, 1), "ix");
    }

    #[test]
    fn the_usage_note_says_when_nothing_was_counted() {
        assert!(usage_note(&SchemaStats::default()).contains("no index usage counts"));
    }
}
