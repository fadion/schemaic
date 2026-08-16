//! Table statistics — the numbers behind *"is this table big enough to worry
//! about"*, and how much of each one to believe.
//!
//! Every field here is an [`Option`], and that is the whole design. The three
//! engines do not publish the same facts, do not refresh them on the same
//! schedule, and one of them publishes almost nothing; a struct of plain `u64`s
//! would have to invent a zero for each gap, and a fabricated `0 rows` is worse
//! than an empty cell. So the UI asks *is this figure present*, never *which
//! database is this* — the capability rule, applied to reading rather than
//! writing.
//!
//! **Nothing in here is exact unless it says so.** InnoDB's `TABLE_ROWS` is
//! sampled from a handful of index pages and is routinely off by tens of
//! percent; PostgreSQL's `reltuples` is only as good as the last `ANALYZE` and
//! is `-1` on a table that has never had one. [`RowCount`] carries which kind a
//! figure is so a caller cannot print an estimate as a fact, and [`Freshness`]
//! carries *why* it might be wrong so the modal can say so in words. The exact
//! answer costs a `SELECT COUNT(*)` and is a thing the user asks for.
//!
//! The app fetches these for a whole database at once and lazily — see
//! `schemaic_db::Db::fetch_table_stats`. Pure and unit-tested here; the SQL that
//! fills it lives in the db crate.

use crate::intel::SqlDialect;
use crate::text::human_count;

/// Does `dialect` publish per-table statistics at all?
///
/// MySQL has `information_schema.TABLES`, PostgreSQL has `pg_class` plus the
/// `pg_stat_*` views. **SQLite has neither, and this is a statement about SQLite
/// rather than unfinished work**: it keeps no per-table row estimate outside
/// `sqlite_stat1`, which exists only after an explicit `ANALYZE` and holds an
/// index sample rather than a table size; per-table byte sizes need the `dbstat`
/// virtual table, which is a compile-time option most builds omit (including the
/// one bundled here); and `page_count * page_size` measures the whole database
/// file, not a table in it.
///
/// So the properties surface tells a SQLite user that the engine doesn't keep
/// these, and offers the one figure SQLite *can* answer exactly — a
/// `SELECT COUNT(*)`, which is what [`crate::stats`] leaves to the caller.
pub fn supports_table_stats(dialect: SqlDialect) -> bool {
    !matches!(dialect, SqlDialect::Sqlite)
}

/// A row figure and whether the engine actually counted.
///
/// Two variants rather than a `u64` plus a `bool` because the label differs in
/// kind, not just in wording: an exact count is worth printing in full
/// (`4,213,551` — the user may be comparing it to something), while an estimate
/// that precise would be a lie told to three more digits than it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowCount {
    /// A real `SELECT COUNT(*)`, as of when it ran.
    Exact(u64),
    /// The server's own estimate, from whatever it samples.
    Estimate(u64),
}

impl RowCount {
    /// The figure itself, whichever kind it is.
    pub fn value(self) -> u64 {
        match self {
            RowCount::Exact(n) | RowCount::Estimate(n) => n,
        }
    }

    pub fn is_estimate(self) -> bool {
        matches!(self, RowCount::Estimate(_))
    }

    /// How the figure is printed: `4,213,551` exact, `~4.2m` estimated.
    ///
    /// The estimate goes through [`human_count`], the same printer the grid's
    /// stats line uses, so `200k` means one thing in this app.
    pub fn label(self) -> String {
        match self {
            RowCount::Exact(n) => group_digits(n),
            RowCount::Estimate(n) => format!("~{}", human_count(n as usize)),
        }
    }
}

/// `4213551` → `4,213,551`. Thousands separators, for a figure precise enough to
/// deserve them.
fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let lead = digits.len() % 3;
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && i % 3 == lead {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Why a statistic might be out of date — the sentence the modal owes the user
/// next to a number it can't stand behind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Freshness {
    /// The engine didn't say, or wasn't asked.
    #[default]
    Unknown,
    /// Refreshed by `ANALYZE` / autovacuum, and no fresher. `Some(when)` is the
    /// server's own timestamp text for the last one; `None` means it has never
    /// been analyzed — which is usually also why the row estimate is missing.
    Analyzed(Option<String>),
    /// Served from a server-side cache with a configured maximum age, in seconds
    /// (MySQL's `information_schema_stats_expiry`, which **defaults to 86400** —
    /// a day). `0` disables the cache, so the figures are read live.
    CachedUpTo(u64),
}

impl Freshness {
    /// The caveat to print under the numbers, or `None` when there is nothing
    /// honest to add.
    pub fn note(&self) -> Option<String> {
        match self {
            Freshness::Unknown => None,
            Freshness::Analyzed(None) => Some(
                "This table has never been analyzed, so the server has no row estimate for it. \
                 Run ANALYZE, or count the rows."
                    .to_string(),
            ),
            Freshness::Analyzed(Some(when)) => {
                Some(format!("Estimated as of the last ANALYZE, {when}."))
            }
            Freshness::CachedUpTo(0) => {
                Some("Read live — this server has information_schema_stats_expiry = 0.".to_string())
            }
            Freshness::CachedUpTo(secs) => Some(format!(
                "Server-cached: these figures may be up to {} old \
                 (information_schema_stats_expiry = {secs}).",
                format_age(*secs)
            )),
        }
    }
}

/// One index's share of the table — what it costs, and whether anything uses it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexStats {
    pub name: String,
    /// On-disk size, where the engine breaks it out per index (PostgreSQL does;
    /// MySQL reports one `INDEX_LENGTH` for the table).
    pub bytes: Option<u64>,
    /// Estimated distinct values in the index's leading columns — MySQL's
    /// `STATISTICS.CARDINALITY`. Low cardinality on a large table is the shape of
    /// an index that can't narrow much.
    pub cardinality: Option<u64>,
    /// How many times the server has used this index **since its counters were
    /// last reset**.
    ///
    /// `None` is *"the engine wouldn't say"* — Performance Schema off or not
    /// granted on MySQL, stats collector off on PostgreSQL — and is emphatically
    /// not zero. [`IndexStats::is_unused`] depends on the distinction.
    pub scans: Option<u64>,
    pub is_primary: bool,
    pub is_unique: bool,
}

impl IndexStats {
    /// Should this index be flagged as unused?
    ///
    /// Only when the server actually counted and the count is zero. `scans:
    /// None` means nobody was counting, and a flag raised on missing data is a
    /// flag that tells a user to drop an index their application depends on.
    ///
    /// A primary key is never flagged — it is the row identity, and its scan
    /// count says nothing about whether the table can live without it. Neither
    /// is a unique index: it is a *constraint* that happens to be implemented as
    /// an index, so "nothing reads it" is not an argument for dropping it.
    ///
    /// Note what this still can't see: an index used only by a nightly job, or
    /// one whose counters reset with the last server restart. The flag is a
    /// prompt to investigate, and the UI must word it that way.
    pub fn is_unused(&self) -> bool {
        self.scans == Some(0) && !self.is_primary && !self.is_unique
    }
}

/// Everything the engine can say about one table's size.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableStats {
    pub table: String,
    /// The namespace, matching [`crate::schema::TableInfo::schema`] exactly —
    /// `None` on MySQL, `Some` on PostgreSQL. Part of the identity, so lookups
    /// compare it (see [`SchemaStats::get`]).
    pub schema: Option<String>,
    /// The server's row estimate. See the module note on how much to believe it.
    pub rows: Option<u64>,
    /// A real count, once someone has asked for one. Set by the properties
    /// surface's **Count rows**, never by the bulk fetch.
    pub exact_rows: Option<u64>,
    /// Bytes of row data (PostgreSQL: heap + TOAST + the visibility map, i.e.
    /// `pg_table_size`).
    pub data_bytes: Option<u64>,
    /// Bytes of indexes, all of them together.
    pub index_bytes: Option<u64>,
    /// Allocated but unused — MySQL's `DATA_FREE`. PostgreSQL expresses the same
    /// idea as [`TableStats::dead_rows`] instead.
    pub free_bytes: Option<u64>,
    /// Row versions that are dead but not yet reclaimed (PostgreSQL
    /// `n_dead_tup`).
    pub dead_rows: Option<u64>,
    /// Next value of the table's auto-increment / identity counter.
    pub auto_increment: Option<u64>,
    /// Storage format, verbatim from the engine (MySQL `ROW_FORMAT`: `Dynamic`,
    /// `Compressed`, …).
    pub row_format: Option<String>,
    /// Storage engine (MySQL `ENGINE`: `InnoDB`, `MyISAM`, …).
    pub engine: Option<String>,
    /// Server-reported creation / last-update times, as text. Kept as the
    /// server's own rendering rather than parsed — these are shown, not
    /// computed with, and every engine formats them differently.
    pub created: Option<String>,
    pub updated: Option<String>,
    /// How much to trust the figures above.
    pub freshness: Freshness,
    pub indexes: Vec<IndexStats>,
}

impl TableStats {
    /// Did the engine say anything at all? `false` drives the properties
    /// surface's "this engine doesn't publish these" state, which must not look
    /// like a table of zeros.
    pub fn has_any(&self) -> bool {
        self.rows.is_some()
            || self.exact_rows.is_some()
            || self.data_bytes.is_some()
            || self.index_bytes.is_some()
            || self.free_bytes.is_some()
            || self.dead_rows.is_some()
            || self.auto_increment.is_some()
            || self.row_format.is_some()
            || self.engine.is_some()
            || self.created.is_some()
            || self.updated.is_some()
            || !self.indexes.is_empty()
    }

    /// The best row figure available: a real count if one has been taken,
    /// otherwise the server's estimate, otherwise nothing.
    pub fn row_count(&self) -> Option<RowCount> {
        self.exact_rows
            .map(RowCount::Exact)
            .or(self.rows.map(RowCount::Estimate))
    }

    /// Data plus indexes. `None` only when the engine reported neither; one
    /// present and one missing still yields a total, because a partial total is
    /// the honest sum of what is known.
    pub fn total_bytes(&self) -> Option<u64> {
        match (self.data_bytes, self.index_bytes) {
            (None, None) => None,
            (d, i) => Some(d.unwrap_or(0).saturating_add(i.unwrap_or(0))),
        }
    }

    /// The three storage shares as fractions of their own sum, for the
    /// breakdown bar: `(data, index, free)`. `None` when there is nothing to
    /// divide — an empty table has no proportions, and a zero denominator has no
    /// answer.
    pub fn storage_split(&self) -> Option<(f64, f64, f64)> {
        let (d, i, f) = (
            self.data_bytes.unwrap_or(0) as f64,
            self.index_bytes.unwrap_or(0) as f64,
            self.free_bytes.unwrap_or(0) as f64,
        );
        let sum = d + i + f;
        (sum > 0.0).then(|| (d / sum, i / sum, f / sum))
    }

    /// Dead row versions as a share of all row versions (live + dead).
    ///
    /// `None` unless both figures are known and there is at least one row
    /// version — PostgreSQL only, and never a guess.
    pub fn dead_ratio(&self) -> Option<f64> {
        let (live, dead) = (self.rows?, self.dead_rows?);
        let total = live.checked_add(dead)?;
        (total > 0).then(|| dead as f64 / total as f64)
    }

    /// Is the dead-row share worth mentioning? See [`DEAD_ROW_WARN`].
    pub fn needs_vacuum(&self) -> bool {
        self.dead_ratio().is_some_and(|r| r >= DEAD_ROW_WARN)
    }

    /// Indexes [`IndexStats::is_unused`] flags, in the order the engine listed
    /// them.
    pub fn unused_indexes(&self) -> Vec<&IndexStats> {
        self.indexes.iter().filter(|i| i.is_unused()).collect()
    }

    /// The whole thing as Markdown, for the modal's **Copy** — the same summary
    /// the user can paste into a ticket or an AI chat.
    ///
    /// `display_name` is how the caller names the table (qualified or not); the
    /// struct holds the parts but not the app's display rule.
    pub fn to_markdown(&self, display_name: &str) -> String {
        let mut out = format!("**{display_name}**\n");
        if !self.has_any() {
            out.push_str("\nThe server reported no statistics for this table.\n");
            return out;
        }
        out.push('\n');

        let mut row = |label: &str, value: String| {
            out.push_str(&format!("- {label}: {value}\n"));
        };
        if let Some(rc) = self.row_count() {
            let kind = if rc.is_estimate() {
                " (estimate)"
            } else {
                " (counted)"
            };
            row("Rows", format!("{}{kind}", rc.label()));
        }
        if let Some(b) = self.data_bytes {
            row("Data", format_bytes(b));
        }
        if let Some(b) = self.index_bytes {
            row("Indexes", format_bytes(b));
        }
        if let (Some(t), true) = (self.total_bytes(), self.data_bytes.is_some()) {
            row("Total", format_bytes(t));
        }
        if let Some(b) = self.free_bytes.filter(|b| *b > 0) {
            row("Free", format_bytes(b));
        }
        if let Some(d) = self.dead_rows {
            let pct = self
                .dead_ratio()
                .map(|r| format!(" ({:.0}%)", r * 100.0))
                .unwrap_or_default();
            row("Dead rows", format!("{}{pct}", group_digits(d)));
        }
        if let Some(e) = &self.engine {
            row("Engine", e.clone());
        }
        if let Some(f) = &self.row_format {
            row("Row format", f.clone());
        }
        if let Some(a) = self.auto_increment {
            row("Auto-increment", group_digits(a));
        }
        if let Some(c) = &self.created {
            row("Created", c.clone());
        }
        if let Some(u) = &self.updated {
            row("Updated", u.clone());
        }

        if !self.indexes.is_empty() {
            out.push_str("\nIndexes:\n\n");
            for i in &self.indexes {
                let mut parts: Vec<String> = Vec::new();
                if let Some(b) = i.bytes {
                    parts.push(format_bytes(b));
                }
                if let Some(c) = i.cardinality {
                    parts.push(format!("cardinality {}", group_digits(c)));
                }
                match i.scans {
                    Some(s) => parts.push(format!("{} scans", group_digits(s))),
                    None => parts.push("scan count unavailable".to_string()),
                }
                if i.is_unused() {
                    parts.push("never used".to_string());
                }
                out.push_str(&format!("- `{}` — {}\n", i.name, parts.join(", ")));
            }
        }

        if let Some(note) = self.freshness.note() {
            out.push_str(&format!("\n{note}\n"));
        }
        out
    }
}

/// Past this share of dead row versions the properties surface says so. Chosen
/// to match PostgreSQL's own default autovacuum threshold
/// (`autovacuum_vacuum_scale_factor = 0.2`): below it, the server does not think
/// the table needs vacuuming either, and a warning the engine disagrees with is
/// noise.
pub const DEAD_ROW_WARN: f64 = 0.2;

/// Stats for every table in one database — what a single lazy fetch returns.
///
/// Fetched whole rather than per table because it costs the same query either
/// way, and having the set is what lets the schema tree show a size beside every
/// table at once. That is also why it is *lazy*: on a server with thousands of
/// tables the underlying catalogue query is not free, and it must never ride
/// along with the schema fetch that runs on every connect.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemaStats {
    pub tables: Vec<TableStats>,
}

impl SchemaStats {
    /// Find one table's stats. Matches on namespace **and** name, because
    /// `sales.orders` and `archive.orders` are different tables.
    pub fn get(&self, schema: Option<&str>, table: &str) -> Option<&TableStats> {
        self.tables
            .iter()
            .find(|t| t.schema.as_deref() == schema && t.table == table)
    }

    /// Every table's total added up — the "this database is N on disk" figure.
    /// `None` when no table reported a size.
    pub fn total_bytes(&self) -> Option<u64> {
        self.tables
            .iter()
            .filter_map(TableStats::total_bytes)
            .reduce(u64::saturating_add)
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }
}

/// Human byte size, 1024-based with IEC units: `0 B`, `912 B`, `1.5 KiB`,
/// `4 GiB`. One decimal, trailing zeros trimmed; bytes stay whole.
///
/// IEC (`KiB`) rather than `KB` because the division here really is by 1024 —
/// every engine reports these in bytes and the powers of two are what the
/// storage actually is. Calling 1024 bytes a kilobyte would be the small
/// dishonesty that makes the rest of the panel harder to trust.
pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut val = n as f64;
    let mut unit = 0;
    while val >= 1024.0 && unit + 1 < UNITS.len() {
        val /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{n} B");
    }
    let s = format!("{val:.1}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s} {}", UNITS[unit])
}

/// The statement behind **Count rows** — the one figure every engine can answer
/// exactly, including the one that publishes no statistics at all.
///
/// Built here rather than three times in the db crate so the quoting has one
/// spelling: [`crate::export::ident_sql`], unconditional and quote-doubling,
/// because this is SQL that is executed rather than read.
///
/// It is a plain `COUNT(*)` with no cap and no `WHERE`, and on a large table it
/// is a full scan that can take a while — which is exactly why it is a button
/// the user presses rather than something the properties surface does on open.
pub fn count_rows_sql(schema: Option<&str>, table: &str, dialect: SqlDialect) -> String {
    let name = match schema {
        Some(ns) => format!(
            "{}.{}",
            crate::export::ident_sql(ns, dialect),
            crate::export::ident_sql(table, dialect)
        ),
        None => crate::export::ident_sql(table, dialect),
    };
    format!("SELECT COUNT(*) FROM {name}")
}

/// A duration in seconds as the shortest sensible unit: `30s`, `5m`, `1h`,
/// `24h`, `2d`. For naming a staleness window, not for precision.
pub fn format_age(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3_600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3_600),
        // A day is still spoken of in hours (MySQL's default expiry *is* 24h and
        // reads wrong as "1d"); past two, days.
        s if s < 172_800 => format!("{}h", s / 3_600),
        s => format!("{}d", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx(name: &str, scans: Option<u64>) -> IndexStats {
        IndexStats {
            name: name.to_string(),
            scans,
            ..Default::default()
        }
    }

    // ── Byte formatting ──────────────────────────────────────────────────────

    #[test]
    fn bytes_under_a_kibibyte_stay_whole() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1), "1 B");
        assert_eq!(format_bytes(912), "912 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn bytes_scale_by_1024_and_trim_trailing_zeros() {
        assert_eq!(format_bytes(1024), "1 KiB", "not 1.0 KiB");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1 MiB");
        assert_eq!(format_bytes(4 * 1024 * 1024 * 1024), "4 GiB");
        assert_eq!(
            format_bytes(1024 * 1024 * 1024 + 512 * 1024 * 1024),
            "1.5 GiB"
        );
    }

    #[test]
    fn the_largest_unit_absorbs_everything_above_it() {
        // A petabyte table is not a real case, but a `u64` overflowing the unit
        // table into a panic or an empty string would be.
        let huge = format_bytes(u64::MAX);
        assert!(huge.ends_with(" TiB"), "{huge}");
    }

    // ── Row counts ───────────────────────────────────────────────────────────

    #[test]
    fn an_exact_count_prints_in_full_with_separators() {
        assert_eq!(RowCount::Exact(4_213_551).label(), "4,213,551");
        assert_eq!(RowCount::Exact(0).label(), "0");
        assert_eq!(RowCount::Exact(999).label(), "999");
        assert_eq!(RowCount::Exact(1_000).label(), "1,000");
    }

    #[test]
    fn an_estimate_prints_rounded_and_marked_as_one() {
        assert_eq!(RowCount::Estimate(4_213_551).label(), "~4.21m");
        assert_eq!(RowCount::Estimate(200_000).label(), "~200k");
        // InnoDB really does report 0 for a small table it hasn't sampled. The
        // tilde is what keeps that from reading as "this table is empty".
        assert_eq!(RowCount::Estimate(0).label(), "~0");
    }

    #[test]
    fn a_real_count_wins_over_the_estimate() {
        let s = TableStats {
            rows: Some(4_000_000),
            exact_rows: Some(4_213_551),
            ..Default::default()
        };
        assert_eq!(s.row_count(), Some(RowCount::Exact(4_213_551)));

        let s = TableStats {
            rows: Some(4_000_000),
            ..Default::default()
        };
        assert_eq!(s.row_count(), Some(RowCount::Estimate(4_000_000)));

        assert_eq!(TableStats::default().row_count(), None);
    }

    // ── Freshness ────────────────────────────────────────────────────────────

    #[test]
    fn an_unknown_freshness_says_nothing() {
        assert_eq!(Freshness::Unknown.note(), None);
    }

    #[test]
    fn a_never_analyzed_table_says_so_rather_than_showing_a_stale_number() {
        let note = Freshness::Analyzed(None).note().expect("a note");
        assert!(note.contains("never"), "{note}");
    }

    #[test]
    fn an_analyzed_table_names_when() {
        let note = Freshness::Analyzed(Some("2026-08-16 03:11:02+02".into()))
            .note()
            .expect("a note");
        assert!(note.contains("2026-08-16 03:11:02+02"), "{note}");
    }

    #[test]
    fn a_cached_statistic_names_how_stale_it_may_be() {
        // MySQL's default: a day. The number is the point of the sentence.
        let note = Freshness::CachedUpTo(86_400).note().expect("a note");
        assert!(note.contains("24h"), "{note}");
        assert!(note.contains("information_schema_stats_expiry"), "{note}");
    }

    #[test]
    fn a_disabled_cache_says_the_figures_are_live() {
        let note = Freshness::CachedUpTo(0).note().expect("a note");
        assert!(!note.contains("24h"), "{note}");
        assert!(note.contains("live"), "{note}");
    }

    #[test]
    fn ages_pick_the_shortest_sensible_unit() {
        assert_eq!(format_age(0), "0s");
        assert_eq!(format_age(30), "30s");
        assert_eq!(format_age(60), "1m");
        assert_eq!(format_age(90), "1m");
        assert_eq!(format_age(3_600), "1h");
        assert_eq!(format_age(86_400), "24h");
        assert_eq!(format_age(172_800), "2d");
    }

    // ── Unused indexes ───────────────────────────────────────────────────────

    #[test]
    fn an_index_the_server_never_used_is_flagged() {
        assert!(idx("idx_orders_note", Some(0)).is_unused());
    }

    #[test]
    fn an_uncounted_index_is_not_flagged() {
        // Performance Schema off. "We don't know" must never render as "drop
        // this" — that is the failure mode this whole predicate exists for.
        assert!(!idx("idx_orders_note", None).is_unused());
    }

    #[test]
    fn a_used_index_is_not_flagged() {
        assert!(!idx("idx_orders_note", Some(1)).is_unused());
        assert!(!idx("idx_orders_note", Some(9_000_000)).is_unused());
    }

    #[test]
    fn keys_and_unique_constraints_are_never_flagged() {
        let pk = IndexStats {
            is_primary: true,
            ..idx("PRIMARY", Some(0))
        };
        assert!(!pk.is_unused(), "a primary key is the row identity");
        let uq = IndexStats {
            is_unique: true,
            ..idx("uq_users_email", Some(0))
        };
        assert!(!uq.is_unused(), "a unique index is a constraint");
    }

    #[test]
    fn unused_indexes_come_back_in_engine_order() {
        let s = TableStats {
            indexes: vec![
                IndexStats {
                    is_primary: true,
                    ..idx("PRIMARY", Some(0))
                },
                idx("idx_a", Some(0)),
                idx("idx_b", Some(42)),
                idx("idx_c", None),
                idx("idx_d", Some(0)),
            ],
            ..Default::default()
        };
        let names: Vec<&str> = s.unused_indexes().iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["idx_a", "idx_d"]);
    }

    // ── Sizes ────────────────────────────────────────────────────────────────

    #[test]
    fn a_total_is_data_plus_indexes() {
        let s = TableStats {
            data_bytes: Some(1_000),
            index_bytes: Some(240),
            ..Default::default()
        };
        assert_eq!(s.total_bytes(), Some(1_240));
    }

    #[test]
    fn a_half_known_total_is_still_a_total() {
        let s = TableStats {
            data_bytes: Some(1_000),
            ..Default::default()
        };
        assert_eq!(s.total_bytes(), Some(1_000));
        let s = TableStats {
            index_bytes: Some(240),
            ..Default::default()
        };
        assert_eq!(s.total_bytes(), Some(240));
        assert_eq!(TableStats::default().total_bytes(), None);
    }

    #[test]
    fn the_storage_split_is_shares_of_what_is_known() {
        let s = TableStats {
            data_bytes: Some(750),
            index_bytes: Some(250),
            ..Default::default()
        };
        let (d, i, f) = s.storage_split().expect("a split");
        assert!((d - 0.75).abs() < 1e-9, "{d}");
        assert!((i - 0.25).abs() < 1e-9, "{i}");
        assert_eq!(f, 0.0);
    }

    #[test]
    fn an_empty_table_has_no_proportions() {
        // Zero denominator. A bar drawn from NaN is a bar drawn wrong.
        let s = TableStats {
            data_bytes: Some(0),
            index_bytes: Some(0),
            free_bytes: Some(0),
            ..Default::default()
        };
        assert_eq!(s.storage_split(), None);
        assert_eq!(TableStats::default().storage_split(), None);
    }

    // ── Bloat ────────────────────────────────────────────────────────────────

    #[test]
    fn dead_rows_are_a_share_of_all_row_versions() {
        let s = TableStats {
            rows: Some(800),
            dead_rows: Some(200),
            ..Default::default()
        };
        let r = s.dead_ratio().expect("a ratio");
        assert!((r - 0.2).abs() < 1e-9, "{r}");
        assert!(s.needs_vacuum());
    }

    #[test]
    fn a_healthy_table_needs_no_vacuum() {
        let s = TableStats {
            rows: Some(1_000),
            dead_rows: Some(10),
            ..Default::default()
        };
        assert!(!s.needs_vacuum());
    }

    #[test]
    fn a_table_with_no_dead_row_figure_is_not_bloated_by_default() {
        // MySQL never reports one. Absence must not read as a clean bill of
        // health *or* as a warning — `dead_ratio` simply has no answer.
        let s = TableStats {
            rows: Some(1_000),
            ..Default::default()
        };
        assert_eq!(s.dead_ratio(), None);
        assert!(!s.needs_vacuum());
    }

    #[test]
    fn an_empty_table_has_no_dead_ratio() {
        let s = TableStats {
            rows: Some(0),
            dead_rows: Some(0),
            ..Default::default()
        };
        assert_eq!(s.dead_ratio(), None);
        assert!(!s.needs_vacuum());
    }

    // ── Presence ─────────────────────────────────────────────────────────────

    #[test]
    fn a_table_the_engine_said_nothing_about_has_nothing() {
        let s = TableStats {
            table: "orders".into(),
            ..Default::default()
        };
        assert!(!s.has_any(), "a name is not a statistic");
    }

    #[test]
    fn any_single_figure_counts_as_something() {
        for s in [
            TableStats {
                rows: Some(0),
                ..Default::default()
            },
            TableStats {
                data_bytes: Some(0),
                ..Default::default()
            },
            TableStats {
                engine: Some("InnoDB".into()),
                ..Default::default()
            },
            TableStats {
                indexes: vec![idx("idx_a", None)],
                ..Default::default()
            },
        ] {
            assert!(s.has_any(), "{s:?}");
        }
    }

    // ── Lookup ───────────────────────────────────────────────────────────────

    fn named(schema: Option<&str>, table: &str, rows: u64) -> TableStats {
        TableStats {
            table: table.to_string(),
            schema: schema.map(str::to_string),
            rows: Some(rows),
            ..Default::default()
        }
    }

    #[test]
    fn a_lookup_matches_name_and_namespace_together() {
        let set = SchemaStats {
            tables: vec![
                named(Some("sales"), "orders", 10),
                named(Some("archive"), "orders", 20),
            ],
        };
        assert_eq!(set.get(Some("sales"), "orders").unwrap().rows, Some(10));
        assert_eq!(set.get(Some("archive"), "orders").unwrap().rows, Some(20));
        assert!(set.get(Some("public"), "orders").is_none());
    }

    #[test]
    fn a_namespaceless_engine_looks_up_by_name_alone() {
        // MySQL: `schema` is `None` on both sides and must match as such — not
        // fall through to "any namespace will do".
        let set = SchemaStats {
            tables: vec![named(None, "orders", 10)],
        };
        assert_eq!(set.get(None, "orders").unwrap().rows, Some(10));
        assert!(set.get(Some("public"), "orders").is_none());
        assert!(set.get(None, "customers").is_none());
    }

    #[test]
    fn a_database_total_adds_up_what_reported_a_size() {
        let set = SchemaStats {
            tables: vec![
                TableStats {
                    data_bytes: Some(1_000),
                    index_bytes: Some(200),
                    ..named(None, "orders", 10)
                },
                TableStats {
                    data_bytes: Some(500),
                    ..named(None, "customers", 5)
                },
                named(None, "sizeless", 1),
            ],
        };
        assert_eq!(set.total_bytes(), Some(1_700));
        assert_eq!(SchemaStats::default().total_bytes(), None);
    }

    // ── Markdown ─────────────────────────────────────────────────────────────

    #[test]
    fn the_markdown_carries_the_figures_and_marks_the_estimate() {
        let s = TableStats {
            table: "orders".into(),
            schema: Some("sales".into()),
            rows: Some(4_213_551),
            data_bytes: Some(1_024 * 1_024),
            index_bytes: Some(512 * 1_024),
            engine: Some("InnoDB".into()),
            freshness: Freshness::CachedUpTo(86_400),
            indexes: vec![idx("idx_orders_note", Some(0))],
            ..Default::default()
        };
        let md = s.to_markdown("sales.orders");
        assert!(md.contains("sales.orders"), "{md}");
        assert!(md.contains("~4.21m"), "estimate stays marked: {md}");
        assert!(md.contains("1 MiB"), "{md}");
        assert!(md.contains("1.5 MiB"), "total: {md}");
        assert!(md.contains("InnoDB"), "{md}");
        assert!(md.contains("idx_orders_note"), "{md}");
        assert!(md.contains("information_schema_stats_expiry"), "{md}");
    }

    #[test]
    fn the_markdown_of_an_empty_stat_invents_nothing() {
        let md = TableStats::default().to_markdown("orders");
        assert!(md.contains("orders"), "{md}");
        assert!(!md.contains(" 0 "), "no fabricated zeroes: {md}");
        assert!(!md.contains("B\n"), "no fabricated sizes: {md}");
    }

    // ── The exact count ──────────────────────────────────────────────────────

    #[test]
    fn a_count_is_quoted_in_the_engines_own_dialect() {
        assert_eq!(
            count_rows_sql(None, "orders", SqlDialect::MySql),
            "SELECT COUNT(*) FROM `orders`"
        );
        assert_eq!(
            count_rows_sql(None, "orders", SqlDialect::Sqlite),
            "SELECT COUNT(*) FROM \"orders\""
        );
    }

    #[test]
    fn a_namespace_qualifies_the_count() {
        assert_eq!(
            count_rows_sql(Some("sales"), "orders", SqlDialect::Postgres),
            "SELECT COUNT(*) FROM \"sales\".\"orders\""
        );
    }

    #[test]
    fn a_name_that_could_break_out_is_quoted_shut() {
        // Unconditional quoting, doubling the quote character — the executed-SQL
        // rule, not the readable-SQL one.
        assert_eq!(
            count_rows_sql(None, "we`ird", SqlDialect::MySql),
            "SELECT COUNT(*) FROM `we``ird`"
        );
        assert_eq!(
            count_rows_sql(Some("s\"ales"), "or\"ders", SqlDialect::Postgres),
            "SELECT COUNT(*) FROM \"s\"\"ales\".\"or\"\"ders\""
        );
    }

    // ── Capability ───────────────────────────────────────────────────────────

    #[test]
    fn only_the_engines_with_a_catalogue_publish_stats() {
        assert!(supports_table_stats(SqlDialect::MySql));
        assert!(supports_table_stats(SqlDialect::Postgres));
        assert!(
            !supports_table_stats(SqlDialect::Sqlite),
            "SQLite keeps no per-table size; see the doc comment"
        );
    }
}
