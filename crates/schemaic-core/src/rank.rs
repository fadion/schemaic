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
    ClauseCtx, Continuation, JoinTarget, SQL_KEYWORDS, STMT_KEYWORDS, StarExpansion, TableRef,
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
    /// A view, which the schema tree tells from a table by its glyph too.
    View,
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
    /// displays `orders` but inserts `orders ON o.customer_id = orders.id` — or
    /// a function row's name alone, for a builtin the engine calls without
    /// parentheses (`CURRENT_TIMESTAMP`), where a function would get `()`. `None`
    /// inserts `text` verbatim (a function: `text()`).
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
    /// A [`SchemaIndex::tables`] name (lowercase) → the namespace of each table
    /// by that name (`None` for one with none). When **no bare name reaches**
    /// any of them ([`crate::filter::bare_name_reaches`]), the row inserts
    /// `schema.table` with the first: PostgreSQL's `sales.ledger`, inserted as
    /// `ledger`, read whatever `search_path` found first — or nothing.
    pub namespaces: HashMap<String, Vec<Option<String>>>,
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
    /// (schema, table) (both lowercase) → its columns, for the tab's own
    /// database's tables that live in a schema (PostgreSQL, SQL Server).
    ///
    /// `intel::statement_scope` reads `sales.orders` as `TableRef.db =
    /// sales`, which on those engines names a namespace inside the database
    /// rather than a database, so [`SchemaIndex::columns_by_db`] answers
    /// nothing for it. The diagnostics `intel::Catalog` keeps the same map
    /// (`schema_qualified`) for the same reason, limited the same way: a
    /// `schema.table` never crosses a database.
    pub columns_by_schema: HashMap<(String, String), Rc<Vec<ColMeta>>>,
    /// schema (lowercase) → its table names, the tab's own database only —
    /// `sales.|`.
    pub tables_by_schema: HashMap<String, Vec<String>>,
    /// Every table of every database that contributes, with its namespace —
    /// what the widened list ([`RankInput::wide`]) names, each through
    /// [`crate::filter::table_name_from`].
    pub all_tables: Vec<IndexedTable>,
    /// `(database, name)` (both lowercase) of every view, so a table row can
    /// say it is one.
    pub views: HashSet<(String, String)>,
    /// A PostgreSQL enum type's labels, in order, by its name (lowercase) and
    /// by `schema.name`: a column of one carries only the name, so its members
    /// are here rather than in its type ([`enum_value_suggestions`]).
    pub enum_types: HashMap<String, Vec<String>>,
    /// The tab's database's stored functions and procedures, offered where
    /// each can be called: a procedure after `CALL`/`EXEC`
    /// (`ClauseCtx::Routine`), a function where an expression goes.
    pub routines: Vec<IndexedRoutine>,
}

/// One stored routine as [`SchemaIndex::routines`] holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedRoutine {
    pub name: String,
    /// Its namespace (`RoutineInfo::schema`): `None` on MySQL, where the
    /// database is the namespace. A routine no bare name reaches
    /// ([`crate::filter::bare_name_reaches`]) — and on SQL Server any
    /// function, which T-SQL calls only by its two-part name — inserts
    /// qualified ([`routine_sql`]).
    pub schema: Option<String>,
    pub kind: crate::schema::RoutineKind,
    /// The parameter list as the server renders it (`RoutineInfo::arguments`).
    pub arguments: String,
    /// A function's return type; empty for a procedure.
    pub returns: String,
}

impl SchemaIndex {
    /// A table's columns as a reference in scope reaches it: through its
    /// database (or, on PostgreSQL and SQL Server, its namespace) when
    /// qualified, the active database's pool when bare. Empty for one not
    /// loaded.
    pub fn columns_of(&self, db: Option<&str>, name: &str) -> &[ColMeta] {
        match db {
            Some(db) => {
                let key = (db.to_ascii_lowercase(), name.to_ascii_lowercase());
                self.columns_by_db
                    .get(&key)
                    .or_else(|| self.columns_by_schema.get(&key))
                    .map_or(&[][..], |m| m.as_slice())
            }
            None => self
                .columns
                .get(&name.to_ascii_lowercase())
                .map_or(&[][..], Vec::as_slice),
        }
    }
}

/// One table as [`SchemaIndex::all_tables`] holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedTable {
    pub database: String,
    pub schema: Option<String>,
    pub name: String,
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

/// The names a statement declares itself, which the schema cannot know.
#[derive(Clone, Debug, Default)]
pub struct LocalNames {
    /// The CTEs declared before the caret ([`crate::intel::ctes_at`]),
    /// offered where a table goes.
    pub ctes: Vec<crate::intel::LocalName>,
    /// The select-list aliases the caret's clause may name
    /// ([`crate::intel::output_aliases_at`]), offered where a column goes.
    pub output_aliases: Vec<crate::intel::LocalName>,
    /// The document's variables ([`crate::intel::variables_at`]), offered
    /// straight after `@`.
    pub variables: Vec<crate::intel::LocalName>,
    /// The document's temporary tables ([`crate::intel::temp_tables_at`]),
    /// offered straight after `#`.
    pub temp_tables: Vec<crate::intel::LocalName>,
    /// The slot takes a column's bare name and nothing else — a join's
    /// `USING (…)` list ([`crate::intel::in_using_list`]) — so a name two
    /// tables share is not qualified there.
    pub bare_columns: bool,
    /// The statement's CTEs and derived tables, by name, with the columns
    /// each exposes ([`crate::intel::local_relations`]). Asked before the
    /// schema for an unqualified name, since a CTE shadows a table of its name.
    pub relations: Vec<(String, Vec<ColMeta>)>,
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
    /// What this connection's user has accepted before, lower-cased, with the
    /// bonus each has earned ([`crate::accepted::AcceptedFile::bonuses`]).
    pub accepted: &'a HashMap<String, i32>,
    /// The tab's database, for the "don't offer the database you are in" rule.
    pub active_db: Option<&'a str>,
    /// The tab's engine, which decides **which builtin catalog** is offered.
    ///
    /// Not a nicety: without it `rank` walked [`crate::intel::FUNCTIONS`] on every engine, so
    /// a PostgreSQL tab was offered MySQL's names and a SQLite one the same —
    /// the typo checker's original bug, in the surface that *inserts* the word
    /// rather than the one that underlines it. It is asked as a dialect and
    /// answered by [`crate::intel::builtin_catalog`], the one place that maps an
    /// engine to its catalog, so a fourth engine lands on `None` and is offered
    /// nothing rather than inheriting whichever list is nearest.
    pub dialect: crate::intel::SqlDialect,
    /// **Which MySQL-family server, where the dialect cannot say.**
    ///
    /// `SqlDialect` has no MariaDB arm on purpose, and for parsing, quoting and
    /// the *shape* of a completion that is right. It is wrong for one thing
    /// only: which names exist. `FUNCTIONS` is one list transcribed from
    /// MariaDB's manual, so a MySQL 8 tab was offered forty-nine functions it
    /// cannot call — `NVL`, `TO_CHAR`, the eight `COLUMN_*`, the nine
    /// `*_ORACLE`, the three `WSREP_*` — and a MariaDB tab five of MySQL's.
    /// Measured on 8.4.11 and 10.11.14; see `intel::MARIADB_ONLY`.
    ///
    /// The caller reads it off the loaded `DbSchema` for the tab's active
    /// database, which is where `collect_schema` stamps it. `Unknown` — a
    /// PostgreSQL or SQLite tab, or one whose schema has not loaded yet —
    /// offers everything, which is what this did before the split existed.
    pub flavour: crate::schema::ServerFlavour,
    /// The statement's own CTEs and select-list aliases.
    pub local: &'a LocalNames,
    /// **The widened list**, a second Ctrl+Space: at a table slot, every
    /// table of every database and schema too, each inserted fully qualified
    /// ([`SchemaIndex::all_tables`]).
    pub wide: bool,
}

/// Rank the candidates a caret position offers.
///
/// Collects raw candidates (dedup by text, first/lowest tier wins), scores each by
/// fuzzy match plus a recency bonus, and sorts by tier, then score, then a shorter
/// candidate. Non-matches drop out. At most [`MAX_ROWS`].
pub fn rank(schema: &SchemaIndex, input: &RankInput<'_>) -> Vec<Suggestion> {
    rank_full(schema, input).items
}

/// What [`rank_full`] answers.
pub struct Ranked {
    /// The rows, as [`rank`] returns them.
    pub items: Vec<Suggestion>,
    /// The word typed already spells a candidate in full.
    pub completes_a_word: bool,
}

/// [`rank`], and whether the word typed already spells a candidate in full.
pub fn rank_full(schema: &SchemaIndex, input: &RankInput<'_>) -> Ranked {
    let (items, completes_a_word) = rank_rows(schema, input);
    Ranked {
        items,
        completes_a_word,
    }
}

fn rank_rows(schema: &SchemaIndex, input: &RankInput<'_>) -> (Vec<Suggestion>, bool) {
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
    // **`detail` is a `&str`, and `worth_offering` is asked before anything is
    // built** — `add_col`'s shape, for `add_col`'s reason and pinned by the same
    // test. A PostgreSQL tab offers 863 builtins where MySQL offers 309, and
    // every one of them was paying a `to_ascii_lowercase`, a `HashSet<String>`
    // insert and an eagerly **allocated** `String` for its signature before this
    // function could reject it — on a path that runs undebounced
    // (`exec_after(Duration::ZERO)`, one tick per keystroke).
    //
    // Measured per keystroke in a release build, `ClauseCtx::Column` with one
    // table in scope, before → after: PostgreSQL 511.2 → 40–45 µs on a prefix
    // of `"cu"` and 493.7 → 20–23 µs on `"sub"`; MySQL 86.6 → 10–12 µs and
    // 72.6 → 6 µs. **An empty prefix is the case this does not help**, and it is
    // reported rather than flattered: nothing is pre-filtered when every
    // candidate matches, so that path gains only the offered subset and the
    // late allocation — PostgreSQL 590.1 → 318–407 µs, while MySQL's 146.7 µs
    // against 171–226 µs moved within build-to-build variation and is not
    // attributable to this change (checked by short-circuiting the pre-filter
    // on an empty prefix, which changed nothing). What that path costs is
    // scoring and sorting the whole pool, which this does not touch.
    //
    // The pre-filter goes above the dedup for the reason `add_col` sets out:
    // `worth_offering` depends on nothing but the candidate's own text and the
    // prefix, so a candidate that does not match cannot be blocking one that
    // does — the later duplicate would have the same text and fail the same
    // test.
    let add = |cands: &mut Vec<Cand>,
               seen: &mut HashSet<String>,
               text: &str,
               kind: SuggestKind,
               detail: &str,
               tier: u8| {
        if !worth_offering(text, prefix) {
            return;
        }
        let tl = text.to_ascii_lowercase();
        // The exact match is left out but still marked seen — `completes_a_word`.
        let exact = tl == pl;
        if !seen.insert(tl) || exact {
            return;
        }
        // A keyword goes in the case the word is being typed in: `sel` takes
        // `select`, `Sel` and `SEL` the conventional `SELECT` — as the inline
        // predictions answer. With nothing typed there is no case to follow.
        let text = if kind == SuggestKind::Keyword && !prefix.is_empty() {
            crate::intel::in_typed_case(text, prefix)
        } else {
            text.to_string()
        };
        cands.push(Cand {
            text,
            kind,
            detail: detail.to_string(),
            table: String::new(),
            alias: String::new(),
            glyph: SuggestGlyph::Kind,
            key: KeyKind::None,
            tier,
            insert: None,
            replace: None,
        });
    };
    // A table row just added (past `before`) that names a view wears the
    // view's glyph, as the schema tree's node does.
    //
    // **Both of these ask whether a row was added before anything else**: at
    // a table slot they run for every table in the catalogue, and nearly all
    // of them were just refused by the prefix — the lower-cased keys, the
    // namespace lookup and its clone were paid for each one regardless (10,000
    // tables, `ord` typed: 3.0 ms a keystroke).
    let mark_view = |cands: &mut Vec<Cand>, before: usize, db: &str, name: &str| {
        if cands.len() == before {
            return;
        }
        let key = (db.to_ascii_lowercase(), name.to_ascii_lowercase());
        if schema.views.contains(&key)
            && let Some(c) = cands.last_mut()
        {
            c.glyph = SuggestGlyph::View;
        }
    };
    // A table row no bare name reaches ([`SchemaIndex::namespaces`]) inserts
    // the name that does, while still reading as the table.
    let qualify_unreached = |cands: &mut Vec<Cand>, before: usize, db: &str, name: &str| {
        if cands.len() == before {
            return;
        }
        let unreached = schema
            .namespaces
            .get(&name.to_ascii_lowercase())
            .filter(|nss| {
                nss.iter()
                    .all(|ns| !crate::filter::bare_name_reaches(input.dialect, ns.as_deref()))
            })
            .and_then(|nss| nss.first().cloned().flatten());
        if let Some(ns) = unreached
            && let Some(c) = cands.last_mut()
        {
            c.insert = Some(crate::filter::qualified_table_name(
                input.dialect,
                db,
                Some(&ns),
                name,
            ));
        }
    };
    // A name the statement declares itself: inserted as the statement spelled
    // it, never re-quoted by the engine's rules (see `intel::LocalName`).
    let add_local = |cands: &mut Vec<Cand>,
                     seen: &mut HashSet<String>,
                     n: &crate::intel::LocalName,
                     kind: SuggestKind,
                     detail: &str,
                     glyph: SuggestGlyph,
                     tier: u8| {
        let before = cands.len();
        add(cands, seen, &n.name, kind, detail, tier);
        if cands.len() > before
            && let Some(c) = cands.last_mut()
        {
            c.glyph = glyph;
            c.insert = Some(n.written.clone());
        }
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
    // by column name (first table in scope wins). A column the statement
    // `written` itself (a CTE's, a derived table's) inserts as it was spelled
    // (`intel::written_name_sql`), never through `accepted_insert`'s quoting:
    // PostgreSQL folded a bare `Total` to `total`, so `"Total"` names nothing.
    let add_col = |cands: &mut Vec<Cand>,
                   seen: &mut HashSet<String>,
                   c: &ColMeta,
                   table: &str,
                   alias: Option<&str>,
                   tier: u8,
                   written: bool| {
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
        let exact = tl == pl;
        if !seen.insert(tl) || exact {
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
            insert: written.then(|| crate::intel::written_name_sql(&c.name, input.dialect)),
            replace: None,
        });
    };
    // The statement's own CTE or derived table of this name, which shadows a
    // table of its name.
    let local_rel = |name: &str| {
        input
            .local
            .relations
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
    };
    // A scope reference's qualifier as SQL: its alias as the statement wrote
    // it, else its name — on a folding engine as the catalogue spells it,
    // which is the table the server resolved however the statement cased it
    // (`intel::written_name_sql`'s reason). Searched for only when the name
    // has a capital in it, which is the one case the two spellings can differ.
    let qualifier_sql = |name: &str, alias: Option<&str>| -> String {
        let d = input.dialect;
        if let Some(a) = alias {
            return crate::intel::written_name_sql(a, d);
        }
        if !d.folds_unquoted_names() {
            return crate::export::ident_if_needed(name, d);
        }
        let catalogued = (local_rel(name).is_none()
            && name.bytes().any(|b| b.is_ascii_uppercase()))
        .then(|| {
            schema
                .tables
                .iter()
                .find(|(t, _)| t.eq_ignore_ascii_case(name))
        })
        .flatten();
        match catalogued {
            Some((t, _)) => crate::export::ident_if_needed(t, d),
            None => crate::intel::written_name_sql(name, d),
        }
    };
    // In-scope table references as qualifier candidates: an alias (`ac`, tag glyph,
    // detail = the table it stands for) or, for an unaliased table, its name (table
    // glyph). Offered in column contexts so `ON a` suggests `ac` before you type `.`.
    // Each inserts as `qualifier_sql` writes it.
    let add_aliases = |cands: &mut Vec<Cand>, seen: &mut HashSet<String>| {
        for r in scope {
            let qtext = r.alias.as_deref().unwrap_or(&r.name);
            let tl = qtext.to_ascii_lowercase();
            let exact = tl == pl;
            if !seen.insert(tl) || exact {
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
                insert: Some(qualifier_sql(&r.name, r.alias.as_deref())),
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
    // A qualifier is a database (MySQL), else a schema of the tab's own
    // database (PostgreSQL, SQL Server) — `SchemaIndex::columns_by_schema`.
    // A bare name is the statement's own CTE or derived table first — one
    // shadows a table of its name — and the schema's otherwise.
    // The flag says which: a local relation's columns are spelled as the
    // statement wrote them (`add_col`'s `written`).
    let cols_of = |db: Option<&str>, name: &str| -> (&[ColMeta], bool) {
        match db.is_none().then(|| local_rel(name)).flatten() {
            Some((_, cols)) => (cols, true),
            None => (schema.columns_of(db, name), false),
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
    // Skipped after a `qualifier.` (there we want only that table's columns),
    // and at a cast's target, which takes a type and nothing else.
    if !qualified
        && !matches!(
            ctx,
            ClauseCtx::Type | ClauseCtx::ColumnType | ClauseCtx::Variable | ClauseCtx::TempTable
        )
    {
        for kw in &cont.keywords {
            add(&mut cands, &mut seen, kw, SuggestKind::Keyword, "", 0);
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
    // The shared keyword lists, less the words this engine has no use for.
    let keywords = |list: &'static [&'static str]| {
        list.iter()
            .copied()
            .filter(|k| crate::intel::keyword_offered(k, input.dialect))
    };
    // …and this engine's own past them (`intel::engine_keywords`), in the same
    // tier wherever the shared ones are offered.
    let clause_keywords = || {
        keywords(SQL_KEYWORDS).chain(crate::intel::engine_keywords(input.dialect, input.flavour))
    };
    // The engine's builtins, in `tier`.
    //
    // **The offered subset, precomputed once per dialect.**
    // `is_offered_builtin` is a binary search on PostgreSQL, and asking it per
    // candidate meant 2,706 of them per keystroke to arrive at the same 863
    // names every time. `offered_builtins` is that filter's answer, held in
    // `intel`'s `CatalogIndex` beside the typo checker's own rearrangement of
    // the same data.
    let add_builtins = |cands: &mut Vec<Cand>, seen: &mut HashSet<String>, tier: u8| {
        for fun in crate::intel::offered_builtins(input.dialect, input.flavour).unwrap_or(&[]) {
            let before = cands.len();
            add(
                cands,
                seen,
                fun.name,
                SuggestKind::Function,
                fun.signature,
                tier,
            );
            // A builtin the engine calls bare goes in bare, not as the
            // `name()` a function row inserts.
            if cands.len() > before
                && crate::intel::called_bare(fun.name, input.dialect)
                && let Some(c) = cands.last_mut()
            {
                c.insert = Some(fun.name.to_string());
            }
        }
    };
    // The database's own functions, where an expression goes — ahead of the
    // builtins, so one named like a builtin claims its row. Not a trigger
    // function, which no expression can call. Each is called by the name
    // that reaches it (`routine_sql`), parens and all.
    let add_user_functions = |cands: &mut Vec<Cand>, seen: &mut HashSet<String>, tier: u8| {
        for r in &schema.routines {
            if r.kind == crate::schema::RoutineKind::Function
                && !r.returns.eq_ignore_ascii_case("trigger")
            {
                let before = cands.len();
                let detail = format!("({}) → {}", r.arguments, r.returns);
                add(cands, seen, &r.name, SuggestKind::Function, &detail, tier);
                if cands.len() > before
                    && let Some(c) = cands.last_mut()
                {
                    c.insert = routine_sql(r, input.dialect).map(|n| format!("{n}()"));
                }
            }
        }
    };

    match ctx {
        // The name a `CALL` or `EXEC` runs: the procedures, whose arguments
        // come in parens where the engine's call takes them and bare after a
        // T-SQL `EXEC` (`intel::procedure_args_in_parens`).
        ClauseCtx::Routine => {
            for r in &schema.routines {
                if r.kind != crate::schema::RoutineKind::Procedure {
                    continue;
                }
                let before = cands.len();
                let detail = format!("({})", r.arguments);
                add(
                    &mut cands,
                    &mut seen,
                    &r.name,
                    SuggestKind::Function,
                    &detail,
                    0,
                );
                // By the name that reaches it (`routine_sql`); a T-SQL
                // `EXEC` takes no parens, so its row's insert is that name
                // even where it is the bare one.
                if cands.len() > before
                    && let Some(c) = cands.last_mut()
                {
                    let name = routine_sql(r, input.dialect);
                    c.insert = if crate::intel::procedure_args_in_parens(input.dialect) {
                        name.map(|n| format!("{n}()"))
                    } else {
                        Some(name.unwrap_or_else(|| r.name.clone()))
                    };
                }
            }
        }
        ClauseCtx::Qualified(q) => {
            // A qualifier is either a table/alias (→ its columns) or a database name
            // (→ its tables, for `db.table`). The scope resolver no longer misparses
            // a dangling `db.` as a table (fixed in `intel::lexer_scope`), so the
            // natural table-first order is safe.
            if let Some((table, db, alias)) = resolve(q) {
                let (cols, written) = cols_of(db.as_deref(), &table);
                for c in cols {
                    add_col(
                        &mut cands,
                        &mut seen,
                        c,
                        &table,
                        alias.as_deref(),
                        0,
                        written,
                    );
                }
            } else if let Some(tbls) = schema
                .tables_by_db
                .get(&q.to_ascii_lowercase())
                .or_else(|| schema.tables_by_schema.get(&q.to_ascii_lowercase()))
            {
                for t in tbls {
                    let before = cands.len();
                    add(&mut cands, &mut seen, t, SuggestKind::Table, q, 0);
                    mark_view(&mut cands, before, q, t);
                }
            }
        }
        // `USE |`: every database but the one the tab is already in, with no
        // prefix needed — there is nothing else the word can be.
        ClauseCtx::Database => {
            for db in &schema.databases {
                if active_db.is_none_or(|a| !a.eq_ignore_ascii_case(db)) {
                    add(&mut cands, &mut seen, db, SuggestKind::Database, "", 0);
                }
            }
        }
        ClauseCtx::Table => {
            // The statement's own CTEs, ahead of everything: one shadows a table
            // of the same name, and claims its row.
            for cte in &input.local.ctes {
                add_local(
                    &mut cands,
                    &mut seen,
                    cte,
                    SuggestKind::Table,
                    "CTE",
                    SuggestGlyph::Table,
                    0,
                );
            }
            // FK-aware JOIN targets first (top tier): a table connected by a foreign
            // key to something in scope, inserting `table ON <predicate>` in one go.
            let mut fk_added = false;
            for jt in input.join_targets {
                let tl = jt.table.to_ascii_lowercase();
                let exact = tl == pl;
                if !seen.insert(tl) || exact {
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
                    insert: Some(match &jt.aliased {
                        Some(a) => format!("{} {} ON {}", jt.table_sql, a.alias, a.predicate),
                        None => format!("{} ON {}", jt.table_sql, jt.predicate),
                    }),
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
                let before = cands.len();
                add(
                    &mut cands,
                    &mut seen,
                    name,
                    SuggestKind::Table,
                    db,
                    plain_tier,
                );
                mark_view(&mut cands, before, db, name);
                qualify_unreached(&mut cands, before, db, name);
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
                        "",
                        table_tier + 1,
                    );
                }
            }
            // The widened list: every table of every database and schema,
            // under the tab's own, each inserting the reference that reaches
            // it from here — and none PostgreSQL could not reach. The row
            // shows that reference with its quotes off, and is matched by it.
            if input.wide {
                for t in &schema.all_tables {
                    let Some(reach) = crate::filter::table_name_from(
                        input.dialect,
                        active_db,
                        &t.database,
                        t.schema.as_deref(),
                        &t.name,
                    ) else {
                        continue;
                    };
                    let shown: String = reach
                        .chars()
                        .filter(|c| !matches!(c, '`' | '"' | '[' | ']'))
                        .collect();
                    let before = cands.len();
                    add(
                        &mut cands,
                        &mut seen,
                        &shown,
                        SuggestKind::Table,
                        &t.database,
                        plain_tier + 1,
                    );
                    if cands.len() > before
                        && let Some(c) = cands.last_mut()
                    {
                        c.insert = Some(reach);
                    }
                }
            }
        }
        ClauseCtx::Column => {
            // The select list's aliases, where the clause may name them (the
            // caller asks `intel::output_aliases_at`): an `ORDER BY` names its
            // output more often than its input.
            for alias in &input.local.output_aliases {
                add_local(
                    &mut cands,
                    &mut seen,
                    alias,
                    SuggestKind::Column,
                    "alias",
                    SuggestGlyph::Alias,
                    0,
                );
            }
            if scope.is_empty() {
                // No FROM yet: offer every column, annotated by its owning table so
                // the broader list stays navigable.
                for (name, _) in &schema.tables {
                    let (cols, written) = cols_of(None, name);
                    for c in cols {
                        add_col(&mut cands, &mut seen, c, name, None, 1, written);
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
                let first_col = cands.len();
                for (i, r) in scope.iter().enumerate().rev() {
                    let tier = if i == last { 0 } else { 1 };
                    let (cols, written) = cols_of(r.db.as_deref(), &r.name);
                    for c in cols {
                        let alias = r.alias.as_deref();
                        add_col(&mut cands, &mut seen, c, &r.name, alias, tier, written);
                    }
                }
                // **A name two tables share is inserted qualified** — bare, the
                // server refuses it as ambiguous — by the table whose row it is
                // (its alias, else its name, through `qualifier_sql`). Not where
                // the slot takes a bare name only (`USING (…)`).
                if scope.len() > 1 && !input.local.bare_columns {
                    let q = |s: &str| crate::export::ident_if_needed(s, input.dialect);
                    // How many references in scope have each name, counted
                    // once up front: asking every scope table's column list
                    // per candidate was quadratic in the joined columns, on
                    // the undebounced path (20 tables × 60 columns: 1.7 ms a
                    // keystroke). Each reference counts a name once, however
                    // many of its columns spell it.
                    let mut holders: HashMap<String, (usize, usize)> = HashMap::new();
                    for (i, r) in scope.iter().enumerate() {
                        for m in cols_of(r.db.as_deref(), &r.name).0 {
                            let e = holders
                                .entry(m.name.to_ascii_lowercase())
                                .or_insert((0, usize::MAX));
                            if e.1 != i {
                                *e = (e.0 + 1, i);
                            }
                        }
                    }
                    for c in &mut cands[first_col..] {
                        if c.kind != SuggestKind::Column {
                            continue;
                        }
                        let shared = holders.get(&c.text.to_ascii_lowercase()).map_or(0, |e| e.0);
                        if shared > 1 {
                            let alias = (!c.alias.is_empty()).then_some(c.alias.as_str());
                            let col = c.insert.take().unwrap_or_else(|| q(&c.text));
                            c.insert = Some(format!("{}.{col}", qualifier_sql(&c.table, alias)));
                        }
                    }
                }
            }
            add_user_functions(&mut cands, &mut seen, 2);
            add_builtins(&mut cands, &mut seen, 2);
            // A keyword the typed word begins competes with the builtins on
            // score, where its prefix bonus lifts it past every builtin that
            // only fuzzy-matches: under them, PostgreSQL's `parti` filled the
            // list with `pg_*` names and buried `PARTITION`. A fuzzy keyword
            // match stays under them all.
            for k in clause_keywords() {
                let tier = if begins_with(k, prefix) { 2 } else { 3 };
                add(&mut cands, &mut seen, k, SuggestKind::Keyword, "", tier);
            }
        }
        // Straight after a sigil: the document's own names, and nothing the
        // schema could put there.
        // A column definition's type: the engine's column types, as a cast's
        // target offers its cast types.
        ClauseCtx::ColumnType => {
            for t in crate::intel::column_types(input.dialect) {
                add(&mut cands, &mut seen, &t, SuggestKind::Keyword, "type", 0);
            }
        }
        ClauseCtx::Variable => {
            for v in &input.local.variables {
                let glyph = SuggestGlyph::Kind;
                add_local(
                    &mut cands,
                    &mut seen,
                    v,
                    SuggestKind::Column,
                    "variable",
                    glyph,
                    0,
                );
            }
        }
        ClauseCtx::TempTable => {
            for t in &input.local.temp_tables {
                let glyph = SuggestGlyph::Table;
                add_local(
                    &mut cands,
                    &mut seen,
                    t,
                    SuggestKind::Table,
                    "temp table",
                    glyph,
                    0,
                );
            }
        }
        // A value with no columns in reach: functions first, then keywords.
        ClauseCtx::Value => {
            add_user_functions(&mut cands, &mut seen, 0);
            add_builtins(&mut cands, &mut seen, 0);
            for k in clause_keywords() {
                let tier = if begins_with(k, prefix) { 0 } else { 1 };
                add(&mut cands, &mut seen, k, SuggestKind::Keyword, "", tier);
            }
        }
        // A cast's target: the engine's cast types and nothing else, in the
        // typed case as a keyword is.
        ClauseCtx::Type => {
            for t in crate::intel::cast_target_types(input.dialect, input.flavour) {
                add(&mut cands, &mut seen, &t, SuggestKind::Keyword, "type", 0);
            }
        }
        ClauseCtx::Start => {
            for k in keywords(STMT_KEYWORDS) {
                add(&mut cands, &mut seen, k, SuggestKind::Keyword, "", 0);
            }
            for k in clause_keywords() {
                add(&mut cands, &mut seen, k, SuggestKind::Keyword, "", 1);
            }
        }
        ClauseCtx::Other => {
            add_aliases(&mut cands, &mut seen);
            for r in scope {
                let (cols, written) = cols_of(r.db.as_deref(), &r.name);
                for c in cols {
                    add_col(
                        &mut cands,
                        &mut seen,
                        c,
                        &r.name,
                        r.alias.as_deref(),
                        0,
                        written,
                    );
                }
            }
            for (name, db) in &schema.tables {
                let before = cands.len();
                add(&mut cands, &mut seen, name, SuggestKind::Table, db, 1);
                qualify_unreached(&mut cands, before, db, name);
            }
            for k in clause_keywords() {
                add(&mut cands, &mut seen, k, SuggestKind::Keyword, "", 2);
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

    // Every candidate that spelled the word typed was marked seen on its way out.
    let completes_a_word = !pl.is_empty() && seen.contains(&pl);

    // Score by fuzzy match (+ recency bonus); sort by tier (context priority), then
    // score, then a shorter candidate. Non-matches drop out.
    let mut scored: Vec<(u8, i32, Cand)> = cands
        .into_iter()
        .filter_map(|c| {
            fuzzy_score(&c.text, prefix).map(|s| {
                let earned = accepted_bonus(&c.text, input.accepted);
                (c.tier, s + recency_bonus(&c.text, c.kind, used) + earned, c)
            })
        })
        .collect();
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.1.cmp(&a.1))
            .then(a.2.text.len().cmp(&b.2.text.len()))
    });
    let items = scored
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
        .collect();
    (items, completes_a_word)
}

/// What accepting `s` splices over the word being typed, when that is not
/// `s.text` as it reads: the row's own `insert` (an FK join target, a
/// snippet's body, the star expansion), or — for a table, column or database
/// row — its name through [`crate::export::ident_if_needed`], where bare it
/// would be something else (`` `order` ``, `` `Unit Price` ``, PostgreSQL's
/// `"OrderDate"`). The row still shows, and was matched by, the bare name.
///
/// **Inside a quoted name the name goes in bare** (`in_quoted_name`):
/// completion runs there on purpose, and the quote before the word is the
/// user's — a quoted insert would double it.
pub fn accepted_insert(
    s: &Suggestion,
    dialect: crate::intel::SqlDialect,
    in_quoted_name: bool,
) -> Option<String> {
    // Inside a quote the user opened, a table row is its name alone: a
    // qualified insert (`sales.ledger`, the widened list's references) would
    // land after the quote as one identifier with a dot in it. So is a column
    // row, which is qualified where two tables share its name.
    if in_quoted_name && matches!(s.kind, SuggestKind::Table | SuggestKind::Column) {
        return None;
    }
    if s.insert.is_some() {
        return s.insert.clone();
    }
    match s.kind {
        SuggestKind::Table | SuggestKind::Column | SuggestKind::Database if !in_quoted_name => {
            Some(crate::export::ident_if_needed(&s.text, dialect)).filter(|q| *q != s.text)
        }
        _ => None,
    }
}

/// The one row offered straight after a fresh `JOIN … ON `: the foreign-key
/// predicate ([`crate::intel::join_condition`]), ready to insert.
///
/// **It carries its own `insert`.** The row is a predicate, not a name, so
/// [`accepted_insert`]'s quoting of a bare column row must never reach it: it
/// wrapped `o.customer_id = c.id` whole in identifier quotes, a server error on
/// three engines and, on SQLite, a string literal that silently matched no row.
pub fn join_condition_row(predicate: String) -> Suggestion {
    Suggestion {
        insert: Some(predicate.clone()),
        text: predicate,
        kind: SuggestKind::Column,
        detail: "foreign key".to_string(),
        table: String::new(),
        alias: String::new(),
        // A purple key-square marks the ready-to-insert FK join predicate.
        glyph: SuggestGlyph::ForeignKey,
        key: KeyKind::Foreign,
        replace: None,
    }
}

/// A stored routine's name as a statement calls it, or `None` where that is
/// its bare name: qualified where no bare name reaches its namespace
/// ([`crate::filter::bare_name_reaches`]) and, for a function, where the
/// engine calls one only by its schema
/// ([`crate::intel::user_function_call_needs_schema`]); each part through
/// [`crate::export::ident_if_needed`], since it is the catalogue's spelling.
pub fn routine_sql(r: &IndexedRoutine, dialect: crate::intel::SqlDialect) -> Option<String> {
    let q = |s: &str| crate::export::ident_if_needed(s, dialect);
    let schema = r.schema.as_deref().filter(|s| !s.is_empty());
    let qualify = schema.is_some()
        && (!crate::filter::bare_name_reaches(dialect, schema)
            || (crate::intel::user_function_call_needs_schema(dialect)
                && r.kind == crate::schema::RoutineKind::Function));
    let name = match schema {
        Some(ns) if qualify => format!("{}.{}", q(ns), q(&r.name)),
        _ => q(&r.name),
    };
    (name != r.name).then_some(name)
}

/// What accepting a row with an override (`accepted_insert`'s answer)
/// splices, and the caret within it. A function row's override that ends in
/// a call's `()` — a routine inserted qualified or quoted ([`routine_sql`]) —
/// is a call like the `name()` a plain function row gets: the caret goes
/// between the parens, and the parens are left out where the call's own
/// already follow. Anything else goes in whole, the caret at its end.
pub fn override_splice(over: String, kind: SuggestKind, parens_follow: bool) -> (String, usize) {
    if kind == SuggestKind::Function
        && let Some(name) = over.strip_suffix("()")
    {
        return if parens_follow {
            (name.to_string(), name.len())
        } else {
            let caret = name.len() + 1;
            (over, caret)
        };
    }
    let len = over.len();
    (over, len)
}

/// The members of the column a string is compared with — `status = '|` —
/// offered in the string ([`crate::intel::enum_value_slot`]), or nothing when
/// the column is not in scope or not an enum or set.
///
/// The column is the one `slot` names in the statement's `scope`: through its
/// qualifier (an alias, or a table's own name), or the first table in scope
/// that has it. Its members are its type's — a MySQL `enum(…)`/`set(…)`,
/// read as the grid's cell editor reads them (`celledit::editor_for_type`) —
/// or, for a PostgreSQL enum, the labels [`SchemaIndex::enum_types`] holds.
/// Kept in declaration order, which for an enum is its meaning, and narrowed
/// to those starting with what is typed.
///
/// Each row replaces the literal's contents, written as the dialect writes a
/// string literal (`export::sql_literal`, quotes off), so `won't` arrives as
/// `won''t`; the closing quote comes with it where the literal had none.
pub fn enum_value_suggestions(
    schema: &SchemaIndex,
    scope: &[TableRef],
    slot: &crate::intel::EnumSlot,
    sql: &str,
    caret: usize,
    dialect: crate::intel::SqlDialect,
) -> Vec<Suggestion> {
    let named = |r: &&TableRef| {
        slot.qualifier.as_deref().is_none_or(|q| {
            r.alias
                .as_deref()
                .unwrap_or(&r.name)
                .eq_ignore_ascii_case(q)
        })
    };
    let Some(type_name) = scope.iter().filter(named).find_map(|r| {
        schema
            .columns_of(r.db.as_deref(), &r.name)
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(&slot.column))
            .map(|c| c.type_name.clone())
    }) else {
        return Vec::new();
    };
    use crate::celledit::{CellEditor, editor_for_type};
    let members = match editor_for_type(&type_name, dialect) {
        CellEditor::Enum(m) | CellEditor::Set(m) => m,
        _ => match schema
            .enum_types
            .get(&type_name.trim().to_ascii_lowercase())
        {
            Some(m) => m.clone(),
            None => return Vec::new(),
        },
    };
    let typed = sql.get(slot.value.0..caret.max(slot.value.0)).unwrap_or("");
    members
        .into_iter()
        .filter(|m| typed.is_empty() || begins_with(m, typed))
        .map(|m| {
            let lit = crate::export::sql_literal(&crate::model::Value::Str(m.clone()), dialect);
            let inner = &lit[lit.find('\'').map_or(0, |i| i + 1)..lit.len() - 1];
            let insert = if slot.closed {
                inner.to_string()
            } else {
                format!("{inner}'")
            };
            Suggestion {
                text: m,
                kind: SuggestKind::Column,
                detail: type_name.clone(),
                table: String::new(),
                alias: String::new(),
                glyph: SuggestGlyph::Kind,
                key: KeyKind::None,
                insert: Some(insert),
                replace: Some(slot.value),
            }
        })
        .collect()
}

/// Does `cand` begin with a non-empty `prefix`, ignoring ASCII case?
fn begins_with(cand: &str, prefix: &str) -> bool {
    !prefix.is_empty()
        && cand
            .as_bytes()
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
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
            // A word starts at the name's start, after `_`, and at a camelCase
            // hump — an upper-case letter after a lower-case one, so `oi`
            // finds `OrderItem`'s two words as it finds `order_item`'s.
            let boundary = ci == 0
                || c[ci - 1] == b'_'
                || (c[ci].is_ascii_uppercase() && c[ci - 1].is_ascii_lowercase());
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

/// The bonus a candidate has earned by being accepted on this connection
/// before ([`RankInput::accepted`]) — any kind, a keyword or a snippet as much
/// as a name: what the user keeps choosing is what they will choose again.
/// Free when nothing has been.
pub fn accepted_bonus(text: &str, accepted: &HashMap<String, i32>) -> i32 {
    if accepted.is_empty() {
        return 0;
    }
    accepted.get(&text.to_lowercase()).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intel::SqlDialect;
    use crate::schema::ServerFlavour;

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
            ..SchemaIndex::default()
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
        ranked_on(schema, SqlDialect::MySql, ctx, scope, prefix)
    }

    /// [`ranked`], on a named engine — with the flavour left `Unknown`, which
    /// is what every engine but the MySQL family answers and what a MySQL tab
    /// answers until its schema loads.
    fn ranked_on(
        schema: &SchemaIndex,
        dialect: SqlDialect,
        ctx: ClauseCtx,
        scope: &[TableRef],
        prefix: &str,
    ) -> Vec<String> {
        ranked_on_flavour(schema, dialect, ServerFlavour::Unknown, ctx, scope, prefix)
    }

    /// [`ranked_on`], on a named MySQL-family server.
    fn ranked_on_flavour(
        schema: &SchemaIndex,
        dialect: SqlDialect,
        flavour: ServerFlavour,
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
                dialect,
                flavour,
                local: &LocalNames::default(),
                wide: false,
                accepted: &Default::default(),
            },
        )
        .into_iter()
        .map(|s| s.text)
        .collect()
    }

    /// **What the user keeps accepting ranks higher**, through `rank` itself:
    /// `customers` sorts below the shorter `orders` until it has been taken
    /// before — and a prefix that matches one name better still wins.
    #[test]
    fn a_word_accepted_before_ranks_above_its_equals() {
        let schema = shop();
        let rank_with = |prefix: &str, accepted: &HashMap<String, i32>| -> Vec<String> {
            let (cont, used) = (Continuation::default(), HashSet::new());
            rank(
                &schema,
                &RankInput {
                    ctx: &ClauseCtx::Table,
                    cont: &cont,
                    scope: &[],
                    prefix,
                    snippets: &[],
                    join_targets: &[],
                    star: None,
                    used: &used,
                    active_db: Some("shop"),
                    dialect: SqlDialect::MySql,
                    flavour: ServerFlavour::Unknown,
                    local: &LocalNames::default(),
                    wide: false,
                    accepted,
                },
            )
            .into_iter()
            .map(|s| s.text)
            .collect()
        };
        let pos = |rows: &[String], w: &str| rows.iter().position(|r| r == w).unwrap();
        let none = rank_with("", &HashMap::new());
        assert!(pos(&none, "orders") < pos(&none, "customers"), "{none:?}");
        let mut taken = HashMap::new();
        taken.insert("customers".to_string(), crate::accepted::bonus(3, 60));
        let after = rank_with("", &taken);
        assert!(
            pos(&after, "customers") < pos(&after, "orders"),
            "{after:?}"
        );
        // `ord` is a prefix of one and not the other: the earned bonus does
        // not outweigh it.
        let typed = rank_with("ord", &taken);
        assert_eq!(
            typed.first().map(String::as_str),
            Some("orders"),
            "{typed:?}"
        );
    }

    /// A builtin the engine calls bare is offered with its bare name to insert,
    /// so accepting it writes no `()`; an ordinary function is left to get them.
    #[test]
    fn a_builtin_called_bare_inserts_without_parentheses() {
        let s = shop();
        let row = |dialect: SqlDialect, prefix: &str, name: &str| {
            let cont = Continuation::default();
            let used = HashSet::new();
            rank(
                &s,
                &RankInput {
                    ctx: &ClauseCtx::Column,
                    cont: &cont,
                    scope: &[tref("orders", None)],
                    prefix,
                    snippets: &[],
                    join_targets: &[],
                    star: None,
                    used: &used,
                    active_db: Some("shop"),
                    dialect,
                    flavour: ServerFlavour::Unknown,
                    local: &LocalNames::default(),
                    wide: false,
                    accepted: &Default::default(),
                },
            )
            .into_iter()
            .find(|r| r.text.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("{name} offered on {dialect:?}"))
        };
        for dialect in SqlDialect::ALL {
            let r = row(dialect, "current_ti", "current_timestamp");
            assert_eq!(r.kind, SuggestKind::Function, "{dialect:?}");
            assert_eq!(r.insert.as_deref(), Some(r.text.as_str()), "{dialect:?}");
        }
        assert_eq!(row(SqlDialect::MySql, "coal", "coalesce").insert, None);
        assert_eq!(row(SqlDialect::Postgres, "coal", "coalesce").insert, None);
    }

    /// A camelCase hump is a word boundary, as `_` is: `oi` scores `OrderItem`
    /// (its `I` starts a word) above `Orderingx` (its `i` is mid-word), and
    /// `oi` scores `order_item` the same as `OrderItem`.
    #[test]
    fn a_camel_case_hump_scores_as_a_word_boundary() {
        let s = |c| fuzzy_score(c, "oi").unwrap();
        assert!(
            s("OrderItem") > s("Orderingx"),
            "{} / {}",
            s("OrderItem"),
            s("Orderingx")
        );
        assert!(s("orderItem") > s("orderingx"));
        // Both find two words; `OrderItem` is a byte shorter and its gap one less.
        assert_eq!(s("OrderItem"), s("order_item") + 2);
        // An all-caps run is not a run of humps.
        assert_eq!(
            fuzzy_score("ORDERINGX", "oi"),
            fuzzy_score("orderingx", "oi")
        );
    }

    /// A keyword row is in the case the word is typed in; a name keeps its own.
    #[test]
    fn a_keyword_is_offered_in_the_typed_case() {
        let s = shop();
        let start = |p| ranked(&s, ClauseCtx::Start, &[], p);
        assert!(start("sel").contains(&"select".to_string()));
        assert!(start("SEL").contains(&"SELECT".to_string()));
        assert!(start("Sel").contains(&"SELECT".to_string()));
        assert!(start("").contains(&"SELECT".to_string()));
        let scope = [tref("orders", None)];
        let col = ranked(&s, ClauseCtx::Column, &scope, "TOT");
        assert!(col.contains(&"total".to_string()), "{col:?}");
    }

    /// After `USE` the databases are offered, with no prefix needed and not
    /// the one the tab is in; nothing else is.
    #[test]
    fn use_offers_the_other_databases() {
        let s = shop();
        assert_eq!(ranked(&s, ClauseCtx::Database, &[], ""), ["archive"]);
        assert_eq!(ranked(&s, ClauseCtx::Database, &[], "arc"), ["archive"]);
        assert!(ranked(&s, ClauseCtx::Database, &[], "ord").is_empty());
    }

    /// A keyword the engine has no use for is not offered: no `LIMIT` on SQL
    /// Server, no `AUTO_INCREMENT` off MySQL, no `DESCRIBE` statement off it.
    #[test]
    fn a_keyword_is_offered_only_on_an_engine_that_takes_it() {
        let s = shop();
        let scope = [tref("orders", None)];
        let col = |d, p| ranked_on(&s, d, ClauseCtx::Column, &scope, p);
        assert!(offers(&col(SqlDialect::MySql, "lim"), "LIMIT"));
        assert!(offers(&col(SqlDialect::Postgres, "lim"), "LIMIT"));
        assert!(!offers(&col(SqlDialect::MsSql, "lim"), "LIMIT"));
        assert!(offers(&col(SqlDialect::MySql, "auto_"), "AUTO_INCREMENT"));
        for d in [SqlDialect::Postgres, SqlDialect::Sqlite, SqlDialect::MsSql] {
            assert!(!offers(&col(d, "auto_"), "AUTO_INCREMENT"), "{d:?}");
        }
        let start = |d, p| ranked_on(&s, d, ClauseCtx::Start, &[], p);
        assert!(offers(&start(SqlDialect::MySql, "desc"), "DESCRIBE"));
        assert!(!offers(&start(SqlDialect::Postgres, "desc"), "DESCRIBE"));
        assert!(offers(&start(SqlDialect::MsSql, "us"), "USE"));
        assert!(!offers(&start(SqlDialect::Sqlite, "trun"), "TRUNCATE"));
    }

    /// Past the forty shared words, each engine's own: window clauses and set
    /// operators everywhere, `TOP` and `APPLY` on SQL Server alone, `RETURNING`
    /// where the engine has it — MariaDB, not MySQL — and `LATERAL` where
    /// MySQL and PostgreSQL take it.
    #[test]
    fn each_engine_is_offered_its_own_keywords() {
        let s = shop();
        let scope = [tref("orders", None)];
        let col = |d, f, p| ranked_on_flavour(&s, d, f, ClauseCtx::Column, &scope, p);
        let (my, maria, any) = (
            ServerFlavour::MySql,
            ServerFlavour::MariaDb,
            ServerFlavour::Unknown,
        );
        for d in [
            SqlDialect::MySql,
            SqlDialect::Postgres,
            SqlDialect::Sqlite,
            SqlDialect::MsSql,
        ] {
            assert!(offers(&col(d, any, "ove"), "OVER"), "{d:?}");
            assert!(offers(&col(d, any, "inters"), "INTERSECT"), "{d:?}");
        }
        for d in [
            SqlDialect::MySql,
            SqlDialect::Postgres,
            SqlDialect::Sqlite,
            SqlDialect::MsSql,
        ] {
            assert!(offers(&col(d, any, "parti"), "PARTITION"), "{d:?}");
        }
        assert!(offers(&col(SqlDialect::MsSql, any, "to"), "TOP"));
        assert!(offers(&col(SqlDialect::MsSql, any, "app"), "APPLY"));
        assert!(!offers(&col(SqlDialect::MySql, my, "app"), "APPLY"));
        assert!(!offers(&col(SqlDialect::Postgres, any, "to"), "TOP"));
        assert!(offers(&col(SqlDialect::Postgres, any, "retu"), "RETURNING"));
        assert!(offers(&col(SqlDialect::Sqlite, any, "retu"), "RETURNING"));
        assert!(offers(&col(SqlDialect::MySql, maria, "retu"), "RETURNING"));
        assert!(!offers(&col(SqlDialect::MySql, my, "retu"), "RETURNING"));
        // Not yet asked: both, as the builtins offer everything then.
        assert!(offers(&col(SqlDialect::MySql, any, "retu"), "RETURNING"));
        assert!(offers(&col(SqlDialect::MySql, any, "late"), "LATERAL"));
        assert!(offers(&col(SqlDialect::MySql, my, "late"), "LATERAL"));
        assert!(!offers(&col(SqlDialect::MySql, maria, "late"), "LATERAL"));
        assert!(!offers(&col(SqlDialect::MsSql, any, "late"), "LATERAL"));
        // `MERGE` begins a statement where the engine has one.
        let start = |d, p| ranked_on(&s, d, ClauseCtx::Start, &[], p);
        assert!(offers(&start(SqlDialect::MsSql, "mer"), "MERGE"));
        assert!(!offers(&start(SqlDialect::Sqlite, "mer"), "MERGE"));
    }

    /// A keyword the typed word begins is ranked with the builtins rather than
    /// under them, so a builtin that only fuzzy-matches cannot bury it:
    /// PostgreSQL's `parti` matches enough `pg_*` names to fill the list, and
    /// `PARTITION` fell past the last row. A keyword it only fuzzy-matches stays
    /// below every builtin, and nothing typed promotes nothing.
    #[test]
    fn a_keyword_the_word_begins_outranks_a_builtin_it_only_fuzzy_matches() {
        let s = shop();
        let scope = [tref("orders", None)];
        let col = |p| ranked_on(&s, SqlDialect::Postgres, ClauseCtx::Column, &scope, p);
        let rows = col("parti");
        let at = |name: &str| rows.iter().position(|r| r.eq_ignore_ascii_case(name));
        let kw = at("PARTITION").unwrap_or_else(|| panic!("PARTITION offered: {rows:?}"));
        let fuzzy = rows
            .iter()
            .position(|r| !r.to_ascii_lowercase().starts_with("parti"))
            .expect("a fuzzy builtin match");
        assert!(kw < fuzzy, "{rows:?}");
        // With nothing typed there is no prefix to begin, and every keyword
        // still sits under the builtins — past the last row on PostgreSQL.
        let rows = col("");
        assert!(!offers(&rows, "PARTITION"), "{rows:?}");
    }

    /// Straight after `@` the document's variables, and after `#` its
    /// temporary tables — those alone, nothing from the schema.
    #[test]
    fn a_sigil_offers_the_documents_variables_or_temp_tables() {
        let s = shop();
        let local = |n: &str| crate::intel::LocalName {
            name: n.to_string(),
            written: n.to_string(),
        };
        let names = LocalNames {
            variables: vec![local("total"), local("tag")],
            temp_tables: vec![local("work")],
            ..LocalNames::default()
        };
        let rows = |ctx: ClauseCtx, prefix: &str| {
            let cont = Continuation::default();
            let used = HashSet::new();
            rank(
                &s,
                &RankInput {
                    ctx: &ctx,
                    cont: &cont,
                    scope: &[tref("orders", None)],
                    prefix,
                    snippets: &[],
                    join_targets: &[],
                    star: None,
                    used: &used,
                    active_db: Some("shop"),
                    dialect: SqlDialect::MsSql,
                    flavour: ServerFlavour::Unknown,
                    local: &names,
                    wide: false,
                    accepted: &Default::default(),
                },
            )
            .into_iter()
            .map(|r| r.text)
            .collect::<Vec<_>>()
        };
        assert_eq!(rows(ClauseCtx::Variable, "t"), ["tag", "total"]);
        assert_eq!(rows(ClauseCtx::TempTable, ""), ["work"]);
        assert!(rows(ClauseCtx::Variable, "to").iter().all(|r| r != "TOP"));
    }

    /// A view's row wears the view glyph, at a table slot and after its
    /// database's qualifier; a table's the table one.
    #[test]
    fn a_view_is_told_from_a_table() {
        let mut s = shop();
        s.tables
            .push(("open_orders".to_string(), "shop".to_string()));
        s.tables_by_db
            .get_mut("shop")
            .unwrap()
            .push("open_orders".to_string());
        s.views
            .insert(("shop".to_string(), "open_orders".to_string()));
        let glyph = |ctx: ClauseCtx, name: &str| {
            let cont = Continuation::default();
            let used = HashSet::new();
            rank(
                &s,
                &RankInput {
                    ctx: &ctx,
                    cont: &cont,
                    scope: &[],
                    prefix: "o",
                    snippets: &[],
                    join_targets: &[],
                    star: None,
                    used: &used,
                    active_db: Some("shop"),
                    dialect: SqlDialect::MySql,
                    flavour: ServerFlavour::Unknown,
                    local: &LocalNames::default(),
                    wide: false,
                    accepted: &Default::default(),
                },
            )
            .into_iter()
            .find(|r| r.text == name)
            .map(|r| r.glyph)
        };
        assert_eq!(
            glyph(ClauseCtx::Table, "open_orders"),
            Some(SuggestGlyph::View)
        );
        assert_eq!(glyph(ClauseCtx::Table, "orders"), Some(SuggestGlyph::Kind));
        let q = ClauseCtx::Qualified("shop".to_string());
        assert_eq!(glyph(q, "open_orders"), Some(SuggestGlyph::View));
    }

    /// A table no bare name reaches — PostgreSQL's `sales.ledger`, outside
    /// `search_path`'s `public` — is offered under its name and inserted
    /// qualified, where a table is offered: after `FROM`, and with no clause
    /// yet. One a bare name does reach still inserts bare.
    #[test]
    fn a_table_outside_the_default_namespace_inserts_qualified() {
        let mut s = shop();
        s.tables.push(("ledger".to_string(), "shop".to_string()));
        s.tables.push(("audit".to_string(), "shop".to_string()));
        let ns = |n: &[Option<&str>]| n.iter().map(|s| s.map(str::to_string)).collect();
        s.namespaces
            .insert("ledger".to_string(), ns(&[Some("sales")]));
        s.namespaces
            .insert("orders".to_string(), ns(&[Some("public")]));
        // One `audit` in `public`, another in `sales`: the bare name reaches one.
        s.namespaces
            .insert("audit".to_string(), ns(&[Some("sales"), Some("public")]));
        for ctx in [ClauseCtx::Table, ClauseCtx::Other] {
            let cont = Continuation::default();
            let used = HashSet::new();
            let rows = rank(
                &s,
                &RankInput {
                    ctx: &ctx,
                    cont: &cont,
                    scope: &[],
                    prefix: "",
                    snippets: &[],
                    join_targets: &[],
                    star: None,
                    used: &used,
                    active_db: Some("shop"),
                    dialect: SqlDialect::Postgres,
                    flavour: ServerFlavour::Unknown,
                    local: &LocalNames::default(),
                    wide: false,
                    accepted: &Default::default(),
                },
            );
            let row = |name: &str| {
                rows.iter()
                    .find(|r| r.text == name)
                    .unwrap_or_else(|| panic!("{name} offered in {ctx:?}: {rows:?}"))
                    .clone()
            };
            assert_eq!(
                row("ledger").insert.as_deref(),
                Some("sales.ledger"),
                "{ctx:?}"
            );
            assert_eq!(row("orders").insert, None, "{ctx:?}");
            assert_eq!(row("audit").insert, None, "{ctx:?}");
        }
    }

    /// **Inside a quoted name a table row is its name**: the user opened the
    /// quote, so `"led|` takes `ledger`, never `sales.ledger` spliced after
    /// the quote — which is one identifier with a dot in it.
    #[test]
    fn a_qualified_table_row_inserts_its_bare_name_inside_a_quote() {
        let row = Suggestion {
            text: "ledger".to_string(),
            kind: SuggestKind::Table,
            detail: String::new(),
            table: String::new(),
            alias: String::new(),
            glyph: SuggestGlyph::Kind,
            key: KeyKind::None,
            insert: Some("sales.ledger".to_string()),
            replace: None,
        };
        assert_eq!(
            accepted_insert(&row, SqlDialect::Postgres, false).as_deref(),
            Some("sales.ledger")
        );
        assert_eq!(accepted_insert(&row, SqlDialect::Postgres, true), None);
    }

    /// **`NEW.` in a trigger completes the trigger table's columns**, through
    /// the scope the editor builds and the alias it holds.
    #[test]
    fn a_triggers_new_completes_its_tables_columns() {
        let s = shop();
        let sql = "CREATE TRIGGER trg BEFORE UPDATE ON orders FOR EACH ROW SET NEW.";
        let scope =
            crate::intel::statement_scope(sql, 0, sql.len(), sql.len(), SqlDialect::MySql).tables;
        let want: Vec<String> = s.columns["orders"].iter().map(|c| c.name.clone()).collect();
        let cont = Continuation::default();
        let used = HashSet::new();
        let texts: Vec<String> = rank(
            &s,
            &RankInput {
                ctx: &ClauseCtx::Qualified("NEW".to_string()),
                cont: &cont,
                scope: &scope,
                prefix: "",
                snippets: &[],
                join_targets: &[],
                star: None,
                used: &used,
                active_db: Some("shop"),
                dialect: SqlDialect::MySql,
                flavour: ServerFlavour::Unknown,
                local: &LocalNames::default(),
                wide: false,
                accepted: &Default::default(),
            },
        )
        .into_iter()
        .map(|r| r.text)
        .collect();
        assert!(!want.is_empty());
        for c in &want {
            assert!(texts.contains(c), "{c} missing from {texts:?}");
        }
    }

    /// **Another database's `dbo.orders` completes its own columns**, read
    /// through the scope the editor builds. The scope kept the schema, `dbo`,
    /// and the schema map answered with the tab's own `dbo.orders`.
    #[test]
    fn another_databases_three_part_table_completes_its_own_columns() {
        let col = |n: &str| ColMeta {
            name: n.to_string(),
            type_name: "int".to_string(),
            nullable: true,
            primary_key: false,
            foreign_key: false,
        };
        let mut s = shop();
        s.columns_by_db.insert(
            ("archive".to_string(), "orders".to_string()),
            Rc::new(vec![col("archived_at")]),
        );
        s.columns_by_schema.insert(
            ("dbo".to_string(), "orders".to_string()),
            Rc::new(vec![col("placed_at")]),
        );
        // Mid-edit (the lexer's scope) and a statement that parses (the
        // parsed scope's `table_ref_of`), each with the caret after `o.`.
        for sql in [
            "SELECT o. FROM archive.dbo.orders o",
            "SELECT o.x FROM archive.dbo.orders o WHERE o.",
            "SELECT o.x FROM archive.dbo.orders o WHERE o.x = 1",
        ] {
            let caret = if sql.ends_with("o.") { sql.len() } else { 9 };
            let scope =
                crate::intel::statement_scope(sql, 0, sql.len(), caret, SqlDialect::MsSql).tables;
            let cont = Continuation::default();
            let used = HashSet::new();
            let texts: Vec<String> = rank(
                &s,
                &RankInput {
                    ctx: &ClauseCtx::Qualified("o".to_string()),
                    cont: &cont,
                    scope: &scope,
                    prefix: "",
                    snippets: &[],
                    join_targets: &[],
                    star: None,
                    used: &used,
                    active_db: Some("shop"),
                    dialect: SqlDialect::MsSql,
                    flavour: ServerFlavour::Unknown,
                    local: &LocalNames::default(),
                    wide: false,
                    accepted: &Default::default(),
                },
            )
            .into_iter()
            .map(|r| r.text)
            .collect();
            assert!(texts.iter().any(|t| t == "archived_at"), "{sql}: {texts:?}");
            assert!(!texts.iter().any(|t| t == "placed_at"), "{sql}: {texts:?}");
        }
    }

    /// **The widened list** names every database's and schema's tables at a
    /// table slot, each inserting the reference that reaches it from the tab's
    /// database — and nothing PostgreSQL could not reach. Without it, the
    /// list is the tab's own.
    #[test]
    fn the_widened_list_offers_every_table_fully_qualified() {
        let mut s = shop();
        let t = |db: &str, ns: Option<&str>, n: &str| IndexedTable {
            database: db.to_string(),
            schema: ns.map(str::to_string),
            name: n.to_string(),
        };
        s.all_tables = vec![
            t("shop", Some("dbo"), "orders"),
            t("archive", Some("old"), "orders"),
        ];
        let rows = |d, wide| {
            let cont = Continuation::default();
            let used = HashSet::new();
            rank(
                &s,
                &RankInput {
                    ctx: &ClauseCtx::Table,
                    cont: &cont,
                    scope: &[],
                    prefix: "ord",
                    snippets: &[],
                    join_targets: &[],
                    star: None,
                    used: &used,
                    active_db: Some("shop"),
                    dialect: d,
                    flavour: ServerFlavour::Unknown,
                    local: &LocalNames::default(),
                    wide,
                    accepted: &Default::default(),
                },
            )
        };
        let ms = rows(SqlDialect::MsSql, true);
        let other = ms
            .iter()
            .find(|r| r.text == "archive.old.orders")
            .expect("{ms:?}");
        assert_eq!(other.insert.as_deref(), Some("archive.old.orders"));
        assert!(ms.iter().any(|r| r.insert.as_deref() == Some("dbo.orders")));
        assert!(ms.iter().any(|r| r.text == "orders"), "the bare row stays");
        let pg = rows(SqlDialect::Postgres, true);
        assert!(pg.iter().all(|r| !r.text.starts_with("archive")), "{pg:?}");
        assert!(
            rows(SqlDialect::MsSql, false)
                .iter()
                .all(|r| !r.text.contains('.'))
        );
    }

    /// **A schema-qualified table has columns.** On PostgreSQL and SQL Server
    /// `sales.orders` reads as `TableRef.db = sales`, and `sales` is a
    /// namespace in the tab's database, not a database: the lookup keyed by
    /// database found nothing, so `s.` offered no columns. The schema's own
    /// map answers it, and `sales.` offers the schema's tables.
    #[test]
    fn a_schema_qualified_table_offers_its_columns() {
        let meta = |n: &str| ColMeta {
            name: n.to_string(),
            type_name: "int".to_string(),
            nullable: false,
            primary_key: false,
            foreign_key: false,
        };
        let mut s = shop();
        s.columns_by_schema.insert(
            ("sales".to_string(), "orders".to_string()),
            Rc::new(vec![meta("region"), meta("amount")]),
        );
        s.tables_by_schema.insert(
            "sales".to_string(),
            vec!["orders".to_string(), "Leads".to_string()],
        );
        let scoped = TableRef {
            name: "orders".to_string(),
            alias: Some("s".to_string()),
            db: Some("sales".to_string()),
        };
        let pg = SqlDialect::Postgres;
        let col = ranked_on(
            &s,
            pg,
            ClauseCtx::Column,
            std::slice::from_ref(&scoped),
            "reg",
        );
        assert!(offers(&col, "region"), "{col:?}");
        let q = ranked_on(&s, pg, ClauseCtx::Qualified("s".into()), &[scoped], "");
        assert!(offers(&q, "amount") && !offers(&q, "total"), "{q:?}");
        let ns = ranked_on(&s, pg, ClauseCtx::Qualified("sales".into()), &[], "");
        assert!(offers(&ns, "Leads"), "{ns:?}");
        // A database of the qualifier's name still answers first.
        let db = ranked_on(&s, pg, ClauseCtx::Qualified("shop".into()), &[], "");
        assert!(offers(&db, "customers"), "{db:?}");
    }

    /// A value slot offers functions first and no columns — not even the
    /// tables in scope, which a `VALUES` tuple or a variable's value cannot
    /// read. A builtin called bare still goes in bare.
    #[test]
    fn a_value_slot_offers_functions_and_no_columns() {
        let s = shop();
        let scope = [tref("orders", None)];
        let v = |d, p| ranked_on(&s, d, ClauseCtx::Value, &scope, p);
        let rows = v(SqlDialect::MySql, "no");
        assert_eq!(rows.first().map(String::as_str), Some("NOW"), "{rows:?}");
        let rows = v(SqlDialect::MySql, "to");
        assert!(!offers(&rows, "total"), "no columns: {rows:?}");
        assert!(!offers(&v(SqlDialect::MySql, ""), "id"));
        assert!(offers(&v(SqlDialect::MsSql, "getd"), "GETDATE"));
        assert!(
            offers(&v(SqlDialect::Postgres, "nul"), "NULL"),
            "keywords too"
        );
    }

    /// The statement's own names: its CTEs where a table goes, its select
    /// list's aliases where the reader says a column may name them — each
    /// inserted as the statement spelled it.
    #[test]
    fn the_statements_own_ctes_and_aliases_are_offered() {
        let s = shop();
        let local = |name: &str, written: &str| crate::intel::LocalName {
            name: name.to_string(),
            written: written.to_string(),
        };
        let names = LocalNames {
            ctes: vec![local("recent", "recent"), local("Big", "\"Big\"")],
            output_aliases: vec![local("Total", "Total")],
            ..LocalNames::default()
        };
        let rows = |ctx: ClauseCtx, scope: &[TableRef], prefix: &str| {
            let cont = Continuation::default();
            let used = HashSet::new();
            rank(
                &s,
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
                    dialect: SqlDialect::Postgres,
                    flavour: ServerFlavour::Unknown,
                    local: &names,
                    wide: false,
                    accepted: &Default::default(),
                },
            )
        };
        let tables = rows(ClauseCtx::Table, &[], "");
        let cte = tables
            .iter()
            .find(|r| r.text == "Big")
            .expect("CTE offered");
        assert_eq!(cte.detail, "CTE");
        assert_eq!(cte.insert.as_deref(), Some("\"Big\""));
        assert!(tables.iter().any(|r| r.text == "recent"));
        assert!(
            tables.iter().any(|r| r.text == "orders"),
            "the schema's too"
        );
        let scope = [tref("orders", None)];
        let cols = rows(ClauseCtx::Column, &scope, "tot");
        let alias = cols
            .iter()
            .find(|r| r.text == "Total")
            .expect("alias offered");
        assert_eq!(alias.glyph, SuggestGlyph::Alias);
        // As written: unquoted on PostgreSQL it is `total`, so no quotes go in.
        assert_eq!(alias.insert.as_deref(), Some("Total"));
        assert_eq!(
            accepted_insert(alias, SqlDialect::Postgres, false).as_deref(),
            Some("Total")
        );
        // A CTE is no column, and an alias no table.
        assert!(cols.iter().all(|r| r.text != "recent"));
        assert!(
            rows(ClauseCtx::Table, &[], "tot")
                .iter()
                .all(|r| r.text != "Total")
        );
    }

    /// A cast's target offers the engine's cast types alone — no columns, no
    /// functions — in the case they are typed, as a keyword is.
    #[test]
    fn a_cast_target_offers_the_engines_types() {
        let s = shop();
        let scope = [tref("orders", None)];
        let ty = |d, f, p| ranked_on_flavour(&s, d, f, ClauseCtx::Type, &scope, p);
        let my = ty(SqlDialect::MySql, ServerFlavour::MySql, "si");
        assert_eq!(my.first().map(String::as_str), Some("signed"), "{my:?}");
        assert!(my.iter().all(|r| r != "id"), "no columns: {my:?}");
        assert!(!offers(
            &ty(SqlDialect::MySql, ServerFlavour::MySql, "in"),
            "INT"
        ));
        assert!(offers(
            &ty(SqlDialect::MySql, ServerFlavour::MariaDb, "in"),
            "INT"
        ));
        let pg = ty(SqlDialect::Postgres, ServerFlavour::Unknown, "INT");
        assert_eq!(pg.first().map(String::as_str), Some("INTEGER"), "{pg:?}");
        // With nothing typed, every type and nothing else.
        let all = ty(SqlDialect::MsSql, ServerFlavour::Unknown, "");
        assert!(
            offers(&all, "nvarchar") && !offers(&all, "total"),
            "{all:?}"
        );
    }

    /// A column definition's type offers the engine's column types — MySQL's
    /// `INT`, which its `CAST` refuses — and nothing else.
    #[test]
    fn a_column_definition_offers_column_types() {
        let s = shop();
        let ty = |d, p| ranked_on(&s, d, ClauseCtx::ColumnType, &[], p);
        let my = ty(SqlDialect::MySql, "in");
        assert_eq!(my.first().map(String::as_str), Some("int"), "{my:?}");
        assert!(!offers(&ty(SqlDialect::MySql, "to"), "total"), "no columns");
        assert!(offers(&ty(SqlDialect::MsSql, "nvar"), "NVARCHAR"));
    }

    /// A name that bare would be something else — a reserved word, a space,
    /// a capital letter on PostgreSQL — inserts quoted, through the one
    /// quoter, while the row still shows and matches the bare name. One that
    /// needs nothing inserts as it reads, and so does any name typed inside a
    /// quote already.
    #[test]
    fn a_name_that_needs_quoting_inserts_quoted() {
        let col = |n: &str| ColMeta {
            name: n.to_string(),
            type_name: "int".to_string(),
            nullable: false,
            primary_key: false,
            foreign_key: false,
        };
        let cols = vec![col("Unit Price"), col("OrderDate"), col("qty")];
        let mut columns = HashMap::new();
        columns.insert("order".to_string(), cols.clone());
        let mut columns_by_db = HashMap::new();
        columns_by_db.insert(("shop".to_string(), "order".to_string()), Rc::new(cols));
        let s = SchemaIndex {
            databases: vec!["shop".to_string()],
            tables: vec![("order".to_string(), "shop".to_string())],
            columns,
            columns_by_db,
            tables_by_db: HashMap::new(),
            ..SchemaIndex::default()
        };
        let row = |dialect, ctx: ClauseCtx, scope: &[TableRef], prefix: &str, name: &str| {
            let cont = Continuation::default();
            let used = HashSet::new();
            rank(
                &s,
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
                    dialect,
                    flavour: ServerFlavour::Unknown,
                    local: &LocalNames::default(),
                    wide: false,
                    accepted: &Default::default(),
                },
            )
            .into_iter()
            .find(|r| r.text == name)
            .unwrap_or_else(|| panic!("{name} offered"))
        };
        let (my, pg) = (SqlDialect::MySql, SqlDialect::Postgres);
        let ins = |d, s: &Suggestion| accepted_insert(s, d, false);
        let table = row(my, ClauseCtx::Table, &[], "ord", "order");
        assert_eq!(ins(my, &table).as_deref(), Some("`order`"));
        // Inside `` `ord `` the quote is the user's.
        assert_eq!(accepted_insert(&table, my, true), None);
        let scope = [tref("order", None)];
        let spaced = row(my, ClauseCtx::Column, &scope, "unit", "Unit Price");
        assert_eq!(ins(my, &spaced).as_deref(), Some("`Unit Price`"));
        let cased = |d| ins(d, &row(d, ClauseCtx::Column, &scope, "orderd", "OrderDate"));
        assert_eq!(cased(my), None);
        assert_eq!(cased(pg).as_deref(), Some("\"OrderDate\""));
        assert_eq!(
            ins(pg, &row(pg, ClauseCtx::Column, &scope, "q", "qty")),
            None
        );
        // A keyword or a function is never quoted; a row's own insert wins.
        let kw = row(my, ClauseCtx::Start, &[], "sel", "select");
        assert_eq!(ins(my, &kw), None);
        let own = Suggestion {
            insert: Some("orders ON o.id = orders.id".to_string()),
            ..table.clone()
        };
        assert_eq!(ins(my, &own), own.insert);
    }

    /// **A name the statement spelled bare is not re-quoted** on PostgreSQL,
    /// which folded it: a CTE's `Total` is `total` there, and `"Total"` names
    /// no column. Nor is a bare alias (`O` → `"O".id`) or table name the user
    /// cased (`Orders`, written as the catalogue's `orders`). A catalogue
    /// column keeps its quoting: `"OrderDate"` is its real name.
    #[test]
    fn a_name_the_statement_wrote_bare_is_not_re_quoted() {
        let pg = SqlDialect::Postgres;
        let meta = |n: &str| ColMeta {
            name: n.to_string(),
            type_name: String::new(),
            nullable: true,
            primary_key: false,
            foreign_key: false,
        };
        let local = LocalNames {
            relations: vec![(
                "t".to_string(),
                vec![meta("Total"), meta("Unit Price"), meta("id")],
            )],
            ..LocalNames::default()
        };
        let cont = Continuation::default();
        let used = HashSet::new();
        let s = shop();
        let rows = |ctx: ClauseCtx, scope: &[TableRef], prefix: &str| -> Vec<Suggestion> {
            rank(
                &s,
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
                    dialect: pg,
                    flavour: ServerFlavour::Unknown,
                    local: &local,
                    wide: false,
                    accepted: &Default::default(),
                },
            )
        };
        let accepted = |rows: &[Suggestion], name: &str| {
            let row = rows
                .iter()
                .find(|r| r.text == name)
                .unwrap_or_else(|| panic!("{name} offered: {rows:?}"));
            accepted_insert(row, pg, false).unwrap_or_else(|| row.text.clone())
        };
        let t = [tref("t", None)];
        let col = rows(ClauseCtx::Column, &t, "");
        assert_eq!(accepted(&col, "Total"), "Total");
        assert_eq!(accepted(&col, "Unit Price"), "\"Unit Price\"");
        let q = rows(ClauseCtx::Qualified("t".into()), &t, "");
        assert_eq!(accepted(&q, "Total"), "Total");
        // A shared column's qualifier, and an alias row, as the user wrote it.
        let scope = [tref("orders", Some("O")), tref("customers", Some("C"))];
        let col = rows(ClauseCtx::Column, &scope, "");
        assert_eq!(accepted(&col, "id"), "C.id");
        assert_eq!(accepted(&col, "O"), "O");
        let scope = [tref("Orders", None), tref("customers", None)];
        let col = rows(ClauseCtx::Column, &scope, "");
        assert_eq!(accepted(&col, "Orders"), "orders");
        let scope = [tref("customers", None), tref("Orders", None)];
        let col = rows(ClauseCtx::Column, &scope, "");
        assert_eq!(accepted(&col, "id"), "orders.id");
    }

    /// **The FK `ON` predicate goes in as written**, on every engine: it is a
    /// predicate, and quoting it whole as one name broke the join everywhere —
    /// silently on SQLite, where `"o.customer_id = c.id"` is a string.
    #[test]
    fn the_join_condition_row_inserts_its_predicate_unquoted() {
        let pred = "o.customer_id = c.id";
        let row = join_condition_row(pred.to_string());
        for d in SqlDialect::ALL {
            assert_eq!(
                accepted_insert(&row, d, false).as_deref(),
                Some(pred),
                "{d:?}"
            );
        }
    }

    /// The engine's own keywords complete a row; they do not make a word an
    /// unaliasable keyword — `FROM t top` is still `t` aliased `top`.
    #[test]
    fn an_engine_keyword_is_still_an_alias() {
        assert!(!crate::intel::is_sql_keyword("top"));
        assert!(!crate::intel::is_sql_keyword("over"));
    }

    /// Does the ranked list offer this name, however either side spells its case?
    fn offers(rows: &[String], name: &str) -> bool {
        rows.iter().any(|r| r.eq_ignore_ascii_case(name))
    }

    /// Does the word typed already spell a candidate in full?
    fn completes(schema: &SchemaIndex, ctx: ClauseCtx, scope: &[TableRef], prefix: &str) -> bool {
        let cont = Continuation::default();
        let used = HashSet::new();
        rank_full(
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
                dialect: SqlDialect::MySql,
                flavour: ServerFlavour::Unknown,
                local: &LocalNames::default(),
                wide: false,
                accepted: &Default::default(),
            },
        )
        .completes_a_word
    }

    /// **A word typed in full is reported, though it is not listed.** The
    /// exact match is left out of the rows, so after `FROM` the list held
    /// `FROM_BASE64()` and `FROM_DAYS()` — and Enter, meant as a newline,
    /// accepted the first of them. The editor asks this to let Enter through.
    #[test]
    fn a_word_typed_in_full_is_reported_though_not_listed() {
        let s = shop();
        let scope = [tref("orders", None)];
        assert!(completes(&s, ClauseCtx::Column, &scope, "from"));
        assert!(completes(&s, ClauseCtx::Column, &scope, "FROM"));
        assert!(!offers(
            &ranked(&s, ClauseCtx::Column, &scope, "from"),
            "from"
        ));
        assert!(completes(&s, ClauseCtx::Table, &[], "orders"));
        assert!(completes(&s, ClauseCtx::Column, &scope, "total"));
        // A part of a word, or nothing typed, completes nothing.
        assert!(!completes(&s, ClauseCtx::Column, &scope, "fro"));
        assert!(!completes(&s, ClauseCtx::Table, &[], "ord"));
        assert!(!completes(&s, ClauseCtx::Column, &scope, ""));
    }

    /// The builtins offered are the engine's own, not whichever catalog is
    /// nearest.
    ///
    /// This is the typo checker's original bug in the other surface: `rank`
    /// walked `FUNCTIONS` whatever the dialect, so a PostgreSQL tab was offered
    /// `IFNULL` — a name that engine does not have, and the completion popup
    /// saying so is worse than it saying nothing — while `btrim`, which it does
    /// have, was not offered at all. `intel::builtin_catalog` is the same answer
    /// to both questions, which is why this asks it rather than a second map.
    ///
    /// Asserted through `rank` rather than over the catalogs, because the
    /// catalogs were already right and the composition was what was broken.
    #[test]
    fn each_dialect_is_offered_its_own_builtins() {
        let s = shop();
        let ctx = ClauseCtx::Column;
        let on = |d, p| ranked_on(&s, d, ctx.clone(), &[], p);

        // MySQL's, and PostgreSQL has no such function.
        assert!(offers(&on(SqlDialect::MySql, "ifnu"), "IFNULL"));
        assert!(offers(&on(SqlDialect::Sqlite, "ifnu"), "ifnull"));
        assert!(!offers(&on(SqlDialect::Postgres, "ifnu"), "ifnull"));

        // PostgreSQL's, and neither of the others has it.
        assert!(offers(&on(SqlDialect::Postgres, "btri"), "btrim"));
        assert!(!offers(&on(SqlDialect::MySql, "btri"), "btrim"));
        assert!(!offers(&on(SqlDialect::Sqlite, "btri"), "btrim"));

        // SQLite's, and MySQL's list is not it either — the third engine was
        // being offered the first's just as silently.
        assert!(offers(&on(SqlDialect::Sqlite, "zerob"), "zeroblob"));
        assert!(!offers(&on(SqlDialect::MySql, "zerob"), "zeroblob"));
        assert!(offers(&on(SqlDialect::MySql, "uuid_s"), "UUID_SHORT"));
        assert!(!offers(&on(SqlDialect::Sqlite, "uuid_s"), "uuid_short"));
    }

    /// PostgreSQL's catalog reaches the popup cut down, and the checker's
    /// unchanged.
    ///
    /// `PG_FUNCTIONS` is read out of `pg_catalog` and so carries the engine's
    /// own plumbing — `int4in`, `btint4cmp`, `texteq`. Those are real functions
    /// and the checker must go on knowing them, but a completion list's whole
    /// value is being short, and `int` is a prefix somebody types.
    ///
    /// The half that matters most is the second: the 24 call forms with no
    /// `pg_proc` row cannot come out of a query over `pg_proc`, so a subset
    /// derived only from the server would have dropped `coalesce` and `cast` —
    /// the most typed names in the list — while looking like a tidy-up.
    #[test]
    fn postgres_offers_the_subset_and_the_checker_still_sees_the_rest() {
        let s = shop();
        let ctx = ClauseCtx::Column;
        let pg = |p| ranked_on(&s, SqlDialect::Postgres, ctx.clone(), &[], p);

        // Plumbing: in the catalog the checker trusts, out of the popup.
        for internal in ["int4in", "btint4cmp", "texteq"] {
            assert!(
                crate::pg_builtins::PG_FUNCTIONS
                    .iter()
                    .any(|f| f.name == internal),
                "{internal} must stay in the catalog — the checker reads it"
            );
            assert!(
                !offers(&pg(&internal[..4]), internal),
                "{internal} is plumbing and should not be offered"
            );
        }

        // Ordinary functions, still offered.
        assert!(offers(&pg("btri"), "btrim"));
        assert!(offers(&pg("jsonb_b"), "jsonb_build_object"));
        assert!(offers(&pg("date_t"), "date_trunc"));

        // The grammar's own call forms, which no `pg_proc` query can return.
        assert!(offers(&pg("coale"), "coalesce"));
        assert!(offers(&pg("grea"), "greatest"));

        // The two hand-written catalogs are already the user-facing list, so
        // nothing is cut from them.
        assert!(offers(
            &ranked_on(&s, SqlDialect::MySql, ctx.clone(), &[], "ifnu"),
            "IFNULL"
        ));
        assert!(offers(
            &ranked_on(&s, SqlDialect::Sqlite, ctx.clone(), &[], "zerob"),
            "zeroblob"
        ));
    }

    /// One table whose columns are the three shapes a prefix can match —
    /// exactly, at a word boundary, and buried inside a longer name.
    fn similar() -> SchemaIndex {
        let col = |n: &str| ColMeta {
            name: n.to_string(),
            type_name: "int".to_string(),
            nullable: false,
            primary_key: false,
            foreign_key: false,
        };
        let cols = Rc::new(vec![
            col("account_customs"),
            col("customer_id"),
            col("custom"),
            col("total"),
        ]);
        let mut columns = HashMap::new();
        columns.insert("ledger".to_string(), (*cols).clone());
        let mut columns_by_db = HashMap::new();
        columns_by_db.insert(("shop".to_string(), "ledger".to_string()), cols);
        let mut tables_by_db = HashMap::new();
        tables_by_db.insert("shop".to_string(), vec!["ledger".to_string()]);
        SchemaIndex {
            databases: vec!["shop".to_string()],
            tables: vec![("ledger".to_string(), "shop".to_string())],
            columns,
            columns_by_db,
            tables_by_db,
            ..SchemaIndex::default()
        }
    }

    /// **What Enter splices, through the comparator that decides it.**
    ///
    /// `recompute_completions` sets `sel = 0` and `accept_completion` splices
    /// `items[sel]`, so `rank`'s sort *is* the decision. The test that claimed
    /// to cover it built its rows from `fuzzy_score` and then sorted them with a
    /// **copy** of the comparator written inline — a copy that omitted the tier
    /// and the `recency_bonus` term. Flip this file's `b.1.cmp(&a.1)` to score
    /// *ascending*, so the popup preselects the worst match, and that replica
    /// stayed green: it sorted its own vector. This one calls `rank`.
    #[test]
    fn the_comparator_preselects_the_prefix_match_and_buries_the_interior_one() {
        let s = similar();
        let out = ranked(&s, ClauseCtx::Column, &[tref("ledger", None)], "cus");
        assert_eq!(out.first().map(String::as_str), Some("custom"), "{out:?}");
        // A non-match never reaches the list at all.
        assert!(!out.iter().any(|t| t == "total"), "{out:?}");
        // And the interior match sorts last of the three that do — asserted
        // against its siblings rather than against the whole list, which also
        // holds keywords and functions.
        let pos = |t: &str| out.iter().position(|x| x == t).unwrap_or(usize::MAX);
        assert!(pos("custom") < pos("customer_id"), "{out:?}");
        assert!(pos("customer_id") < pos("account_customs"), "{out:?}");
    }

    /// **And the recency term is part of that sort, not a decoration on it.**
    /// The replica dropped it, so a change to `recency_bonus`' effect on order
    /// was invisible. A name the user has already typed in this statement is
    /// lifted past a better-scoring one.
    #[test]
    fn a_recently_used_name_is_lifted_past_a_better_scoring_one() {
        let s = similar();
        let cont = Continuation::default();
        let plain = ranked(&s, ClauseCtx::Column, &[tref("ledger", None)], "cus");
        assert_eq!(plain.first().map(String::as_str), Some("custom"));

        let used: HashSet<String> = ["customer_id".to_string()].into_iter().collect();
        let out: Vec<String> = rank(
            &s,
            &RankInput {
                ctx: &ClauseCtx::Column,
                cont: &cont,
                scope: &[tref("ledger", None)],
                prefix: "cus",
                snippets: &[],
                join_targets: &[],
                star: None,
                used: &used,
                active_db: Some("shop"),
                dialect: SqlDialect::MySql,
                flavour: ServerFlavour::Unknown,
                local: &LocalNames::default(),
                wide: false,
                accepted: &Default::default(),
            },
        )
        .into_iter()
        .map(|s| s.text)
        .collect();
        assert_eq!(
            out.first().map(String::as_str),
            Some("customer_id"),
            "the recency term did not move the row: {out:?}"
        );
    }

    /// **The star-expansion arm, end to end.** Nothing in the workspace called
    /// `rank` with `star: Some(_)` — all six construction sites passed `None` —
    /// so the row that replaces the `*` itself was asserted nowhere: not its
    /// wording, not its `replace` range, not its `insert`. `fda8449` is named
    /// after making it say "1 column", and tested the *count* one crate over;
    /// the half that turns the count into words had no test, so restoring a
    /// hard-coded `"{ncols} columns"` passed the whole suite.
    #[test]
    fn the_star_expansion_row_says_its_column_count_and_replaces_the_star() {
        let s = similar();
        let cont = Continuation::default();
        let used = HashSet::new();
        let row = |exp: &crate::intel::StarExpansion| {
            rank(
                &s,
                &RankInput {
                    ctx: &ClauseCtx::Column,
                    cont: &cont,
                    scope: &[tref("ledger", None)],
                    prefix: "",
                    snippets: &[],
                    join_targets: &[],
                    star: Some(exp),
                    used: &used,
                    active_db: Some("shop"),
                    dialect: SqlDialect::MySql,
                    flavour: ServerFlavour::Unknown,
                    local: &LocalNames::default(),
                    wide: false,
                    accepted: &Default::default(),
                },
            )
            .into_iter()
            .find(|r| r.text == "expand *")
            .expect("the star row")
        };
        let one = row(&crate::intel::StarExpansion {
            range: (7, 8),
            replacement: "id".to_string(),
            columns: 1,
        });
        assert_eq!(
            one.detail, "1 column",
            "the singular is the whole of fda8449"
        );
        assert_eq!(one.insert.as_deref(), Some("id"));
        // The one suggestion that replaces a range other than the word at the
        // caret. Without it, accepting the row inserts the column list over the
        // empty word *after* the star: `SELECT *id, name`.
        assert_eq!(one.replace, Some((7, 8)));

        let three = row(&crate::intel::StarExpansion {
            range: (7, 8),
            replacement: "id, name, email".to_string(),
            columns: 3,
        });
        assert_eq!(three.detail, "3 columns");
        assert_eq!(three.insert.as_deref(), Some("id, name, email"));
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
                    dialect: SqlDialect::MySql,
                    flavour: ServerFlavour::Unknown,
                    local: &LocalNames::default(),
                    wide: false,
                    accepted: &Default::default(),
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
                dialect: SqlDialect::MySql,
                flavour: ServerFlavour::Unknown,
                local: &LocalNames::default(),
                wide: false,
                accepted: &Default::default(),
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
                    dialect: SqlDialect::MySql,
                    flavour: ServerFlavour::Unknown,
                    local: &LocalNames::default(),
                    wide: false,
                    accepted: &Default::default(),
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
                dialect: SqlDialect::MySql,
                flavour: ServerFlavour::Unknown,
                local: &LocalNames::default(),
                wide: false,
                accepted: &Default::default(),
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
            aliased: None,
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
                dialect: SqlDialect::MySql,
                flavour: ServerFlavour::Unknown,
                local: &LocalNames::default(),
                wide: false,
                accepted: &Default::default(),
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

    /// `(text, insert)` of each column row ranked at a column slot over `scope`.
    fn column_inserts(
        scope: &[TableRef],
        prefix: &str,
        bare: bool,
    ) -> Vec<(String, Option<String>)> {
        let s = shop();
        let cont = Continuation::default();
        let used = HashSet::new();
        let local = LocalNames {
            bare_columns: bare,
            ..LocalNames::default()
        };
        rank(
            &s,
            &RankInput {
                ctx: &ClauseCtx::Column,
                cont: &cont,
                scope,
                prefix,
                snippets: &[],
                join_targets: &[],
                star: None,
                used: &used,
                active_db: Some("shop"),
                dialect: SqlDialect::MySql,
                flavour: ServerFlavour::Unknown,
                local: &local,
                wide: false,
                accepted: &Default::default(),
            },
        )
        .into_iter()
        .filter(|r| r.kind == SuggestKind::Column)
        .map(|r| (r.text, r.insert))
        .collect()
    }

    #[test]
    fn the_databases_routines_are_offered_where_each_can_be_called() {
        use crate::schema::RoutineKind;
        let routine = |name: &str, kind, args: &str, returns: &str| IndexedRoutine {
            name: name.to_string(),
            schema: None,
            kind,
            arguments: args.to_string(),
            returns: returns.to_string(),
        };
        let s = SchemaIndex {
            routines: vec![
                routine("refresh_totals", RoutineKind::Procedure, "IN d DATE", ""),
                routine(
                    "order_total",
                    RoutineKind::Function,
                    "o INT",
                    "decimal(10,2)",
                ),
                routine("audit_row", RoutineKind::Function, "", "trigger"),
            ],
            ..shop()
        };
        let cont = Continuation::default();
        let used = HashSet::new();
        let local = LocalNames::default();
        let rows = |ctx: ClauseCtx, dialect, prefix: &str| -> Vec<Suggestion> {
            rank(
                &s,
                &RankInput {
                    ctx: &ctx,
                    cont: &cont,
                    scope: &[],
                    prefix,
                    snippets: &[],
                    join_targets: &[],
                    star: None,
                    used: &used,
                    active_db: Some("shop"),
                    dialect,
                    flavour: ServerFlavour::Unknown,
                    local: &local,
                    wide: false,
                    accepted: &Default::default(),
                },
            )
        };
        // After `CALL`: the procedures, and nothing a call cannot run.
        let call = rows(ClauseCtx::Routine, SqlDialect::MySql, "");
        let names: Vec<&str> = call.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(names, ["refresh_totals"]);
        assert_eq!(call[0].kind, SuggestKind::Function);
        assert_eq!(call[0].detail, "(IN d DATE)");
        assert_eq!(call[0].insert, None, "the parens come with a function row");
        // T-SQL's `EXEC` passes its arguments without parens.
        let exec = rows(ClauseCtx::Routine, SqlDialect::MsSql, "");
        assert_eq!(exec[0].insert.as_deref(), Some("refresh_totals"));
        // Where an expression goes: the functions, never a trigger's.
        let col = rows(ClauseCtx::Column, SqlDialect::Postgres, "order_t");
        let f = col
            .iter()
            .find(|r| r.text == "order_total")
            .expect("offered");
        assert_eq!(f.kind, SuggestKind::Function);
        assert_eq!(f.detail, "(o INT) → decimal(10,2)");
        for other in ["refresh", "audit"] {
            let col = rows(ClauseCtx::Column, SqlDialect::Postgres, other);
            assert!(
                !col.iter()
                    .any(|r| r.text == "refresh_totals" || r.text == "audit_row"),
                "{other}"
            );
        }
    }

    /// **A routine goes in by the name that reaches it**: qualified outside
    /// the default schema, and on SQL Server every function, which T-SQL
    /// calls only by its two-part name (Msg 195 otherwise); quoted where its
    /// name needs it (PostgreSQL's `"calcTotal"`). The parens still come with
    /// a call, and not with a T-SQL `EXEC`.
    #[test]
    fn a_routine_inserts_the_name_that_reaches_it() {
        use crate::schema::RoutineKind;
        let routine = |schema: Option<&str>, name: &str, kind| IndexedRoutine {
            name: name.to_string(),
            schema: schema.map(str::to_string),
            kind,
            arguments: String::new(),
            returns: if kind == RoutineKind::Function {
                "int".to_string()
            } else {
                String::new()
            },
        };
        let (f, p) = (RoutineKind::Function, RoutineKind::Procedure);
        let s = SchemaIndex {
            routines: vec![
                routine(Some("dbo"), "fn_order_total", f),
                routine(Some("reporting"), "monthly_total", f),
                routine(Some("public"), "calcTotal", f),
                routine(Some("public"), "order_count", f),
                routine(Some("sales"), "refresh_totals", p),
                routine(Some("dbo"), "purge_logs", p),
                routine(None, "my_fn", f),
            ],
            ..shop()
        };
        let cont = Continuation::default();
        let used = HashSet::new();
        let local = LocalNames::default();
        let insert = |ctx: ClauseCtx, dialect, name: &str| -> Option<String> {
            rank(
                &s,
                &RankInput {
                    ctx: &ctx,
                    cont: &cont,
                    scope: &[],
                    // Short of the whole name, which is not offered back.
                    prefix: &name[..name.len() - 1],
                    snippets: &[],
                    join_targets: &[],
                    star: None,
                    used: &used,
                    active_db: Some("shop"),
                    dialect,
                    flavour: ServerFlavour::Unknown,
                    local: &local,
                    wide: false,
                    accepted: &Default::default(),
                },
            )
            .into_iter()
            .find(|r| r.text == name)
            .unwrap_or_else(|| panic!("{name} offered"))
            .insert
        };
        let (ms, pg, my) = (SqlDialect::MsSql, SqlDialect::Postgres, SqlDialect::MySql);
        let value = ClauseCtx::Value;
        assert_eq!(
            insert(value.clone(), ms, "fn_order_total").as_deref(),
            Some("dbo.fn_order_total()")
        );
        assert_eq!(
            insert(value.clone(), pg, "monthly_total").as_deref(),
            Some("reporting.monthly_total()")
        );
        assert_eq!(
            insert(value.clone(), pg, "calcTotal").as_deref(),
            Some("\"calcTotal\"()")
        );
        // Reached bare and spelled plain: the row's own name, parens added on accept.
        assert_eq!(insert(value.clone(), pg, "order_count"), None);
        assert_eq!(insert(value, my, "my_fn"), None);
        // A procedure: after `CALL` with its parens, after `EXEC` without.
        let call = ClauseCtx::Routine;
        assert_eq!(
            insert(call.clone(), pg, "refresh_totals").as_deref(),
            Some("sales.refresh_totals()")
        );
        assert_eq!(
            insert(call.clone(), ms, "refresh_totals").as_deref(),
            Some("sales.refresh_totals")
        );
        assert_eq!(
            insert(call, ms, "purge_logs").as_deref(),
            Some("purge_logs")
        );
    }

    /// An override ending in a call's `()` puts the caret between them, and
    /// drops them where the call's parens already follow.
    #[test]
    fn a_routine_override_places_the_caret_in_its_parens() {
        let f = SuggestKind::Function;
        assert_eq!(
            override_splice("dbo.f()".to_string(), f, false),
            ("dbo.f()".to_string(), 6)
        );
        assert_eq!(
            override_splice("dbo.f()".to_string(), f, true),
            ("dbo.f".to_string(), 5)
        );
        // A builtin called bare, or any other row, goes in whole.
        assert_eq!(
            override_splice("CURRENT_DATE".to_string(), f, false),
            ("CURRENT_DATE".to_string(), 12)
        );
        assert_eq!(
            override_splice("a()".to_string(), SuggestKind::Snippet, true),
            ("a()".to_string(), 3)
        );
    }

    #[test]
    fn a_ctes_and_a_derived_tables_columns_are_offered() {
        let s = shop();
        let meta = |n: &str| ColMeta {
            name: n.to_string(),
            type_name: String::new(),
            nullable: true,
            primary_key: false,
            foreign_key: false,
        };
        let local = LocalNames {
            relations: vec![
                ("recent".to_string(), vec![meta("id"), meta("pay")]),
                // A CTE shadows the table it is named after.
                ("orders".to_string(), vec![meta("only_this")]),
            ],
            ..LocalNames::default()
        };
        let cont = Continuation::default();
        let used = HashSet::new();
        let texts = |ctx: ClauseCtx, scope: &[TableRef]| -> Vec<String> {
            rank(
                &s,
                &RankInput {
                    ctx: &ctx,
                    cont: &cont,
                    scope,
                    prefix: "",
                    snippets: &[],
                    join_targets: &[],
                    star: None,
                    used: &used,
                    active_db: Some("shop"),
                    dialect: SqlDialect::MySql,
                    flavour: ServerFlavour::Unknown,
                    local: &local,
                    wide: false,
                    accepted: &Default::default(),
                },
            )
            .into_iter()
            .filter(|r| r.kind == SuggestKind::Column)
            .map(|r| r.text)
            .collect()
        };
        let recent = [tref("recent", Some("r"))];
        assert_eq!(
            texts(ClauseCtx::Qualified("r".into()), &recent),
            ["id", "pay"]
        );
        let bare = texts(ClauseCtx::Column, &recent);
        assert!(bare.contains(&"pay".to_string()), "{bare:?}");
        assert_eq!(
            texts(
                ClauseCtx::Qualified("orders".into()),
                &[tref("orders", None)]
            ),
            ["only_this"]
        );
    }

    #[test]
    fn a_column_two_tables_share_is_inserted_qualified() {
        let scope = [tref("orders", Some("o")), tref("customers", Some("c"))];
        let rows = column_inserts(&scope, "", false);
        let insert = |n: &str| rows.iter().find(|r| r.0 == n).map(|r| r.1.clone());
        // `id` is in both: the row is the last table's, and says which.
        assert_eq!(insert("id"), Some(Some("c.id".to_string())));
        // A name only one table has stays bare.
        assert_eq!(insert("total"), Some(None));
        assert_eq!(insert("email"), Some(None));
        // A table with no alias is qualified by its own name.
        let rows = column_inserts(&[tref("orders", None), tref("customers", None)], "i", false);
        assert!(
            rows.contains(&("id".to_string(), Some("customers.id".to_string()))),
            "{rows:?}"
        );
        // One table in scope: nothing to tell apart.
        let rows = column_inserts(&[tref("orders", Some("o"))], "i", false);
        assert!(rows.contains(&("id".to_string(), None)), "{rows:?}");
        // A `USING (…)` list takes the bare name, whatever is in scope.
        let rows = column_inserts(&scope, "i", true);
        assert!(rows.contains(&("id".to_string(), None)), "{rows:?}");
    }

    /// `enum_value_suggestions` at the `|`, as `(text, insert, replaced text)`.
    fn enum_rows(sql: &str, s: &SchemaIndex, dialect: SqlDialect) -> Vec<(String, String, String)> {
        let caret = sql.find('|').unwrap();
        let sql = sql.replace('|', "");
        let Some(slot) = crate::intel::enum_value_slot(&sql, 0, caret, dialect) else {
            return Vec::new();
        };
        let scope = crate::intel::statement_scope(&sql, 0, sql.len(), caret, dialect).tables;
        enum_value_suggestions(s, &scope, &slot, &sql, caret, dialect)
            .into_iter()
            .map(|r| {
                let (a, b) = r.replace.unwrap();
                (r.text, r.insert.unwrap(), sql[a..b].to_string())
            })
            .collect()
    }

    fn ticketed() -> SchemaIndex {
        let col = |n: &str, t: &str| ColMeta {
            name: n.to_string(),
            type_name: t.to_string(),
            nullable: false,
            primary_key: false,
            foreign_key: false,
        };
        let tickets = vec![
            col("id", "int"),
            col("status", "enum('open','in review','won''t fix')"),
            col("mood", "mood"),
        ];
        let mut columns = HashMap::new();
        columns.insert("tickets".to_string(), tickets);
        let mut enum_types = HashMap::new();
        enum_types.insert(
            "mood".to_string(),
            vec!["happy".to_string(), "sad".to_string()],
        );
        SchemaIndex {
            tables: vec![("tickets".to_string(), "shop".to_string())],
            columns,
            enum_types,
            ..SchemaIndex::default()
        }
    }

    #[test]
    fn a_column_s_enum_members_are_offered_in_its_string() {
        let s = ticketed();
        let texts = |sql: &str, d| -> Vec<String> {
            enum_rows(sql, &s, d).into_iter().map(|r| r.0).collect()
        };
        // In declaration order, and narrowed by what is typed.
        assert_eq!(
            texts("SELECT * FROM tickets WHERE status = '|", SqlDialect::MySql),
            ["open", "in review", "won't fix"]
        );
        assert_eq!(
            texts(
                "SELECT * FROM tickets t WHERE t.status = 'IN|",
                SqlDialect::MySql
            ),
            ["in review"]
        );
        // A PostgreSQL enum is a named type, its labels the schema's.
        assert_eq!(
            texts(
                "SELECT * FROM tickets WHERE mood IN ('happy', '|",
                SqlDialect::Postgres
            ),
            ["happy", "sad"]
        );
        // Not an enum, not a column, not in scope: nothing.
        assert!(texts("SELECT * FROM tickets WHERE id = '|", SqlDialect::MySql).is_empty());
        assert!(texts("SELECT * FROM tickets WHERE nope = '|", SqlDialect::MySql).is_empty());
        assert!(texts("SELECT * FROM other WHERE status = '|", SqlDialect::MySql).is_empty());
    }

    #[test]
    fn an_enum_member_is_written_as_a_literal_and_closes_its_quote() {
        let s = ticketed();
        let rows = enum_rows(
            "SELECT * FROM tickets WHERE status = 'w|",
            &s,
            SqlDialect::MySql,
        );
        assert_eq!(
            rows,
            [(
                "won't fix".to_string(),
                "won''t fix'".to_string(),
                "w".to_string()
            )]
        );
        // A closed literal is replaced inside its quotes, and keeps them.
        let rows = enum_rows(
            "SELECT * FROM tickets WHERE status = 'op|x' AND 1",
            &s,
            SqlDialect::MySql,
        );
        assert_eq!(
            rows,
            [("open".to_string(), "open".to_string(), "opx".to_string())]
        );
    }

    /// A target that carries an aliased form inserts it — the alias and the
    /// predicate that uses it. The caller has already dropped it where the
    /// setting is off.
    #[test]
    fn an_aliased_join_target_inserts_its_alias() {
        let s = shop();
        let cont = Continuation::default();
        let used = HashSet::new();
        let targets = vec![JoinTarget {
            table: "customers".to_string(),
            table_sql: "customers".to_string(),
            predicate: "customers.id = o.customer_id".to_string(),
            aliased: Some(crate::intel::AliasedJoin {
                alias: "c".to_string(),
                predicate: "c.id = o.customer_id".to_string(),
            }),
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
                dialect: SqlDialect::MySql,
                flavour: ServerFlavour::Unknown,
                local: &LocalNames::default(),
                wide: false,
                accepted: &Default::default(),
            },
        );
        assert_eq!(
            out.first().and_then(|s| s.insert.as_deref()),
            Some("customers c ON c.id = o.customer_id")
        );
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
                dialect: SqlDialect::MySql,
                flavour: ServerFlavour::Unknown,
                local: &LocalNames::default(),
                wide: false,
                accepted: &Default::default(),
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
