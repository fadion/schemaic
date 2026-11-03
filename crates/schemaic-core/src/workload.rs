//! Workload and performance insight — the server-wide view beside the
//! per-table statistics of [`crate::stats`]: which statements cost the most,
//! which indexes nothing uses or another index already covers, which tables
//! and indexes are largest, and (SQL Server) which indexes the optimiser
//! wished it had.
//!
//! **One backend for two front ends.** The app's *Performance* view and the MCP
//! server's performance tools read through the same `Db` methods and rank with
//! the same functions here, so an agent and a person looking at one server are
//! told the same thing.
//!
//! **Every source may be absent**, and an absent source is said, never drawn as
//! an empty list: `pg_stat_statements` is an extension that may not be
//! installed or not preloaded, MySQL's `performance_schema` can be off (it is by
//! default on MariaDB) and its digest consumer disabled, and SQL Server's Query
//! Store is opt-in per database. [`Unavailable`] names which, and what would
//! turn it on — an empty table would read as "nothing is slow".
//!
//! Index health and the size ranking add **no catalogue reads of their own**:
//! they sweep the [`SchemaStats`] `Db::fetch_table_stats` already returns and
//! the index definitions `Db::fetch_schema` already holds.

use crate::intel::SqlDialect;
use crate::schema::{IndexColumn, IndexInfo, TableInfo};
use crate::stats::{IndexStats, SchemaStats};

/// How many statements a fetch returns by default. Enough to see the shape of
/// a workload; a list of five hundred digests is a list nobody reads.
pub const DEFAULT_STATEMENT_LIMIT: usize = 50;

/// The ceiling on a requested limit, so an agent asking for "all" cannot make
/// the fetch carry every digest a busy server has ever seen.
pub const MAX_STATEMENT_LIMIT: usize = 500;

/// Clamp a requested statement count into `1..=MAX_STATEMENT_LIMIT`, with
/// `None` meaning the default.
pub fn statement_limit(asked: Option<usize>) -> usize {
    asked
        .unwrap_or(DEFAULT_STATEMENT_LIMIT)
        .clamp(1, MAX_STATEMENT_LIMIT)
}

/// Does this engine keep per-statement execution statistics a client can
/// read? SQLite has no server and no statement history.
pub fn supports_statement_stats(dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::MsSql => true,
        SqlDialect::Sqlite => false,
    }
}

/// Does this engine record the indexes its optimiser looked for and did not
/// find? SQL Server's missing-index DMVs are the only such source; PostgreSQL
/// and MySQL keep nothing of the kind, and inventing one from the statement
/// list would be guessing.
pub fn supports_missing_index_hints(dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::MsSql => true,
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::Sqlite => false,
    }
}

/// Which way the statement list is ranked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatementOrder {
    /// Total time across every call — what the server spent most on.
    #[default]
    Total,
    /// Mean time per call — the individually slowest statements.
    Mean,
    /// Call count.
    Calls,
}

impl StatementOrder {
    /// The stable name the CLI flag and the MCP argument take.
    pub fn name(self) -> &'static str {
        match self {
            StatementOrder::Total => "total",
            StatementOrder::Mean => "mean",
            StatementOrder::Calls => "calls",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "total" => Some(StatementOrder::Total),
            "mean" => Some(StatementOrder::Mean),
            "calls" => Some(StatementOrder::Calls),
            _ => None,
        }
    }
}

/// One normalised statement and what it has cost since the counters were
/// last reset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatementStat {
    /// The statement as the engine normalised it — literals replaced by
    /// placeholders (`$1`, `?`) on PostgreSQL and MySQL; Query Store keeps
    /// the text as sent. Data to show, never to run.
    pub query: String,
    /// The database it ran in, where the source records one.
    pub database: Option<String>,
    pub calls: u64,
    pub total_ms: f64,
    pub mean_ms: f64,
    /// Rows returned or affected in total, where the source counts them.
    pub rows: Option<u64>,
}

/// Why there is no statement list to show — each with what would turn the
/// source on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unavailable {
    /// The engine keeps no statement history at all (SQLite).
    NotSupported,
    /// PostgreSQL: `CREATE EXTENSION pg_stat_statements` has not been run in
    /// this database.
    ExtensionNotInstalled,
    /// PostgreSQL: the extension exists but the library is not in
    /// `shared_preload_libraries`, so it has collected nothing.
    ExtensionNotLoaded,
    /// MySQL/MariaDB: `performance_schema` is off — a startup option, so it
    /// takes a restart.
    PerformanceSchemaOff,
    /// MySQL/MariaDB: `performance_schema` is on but its
    /// `statements_digest` consumer is disabled.
    DigestConsumerOff,
    /// SQL Server: Query Store is off for this database.
    QueryStoreOff,
    /// SQL Server: Query Store is in its `ERROR` state.
    QueryStoreError,
    /// The source is there but this login may not read it — the engine's own
    /// words.
    NoPermission(String),
}

impl Unavailable {
    /// What is missing, in English — the interface says it in its own
    /// language from the same variants.
    pub fn reason(&self) -> String {
        match self {
            Unavailable::NotSupported => "This engine keeps no statement statistics.".into(),
            Unavailable::ExtensionNotInstalled => {
                "pg_stat_statements is not installed in this database.".into()
            }
            Unavailable::ExtensionNotLoaded => {
                "pg_stat_statements is installed but not loaded: it must be listed in \
                 shared_preload_libraries, which takes a server restart."
                    .into()
            }
            Unavailable::PerformanceSchemaOff => {
                "performance_schema is off on this server. It is a startup option, so turning \
                 it on takes a restart."
                    .into()
            }
            Unavailable::DigestConsumerOff => {
                "performance_schema is on, but its statements_digest consumer is disabled.".into()
            }
            Unavailable::QueryStoreOff => "Query Store is off for this database.".into(),
            Unavailable::QueryStoreError => {
                "Query Store is in an error state for this database.".into()
            }
            Unavailable::NoPermission(e) => {
                format!("This login may not read the statement statistics: {e}")
            }
        }
    }

    /// The statement that would turn the source on, where one statement
    /// does — shown for the user to run themselves, never run by the app.
    /// `None` where it takes a configuration change and a restart, or a grant
    /// only an administrator can make.
    pub fn enable_sql(&self) -> Option<&'static str> {
        match self {
            Unavailable::ExtensionNotInstalled => Some("CREATE EXTENSION pg_stat_statements;"),
            Unavailable::DigestConsumerOff => Some(
                "UPDATE performance_schema.setup_consumers SET ENABLED = 'YES' \
                 WHERE NAME = 'statements_digest';",
            ),
            Unavailable::QueryStoreOff => Some("ALTER DATABASE CURRENT SET QUERY_STORE = ON;"),
            Unavailable::NotSupported
            | Unavailable::ExtensionNotLoaded
            | Unavailable::PerformanceSchemaOff
            | Unavailable::QueryStoreError
            | Unavailable::NoPermission(_) => None,
        }
    }
}

/// What a statement fetch found.
#[derive(Clone, Debug, PartialEq)]
pub enum StatementReport {
    Ready {
        statements: Vec<StatementStat>,
        /// When the counters started, as the server renders it, where the
        /// source says — every figure is "since then".
        since: Option<String>,
    },
    Unavailable(Unavailable),
}

/// PostgreSQL's refusal to read `pg_stat_statements`, classified: the
/// extension present but not preloaded, or a permission denial. Anything else
/// is a real error and stays one (`None`).
pub fn classify_pg_statements_error(message: &str) -> Option<Unavailable> {
    let m = message.to_ascii_lowercase();
    if m.contains("shared_preload_libraries") {
        Some(Unavailable::ExtensionNotLoaded)
    } else if m.contains("permission denied") {
        Some(Unavailable::NoPermission(message.trim().to_string()))
    } else {
        None
    }
}

/// MySQL's digest source, from `@@performance_schema` and the
/// `statements_digest` consumer's `ENABLED` (`None` when the row could not be
/// read). `None` back means the source is readable.
pub fn mysql_digest_availability(
    performance_schema_on: bool,
    consumer_enabled: Option<bool>,
) -> Option<Unavailable> {
    if !performance_schema_on {
        Some(Unavailable::PerformanceSchemaOff)
    } else if consumer_enabled == Some(false) {
        Some(Unavailable::DigestConsumerOff)
    } else {
        None
    }
}

/// SQL Server's Query Store, from `sys.database_query_store_options
/// .actual_state_desc`. `READ_ONLY` still has the history it gathered, so it is
/// readable; `None` (no row) is a database where it was never configured.
pub fn query_store_availability(actual_state: Option<&str>) -> Option<Unavailable> {
    match actual_state.map(|s| s.trim().to_ascii_uppercase()) {
        Some(s) if s == "READ_WRITE" || s == "READ_ONLY" => None,
        Some(s) if s == "ERROR" => Some(Unavailable::QueryStoreError),
        _ => Some(Unavailable::QueryStoreOff),
    }
}

// ── Index health ────────────────────────────────────────────────────────────

/// What is wrong with an index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexProblem {
    /// Not read since the counters were last reset ([`crate::stats::IndexStats::is_unused`]).
    Unused,
    /// The same key, predicate and method as `of`, which is kept.
    Duplicate { of: String },
    /// Its key is a leading prefix of `by`'s, so `by` serves every lookup it
    /// does.
    Redundant { by: String },
}

/// One index worth a look.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexFinding {
    pub schema: Option<String>,
    pub table: String,
    pub index: String,
    pub problem: IndexProblem,
    /// Its size, where the engine reports one per index.
    pub bytes: Option<u64>,
}

impl IndexFinding {
    /// The problem in English.
    pub fn describe(&self) -> String {
        match &self.problem {
            IndexProblem::Unused => "no reads since the counters were last reset".into(),
            IndexProblem::Duplicate { of } => format!("duplicates {of}"),
            IndexProblem::Redundant { by } => format!("covered by {by}"),
        }
    }
}

/// An index whose definition this module can compare at all.
///
/// Not one read only in part ([`IndexInfo::lossy`] — an operator class or a
/// `NULLS` ordering it cannot see could be the whole difference), not SQL
/// Server's XML and spatial kinds, and not a partition's copy of its parent's
/// index, which goes when the parent's does.
fn comparable(ix: &IndexInfo) -> bool {
    !ix.lossy && !ix.is_tsql_xml_or_spatial() && !ix.inherited
}

/// The access method with the engine's default spelled as nothing: PostgreSQL
/// names `btree` where MySQL leaves it unset, and the two mean the same.
fn method_of(ix: &IndexInfo) -> Option<String> {
    ix.method
        .as_deref()
        .map(str::to_ascii_lowercase)
        .filter(|m| m != "btree")
}

fn same_key_column(a: &IndexColumn, b: &IndexColumn) -> bool {
    a == b
}

/// Is `a`'s key the same as `b`'s, with everything else that decides what
/// rows an index holds and how — predicate, method, included columns?
fn same_index(a: &IndexInfo, b: &IndexInfo) -> bool {
    let mut ia = a.include.clone();
    let mut ib = b.include.clone();
    ia.sort();
    ib.sort();
    a.columns.len() == b.columns.len()
        && a.columns
            .iter()
            .zip(&b.columns)
            .all(|(x, y)| same_key_column(x, y))
        && a.predicate == b.predicate
        && method_of(a) == method_of(b)
        && ia == ib
        && a.using == b.using
}

/// Does `wide` serve every lookup `narrow` does — `narrow`'s key a strict
/// leading prefix of `wide`'s, both plain B-trees over every row?
///
/// A unique `narrow` is never covered: its uniqueness is a constraint on
/// fewer columns, which the wider index does not enforce. Neither is one with
/// included columns, which the wider key may not carry, nor a clustered one
/// ([`is_clustered`]), which is the table's own row order rather than a lookup
/// structure beside it.
fn covers(wide: &IndexInfo, narrow: &IndexInfo) -> bool {
    !narrow.unique
        && !narrow.is_primary()
        && !is_clustered(narrow)
        && narrow.include.is_empty()
        && narrow.predicate.is_none()
        && wide.predicate.is_none()
        && method_of(narrow).is_none()
        && method_of(wide).is_none()
        && narrow.columns.len() < wide.columns.len()
        && narrow
            .columns
            .iter()
            .zip(&wide.columns)
            .all(|(x, y)| same_key_column(x, y))
}

/// Is `ix` the table's clustered index (SQL Server's `CLUSTERED`)? It is the
/// table itself, in key order: dropping it leaves a heap and rebuilds every
/// other index, so it is never reported, and it is the one kept of a pair.
fn is_clustered(ix: &IndexInfo) -> bool {
    ix.clustered == Some(true)
}

/// Of two identical indexes, the one to keep: the primary key, then the
/// clustered one, then a unique one or one backing a constraint (dropping
/// those changes what the table accepts), then one backing a foreign key, then
/// one something reads over one that would itself be reported unused
/// (`idle`), then the first by name.
fn keeper_rank<'a>(ix: &'a IndexInfo, idle: &dyn Fn(&str) -> bool) -> (u8, bool, &'a str) {
    let rank = if ix.is_primary() {
        0
    } else if is_clustered(ix) {
        1
    } else if ix.unique || ix.constraint.is_some() {
        2
    } else if ix.foreign {
        3
    } else {
        4
    };
    (rank, idle(&ix.name), ix.name.as_str())
}

/// The duplicate and redundant indexes of one table.
///
/// `idle` says whether an index would be reported unused on its own count.
/// **Usage decides which of a pair is named.** Choosing by name alone kept an
/// unread twin and reported the read one, and named an unread wider index as a
/// read prefix's stand-in — two findings that, both followed, dropped every
/// index on the column. So the read one is kept; a read prefix is not called
/// redundant beside an unread wider index (that one is the unused finding);
/// and [`index_health`] never reports unused an index a finding here names.
fn table_overlaps(t: &TableInfo, idle: &dyn Fn(&str) -> bool) -> Vec<(String, IndexProblem)> {
    let ixs: Vec<&IndexInfo> = t.indexes.iter().filter(|ix| comparable(ix)).collect();
    let mut out: Vec<(String, IndexProblem)> = Vec::new();
    let mut reported = std::collections::HashSet::new();
    // Exact duplicates first: each group keeps its best and reports the rest.
    for (i, a) in ixs.iter().enumerate() {
        if reported.contains(&a.name) {
            continue;
        }
        let mut group: Vec<&IndexInfo> = vec![a];
        group.extend(ixs[i + 1..].iter().filter(|b| same_index(a, b)).copied());
        if group.len() < 2 {
            continue;
        }
        group.sort_by(|x, y| keeper_rank(x, idle).cmp(&keeper_rank(y, idle)));
        let keep = group[0].name.clone();
        for dup in &group[1..] {
            if is_clustered(dup) {
                continue;
            }
            // A unique index identical to a kept one still enforces nothing
            // the kept one doesn't — unless the kept one is not unique, in
            // which case dropping the unique one loosens the table.
            if (dup.unique || dup.is_primary()) && !(group[0].unique || group[0].is_primary()) {
                continue;
            }
            reported.insert(dup.name.clone());
            out.push((
                dup.name.clone(),
                IndexProblem::Duplicate { of: keep.clone() },
            ));
        }
    }
    // Then prefixes, for an index not already reported as a duplicate. A read
    // wider index is named over an unread one, then the shortest — the
    // closest stand-in.
    for narrow in &ixs {
        if reported.contains(&narrow.name) {
            continue;
        }
        let by = ixs
            .iter()
            .filter(|w| w.name != narrow.name && !reported.contains(&w.name) && covers(w, narrow))
            .min_by_key(|w| (idle(&w.name), w.columns.len(), w.name.as_str()));
        // Only an unread stand-in covers a read prefix: the advice is then to
        // drop the stand-in, which the unused pass gives.
        if let Some(by) = by.filter(|by| !idle(&by.name) || idle(&narrow.name)) {
            reported.insert(narrow.name.clone());
            out.push((
                narrow.name.clone(),
                IndexProblem::Redundant {
                    by: by.name.clone(),
                },
            ));
        }
    }
    out
}

/// Every index in one database worth a look: the unused ones the stats
/// counted, and the duplicate and redundant ones the definitions show.
///
/// **An unused index that backs a foreign key is left out.** MySQL requires
/// one, and refuses to drop it while the key stands; telling someone to drop
/// it is advice the server will refuse. Views are skipped — their "indexes" are
/// a materialised view's on PostgreSQL, whose cost is the refresh, a different
/// question.
///
/// Ordered with the largest first within each kind (unused, duplicate,
/// redundant), since size is what dropping one buys back.
pub fn index_health(tables: &[TableInfo], stats: &SchemaStats) -> Vec<IndexFinding> {
    let bytes_of = |schema: Option<&str>, table: &str, index: &str| {
        stats
            .get(schema, table)
            .and_then(|t| t.indexes.iter().find(|i| i.name == index))
            .and_then(|i| i.bytes)
    };
    // Would `ix` be reported unused on its own count? Not a key or a unique
    // index (`is_unused`), not one backing a foreign key, not the clustered one.
    let idle = |def: Option<&TableInfo>, ix: &IndexStats| {
        let info = def.and_then(|t| t.indexes.iter().find(|i| i.name == ix.name));
        ix.is_unused() && !info.is_some_and(|i| i.foreign || is_clustered(i))
    };
    let mut unused = Vec::new();
    let mut overlaps = Vec::new();
    for t in tables.iter().filter(|t| !t.is_view) {
        let counts = stats.get(t.schema.as_deref(), &t.name);
        let idle_here = |name: &str| {
            counts
                .and_then(|c| c.indexes.iter().find(|i| i.name == name))
                .is_some_and(|i| idle(Some(t), i))
        };
        for (index, problem) in table_overlaps(t, &idle_here) {
            overlaps.push(IndexFinding {
                schema: t.schema.clone(),
                table: t.name.clone(),
                bytes: bytes_of(t.schema.as_deref(), &t.name, &index),
                index,
                problem,
            });
        }
    }
    for ts in stats.tables() {
        let def = tables
            .iter()
            .find(|t| t.name == ts.table && t.schema == ts.schema);
        if def.is_some_and(|t| t.is_view) {
            continue;
        }
        for ix in ts.indexes.iter().filter(|i| idle(def, i)) {
            // **Once per index.** A duplicate that is also unread is reported
            // as the duplicate: that finding holds whatever the counters say,
            // and two rows for one index read as two problems. **And never the
            // stand-in** a finding names: dropping both is no index at all.
            let overlapping = overlaps.iter().any(|f| {
                f.table == ts.table
                    && f.schema == ts.schema
                    && (f.index == ix.name
                        || match &f.problem {
                            IndexProblem::Duplicate { of } => *of == ix.name,
                            IndexProblem::Redundant { by } => *by == ix.name,
                            IndexProblem::Unused => false,
                        })
            });
            if overlapping {
                continue;
            }
            unused.push(IndexFinding {
                schema: ts.schema.clone(),
                table: ts.table.clone(),
                index: ix.name.clone(),
                problem: IndexProblem::Unused,
                bytes: ix.bytes,
            });
        }
    }
    let by_size = |a: &IndexFinding, b: &IndexFinding| {
        b.bytes
            .unwrap_or(0)
            .cmp(&a.bytes.unwrap_or(0))
            .then_with(|| a.table.cmp(&b.table))
            .then_with(|| a.index.cmp(&b.index))
    };
    unused.sort_by(by_size);
    let rank = |p: &IndexProblem| match p {
        IndexProblem::Unused => 0,
        IndexProblem::Duplicate { .. } => 1,
        IndexProblem::Redundant { .. } => 2,
    };
    overlaps.sort_by(|a, b| {
        rank(&a.problem)
            .cmp(&rank(&b.problem))
            .then_with(|| by_size(a, b))
    });
    unused.extend(overlaps);
    unused
}

/// Whether any index in the stats was **counted** at all. With none counted
/// (`performance_schema` off on MySQL, the collector off on PostgreSQL) an
/// empty unused list means "nobody could say", and the view must word it so.
pub fn usage_counted(stats: &SchemaStats) -> bool {
    stats
        .tables()
        .iter()
        .flat_map(|t| &t.indexes)
        .any(|i| i.scans.is_some())
}

// ── Sizes ───────────────────────────────────────────────────────────────────

/// One table or index in the size ranking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SizeEntry {
    pub schema: Option<String>,
    pub table: String,
    /// `None` for the table itself.
    pub index: Option<String>,
    pub bytes: u64,
    /// The row estimate, for a table.
    pub rows: Option<u64>,
}

/// The `n` largest tables by data plus indexes, and the `n` largest indexes
/// where the engine reports a size per index (MySQL reports one figure for all
/// of a table's indexes, so its index list is empty rather than guessed).
pub fn largest(stats: &SchemaStats, n: usize) -> (Vec<SizeEntry>, Vec<SizeEntry>) {
    let mut tables: Vec<SizeEntry> = stats
        .tables()
        .iter()
        .filter_map(|t| {
            Some(SizeEntry {
                schema: t.schema.clone(),
                table: t.table.clone(),
                index: None,
                bytes: t.total_bytes()?,
                rows: t.exact_rows.or(t.rows),
            })
        })
        .collect();
    let mut indexes: Vec<SizeEntry> = stats
        .tables()
        .iter()
        .flat_map(|t| {
            t.indexes.iter().filter_map(move |i| {
                Some(SizeEntry {
                    schema: t.schema.clone(),
                    table: t.table.clone(),
                    index: Some(i.name.clone()),
                    bytes: i.bytes?,
                    rows: None,
                })
            })
        })
        .collect();
    let order = |a: &SizeEntry, b: &SizeEntry| {
        b.bytes
            .cmp(&a.bytes)
            .then_with(|| a.table.cmp(&b.table))
            .then_with(|| a.index.cmp(&b.index))
    };
    tables.sort_by(order);
    indexes.sort_by(order);
    tables.truncate(n);
    indexes.truncate(n);
    (tables, indexes)
}

// ── Missing indexes (SQL Server) ────────────────────────────────────────────

/// One index SQL Server's optimiser looked for and did not find
/// (`sys.dm_db_missing_index_*`), since the last restart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MissingIndex {
    pub schema: Option<String>,
    pub table: String,
    /// Columns compared with `=`, then those compared with a range — the key,
    /// in that order, which is the order the DMV's own documentation gives.
    pub equality: Vec<String>,
    pub inequality: Vec<String>,
    /// Columns the queries read but did not filter on — `INCLUDE`.
    pub include: Vec<String>,
    /// Seeks and scans that would have used it.
    pub uses: u64,
    /// The optimiser's estimate of the average cost of the queries that
    /// wanted it.
    pub avg_cost: f64,
    /// The estimated percentage improvement, `0..=100`.
    pub avg_impact: f64,
}

impl MissingIndex {
    /// The ranking figure the DMV documentation suggests: uses × cost ×
    /// impact. A relative measure for ordering, not a time.
    pub fn score(&self) -> f64 {
        self.uses as f64 * self.avg_cost * (self.avg_impact / 100.0)
    }

    /// A name for the suggested index: `IX_<table>_<key columns>`, cut to
    /// SQL Server's 128-character limit on a character boundary.
    pub fn suggested_name(&self) -> String {
        let mut name = format!("IX_{}", self.table);
        for c in self.equality.iter().chain(&self.inequality) {
            name.push('_');
            name.push_str(c);
        }
        if name.chars().count() > 128 {
            name = name.chars().take(128).collect();
        }
        name
    }

    /// The index as the schema model states it — for `ddl::diff` to emit,
    /// so a suggestion becomes DDL by the one path every index edit takes,
    /// and reaches the preview modal rather than the server.
    pub fn index_info(&self) -> IndexInfo {
        IndexInfo {
            name: self.suggested_name(),
            columns: self
                .equality
                .iter()
                .chain(&self.inequality)
                .map(IndexColumn::plain)
                .collect(),
            include: self.include.clone(),
            clustered: Some(false),
            ..Default::default()
        }
    }

    /// Add this suggestion to a table's draft as a new index — the one edit
    /// both [`suggestion_ddl`] and the app's *Create index* make, so how a
    /// suggestion becomes an index is decided once.
    pub fn add_to(&self, draft: &mut crate::ddl::TableDraft) {
        draft
            .indexes
            .push(crate::ddl::IndexDraft::new(self.index_info()));
    }
}

/// Does this engine report its index sizes only per table — some table has
/// index bytes, and no index has a figure of its own? MySQL's
/// `INDEX_LENGTH` is one number for all of a table's indexes. An empty index
/// ranking means *that* only when this holds; otherwise it means there are
/// no indexes to rank.
pub fn index_sizes_per_table_only(stats: &SchemaStats) -> bool {
    let tables = stats.tables();
    tables.iter().any(|t| t.index_bytes.unwrap_or(0) > 0)
        && !tables
            .iter()
            .flat_map(|t| &t.indexes)
            .any(|i| i.bytes.is_some())
}

/// The DMV's column list — `[a], [b]]c], [d]` — as names. A `]` inside a
/// bracketed name is doubled, as T-SQL quotes it. Text outside brackets is
/// ignored; `None` (a NULL list) is no columns.
pub fn parse_bracketed_list(list: Option<&str>) -> Vec<String> {
    let Some(s) = list else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '[' {
            continue;
        }
        let mut name = String::new();
        while let Some(c) = chars.next() {
            if c == ']' {
                if chars.peek() == Some(&']') {
                    chars.next();
                    name.push(']');
                } else {
                    break;
                }
            } else {
                name.push(c);
            }
        }
        out.push(name);
    }
    out
}

/// The DDL that would create `m` on `table`, by the one path every index edit
/// takes — the table's draft with the index added, [`crate::ddl::diff`], then
/// `emit` — so a suggestion is never a second, hand-written `CREATE INDEX`.
/// Text to propose; nothing here runs it.
///
/// **The reader's script, not the raw statements** —
/// [`crate::ddl::ChangeSet::editor_script`], the form a plan takes on its way
/// to a person or a model, so a plan the emitter could only partly express
/// arrives under its *INCOMPLETE* header and in batches a client can run,
/// rather than as bare `emit()` output joined by the caller (`invariants.md`,
/// `client_script`). Empty when the diff finds nothing to do, which a
/// well-formed suggestion never is.
pub fn suggestion_ddl(
    table: &TableInfo,
    m: &MissingIndex,
    target: impl Into<crate::ddl::Target>,
) -> String {
    let mut draft = crate::ddl::TableDraft::from_table(table);
    m.add_to(&mut draft);
    let cs = crate::ddl::diff(table, &draft, target);
    if cs.is_empty() {
        return String::new();
    }
    cs.editor_script()
}

/// Rank suggestions by [`MissingIndex::score`], highest first, and keep `n`.
pub fn rank_missing(mut v: Vec<MissingIndex>, n: usize) -> Vec<MissingIndex> {
    v.sort_by(|a, b| {
        b.score()
            .partial_cmp(&a.score())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.table.cmp(&b.table))
    });
    v.truncate(n);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::{IndexStats, TableStats};

    fn ix(name: &str, cols: &[&str]) -> IndexInfo {
        IndexInfo {
            name: name.into(),
            columns: cols.iter().map(|c| IndexColumn::plain(*c)).collect(),
            ..Default::default()
        }
    }

    fn table(name: &str, indexes: Vec<IndexInfo>) -> TableInfo {
        TableInfo {
            name: name.into(),
            indexes,
            ..Default::default()
        }
    }

    fn istat(name: &str, scans: Option<u64>, bytes: Option<u64>) -> IndexStats {
        IndexStats {
            name: name.into(),
            scans,
            bytes,
            is_primary: name == "PRIMARY",
            ..Default::default()
        }
    }

    fn tstat(name: &str, indexes: Vec<IndexStats>) -> TableStats {
        TableStats {
            table: name.into(),
            indexes,
            ..Default::default()
        }
    }

    fn problems(f: &[IndexFinding]) -> Vec<(&str, &IndexProblem)> {
        f.iter().map(|f| (f.index.as_str(), &f.problem)).collect()
    }

    #[test]
    fn a_requested_limit_is_clamped_and_none_is_the_default() {
        assert_eq!(statement_limit(None), DEFAULT_STATEMENT_LIMIT);
        assert_eq!(statement_limit(Some(0)), 1);
        assert_eq!(statement_limit(Some(10)), 10);
        assert_eq!(statement_limit(Some(1_000_000)), MAX_STATEMENT_LIMIT);
    }

    #[test]
    fn sqlite_keeps_no_statements_and_only_sql_server_hints_at_indexes() {
        assert!(!supports_statement_stats(SqlDialect::Sqlite));
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::MsSql] {
            assert!(supports_statement_stats(d), "{d:?}");
        }
        assert!(supports_missing_index_hints(SqlDialect::MsSql));
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert!(!supports_missing_index_hints(d), "{d:?}");
        }
    }

    #[test]
    fn an_order_round_trips_through_its_name() {
        for o in [
            StatementOrder::Total,
            StatementOrder::Mean,
            StatementOrder::Calls,
        ] {
            assert_eq!(StatementOrder::parse(o.name()), Some(o));
        }
        assert_eq!(StatementOrder::parse(" MEAN "), Some(StatementOrder::Mean));
        assert_eq!(StatementOrder::parse("slowest"), None);
    }

    #[test]
    fn pg_errors_are_classified_only_when_they_mean_the_source_is_off() {
        assert_eq!(
            classify_pg_statements_error(
                "pg_stat_statements must be loaded via \"shared_preload_libraries\""
            ),
            Some(Unavailable::ExtensionNotLoaded)
        );
        assert!(matches!(
            classify_pg_statements_error("permission denied for view pg_stat_statements"),
            Some(Unavailable::NoPermission(_))
        ));
        assert_eq!(classify_pg_statements_error("connection reset"), None);
    }

    #[test]
    fn mysql_digests_need_the_schema_and_the_consumer() {
        assert_eq!(
            mysql_digest_availability(false, Some(true)),
            Some(Unavailable::PerformanceSchemaOff)
        );
        assert_eq!(
            mysql_digest_availability(true, Some(false)),
            Some(Unavailable::DigestConsumerOff)
        );
        assert_eq!(mysql_digest_availability(true, Some(true)), None);
        // An unreadable consumer row is not a disabled one: try the read.
        assert_eq!(mysql_digest_availability(true, None), None);
    }

    #[test]
    fn query_store_is_readable_read_only_and_off_without_a_row() {
        assert_eq!(query_store_availability(Some("READ_WRITE")), None);
        assert_eq!(query_store_availability(Some("READ_ONLY")), None);
        assert_eq!(
            query_store_availability(Some("OFF")),
            Some(Unavailable::QueryStoreOff)
        );
        assert_eq!(
            query_store_availability(Some("ERROR")),
            Some(Unavailable::QueryStoreError)
        );
        assert_eq!(
            query_store_availability(None),
            Some(Unavailable::QueryStoreOff)
        );
    }

    #[test]
    fn only_a_one_statement_fix_offers_its_statement() {
        assert!(Unavailable::ExtensionNotInstalled.enable_sql().is_some());
        assert!(Unavailable::QueryStoreOff.enable_sql().is_some());
        assert!(Unavailable::DigestConsumerOff.enable_sql().is_some());
        assert!(Unavailable::ExtensionNotLoaded.enable_sql().is_none());
        assert!(Unavailable::PerformanceSchemaOff.enable_sql().is_none());
        assert!(
            Unavailable::NoPermission("x".into())
                .reason()
                .ends_with(": x")
        );
    }

    #[test]
    fn an_identical_index_is_a_duplicate_of_the_one_worth_keeping() {
        let mut uq = ix("uq_email", &["email"]);
        uq.unique = true;
        let t = table("users", vec![ix("ix_email", &["email"]), uq]);
        let f = index_health(&[t], &SchemaStats::default());
        assert_eq!(
            problems(&f),
            vec![(
                "ix_email",
                &IndexProblem::Duplicate {
                    of: "uq_email".into()
                }
            )]
        );
    }

    #[test]
    fn a_unique_index_is_never_reported_against_a_plain_twin() {
        // Keeping the plain one and dropping the unique one would loosen the
        // table, so the plain one is the one reported.
        let mut uq = ix("a_unique", &["email"]);
        uq.unique = true;
        let t = table("users", vec![uq, ix("z_plain", &["email"])]);
        let f = index_health(&[t], &SchemaStats::default());
        assert_eq!(problems(&f).len(), 1);
        assert_eq!(f[0].index, "z_plain");
    }

    #[test]
    fn two_plain_twins_keep_the_first_by_name() {
        let t = table("t", vec![ix("ix_b", &["a"]), ix("ix_a", &["a"])]);
        let f = index_health(&[t], &SchemaStats::default());
        assert_eq!(
            problems(&f),
            vec![("ix_b", &IndexProblem::Duplicate { of: "ix_a".into() })]
        );
    }

    #[test]
    fn a_different_order_direction_predicate_or_method_is_not_a_duplicate() {
        let mut desc = ix("ix_desc", &["a"]);
        desc.columns[0].descending = true;
        let mut partial = ix("ix_partial", &["a"]);
        partial.predicate = Some("a > 0".into());
        let mut hash = ix("ix_hash", &["a"]);
        hash.method = Some("hash".into());
        let t = table(
            "t",
            vec![
                ix("ix_a", &["a"]),
                desc,
                partial,
                hash,
                ix("ix_ba", &["b", "a"]),
            ],
        );
        assert!(index_health(&[t], &SchemaStats::default()).is_empty());
    }

    #[test]
    fn btree_named_and_unnamed_are_the_same_method() {
        let mut named = ix("ix_named", &["a"]);
        named.method = Some("BTREE".into());
        let t = table("t", vec![ix("ix_plain", &["a"]), named]);
        assert_eq!(index_health(&[t], &SchemaStats::default()).len(), 1);
    }

    #[test]
    fn a_leading_prefix_is_covered_by_the_shortest_wider_index() {
        let t = table(
            "orders",
            vec![
                ix("ix_c", &["customer_id"]),
                ix("ix_c_d_s", &["customer_id", "date", "status"]),
                ix("ix_c_d", &["customer_id", "date"]),
            ],
        );
        let f = index_health(&[t], &SchemaStats::default());
        assert_eq!(
            problems(&f),
            vec![
                (
                    "ix_c",
                    &IndexProblem::Redundant {
                        by: "ix_c_d".into()
                    }
                ),
                (
                    "ix_c_d",
                    &IndexProblem::Redundant {
                        by: "ix_c_d_s".into()
                    }
                ),
            ]
        );
    }

    #[test]
    fn a_unique_partial_or_including_prefix_is_not_covered() {
        let mut uq = ix("uq_a", &["a"]);
        uq.unique = true;
        let mut inc = ix("ix_a_inc", &["a"]);
        inc.include = vec!["z".into()];
        let mut part = ix("ix_a_part", &["a"]);
        part.predicate = Some("a is not null".into());
        let t = table("t", vec![uq, inc, part, ix("ix_ab", &["a", "b"])]);
        assert!(index_health(&[t], &SchemaStats::default()).is_empty());
    }

    #[test]
    fn a_non_leading_overlap_is_not_covered() {
        let t = table("t", vec![ix("ix_b", &["b"]), ix("ix_ab", &["a", "b"])]);
        assert!(index_health(&[t], &SchemaStats::default()).is_empty());
    }

    #[test]
    fn a_lossy_index_is_never_compared() {
        let mut lossy = ix("ix_lossy", &["a"]);
        lossy.lossy = true;
        let t = table("t", vec![ix("ix_a", &["a"]), lossy]);
        assert!(index_health(&[t], &SchemaStats::default()).is_empty());
    }

    #[test]
    fn unused_indexes_come_from_the_counts_largest_first_and_never_a_key() {
        let stats = SchemaStats::new(vec![
            tstat(
                "a",
                vec![
                    istat("PRIMARY", Some(0), Some(10)),
                    istat("ix_small", Some(0), Some(100)),
                    istat("ix_used", Some(5), Some(1_000)),
                    istat("ix_uncounted", None, Some(1_000)),
                ],
            ),
            tstat("b", vec![istat("ix_big", Some(0), Some(9_000))]),
        ]);
        let f = index_health(&[], &stats);
        assert_eq!(
            f.iter().map(|f| f.index.as_str()).collect::<Vec<_>>(),
            vec!["ix_big", "ix_small"]
        );
        assert!(f.iter().all(|f| f.problem == IndexProblem::Unused));
        assert_eq!(f[0].bytes, Some(9_000));
    }

    #[test]
    fn an_unused_index_backing_a_foreign_key_is_left_alone() {
        let mut fk = ix("fk_customer", &["customer_id"]);
        fk.foreign = true;
        let defs = vec![table("orders", vec![fk])];
        let stats = SchemaStats::new(vec![tstat(
            "orders",
            vec![istat("fk_customer", Some(0), Some(500))],
        )]);
        assert!(index_health(&defs, &stats).is_empty());
    }

    #[test]
    fn an_unread_duplicate_is_reported_once_as_the_duplicate() {
        let defs = vec![table("t", vec![ix("ix_a", &["a"]), ix("ix_a2", &["a"])])];
        let stats = SchemaStats::new(vec![tstat(
            "t",
            vec![istat("ix_a", Some(4), None), istat("ix_a2", Some(0), None)],
        )]);
        let f = index_health(&defs, &stats);
        assert_eq!(
            problems(&f),
            vec![("ix_a2", &IndexProblem::Duplicate { of: "ix_a".into() })]
        );
    }

    fn clustered(name: &str, cols: &[&str]) -> IndexInfo {
        IndexInfo {
            clustered: Some(true),
            ..ix(name, cols)
        }
    }

    #[test]
    fn a_clustered_index_is_never_reported_unused() {
        // Dropping it leaves the table a heap and rebuilds every other index.
        let defs = vec![table("audit", vec![clustered("cx_audit", &["created_at"])])];
        let stats = SchemaStats::new(vec![tstat("audit", vec![istat("cx_audit", Some(0), None)])]);
        assert_eq!(problems(&index_health(&defs, &stats)), vec![]);
    }

    #[test]
    fn a_clustered_index_is_not_covered_by_a_wider_nonclustered_one() {
        let defs = vec![table(
            "audit",
            vec![
                clustered("cx_audit", &["created_at"]),
                ix("ix_created_who", &["created_at", "who"]),
            ],
        )];
        let f = index_health(&defs, &SchemaStats::default());
        assert_eq!(problems(&f), vec![]);
    }

    #[test]
    fn of_a_clustered_and_a_nonclustered_twin_the_clustered_one_is_kept() {
        // `IX_…` sorts before `cx_…` byte-wise, which used to decide it.
        let defs = vec![table(
            "audit",
            vec![
                clustered("cx_audit", &["created_at"]),
                ix("IX_audit_created", &["created_at"]),
            ],
        )];
        let f = index_health(&defs, &SchemaStats::default());
        assert_eq!(
            problems(&f),
            vec![(
                "IX_audit_created",
                &IndexProblem::Duplicate {
                    of: "cx_audit".into()
                }
            )]
        );
    }

    #[test]
    fn a_read_index_is_not_called_redundant_beside_an_unread_wider_one() {
        // The advice is to drop the wide one nothing reads, not the one the
        // queries use — and never both.
        let defs = vec![table(
            "t",
            vec![ix("ix_a", &["a"]), ix("ix_ab", &["a", "b"])],
        )];
        let stats = SchemaStats::new(vec![tstat(
            "t",
            vec![istat("ix_a", Some(40), None), istat("ix_ab", Some(0), None)],
        )]);
        let f = index_health(&defs, &stats);
        assert_eq!(problems(&f), vec![("ix_ab", &IndexProblem::Unused)]);
    }

    #[test]
    fn of_twins_the_read_one_is_kept_whatever_their_names() {
        let defs = vec![table("t", vec![ix("ix_a", &["a"]), ix("ix_a2", &["a"])])];
        let stats = SchemaStats::new(vec![tstat(
            "t",
            vec![istat("ix_a", Some(0), None), istat("ix_a2", Some(9), None)],
        )]);
        let f = index_health(&defs, &stats);
        assert_eq!(
            problems(&f),
            vec![("ix_a", &IndexProblem::Duplicate { of: "ix_a2".into() })]
        );
    }

    #[test]
    fn the_index_a_finding_names_as_its_stand_in_is_not_reported_unused() {
        // Neither twin is read: one goes as the duplicate, the other stays.
        let defs = vec![table("t", vec![ix("ix_a", &["a"]), ix("ix_a2", &["a"])])];
        let stats = SchemaStats::new(vec![tstat(
            "t",
            vec![istat("ix_a", Some(0), None), istat("ix_a2", Some(0), None)],
        )]);
        let f = index_health(&defs, &stats);
        assert_eq!(
            problems(&f),
            vec![("ix_a2", &IndexProblem::Duplicate { of: "ix_a".into() })]
        );
        // Likewise a prefix and the unread index that covers it.
        let defs = vec![table(
            "t",
            vec![ix("ix_a", &["a"]), ix("ix_ab", &["a", "b"])],
        )];
        let stats = SchemaStats::new(vec![tstat(
            "t",
            vec![istat("ix_a", Some(0), None), istat("ix_ab", Some(0), None)],
        )]);
        let f = index_health(&defs, &stats);
        assert_eq!(
            problems(&f),
            vec![("ix_a", &IndexProblem::Redundant { by: "ix_ab".into() })]
        );
    }

    #[test]
    fn findings_carry_the_namespace_and_overlap_sizes_come_from_the_stats() {
        let mut t = table("t", vec![ix("ix_a", &["a"]), ix("ix_ab", &["a", "b"])]);
        t.schema = Some("sales".into());
        let mut ts = tstat("t", vec![istat("ix_a", Some(3), Some(4_096))]);
        ts.schema = Some("sales".into());
        let f = index_health(&[t], &SchemaStats::new(vec![ts]));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].schema.as_deref(), Some("sales"));
        assert_eq!(f[0].bytes, Some(4_096));
    }

    #[test]
    fn usage_is_counted_only_when_some_index_has_a_figure() {
        let none = SchemaStats::new(vec![tstat("t", vec![istat("ix", None, None)])]);
        assert!(!usage_counted(&none));
        let some = SchemaStats::new(vec![tstat("t", vec![istat("ix", Some(0), None)])]);
        assert!(usage_counted(&some));
        assert!(!usage_counted(&SchemaStats::default()));
    }

    #[test]
    fn largest_ranks_tables_by_total_and_skips_unreported_index_sizes() {
        let mut a = tstat("a", vec![istat("ix_a", None, Some(50))]);
        a.data_bytes = Some(100);
        a.index_bytes = Some(50);
        let mut b = tstat("b", vec![istat("ix_b", None, None)]);
        b.data_bytes = Some(1_000);
        let c = tstat("c", vec![]); // no figures at all
        let (tables, indexes) = largest(&SchemaStats::new(vec![a, b, c]), 10);
        assert_eq!(
            tables
                .iter()
                .map(|t| (t.table.as_str(), t.bytes))
                .collect::<Vec<_>>(),
            vec![("b", 1_000), ("a", 150)]
        );
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].index.as_deref(), Some("ix_a"));
        let (one, _) = largest(&SchemaStats::new(vec![tstat("x", vec![])]), 0);
        assert!(one.is_empty());
    }

    #[test]
    fn only_a_table_wide_index_figure_means_sizes_are_per_table() {
        let mut my = tstat("t", vec![istat("ix", Some(1), None)]);
        my.index_bytes = Some(16_384);
        assert!(index_sizes_per_table_only(&SchemaStats::new(vec![my])));
        // PostgreSQL: a figure per index.
        let mut pg = tstat("t", vec![istat("ix", Some(1), Some(8_192))]);
        pg.index_bytes = Some(8_192);
        assert!(!index_sizes_per_table_only(&SchemaStats::new(vec![pg])));
        // A database with no indexes at all is not the per-table case.
        let bare = tstat("t", vec![]);
        assert!(!index_sizes_per_table_only(&SchemaStats::new(vec![bare])));
    }

    #[test]
    fn a_bracketed_list_unquotes_doubled_brackets() {
        assert_eq!(
            parse_bracketed_list(Some("[a], [b]]c], [d e]")),
            vec!["a", "b]c", "d e"]
        );
        assert!(parse_bracketed_list(None).is_empty());
        assert!(parse_bracketed_list(Some("")).is_empty());
    }

    #[test]
    fn a_missing_index_keys_equality_then_inequality_and_includes_the_rest() {
        let m = MissingIndex {
            table: "Orders".into(),
            equality: vec!["CustomerID".into()],
            inequality: vec!["OrderDate".into()],
            include: vec!["Total".into()],
            ..Default::default()
        };
        let info = m.index_info();
        assert_eq!(info.name, "IX_Orders_CustomerID_OrderDate");
        assert_eq!(
            info.column_names().collect::<Vec<_>>(),
            vec!["CustomerID", "OrderDate"]
        );
        assert_eq!(info.include, vec!["Total"]);
        assert_eq!(info.clustered, Some(false));
        assert!(!info.unique);
    }

    #[test]
    fn a_long_suggested_name_is_cut_to_128_characters() {
        let m = MissingIndex {
            table: "t".into(),
            equality: vec!["é".repeat(200)],
            ..Default::default()
        };
        assert_eq!(m.suggested_name().chars().count(), 128);
    }

    #[test]
    fn a_suggestion_becomes_the_differs_create_index() {
        use crate::schema::{ColumnInfo, ServerFlavour};
        let col = |n: &str| ColumnInfo {
            name: n.into(),
            type_name: "int".into(),
            nullable: true,
            ..Default::default()
        };
        let t = TableInfo {
            name: "Orders".into(),
            schema: Some("dbo".into()),
            columns: vec![col("Id"), col("CustomerID"), col("Total")],
            ..Default::default()
        };
        let m = MissingIndex {
            schema: Some("dbo".into()),
            table: "Orders".into(),
            equality: vec!["CustomerID".into()],
            include: vec!["Total".into()],
            ..Default::default()
        };
        let ddl = suggestion_ddl(
            &t,
            &m,
            crate::ddl::Target::new(SqlDialect::MsSql, ServerFlavour::Unknown),
        );
        // Nonclustered is T-SQL's default for a non-key index, so the differ
        // writes the plain form.
        assert!(
            ddl.contains("CREATE INDEX [IX_Orders_CustomerID] ON [dbo].[Orders] ([CustomerID])"),
            "{ddl}"
        );
        assert!(ddl.contains("INCLUDE ([Total])"), "{ddl}");
    }

    #[test]
    fn suggestions_rank_by_uses_cost_and_impact() {
        let s = |table: &str, uses, avg_cost, avg_impact| MissingIndex {
            table: table.into(),
            uses,
            avg_cost,
            avg_impact,
            ..Default::default()
        };
        let ranked = rank_missing(
            vec![
                s("low", 10, 1.0, 10.0),
                s("high", 100, 5.0, 90.0),
                s("mid", 50, 2.0, 50.0),
            ],
            2,
        );
        assert_eq!(
            ranked.iter().map(|m| m.table.as_str()).collect::<Vec<_>>(),
            vec!["high", "mid"]
        );
    }
}
