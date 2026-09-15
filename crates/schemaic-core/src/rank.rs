//! The autocomplete **ranking policy**: which candidates a caret position offers,
//! in what order.
//!
//! **It lives here because it could not be tested where it was.** All of it was
//! the middle of `completion::recompute_completions`, a 519-line function taking
//! `&Editor` — a mounted Floem view — so every decision in it was unreachable
//! from a test: which context tier a candidate lands in, the FK demotion, the
//! last-table-in-scope bias, the dedup rule and its `tl == pl` exclusion,
//! qualifier resolution through alias → bare table → active-database pool, the
//! cross-database column split, the empty-scope fallback, and the final
//! comparator. That module's test block covered fifteen leaf helpers and **not
//! one test exercised a ranked list**, which is why two separate performance
//! findings had to re-implement parts of it in a replica in order to measure them.
//!
//! What stayed in the view is what genuinely needs one: the caret offset, the
//! document text, the popup anchor, and the signal writes. Everything ranking
//! needs is already assembled by the time it starts.
//!
//! **No glyphs here.** A suggestion carries a [`SuggestGlyph`] — *why* it gets the
//! icon it gets — and the UI turns that into an SVG. Moving `&'static str` icon
//! bodies into core would be the wrong direction, and the UI already owns the
//! [`ColumnTypeClass`] → icon mapping the schema tree uses.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::intel::{
    ClauseCtx, Continuation, FUNCTIONS, JoinTarget, SQL_KEYWORDS, STMT_KEYWORDS, StarExpansion,
    TableRef,
};
use crate::schema::{ColumnTypeClass, classify_column_type};
use crate::sql::{is_word_byte, is_word_start};

/// How many rows the popup will show.
pub const MAX_ROWS: usize = 40;

/// What an autocomplete row represents (drives its colour + the detail shown).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuggestKind {
    Keyword,
    Function,
    Table,
    Column,
    Database,
    /// A saved snippet, offered by its abbrev. Its row inserts the snippet's
    /// *body*, not the abbrev — the `insert` override every FK-JOIN row uses.
    Snippet,
}

/// Whether a column suggestion participates in a key — tints its leading icon gold
/// (PK) / purple (FK), mirroring the schema tree. Non-columns are always `None`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyKind {
    None,
    Primary,
    Foreign,
}

/// Why a row gets the leading glyph it gets.
///
/// The *reason*, not the picture: the UI maps this to an SVG. Five cases, because
/// the glyph is genuinely not a function of [`SuggestKind`] alone — a column takes
/// its type family's, an FK join target takes the key square, and an in-scope
/// alias takes a tag where the same kind of row for a bare table takes the table
/// glyph.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuggestGlyph {
    /// Whatever this row's [`SuggestKind`] wears.
    Kind,
    /// A column's type family.
    ColumnType(ColumnTypeClass),
    /// A ready-to-insert foreign-key row.
    ForeignKey,
    /// An in-scope alias, standing for a table.
    Alias,
    /// A table, whatever the row's kind says.
    Table,
}

/// One ranked autocomplete row: the text inserted, its kind, a dim detail (a
/// column's type + nullability, or a table's database), the owning table + in-scope
/// alias for a column (so a column row reads `id   orders o   int`), and whether
/// it participates in a key (drives the gold/purple glyph on column rows).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub text: String,
    pub kind: SuggestKind,
    pub detail: String,
    /// Owning table of a column suggestion (empty otherwise) — the mid annotation.
    pub table: String,
    /// In-scope alias of that table, if any (empty otherwise).
    pub alias: String,
    /// Which glyph this row earns, and why. See [`SuggestGlyph`].
    pub glyph: SuggestGlyph,
    /// Key membership — tints a column's glyph gold/purple like the schema tree.
    pub key: KeyKind,
    /// Text spliced on accept when it differs from `text` — e.g. an FK JOIN target
    /// displays `orders` but inserts `orders ON o.customer_id = orders.id`. `None`
    /// inserts `text` verbatim.
    pub insert: Option<String>,
    /// Absolute byte range to replace on accept, overriding the default word range —
    /// used by `SELECT *` expansion to swap the `*` (or `t.*`) for the column list.
    pub replace: Option<(usize, usize)>,
}

/// One column, as ranking needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColMeta {
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
    pub primary_key: bool,
    pub foreign_key: bool,
}

/// A schema view built once per recompute: which databases/tables exist and each
/// table's columns, all indexed case-insensitively. Columns of same-named tables
/// across databases are merged (dedup by column name).
///
/// The *builder* stays in the UI, because its input is the schema tree's signals;
/// the shape is here, because ranking is what reads it.
#[derive(Clone, Debug, Default)]
pub struct SchemaIndex {
    pub databases: Vec<String>,
    /// (table name, database it lives in).
    pub tables: Vec<(String, String)>,
    /// table name (lowercase) → its columns — the *active-database* unqualified pool.
    pub columns: HashMap<String, Vec<ColMeta>>,
    /// (database, table) (both lowercase) → its columns, for *every* loaded database.
    /// Backs qualified completion of a cross-database table (`otherdb.t` or an alias
    /// pointing at one), which the active-db-only `columns` map cannot answer.
    ///
    /// `Rc` because the same `Vec` also feeds the unqualified merge, and this map
    /// is populated for every loaded database on every recompute — storing it by
    /// value cloned each table's columns a second time.
    pub columns_by_db: HashMap<(String, String), Rc<Vec<ColMeta>>>,
    /// database name (lowercase) → its table names.
    pub tables_by_db: HashMap<String, Vec<String>>,
}

/// One snippet-abbrev row, as ranking needs it: what the row shows, its snippet's
/// name, and the body accepting it splices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbbrevRow {
    pub abbrev: String,
    pub name: String,
    pub body: String,
}

/// A candidate before scoring — a [`Suggestion`] plus the context tier it was
/// collected in.
#[derive(Clone, Debug)]
struct Cand {
    text: String,
    kind: SuggestKind,
    detail: String,
    table: String,
    alias: String,
    glyph: SuggestGlyph,
    key: KeyKind,
    tier: u8,
    insert: Option<String>,
    replace: Option<(usize, usize)>,
}

/// Everything ranking needs that is not the schema.
///
/// A struct rather than nine arguments, and the fields are exactly the caller's
/// reads: each is computed once by `recompute_completions` from the document and
/// the caret, and none of them is a signal.
pub struct RankInput<'a> {
    /// The clause the caret is in, lexer-derived.
    pub ctx: &'a ClauseCtx,
    /// The keyword continuations the grammar predicts here.
    pub cont: &'a Continuation,
    /// Table references in scope, in the order the statement lists them.
    pub scope: &'a [TableRef],
    /// The identifier being typed.
    pub prefix: &'a str,
    /// Snippet abbrevs that match, already filtered for dialect and connection.
    pub snippets: &'a [AbbrevRow],
    /// FK-aware JOIN targets — empty unless the caret is at a table slot, which is
    /// the caller's decision because building the catalog costs a walk of the tree.
    pub join_targets: &'a [JoinTarget],
    /// A `SELECT *` the caret sits just after, if any.
    pub star: Option<&'a StarExpansion>,
    /// Lowercased identifiers already written in this statement (recency).
    pub used: &'a HashSet<String>,
    /// The tab's database, for the "don't offer the database you are in" rule.
    pub active_db: Option<&'a str>,
}

/// Rank the candidates a caret position offers.
///
/// Collects raw candidates (dedup by text, first/lowest tier wins), scores each by
/// fuzzy match plus a recency bonus, and sorts by tier, then score, then a shorter
/// candidate. Non-matches drop out. At most [`MAX_ROWS`].
pub fn rank(schema: &SchemaIndex, input: &RankInput<'_>) -> Vec<Suggestion> {
    let (ctx, cont, scope, prefix, active_db) = (
        input.ctx,
        input.cont,
        input.scope,
        input.prefix,
        input.active_db,
    );
    let used = input.used;
    let pl = prefix.to_ascii_lowercase();
    let qualified = matches!(ctx, ClauseCtx::Qualified(_));

    let mut cands: Vec<Cand> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let add = |cands: &mut Vec<Cand>,
               seen: &mut HashSet<String>,
               text: &str,
               kind: SuggestKind,
               detail: String,
               tier: u8| {
        let tl = text.to_ascii_lowercase();
        if tl == pl || !seen.insert(tl) {
            return;
        }
        cands.push(Cand {
            text: text.to_string(),
            kind,
            detail,
            table: String::new(),
            alias: String::new(),
            glyph: SuggestGlyph::Kind,
            key: KeyKind::None,
            tier,
            insert: None,
            replace: None,
        });
    };
    // The detail string for a column typed against its own metadata: the type,
    // suffixed with a dim `· NULL` for nullable columns (NOT NULL stays clean).
    let col_type_detail = |c: &ColMeta| -> String {
        if c.nullable {
            format!("{} · NULL", c.type_name)
        } else {
            c.type_name.clone()
        }
    };
    // Column candidates carry their owning `table` (+ in-scope `alias`) as the mid
    // annotation and the type as `detail`. The leading glyph is the column's *type
    // family* (schema-tree style), tinted gold (PK) / purple (FK) via `key`. Deduped
    // by column name (first table in scope wins).
    let add_col = |cands: &mut Vec<Cand>,
                   seen: &mut HashSet<String>,
                   c: &ColMeta,
                   table: &str,
                   alias: Option<&str>,
                   tier: u8| {
        // **Ask the question that decides it before paying for the answer.**
        // Every `Cand` costs four fresh `String`s, and the ranking below drops any
        // candidate `fuzzy_score` refuses — so building one for a column that
        // cannot match is pure waste, and with no FROM yet that is *every column in
        // the database* on every keystroke. Same predicate, same argument
        // (`Cand::text` is `c.name`), so nothing that would have been offered stops
        // being offered; an empty prefix still matches everything, which is the
        // case that keeps the no-FROM list complete.
        //
        // **And it goes first, above the dedup.** It sat below, on the grounds
        // that "skipping the dedup here would let a same-named column from a
        // later table take the slot" — which cannot happen: `worth_offering` is
        // `fuzzy_score(name, prefix).is_some()` and depends on nothing but the
        // name, so two candidates sharing a lowercase name share a verdict. A
        // column this refuses can only block candidates this would refuse too,
        // and the `filter_map` below drops every one of them anyway. So the
        // `seen` claim it was making was inert, while the lowercase `String` and
        // the `HashSet` insert it exists to avoid were paid on every column —
        // 12,500 of each per keystroke at the 500 × 25 scale this pre-filter was
        // measured at, on a path that runs undebounced.
        if !worth_offering(&c.name, prefix) {
            return;
        }
        let tl = c.name.to_ascii_lowercase();
        if tl == pl || !seen.insert(tl) {
            return;
        }
        let key = if c.primary_key {
            KeyKind::Primary
        } else if c.foreign_key {
            KeyKind::Foreign
        } else {
            KeyKind::None
        };
        cands.push(Cand {
            text: c.name.clone(),
            kind: SuggestKind::Column,
            detail: col_type_detail(c),
            table: table.to_string(),
            alias: alias.unwrap_or("").to_string(),
            glyph: SuggestGlyph::ColumnType(classify_column_type(&c.type_name)),
            key,
            tier,
            insert: None,
            replace: None,
        });
    };
    // In-scope table references as qualifier candidates: an alias (`ac`, tag glyph,
    // detail = the table it stands for) or, for an unaliased table, its name (table
    // glyph). Offered in column contexts so `ON a` suggests `ac` before you type `.`.
    let add_aliases = |cands: &mut Vec<Cand>, seen: &mut HashSet<String>| {
        for r in scope {
            let qtext = r.alias.as_deref().unwrap_or(&r.name);
            let tl = qtext.to_ascii_lowercase();
            if tl == pl || !seen.insert(tl) {
                continue;
            }
            let detail = match (&r.alias, &r.db) {
                (Some(_), Some(db)) => format!("{db}.{}", r.name),
                (Some(_), None) => r.name.clone(),
                (None, _) => String::new(),
            };
            cands.push(Cand {
                text: qtext.to_string(),
                kind: SuggestKind::Table,
                detail,
                table: String::new(),
                alias: String::new(),
                glyph: if r.alias.is_some() {
                    SuggestGlyph::Alias
                } else {
                    SuggestGlyph::Table
                },
                key: KeyKind::None,
                tier: 0,
                insert: None,
                replace: None,
            });
        }
    };
    // A table's columns: keyed by (db, table) when the table is database-qualified
    // (incl. a cross-database one), else the active-database unqualified pool.
    // **A borrow, not a copy.** This runs once per table, and the unqualified arm
    // runs once per table *in the database* when there is no FROM yet — so
    // returning `Vec<ColMeta>` deep-copied every column of every table on every
    // keystroke, 4.0 ms at 500×25 and 11.4 ms at 1,000×30, with the popup often
    // showing nothing. The `Rc` at `columns_by_db` exists precisely to stop a
    // second copy of each table's columns, and this took one anyway.
    let cols_of = |db: Option<&str>, name: &str| -> &[ColMeta] {
        match db {
            Some(db) => schema
                .columns_by_db
                .get(&(db.to_ascii_lowercase(), name.to_ascii_lowercase()))
                .map_or(&[][..], |m| m.as_slice()),
            None => schema
                .columns
                .get(&name.to_ascii_lowercase())
                .map_or(&[][..], Vec::as_slice),
        }
    };
    // Snippet abbrevs, in the top tier and ahead of the keyword continuations: an
    // abbrev is a name its owner chose *in order to type it*, so when one matches
    // what is being typed it is not a guess the way a ranked keyword is.
    for row in input.snippets {
        cands.push(Cand {
            text: row.abbrev.clone(),
            kind: SuggestKind::Snippet,
            detail: row.name.clone(),
            table: String::new(),
            alias: String::new(),
            glyph: SuggestGlyph::Kind,
            key: KeyKind::None,
            tier: 0,
            // The row shows the abbrev and inserts the query.
            insert: Some(row.body.clone()),
            replace: None,
        });
    }
    // Expected clause-keyword continuations go in the *top* tier (above columns,
    // functions, and — after a complete table ref — schema table names), so the
    // legal next keyword the grammar predicts wins ties. Added before the
    // per-context candidates so they claim tier 0 (dedup keeps the first entry).
    // Skipped after a `qualifier.` (there we want only that table's columns).
    if !qualified {
        for kw in &cont.keywords {
            add(
                &mut cands,
                &mut seen,
                kw,
                SuggestKind::Keyword,
                String::new(),
                0,
            );
        }
    }
    // Once a clause continuation is expected (a complete table ref sits before the
    // caret), the schema table names are no longer the primary suggestion — demote
    // them below the keyword continuations.
    let table_tier: u8 = if cont.keywords.is_empty() { 0 } else { 1 };

    // A qualifier resolves to a table — (name, its database, the alias to annotate
    // with) — via an in-scope alias, else a bare table name (whether or not it is in
    // FROM). The database is carried so a cross-database table's columns resolve.
    let resolve = |q: &str| -> Option<(String, Option<String>, Option<String>)> {
        for r in scope {
            if r.alias
                .as_deref()
                .is_some_and(|a| a.eq_ignore_ascii_case(q))
            {
                return Some((r.name.clone(), r.db.clone(), r.alias.clone()));
            }
        }
        for r in scope {
            if r.alias.is_none() && r.name.eq_ignore_ascii_case(q) {
                return Some((r.name.clone(), r.db.clone(), None));
            }
        }
        if schema.columns.contains_key(&q.to_ascii_lowercase()) {
            return Some((q.to_string(), None, None));
        }
        None
    };

    match ctx {
        ClauseCtx::Qualified(q) => {
            // A qualifier is either a table/alias (→ its columns) or a database name
            // (→ its tables, for `db.table`). The scope resolver no longer misparses
            // a dangling `db.` as a table (fixed in `intel::lexer_scope`), so the
            // natural table-first order is safe.
            if let Some((table, db, alias)) = resolve(q) {
                for c in cols_of(db.as_deref(), &table) {
                    add_col(&mut cands, &mut seen, c, &table, alias.as_deref(), 0);
                }
            } else if let Some(tbls) = schema.tables_by_db.get(&q.to_ascii_lowercase()) {
                for t in tbls {
                    add(&mut cands, &mut seen, t, SuggestKind::Table, q.clone(), 0);
                }
            }
        }
        ClauseCtx::Table => {
            // FK-aware JOIN targets first (top tier): a table connected by a foreign
            // key to something in scope, inserting `table ON <predicate>` in one go.
            let mut fk_added = false;
            for jt in input.join_targets {
                let tl = jt.table.to_ascii_lowercase();
                if tl == pl || !seen.insert(tl) {
                    continue;
                }
                fk_added = true;
                cands.push(Cand {
                    text: jt.table.clone(),
                    kind: SuggestKind::Table,
                    detail: "foreign key".to_string(),
                    table: String::new(),
                    alias: String::new(),
                    glyph: SuggestGlyph::ForeignKey,
                    key: KeyKind::Foreign,
                    tier: 0,
                    insert: Some(format!("{} ON {}", jt.table_sql, jt.predicate)),
                    replace: None,
                });
            }
            // With FK targets present, keep them strictly above the plain table list.
            let plain_tier = if fk_added {
                table_tier.max(1)
            } else {
                table_tier
            };
            for (name, db) in &schema.tables {
                add(
                    &mut cands,
                    &mut seen,
                    name,
                    SuggestKind::Table,
                    db.clone(),
                    plain_tier,
                );
            }
            // Databases are offered only once a prefix is typed (so an empty
            // FROM/JOIN list stays tables-only) and never the active one — cross-db
            // `otherdb.table` stays reachable by typing the other database's name.
            for db in &schema.databases {
                if database_suggestion_visible(db, prefix, active_db) {
                    add(
                        &mut cands,
                        &mut seen,
                        db,
                        SuggestKind::Database,
                        String::new(),
                        table_tier + 1,
                    );
                }
            }
        }
        ClauseCtx::Column => {
            if scope.is_empty() {
                // No FROM yet: offer every column, annotated by its owning table so
                // the broader list stays navigable.
                for (name, _) in &schema.tables {
                    for c in cols_of(None, name) {
                        add_col(&mut cands, &mut seen, c, name, None, 1);
                    }
                }
            } else {
                // In-scope aliases/table names as qualifier candidates (`ac`, `ord`).
                add_aliases(&mut cands, &mut seen);
                // Bias toward the most recently added (last) table in the FROM/JOIN
                // list — the one you are most likely about to reference: its columns
                // rank first (tier 0) and claim shared names; earlier tables fall to
                // tier 1 (still above functions/keywords).
                let last = scope.len() - 1;
                for (i, r) in scope.iter().enumerate().rev() {
                    let tier = if i == last { 0 } else { 1 };
                    for c in cols_of(r.db.as_deref(), &r.name) {
                        add_col(&mut cands, &mut seen, c, &r.name, r.alias.as_deref(), tier);
                    }
                }
            }
            for fun in FUNCTIONS {
                add(
                    &mut cands,
                    &mut seen,
                    fun.name,
                    SuggestKind::Function,
                    fun.signature.to_string(),
                    2,
                );
            }
            for &k in SQL_KEYWORDS {
                add(
                    &mut cands,
                    &mut seen,
                    k,
                    SuggestKind::Keyword,
                    String::new(),
                    3,
                );
            }
        }
        ClauseCtx::Start => {
            for &k in STMT_KEYWORDS {
                add(
                    &mut cands,
                    &mut seen,
                    k,
                    SuggestKind::Keyword,
                    String::new(),
                    0,
                );
            }
            for &k in SQL_KEYWORDS {
                add(
                    &mut cands,
                    &mut seen,
                    k,
                    SuggestKind::Keyword,
                    String::new(),
                    1,
                );
            }
        }
        ClauseCtx::Other => {
            add_aliases(&mut cands, &mut seen);
            for r in scope {
                for c in cols_of(r.db.as_deref(), &r.name) {
                    add_col(&mut cands, &mut seen, c, &r.name, r.alias.as_deref(), 0);
                }
            }
            for (name, db) in &schema.tables {
                add(
                    &mut cands,
                    &mut seen,
                    name,
                    SuggestKind::Table,
                    db.clone(),
                    1,
                );
            }
            for &k in SQL_KEYWORDS {
                add(
                    &mut cands,
                    &mut seen,
                    k,
                    SuggestKind::Keyword,
                    String::new(),
                    2,
                );
            }
        }
    }

    // SELECT * expansion: when the caret sits right after a projection `*`/`t.*`,
    // offer an item that rewrites it into the explicit column list (shown when the
    // popup opens here — e.g. via Ctrl+Space, since the list does not auto-open on
    // `*`).
    if let Some(exp) = input.star {
        // `exp.columns`, not the commas in the SQL: a quoted identifier holding one
        // (`` `a,b` ``, legal on MySQL) over-reported. And `plural`, which this
        // workspace has 33 other call sites for — a one-column table is ordinary
        // (an id-only join table, a `settings(key)` lookup) and the row read
        // "1 columns".
        let ncols = exp.columns;
        cands.push(Cand {
            text: "expand *".to_string(),
            kind: SuggestKind::Column,
            detail: format!(
                "{ncols} {}",
                crate::text::plural(ncols, "column", "columns")
            ),
            table: String::new(),
            alias: String::new(),
            glyph: SuggestGlyph::Table,
            key: KeyKind::None,
            tier: 0,
            insert: Some(exp.replacement.clone()),
            replace: Some(exp.range),
        });
    }

    // Score by fuzzy match (+ recency bonus); sort by tier (context priority), then
    // score, then a shorter candidate. Non-matches drop out.
    let mut scored: Vec<(u8, i32, Cand)> = cands
        .into_iter()
        .filter_map(|c| {
            fuzzy_score(&c.text, prefix)
                .map(|s| (c.tier, s + recency_bonus(&c.text, c.kind, used), c))
        })
        .collect();
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.1.cmp(&a.1))
            .then(a.2.text.len().cmp(&b.2.text.len()))
    });
    scored
        .into_iter()
        .take(MAX_ROWS)
        .map(|(_, _, c)| Suggestion {
            text: c.text,
            kind: c.kind,
            detail: c.detail,
            table: c.table,
            alias: c.alias,
            glyph: c.glyph,
            key: c.key,
            insert: c.insert,
            replace: c.replace,
        })
        .collect()
}

/// Would this candidate survive scoring at all? The same question
/// [`fuzzy_score`] answers, asked before a `Cand`'s four `String`s are built.
pub fn worth_offering(name: &str, prefix: &str) -> bool {
    fuzzy_score(name, prefix).is_some()
}

/// Subsequence match with a score: boundaries and runs score well, gaps and a
/// late first hit score badly, a prefix match wins outright. `None` when the query
/// is not a subsequence of the candidate at all.
pub fn fuzzy_score(cand: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let c = cand.as_bytes();
    let q = query.as_bytes();
    let lc = |x: u8| x.to_ascii_lowercase();
    let mut score = 0i32;
    let mut qi = 0usize;
    let mut prev: Option<usize> = None;
    let mut first: Option<usize> = None;
    for ci in 0..c.len() {
        if qi >= q.len() {
            break;
        }
        if lc(c[ci]) == lc(q[qi]) {
            if first.is_none() {
                first = Some(ci);
            }
            let boundary = ci == 0 || c[ci - 1] == b'_';
            score += if boundary { 18 } else { 4 };
            if let Some(p) = prev {
                if ci == p + 1 {
                    score += 12;
                } else {
                    score -= (ci - p - 1).min(10) as i32;
                }
            }
            prev = Some(ci);
            qi += 1;
        }
    }
    if qi < q.len() {
        return None;
    }
    let is_prefix = c.len() >= q.len() && (0..q.len()).all(|k| lc(c[k]) == lc(q[k]));
    if is_prefix {
        score += 40;
    }
    score -= first.unwrap_or(0) as i32;
    score -= (c.len() as i32) / 5;
    Some(score)
}

/// Lowercased identifier words already present in `text[lo..hi]`, excluding the
/// word being typed at `skip` — a recency signal for ranking (you tend to reference
/// the same columns/tables again in a statement). Strings/comments are not filtered
/// out; a stray hit only mildly reorders suggestions, never changes correctness.
pub fn statement_identifiers(
    text: &str,
    lo: usize,
    hi: usize,
    skip: (usize, usize),
) -> HashSet<String> {
    let b = text.as_bytes();
    let mut out = HashSet::new();
    let mut i = lo;
    while i < hi {
        let c = b[i];
        if is_word_start(c) {
            let s = i;
            let mut j = i + 1;
            while j < hi && is_word_byte(b[j]) {
                j += 1;
            }
            if (s, j) != skip {
                out.insert(text[s..j].to_ascii_lowercase());
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// Whether a database name should be offered at a table position. Only once the
/// user has typed a prefix — so an empty `FROM`/`JOIN` list stays tables-only — and
/// never the database already in use (qualifying a table with the current database
/// is redundant). Cross-database `otherdb.table` completion stays reachable: start
/// typing the other database's name and it surfaces.
pub fn database_suggestion_visible(db: &str, prefix: &str, active_db: Option<&str>) -> bool {
    !prefix.is_empty() && active_db.is_none_or(|a| !a.eq_ignore_ascii_case(db))
}

/// The snippet abbrevs to offer at a caret with `prefix` already typed.
///
/// **Only once something has been typed.** An abbrev is a name its owner chose
/// *in order to type it*, so one nobody has started typing is not a match — and
/// on an empty prefix it is not merely a weak row, it is the **selected** one:
/// [`fuzzy_score`] answers `Some(0)` for every candidate, snippets take no
/// recency bonus, and the sort falls through to shortest-text. On a stock install
/// with no snippets of the user's own, `SELECT * FROM ` auto-opened the popup with
/// the two-character built-in `ps` preselected, and Enter — for a line break — or
/// Tab — for the first table — spliced a whole `;`-terminated statement into the
/// one being typed.
///
/// Not offered after a `qualifier.` either: there the only sensible answers are
/// that table's columns.
///
/// One row per distinct spelling, resolved through [`crate::snippet::by_abbrev`]
/// so the narrowest scope wins a shared abbrev by the same rule everywhere rather
/// than by whichever the list happened to reach first.
///
/// **It deliberately does not claim the caller's `seen` set.** That set is shared
/// by every candidate producer and first writer wins, and this block runs before
/// all of them — so a table named `locks`, a column named `idx` or a lookup table
/// named `sizes` (all shipped abbrevs) never reached the popup at all, at any
/// position, with no way out: a built-in cannot be deleted or re-spelled. Two rows
/// of one spelling is the right answer; they differ by glyph and detail.
pub fn snippet_abbrev_rows(
    all: &[crate::snippet::Snippet],
    prefix: &str,
    qualified: bool,
    dialect: crate::intel::SqlDialect,
    conn_id: u64,
) -> Vec<AbbrevRow> {
    if qualified || prefix.is_empty() {
        return Vec::new();
    }
    let mut spellings: Vec<String> = Vec::new();
    for s in all
        .iter()
        .filter(|s| crate::snippet::applies(s, dialect, conn_id))
    {
        if let Some(a) = s.abbrev.as_deref().filter(|a| !a.is_empty())
            && !spellings.iter().any(|s| s.eq_ignore_ascii_case(a))
        {
            spellings.push(a.to_string());
        }
    }
    spellings
        .into_iter()
        .filter_map(|spelling| {
            let s = crate::snippet::by_abbrev(all, &spelling, dialect, conn_id)?;
            Some(AbbrevRow {
                abbrev: spelling,
                name: s.name.clone(),
                body: s.body.clone(),
            })
        })
        .collect()
}

/// Ranking bonus for a candidate identifier (table/column/database) already used
/// elsewhere in the statement. Keywords/functions don't get it — repeating `SELECT`
/// or `COUNT` isn't a relevance signal. Modest, so a strong prefix match on a fresh
/// name still wins.
pub fn recency_bonus(text: &str, kind: SuggestKind, used: &HashSet<String>) -> i32 {
    let is_ident = matches!(
        kind,
        SuggestKind::Table | SuggestKind::Column | SuggestKind::Database
    );
    if is_ident && used.contains(&text.to_ascii_lowercase()) {
        18
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intel::SqlDialect;

    /// A two-table schema: `orders(id pk, customer_id fk, total)` and
    /// `customers(id pk, name, email)`, both in `shop`.
    fn shop() -> SchemaIndex {
        let col = |n: &str, t: &str, pk: bool, fk: bool| ColMeta {
            name: n.to_string(),
            type_name: t.to_string(),
            nullable: false,
            primary_key: pk,
            foreign_key: fk,
        };
        let orders = Rc::new(vec![
            col("id", "int", true, false),
            col("customer_id", "int", false, true),
            col("total", "decimal(10,2)", false, false),
        ]);
        let customers = Rc::new(vec![
            col("id", "int", true, false),
            col("name", "varchar(80)", false, false),
            col("email", "varchar(160)", false, false),
        ]);
        let mut columns = HashMap::new();
        columns.insert("orders".to_string(), (*orders).clone());
        columns.insert("customers".to_string(), (*customers).clone());
        let mut columns_by_db = HashMap::new();
        columns_by_db.insert(("shop".to_string(), "orders".to_string()), orders);
        columns_by_db.insert(("shop".to_string(), "customers".to_string()), customers);
        let mut tables_by_db = HashMap::new();
        tables_by_db.insert(
            "shop".to_string(),
            vec!["orders".to_string(), "customers".to_string()],
        );
        SchemaIndex {
            databases: vec!["shop".to_string(), "archive".to_string()],
            tables: vec![
                ("orders".to_string(), "shop".to_string()),
                ("customers".to_string(), "shop".to_string()),
            ],
            columns,
            columns_by_db,
            tables_by_db,
        }
    }

    fn tref(name: &str, alias: Option<&str>) -> TableRef {
        TableRef {
            name: name.to_string(),
            alias: alias.map(str::to_string),
            db: None,
        }
    }

    /// Rank one case, with everything not named at its default.
    fn ranked(
        schema: &SchemaIndex,
        ctx: ClauseCtx,
        scope: &[TableRef],
        prefix: &str,
    ) -> Vec<String> {
        let cont = Continuation::default();
        let used = HashSet::new();
        rank(
            schema,
            &RankInput {
                ctx: &ctx,
                cont: &cont,
                scope,
                prefix,
                snippets: &[],
                join_targets: &[],
                star: None,
                used: &used,
                active_db: Some("shop"),
            },
        )
        .into_iter()
        .map(|s| s.text)
        .collect()
    }

    /// **The last table in the FROM list claims a shared column name.**
    ///
    /// The bias exists because the table you just typed is the one you are about
    /// to reference — its columns take tier 0, earlier tables fall to tier 1.
    /// Both tables here have an `id` and the dedup is first-writer-wins, so which
    /// one annotates `id` is decided entirely by this rule. Nothing tested it: it
    /// was inside a function taking `&Editor`.
    #[test]
    fn the_last_table_in_scope_claims_a_shared_column_name() {
        let s = shop();
        let cont = Continuation::default();
        let used = HashSet::new();
        // `"i"`, not `"id"`: a candidate whose text *equals* the prefix is dropped
        // (`the_word_already_typed_is_not_offered_back`), so asking with the whole
        // word would assert over a row that is deliberately absent.
        let at = |scope: &[TableRef]| {
            rank(
                &s,
                &RankInput {
                    ctx: &ClauseCtx::Column,
                    cont: &cont,
                    scope,
                    prefix: "i",
                    snippets: &[],
                    join_targets: &[],
                    star: None,
                    used: &used,
                    active_db: Some("shop"),
                },
            )
        };

        let out = at(&[tref("orders", Some("o")), tref("customers", Some("c"))]);
        let id = out
            .iter()
            .find(|s| s.text == "id")
            .expect("`id` is offered");
        assert_eq!(
            id.table, "customers",
            "the last table in the FROM list claims the shared name"
        );
        assert_eq!(id.alias, "c");
        assert_eq!(id.key, KeyKind::Primary, "and keeps its key membership");

        // Reverse the scope and the other table claims it — otherwise this would
        // pass on alphabetical order or on map iteration.
        let out = at(&[tref("customers", Some("c")), tref("orders", Some("o"))]);
        assert_eq!(out.iter().find(|s| s.text == "id").unwrap().table, "orders");
    }

    /// A qualifier resolves through the in-scope alias first, then a bare table
    /// name, then the active database's pool — and a *database* name offers its
    /// tables instead.
    #[test]
    fn a_qualifier_resolves_alias_then_table_then_database() {
        let s = shop();
        let scope = [tref("customers", Some("c"))];

        let by_alias = ranked(&s, ClauseCtx::Qualified("c".into()), &scope, "");
        assert!(by_alias.contains(&"email".to_string()), "{by_alias:?}");
        assert!(!by_alias.contains(&"total".to_string()), "{by_alias:?}");

        // A bare table not in scope still completes.
        let by_table = ranked(&s, ClauseCtx::Qualified("orders".into()), &[], "");
        assert!(by_table.contains(&"total".to_string()), "{by_table:?}");

        // A database name offers its tables.
        let by_db = ranked(&s, ClauseCtx::Qualified("shop".into()), &[], "");
        assert!(by_db.contains(&"orders".to_string()), "{by_db:?}");
        assert!(!by_db.contains(&"total".to_string()), "{by_db:?}");
    }

    /// **No FROM yet is not an empty list.** With no tables in scope a column
    /// context offers every column in the database, annotated by its owner — the
    /// fallback that makes the list browsable before a FROM is typed.
    #[test]
    fn an_empty_scope_offers_every_column_annotated_by_its_table() {
        let s = shop();
        let cont = Continuation::default();
        let used = HashSet::new();
        let out = rank(
            &s,
            &RankInput {
                ctx: &ClauseCtx::Column,
                cont: &cont,
                scope: &[],
                prefix: "ema",
                snippets: &[],
                join_targets: &[],
                star: None,
                used: &used,
                active_db: Some("shop"),
            },
        );
        let email = out.iter().find(|s| s.text == "email").expect("offered");
        assert_eq!(email.table, "customers", "annotated by its owner");
        assert_eq!(email.alias, "", "there is no alias without a FROM");
    }

    /// **The dedup's output does not depend on which side of it the pre-filter
    /// sits.** `add_col` paid the lowercase `String` and the `HashSet` insert
    /// *before* asking `worth_offering`, on the grounds that the dedup had to go
    /// first or "a same-named column from a later table would take the slot".
    /// The hoist rests on that being false: `worth_offering` depends on nothing
    /// but the name, so two candidates sharing a lowercase name share a verdict,
    /// and a refused column can only block columns that would also be refused.
    ///
    /// `shop` has an `id` in both tables, which is the case the old comment was
    /// about. Green before and after the reorder by design — that is the point
    /// of it, and it is said here rather than implied.
    #[test]
    fn a_column_two_tables_share_is_annotated_by_the_first_of_them_either_way() {
        let s = shop();
        let cont = Continuation::default();
        let used = HashSet::new();
        let ranked = |prefix: &str| {
            rank(
                &s,
                &RankInput {
                    ctx: &ClauseCtx::Column,
                    cont: &cont,
                    scope: &[],
                    prefix,
                    snippets: &[],
                    join_targets: &[],
                    star: None,
                    used: &used,
                    active_db: Some("shop"),
                },
            )
        };
        // Offered once, from the table the walk reaches first. Not the prefix
        // `id` itself: a candidate equal to the prefix is dropped by the
        // `tl == pl` arm, which the hoist leaves exactly where it was.
        let out = ranked("i");
        let ids: Vec<&str> = out
            .iter()
            .filter(|s| s.text == "id")
            .map(|s| s.table.as_str())
            .collect();
        assert_eq!(ids, ["orders"], "{out:?}");
        // And a prefix that matches neither copy offers neither, rather than one
        // copy consuming the slot and the other being dropped later.
        assert!(ranked("zqx").iter().all(|s| s.text != "id"));
    }

    /// **A predicted keyword demotes the table list.** With a complete table
    /// reference before the caret the grammar's next keyword is the primary
    /// suggestion, and the schema's table names fall below it.
    #[test]
    fn a_predicted_keyword_outranks_the_table_list() {
        let s = shop();
        let cont = Continuation {
            keywords: vec!["WHERE".to_string()],
            auto_show: false,
        };
        let used = HashSet::new();
        let out: Vec<String> = rank(
            &s,
            &RankInput {
                ctx: &ClauseCtx::Table,
                cont: &cont,
                scope: &[],
                prefix: "",
                snippets: &[],
                join_targets: &[],
                star: None,
                used: &used,
                active_db: Some("shop"),
            },
        )
        .into_iter()
        .map(|s| s.text)
        .collect();
        assert_eq!(out.first().map(String::as_str), Some("WHERE"), "{out:?}");

        // Without one, the tables lead.
        let out = ranked(&s, ClauseCtx::Table, &[], "");
        assert!(
            matches!(
                out.first().map(String::as_str),
                Some("orders") | Some("customers")
            ),
            "{out:?}"
        );
    }

    /// **An FK join target sits strictly above the plain table list**, and takes
    /// the key glyph and the ready-to-insert `ON` predicate with it.
    #[test]
    fn a_foreign_key_join_target_leads_the_table_list() {
        let s = shop();
        let cont = Continuation::default();
        let used = HashSet::new();
        let targets = vec![JoinTarget {
            table: "customers".to_string(),
            table_sql: "customers".to_string(),
            predicate: "customers.id = o.customer_id".to_string(),
        }];
        let out = rank(
            &s,
            &RankInput {
                ctx: &ClauseCtx::Table,
                cont: &cont,
                scope: &[tref("orders", Some("o"))],
                prefix: "",
                snippets: &[],
                join_targets: &targets,
                star: None,
                used: &used,
                active_db: Some("shop"),
            },
        );
        let first = out.first().expect("something is offered");
        assert_eq!(first.text, "customers");
        assert_eq!(first.glyph, SuggestGlyph::ForeignKey);
        assert_eq!(first.key, KeyKind::Foreign);
        assert_eq!(
            first.insert.as_deref(),
            Some("customers ON customers.id = o.customer_id")
        );
        // And the plain `orders` row is still below it rather than gone.
        assert!(out.iter().any(|s| s.text == "orders"), "{out:?}");
    }

    /// **A candidate equal to what is typed is dropped**, whatever tier it would
    /// have had: completing `orders` to `orders` is a row that does nothing, and
    /// on an empty prefix it would be the selected one.
    #[test]
    fn the_word_already_typed_is_not_offered_back() {
        let s = shop();
        let out = ranked(&s, ClauseCtx::Table, &[], "orders");
        assert!(!out.contains(&"orders".to_string()), "{out:?}");
        // Case-insensitively, since the dedup key is lowercased.
        let out = ranked(&s, ClauseCtx::Table, &[], "ORDERS");
        assert!(!out.contains(&"orders".to_string()), "{out:?}");
    }

    /// The active database is never offered as a qualifier, and no database is
    /// until a prefix is typed.
    #[test]
    fn the_database_you_are_in_is_never_offered() {
        let s = shop();
        let empty = ranked(&s, ClauseCtx::Table, &[], "");
        assert!(!empty.contains(&"archive".to_string()), "{empty:?}");
        let typed = ranked(&s, ClauseCtx::Table, &[], "arc");
        assert!(typed.contains(&"archive".to_string()), "{typed:?}");
        let own = ranked(&s, ClauseCtx::Table, &[], "sho");
        assert!(!own.contains(&"shop".to_string()), "{own:?}");
    }

    /// A snippet abbrev leads, and inserts its body rather than its name.
    #[test]
    fn a_matching_snippet_abbrev_leads_and_inserts_its_body() {
        let s = shop();
        let cont = Continuation::default();
        let used = HashSet::new();
        let rows = [AbbrevRow {
            abbrev: "sel".to_string(),
            name: "Select all".to_string(),
            body: "SELECT * FROM ".to_string(),
        }];
        let out = rank(
            &s,
            &RankInput {
                ctx: &ClauseCtx::Start,
                cont: &cont,
                scope: &[],
                prefix: "sel",
                snippets: &rows,
                join_targets: &[],
                star: None,
                used: &used,
                active_db: Some("shop"),
            },
        );
        let first = out.first().expect("something is offered");
        assert_eq!(first.text, "sel");
        assert_eq!(first.kind, SuggestKind::Snippet);
        assert_eq!(first.insert.as_deref(), Some("SELECT * FROM "));
    }

    /// At most [`MAX_ROWS`], however many match.
    #[test]
    fn the_list_is_capped() {
        let s = shop();
        let out = ranked(&s, ClauseCtx::Start, &[], "");
        assert!(out.len() <= MAX_ROWS, "{}", out.len());
    }

    /// An abbrev is not offered on an empty prefix, after a qualifier, or for
    /// another engine — the three refusals its doc records a shipped bug for.
    #[test]
    fn an_abbrev_is_offered_only_once_someone_starts_typing_it() {
        let s = crate::snippet::Snippet {
            id: 1,
            name: "Processes".to_string(),
            abbrev: Some("ps".to_string()),
            body: "SHOW PROCESSLIST;".to_string(),
            scope: crate::snippet::Scope::Dialect(SqlDialect::MySql),
            source: crate::snippet::Source::User,
            last_used: None,
        };
        let all = [s];
        assert!(snippet_abbrev_rows(&all, "", false, SqlDialect::MySql, 7).is_empty());
        assert!(snippet_abbrev_rows(&all, "p", true, SqlDialect::MySql, 7).is_empty());
        assert!(snippet_abbrev_rows(&all, "p", false, SqlDialect::Postgres, 7).is_empty());
        assert_eq!(
            snippet_abbrev_rows(&all, "p", false, SqlDialect::MySql, 7).len(),
            1
        );
    }

    /// **The pre-filter admits exactly what the ranking keeps.**
    ///
    /// `add_col` asks `worth_offering` before building a `Cand`, so that a
    /// no-FROM `SELECT ` stops allocating four `String`s for every column in the
    /// database only to have the ranking drop them (4.0 ms per keystroke at 500
    /// tables × 25 columns, 11.4 ms at 1,000 × 30, and the same cost when nothing
    /// matches and the popup is empty). That is only safe while the two
    /// predicates are the *same* predicate, and the empty-prefix case is the one
    /// that matters most: it must admit everything, or the no-FROM list — which
    /// exists to be browsed, not filtered — would come back empty.
    ///
    /// The source half used to read `completion.rs`, where the ranking then
    /// lived. Both halves are in this file now, which is the point of the move.
    #[test]
    fn the_pre_filter_admits_exactly_what_the_ranking_keeps() {
        for name in ["id", "customer_id", "ORDERS", "created_at", "x"] {
            assert!(
                worth_offering(name, ""),
                "{name:?} must survive an empty prefix"
            );
            for prefix in ["", "i", "cid", "zz", "ORD"] {
                assert_eq!(
                    worth_offering(name, prefix),
                    fuzzy_score(name, prefix).is_some(),
                    "{name:?} / {prefix:?}"
                );
            }
        }

        // And the ranking still filters on that same call, so the pre-filter
        // cannot start admitting a superset or a subset of it unnoticed.
        // Assembled, not written: spelled out, this assertion's own source is a
        // second hit and the gate passes on itself. That is the trap every source
        // gate in this workspace has to dodge, and this one fell into it once.
        let needle = format!("{}(&c.text, prefix)", "fuzzy_score");
        let src = include_str!("rank.rs");
        assert!(
            src.contains(&needle),
            "the ranking no longer filters on `fuzzy_score` — `worth_offering` \
             is now dropping candidates the list would have shown, or paying for \
             ones it would not"
        );
    }
}
