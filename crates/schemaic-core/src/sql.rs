//! Shared SQL lexing/analysis — pure `&str` → data, no UI or DB dependency.
//!
//! Everything here is built on one boundary primitive, [`skip_noncode`], so the
//! statement splitter, the unsafe-statement guard, and the AI read-only gate all
//! agree on where strings / identifiers / comments begin and end. (Previously
//! these were hand-rolled separately and disagreed — review §3.4: a `#` comment
//! or a backtick identifier could hide a `WHERE` from the guard, etc.)
//!
//! **Dialect-aware.** The primitive takes a [`SqlDialect`] because the boundaries
//! genuinely differ between engines: MySQL has `#` line comments, backtick
//! identifiers, `\`-escapes inside all quotes, and requires whitespace after `--`
//! (so `1--2` is arithmetic); PostgreSQL instead uses `#` in operators
//! (`#>`/`#>>`/`#-`), double-quoted identifiers, dollar-quoted strings
//! (`$tag$ … $tag$`), `\`-escapes only in `E'…'` strings, and follows the
//! standard in starting a comment at `--` with no whitespace needed. Every caller
//! passes the connection's dialect so splitting/guards/highlighting agree with
//! the AST.

use crate::intel::SqlDialect;

/// If `b[i..]` starts a comment, return the index just past it. Handles `--`
/// (**MySQL only** requires whitespace/EOL after it, so there `1--2` is
/// `1 - -2`, not a comment; Postgres follows the standard and needs none),
/// `#` line comments (**MySQL only** — Postgres uses `#` in operators), and
/// `/* … */` block comments (both dialects; non-nesting).
fn skip_comment(b: &[u8], i: usize, dialect: SqlDialect) -> Option<usize> {
    let n = b.len();
    if b[i] == b'-'
        && i + 1 < n
        && b[i + 1] == b'-'
        && (dialect == SqlDialect::Postgres || i + 2 >= n || b[i + 2].is_ascii_whitespace())
    {
        let mut j = i + 2;
        while j < n && b[j] != b'\n' {
            j += 1;
        }
        return Some(j);
    }
    if b[i] == b'#' && dialect != SqlDialect::Postgres {
        let mut j = i + 1;
        while j < n && b[j] != b'\n' {
            j += 1;
        }
        return Some(j);
    }
    if b[i] == b'/' && i + 1 < n && b[i + 1] == b'*' {
        let mut j = i + 2;
        while j + 1 < n && !(b[j] == b'*' && b[j + 1] == b'/') {
            j += 1;
        }
        return Some((j + 2).min(n));
    }
    None
}

/// Scan a quoted span opening at `b[i]` (quote byte `q`) to just past its close,
/// honoring `\` escapes when `backslash` and always the doubled-quote (`qq`)
/// escape. Unterminated → end of input.
fn scan_quoted(b: &[u8], i: usize, q: u8, backslash: bool) -> usize {
    let n = b.len();
    let mut j = i + 1;
    while j < n {
        if backslash && b[j] == b'\\' && j + 1 < n {
            j += 2;
            continue;
        }
        if b[j] == q {
            if j + 1 < n && b[j + 1] == q {
                j += 2; // doubled quote → escaped, stay inside
                continue;
            }
            return j + 1;
        }
        j += 1;
    }
    n
}

/// Scan a PostgreSQL dollar-quoted string opening at `b[i] == '$'`. The opening
/// tag is `$[tag]$` (tag = optional word chars; `$$` is the empty tag); the span
/// runs to the matching closing `$tag$`. Returns `None` when this isn't actually
/// a dollar-quote (e.g. a `$1` positional parameter), so the caller scans it as
/// ordinary code.
fn scan_dollar(b: &[u8], i: usize) -> Option<usize> {
    let n = b.len();
    let mut j = i + 1;
    while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
        j += 1;
    }
    if j >= n || b[j] != b'$' {
        return None; // not `$tag$` — a `$1` param or a lone `$`
    }
    let marker = &b[i..=j]; // the opening `$tag$`
    let mut k = j + 1;
    while k + marker.len() <= n {
        if &b[k..k + marker.len()] == marker {
            return Some(k + marker.len());
        }
        k += 1;
    }
    Some(n) // unterminated → to end
}

/// If `b[i..]` starts a string literal, quoted/backtick identifier, dollar-quoted
/// string, or comment, return the index just past it; otherwise `None`. Boundary
/// rules follow `dialect` (see the module docs): MySQL `\`-escapes every quote and
/// treats `"`/`` ` `` as strings/identifiers; PostgreSQL uses `"` identifiers,
/// `$tag$` strings, and `\`-escapes only in `E'…'`.
pub fn skip_noncode(b: &[u8], i: usize, dialect: SqlDialect) -> Option<usize> {
    if let Some(j) = skip_comment(b, i, dialect) {
        return Some(j);
    }
    let pg = dialect == SqlDialect::Postgres;
    match b[i] {
        b'\'' => {
            // MySQL: `\` escapes. Postgres: only in an `E'…'`/`e'…'` string — a
            // standalone `E` prefix (not the tail of a longer word).
            let backslash = if pg {
                i >= 1
                    && matches!(b[i - 1], b'e' | b'E')
                    && (i < 2 || !(b[i - 2].is_ascii_alphanumeric() || b[i - 2] == b'_'))
            } else {
                true
            };
            Some(scan_quoted(b, i, b'\'', backslash))
        }
        // MySQL: `"` is a string (with `\` escapes). Postgres: a quoted identifier
        // (no `\` escapes; only `""` doubling).
        b'"' => Some(scan_quoted(b, i, b'"', !pg)),
        // MySQL backtick identifier (Postgres doesn't use backticks).
        b'`' if !pg => Some(scan_quoted(b, i, b'`', false)),
        // Postgres dollar-quoted string (`$tag$ … $tag$`); `None` for `$1` params.
        b'$' if pg => scan_dollar(b, i),
        _ => None,
    }
}

/// Byte offsets bounding each top-level statement: `[0, after-`;`, …, len]`.
/// `;` inside strings / identifiers / comments does not split.
pub fn statement_bounds(sql: &str, dialect: SqlDialect) -> Vec<usize> {
    let b = sql.as_bytes();
    let n = b.len();
    let mut bounds = vec![0usize];
    let mut i = 0;
    while i < n {
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j;
            continue;
        }
        if b[i] == b';' {
            bounds.push(i + 1);
        }
        i += 1;
    }
    bounds.push(n);
    bounds
}

/// Trim ASCII whitespace off both ends of `sql[lo..hi]`.
pub fn trim_range(sql: &str, lo: usize, hi: usize) -> (usize, usize) {
    let b = sql.as_bytes();
    let (mut lo, mut hi) = (lo, hi);
    while lo < hi && b[lo].is_ascii_whitespace() {
        lo += 1;
    }
    while hi > lo && b[hi - 1].is_ascii_whitespace() {
        hi -= 1;
    }
    (lo, hi)
}

/// The trimmed byte range of the statement containing `offset`.
pub fn statement_range(sql: &str, offset: usize, dialect: SqlDialect) -> (usize, usize) {
    let offset = offset.min(sql.len());
    let bounds = statement_bounds(sql, dialect);
    let mut k = 0;
    for (w, &b) in bounds.iter().enumerate().take(bounds.len() - 1) {
        if b <= offset {
            k = w;
        }
    }
    let (lo, hi) = trim_range(sql, bounds[k], bounds[k + 1]);
    if lo == hi && k > 0 {
        // Blank segment (e.g. caret after the final `;`) → previous statement.
        return trim_range(sql, bounds[k - 1], bounds[k]);
    }
    (lo, hi)
}

/// Does `sql[lo..hi]` contain any actual SQL (not just whitespace + comments)?
fn segment_has_code(sql: &str, lo: usize, hi: usize, dialect: SqlDialect) -> bool {
    let b = sql.as_bytes();
    let mut i = lo;
    while i < hi {
        if b[i].is_ascii_whitespace() {
            i += 1;
        } else if let Some(j) = skip_comment(b, i, dialect) {
            i = j;
        } else {
            return true;
        }
    }
    false
}

/// Every top-level statement's trimmed byte range that contains real SQL, in
/// order. Comment/whitespace-only segments (e.g. a trailing `# note` after the
/// last `;`) are dropped so Run Everything doesn't emit an "empty query" tab.
pub fn statement_ranges(sql: &str, dialect: SqlDialect) -> Vec<(usize, usize)> {
    statement_bounds(sql, dialect)
        .windows(2)
        .map(|w| trim_range(sql, w[0], w[1]))
        .filter(|&(lo, hi)| lo < hi && segment_has_code(sql, lo, hi, dialect))
        .collect()
}

/// The uppercased first keyword of `sql` (skipping leading whitespace and
/// comments), or `None` if it doesn't start with a word.
pub fn leading_keyword(sql: &str, dialect: SqlDialect) -> Option<String> {
    leading_keyword_span(sql, dialect).map(|(s, e)| sql[s..e].to_ascii_uppercase())
}

/// The byte offset just past [`leading_keyword`] — where the rest of the
/// statement begins. `None` on the same inputs `leading_keyword` returns `None`
/// for, so a caller that needs the second token can't accidentally read from
/// offset 0 of a comment-only string.
pub fn leading_keyword_end(sql: &str, dialect: SqlDialect) -> Option<usize> {
    leading_keyword_span(sql, dialect).map(|(_, e)| e)
}

/// Byte range of the leading keyword, skipping whitespace and comments.
fn leading_keyword_span(sql: &str, dialect: SqlDialect) -> Option<(usize, usize)> {
    let b = sql.as_bytes();
    let n = b.len();
    let mut i = 0;
    loop {
        while i < n && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < n
            && let Some(j) = skip_comment(b, i, dialect)
        {
            i = j;
            continue;
        }
        break;
    }
    if i < n && (b[i].is_ascii_alphabetic() || b[i] == b'_') {
        let s = i;
        let mut j = i + 1;
        while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
            j += 1;
        }
        return Some((s, j));
    }
    None
}

/// Does `sql` contain a `WHERE` keyword at paren depth 0 (not inside a
/// subquery, string, identifier, or comment)?
pub fn has_top_level_where(sql: &str, dialect: SqlDialect) -> bool {
    let b = sql.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut depth: i32 = 0;
    while i < n {
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j;
            continue;
        }
        match b[i] {
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth = (depth - 1).max(0); // unbalanced `)` must not go negative
                i += 1;
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let s = i;
                let mut j = i + 1;
                while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                    j += 1;
                }
                if depth == 0 && sql[s..j].eq_ignore_ascii_case("WHERE") {
                    return true;
                }
                i = j;
            }
            _ => i += 1,
        }
    }
    false
}

/// If `stmt` would rewrite/erase every row (DELETE/UPDATE without a top-level
/// WHERE, or TRUNCATE), the warning to show the user; else `None`.
pub fn unsafe_reason(stmt: &str, dialect: SqlDialect) -> Option<String> {
    match leading_keyword(stmt, dialect)?.as_str() {
        "TRUNCATE" => Some("TRUNCATE removes every row in the table.".to_string()),
        kind @ ("DELETE" | "UPDATE") => {
            if has_top_level_where(stmt, dialect) {
                None
            } else {
                Some(format!("{kind} statement without WHERE clause detected."))
            }
        }
        // A data-modifying CTE hides the write inside the statement, where
        // neither the head keyword nor a *top-level* WHERE scan can reach it.
        "WITH" => cte_unsafe_reason(stmt, dialect),
        _ => None,
    }
}

/// The unsafe-statement warning for a `WITH …` whose body modifies data.
///
/// The write sits inside parentheses, so `has_top_level_where` can't judge it —
/// this asks the weaker question *is there a WHERE anywhere in the statement*.
/// That errs toward silence on an unusual scoped write and toward warning on the
/// all-rows case, which is the right way round: a false warning costs a click, a
/// missed one costs the table.
fn cte_unsafe_reason(stmt: &str, dialect: SqlDialect) -> Option<String> {
    let (words, _) = word_tokens(stmt, dialect);
    if words.iter().any(|w| w == "TRUNCATE") {
        return Some("TRUNCATE removes every row in the table.".to_string());
    }
    let kind = words.iter().find(|w| *w == "DELETE" || *w == "UPDATE")?;
    if words.iter().any(|w| w == "WHERE") {
        None
    } else {
        Some(format!("{kind} statement without WHERE clause detected."))
    }
}

/// The first unsafe statement's warning across all statements in `sql`.
pub fn first_unsafe(sql: &str, dialect: SqlDialect) -> Option<String> {
    statement_ranges(sql, dialect)
        .into_iter()
        .find_map(|(lo, hi)| sql.get(lo..hi).and_then(|s| unsafe_reason(s, dialect)))
}

/// Bounded Levenshtein edit distance between two ASCII strings.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

// ── AI read-only gate ────────────────────────────────────────────────────────

/// Keywords that make a statement non-read-only or dangerous, matched as whole
/// top-level tokens (outside strings / identifiers / comments). The AI consumes
/// untrusted result data, so it must not mutate, lock, sleep, or touch the
/// filesystem — this is a security boundary (review C7).
const DENY_KEYWORDS: &[&str] = &[
    "INSERT",
    "UPDATE",
    "DELETE",
    "REPLACE",
    "MERGE",
    "DROP",
    "CREATE",
    "ALTER",
    "TRUNCATE",
    "RENAME",
    "GRANT",
    "REVOKE",
    "CALL",
    "DO",
    "LOAD",
    "IMPORT",
    "HANDLER",
    "LOCK",
    "UNLOCK",
    "PREPARE",
    "EXECUTE",
    "DEALLOCATE",
    "SET",
    "RESET",
    "FLUSH",
    "KILL",
    "SHUTDOWN",
    "START",
    "COMMIT",
    "ROLLBACK",
    "SAVEPOINT",
    "USE",
    "OUTFILE",
    "DUMPFILE",
    "ANALYZE",
    "OPTIMIZE",
    "REPAIR",
    "SLEEP",
    "BENCHMARK",
    "GET_LOCK",
    "RELEASE_LOCK",
];

/// Split SQL into upper-cased word tokens, skipping string/identifier/comment
/// content. The bool is set once a top-level `;` is followed by more real
/// content (i.e. the input is multiple statements).
fn word_tokens(sql: &str, dialect: SqlDialect) -> (Vec<String>, bool) {
    let b = sql.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut words = Vec::new();
    let mut word = String::new();
    let mut ended = false;
    let mut multi = false;
    macro_rules! flush {
        () => {
            if !word.is_empty() {
                if ended {
                    multi = true;
                }
                words.push(std::mem::take(&mut word));
            }
        };
    }
    while i < n {
        if let Some(j) = skip_noncode(b, i, dialect) {
            flush!();
            i = j;
            continue;
        }
        let c = b[i];
        if c == b';' {
            flush!();
            ended = true;
        } else if c.is_ascii_alphanumeric() || c == b'_' {
            word.push(c.to_ascii_uppercase() as char);
        } else {
            flush!();
        }
        i += 1;
    }
    flush!();
    (words, multi)
}

/// Is `sql` a single read-only statement we're willing to run on the AI's
/// behalf? Returns the rejection reason on failure.
pub fn read_only_reason(sql: &str, dialect: SqlDialect) -> Result<(), String> {
    let (words, multi) = word_tokens(sql, dialect);
    if multi {
        return Err("only a single statement is allowed".to_string());
    }
    let head = words.first().map(|s| s.as_str()).unwrap_or("");
    if !matches!(
        head,
        "SELECT" | "SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN" | "WITH"
    ) {
        return Err(
            "only read-only queries (SELECT/SHOW/DESCRIBE/EXPLAIN/WITH) are allowed".to_string(),
        );
    }
    if let Some(bad) = words.iter().find(|w| DENY_KEYWORDS.contains(&w.as_str())) {
        return Err(format!("`{bad}` is not permitted in an AI query"));
    }
    Ok(())
}

/// Keywords that make a statement a write *wherever* they appear in it, not just
/// at its head — the set that survives the read-head allowlist below.
///
/// Deliberately narrower than [`DENY_KEYWORDS`]: this gate allows several read
/// statements and only needs the ones that change data or write a file, whereas
/// the AI gate also refuses locks, sleeps and session state. Keeping them
/// separate is what stops this scan from over-blocking an ordinary query.
const WRITE_KEYWORDS: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "REPLACE", "MERGE", "TRUNCATE", "DROP", "CREATE", "ALTER",
    "RENAME", "GRANT", "REVOKE", "OUTFILE", "DUMPFILE",
];

/// Does `sql` contain any statement that isn't a plain read? Used to block
/// mutations on a read-only connection. Unlike the single-statement AI gate
/// (`read_only_reason`), this allows several read statements and only flags the
/// actual writes.
///
/// Two tests per statement, because the head keyword alone is not enough. A head
/// outside the read set (UPDATE/DELETE/INSERT/CREATE/DROP/…, or a stored-proc
/// CALL/DO/SET/USE) is a write; so is a read-headed statement carrying a
/// [`WRITE_KEYWORDS`] token anywhere in it. **The second test is what catches a
/// PostgreSQL data-modifying CTE** — `WITH gone AS (DELETE FROM city RETURNING
/// *) SELECT …` is headed `WITH`, which is on the read allowlist, and it deletes
/// every row. It also catches MySQL's `SELECT … INTO OUTFILE`, which writes a
/// file on the server behind a `SELECT` head.
///
/// The scan reads whole tokens outside strings / quoted identifiers / comments,
/// so `SELECT "delete" FROM t` and `SELECT * FROM delete_log` stay reads. Where
/// it is imprecise it is imprecise toward blocking, which on a connection the
/// user marked read-only is the correct direction.
pub fn contains_write(sql: &str, dialect: SqlDialect) -> bool {
    for (lo, hi) in statement_ranges(sql, dialect) {
        let (words, _) = word_tokens(&sql[lo..hi], dialect);
        match words.first().map(|s| s.as_str()) {
            None => continue, // empty / comment-only statement
            Some(head) => {
                if !matches!(
                    head,
                    "SELECT"
                        | "SHOW"
                        | "DESCRIBE"
                        | "DESC"
                        | "EXPLAIN"
                        | "WITH"
                        | "VALUES"
                        | "TABLE"
                ) {
                    return true;
                }
                if words.iter().any(|w| WRITE_KEYWORDS.contains(&w.as_str())) {
                    return true;
                }
            }
        }
    }
    false
}

/// Does any statement here carry a **credential in its text** — the class the
/// `mysql` CLI's default `histignore` (`*IDENTIFIED*:*PASSWORD*`) keeps out of
/// `~/.mysql_history`?
///
/// Used to keep such statements out of `history.json`. Schemaic goes out of its
/// way to keep the connection password off disk (`core::secrets`), and a user who
/// trusts that has no reason to expect the same directory to hold the password
/// they typed into a `CREATE USER`.
///
/// Per statement, on whole tokens outside strings / quoted identifiers / comments
/// (so `SELECT password FROM users` is *not* a credential statement — it names a
/// column):
///
/// - anything containing `IDENTIFIED` — `CREATE`/`ALTER USER … IDENTIFIED BY`,
///   and `GRANT … IDENTIFIED BY`;
/// - a `SET` statement containing `PASSWORD` — `SET PASSWORD FOR … = …`;
/// - a `CREATE`/`ALTER`/`DROP`/`GRANT`/`REVOKE` naming a `USER` or `ROLE` *and*
///   `PASSWORD` — PostgreSQL's `CREATE ROLE … WITH PASSWORD '…'`.
///
/// Where it is imprecise it is imprecise toward omitting: a dropped history entry
/// costs the user a scroll, a kept one writes their secret to disk.
pub fn carries_credential(sql: &str, dialect: SqlDialect) -> bool {
    for (lo, hi) in statement_ranges(sql, dialect) {
        let (words, _) = word_tokens(&sql[lo..hi], dialect);
        if words.iter().any(|w| w == "IDENTIFIED") {
            return true;
        }
        if !words.iter().any(|w| w == "PASSWORD") {
            continue;
        }
        let names_a_principal = words.iter().any(|w| w == "USER" || w == "ROLE");
        match words.first().map(|s| s.as_str()) {
            Some("SET") => return true,
            Some("CREATE" | "ALTER" | "DROP" | "GRANT" | "REVOKE") if names_a_principal => {
                return true;
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intel::SqlDialect;

    // These legacy tests predate the dialect parameter and assert MySQL boundary
    // behavior; thin MySQL-defaulting wrappers (shadowing the glob import) keep them
    // unchanged. New Postgres tests below call the real `super::` functions with an
    // explicit `SqlDialect::Postgres`.
    fn skip_noncode(b: &[u8], i: usize) -> Option<usize> {
        super::skip_noncode(b, i, SqlDialect::MySql)
    }
    fn statement_ranges(s: &str) -> Vec<(usize, usize)> {
        super::statement_ranges(s, SqlDialect::MySql)
    }
    fn statement_range(s: &str, o: usize) -> (usize, usize) {
        super::statement_range(s, o, SqlDialect::MySql)
    }
    fn leading_keyword(s: &str) -> Option<String> {
        super::leading_keyword(s, SqlDialect::MySql)
    }
    fn has_top_level_where(s: &str) -> bool {
        super::has_top_level_where(s, SqlDialect::MySql)
    }
    fn unsafe_reason(s: &str) -> Option<String> {
        super::unsafe_reason(s, SqlDialect::MySql)
    }
    fn first_unsafe(s: &str) -> Option<String> {
        super::first_unsafe(s, SqlDialect::MySql)
    }
    fn contains_write(s: &str) -> bool {
        super::contains_write(s, SqlDialect::MySql)
    }
    fn read_only_reason(s: &str) -> Result<(), String> {
        super::read_only_reason(s, SqlDialect::MySql)
    }
    fn carries_credential(s: &str) -> bool {
        super::carries_credential(s, SqlDialect::MySql)
    }

    // ── Postgres dialect boundary tests ──────────────────────────────────────
    const PG: SqlDialect = SqlDialect::Postgres;

    /// A PostgreSQL data-modifying CTE puts the write *inside* the statement,
    /// where a head-keyword check can't see it — and `WITH` is on the read
    /// allowlist, so the read-only gate let a DELETE of every row straight
    /// through.
    #[test]
    fn pg_data_modifying_cte_is_a_write() {
        for s in [
            "WITH gone AS (DELETE FROM city RETURNING *) SELECT count(*) FROM gone",
            "WITH x AS (INSERT INTO t VALUES (1) RETURNING *) SELECT * FROM x",
            "WITH x AS (UPDATE t SET a=1 RETURNING *) SELECT * FROM x",
        ] {
            assert!(super::contains_write(s, PG), "must be a write: {s}");
        }
    }

    /// Over-blocking is the real risk of a whole-statement scan, so pin the
    /// shapes that must stay allowed on a read-only connection.
    #[test]
    fn a_read_only_cte_and_a_quoted_keyword_are_not_writes() {
        assert!(!super::contains_write(
            "WITH x AS (SELECT 1) SELECT * FROM x",
            PG
        ));
        // A quoted identifier is skipped by the lexer, in either dialect.
        assert!(!super::contains_write("SELECT \"delete\" FROM t", PG));
        assert!(!super::contains_write(
            "SELECT `delete` FROM t",
            SqlDialect::MySql
        ));
        // An underscore keeps the word whole — `delete_log` is not `DELETE`.
        assert!(!super::contains_write("SELECT * FROM delete_log", PG));
    }

    /// Same blind spot, other guard: the no-WHERE warning also classified by
    /// head keyword, so the CTE form was silently unwarned.
    #[test]
    fn a_data_modifying_cte_without_a_where_is_warned() {
        assert!(
            super::unsafe_reason(
                "WITH gone AS (DELETE FROM city RETURNING *) SELECT count(*) FROM gone",
                PG
            )
            .is_some()
        );
        assert!(
            super::unsafe_reason(
                "WITH gone AS (DELETE FROM city WHERE id < 10 RETURNING *) SELECT * FROM gone",
                PG
            )
            .is_none(),
            "a scoped delete is not the all-rows case"
        );
        assert!(
            super::unsafe_reason("WITH x AS (SELECT 1) SELECT * FROM x", PG).is_none(),
            "a read-only CTE warns about nothing"
        );
    }

    /// `SELECT … INTO OUTFILE` writes a file on the MySQL server, and passed the
    /// read-only gate on its `SELECT` head — same root cause.
    #[test]
    fn mysql_select_into_outfile_is_a_write() {
        assert!(super::contains_write(
            "SELECT * FROM t INTO OUTFILE '/tmp/x'",
            SqlDialect::MySql
        ));
    }

    /// MySQL requires whitespace after `--`; PostgreSQL and the SQL standard do
    /// not. Applying MySQL's rule to PostgreSQL means `--WHERE` reads as code.
    #[test]
    fn pg_double_dash_needs_no_whitespace_to_start_a_comment() {
        assert!(super::skip_noncode(b"--x", 0, PG).is_some());
        assert!(
            super::skip_noncode(b"--x", 0, SqlDialect::MySql).is_none(),
            "MySQL: `1--2` is `1 - -2`, not a comment — must not move"
        );
        // The spaced form is a comment in both.
        assert!(super::skip_noncode(b"-- x", 0, PG).is_some());
        assert!(super::skip_noncode(b"-- x", 0, SqlDialect::MySql).is_some());
    }

    /// The guard consequence: a commented-out WHERE must not count as a WHERE,
    /// or an ordinary mid-edit `DELETE FROM t --WHERE …` empties the table with
    /// no warning.
    #[test]
    fn pg_commented_out_where_does_not_satisfy_the_guard() {
        assert!(!super::has_top_level_where("DELETE FROM t --where", PG));
        assert!(super::unsafe_reason("DELETE FROM t --WHERE id=1", PG).is_some());
        assert!(super::unsafe_reason("DELETE FROM t -- where", PG).is_some());
        // A real WHERE still clears it, and MySQL's reading is unchanged.
        assert!(super::unsafe_reason("DELETE FROM t WHERE id=1", PG).is_none());
        assert!(
            super::has_top_level_where("DELETE FROM t --where", SqlDialect::MySql),
            "MySQL: `--where` is not a comment, so this really is a WHERE"
        );
    }

    /// The splitter consequence: a `;` inside a mis-lexed comment split the
    /// statement, and the tail was sent *without* its leading `--` — running the
    /// statement the user had commented out.
    #[test]
    fn pg_semicolon_inside_a_line_comment_does_not_split() {
        let s = "SELECT 1;\n--a; DROP TABLE t";
        let stmts = |d| -> Vec<String> {
            super::statement_ranges(s, d)
                .into_iter()
                .map(|(a, b)| s[a..b].to_string())
                .collect()
        };
        // The whole tail is one comment, so it yields no statement at all — the
        // DROP the user commented out is not merely un-split, it is unrunnable.
        assert_eq!(stmts(PG), vec!["SELECT 1;"]);
        // MySQL reads `--a` as code, so the `;` really does split there, and the
        // tail arrives stripped of its leading `--`. That is the bug this fix is
        // about, preserved here as the contrast that makes the dialect split real.
        assert_eq!(
            stmts(SqlDialect::MySql),
            vec!["SELECT 1;", "--a;", "DROP TABLE t"]
        );
    }

    #[test]
    fn pg_hash_is_not_a_comment() {
        // MySQL: `#` starts a line comment. Postgres: `#` is an operator byte
        // (jsonb `#>`/`#>>`), so `skip_noncode` must NOT treat it as a comment.
        let s = b"data #> '{a}'";
        assert!(super::skip_noncode(s, 5, SqlDialect::MySql).is_some()); // MySQL: comment
        assert!(super::skip_noncode(s, 5, PG).is_none()); // Postgres: code
    }

    #[test]
    fn pg_hash_operator_does_not_break_statement_split() {
        // The `#>` must not swallow the `;` as a comment would → two statements.
        let sql = "SELECT x #> '{a}' FROM t; SELECT 2";
        assert_eq!(super::statement_ranges(sql, PG).len(), 2);
        // MySQL treats `#...` as a comment to EOL, hiding the `;` → one statement.
        assert_eq!(super::statement_ranges(sql, SqlDialect::MySql).len(), 1);
    }

    #[test]
    fn pg_dollar_quoted_string_is_one_span() {
        // `$$ … $$` is a single string span; an inner `;` must not split.
        let s = "$$a;b$$ rest";
        let end = super::skip_noncode(s.as_bytes(), 0, PG).unwrap();
        assert_eq!(&s[..end], "$$a;b$$");
        assert_eq!(
            super::statement_ranges("SELECT $$a;b$$; SELECT 2", PG).len(),
            2
        );
        // A tagged dollar-quote too.
        let t = "$tag$x;y$tag$ z";
        let end = super::skip_noncode(t.as_bytes(), 0, PG).unwrap();
        assert_eq!(&t[..end], "$tag$x;y$tag$");
        // `$1` is a positional param, not a dollar-quote → scanned as code.
        assert_eq!(super::skip_noncode(b"$1 = x", 0, PG), None);
    }

    #[test]
    fn pg_double_quote_is_an_identifier_no_backslash() {
        // Postgres `"..."` is a quoted identifier: `\` is literal (only `""`
        // doubles). An embedded `;` must not split.
        let s = "\"we;ird\" rest";
        let end = super::skip_noncode(s.as_bytes(), 0, PG).unwrap();
        assert_eq!(&s[..end], "\"we;ird\"");
        assert_eq!(
            super::statement_ranges("SELECT \"a;b\" FROM t", PG).len(),
            1
        );
    }

    #[test]
    fn pg_plain_string_no_backslash_escape_but_estring_has_it() {
        // Plain PG string: `\` is literal → `'a\'` closes at the 2nd quote.
        let plain = "'a\\' rest";
        let end = super::skip_noncode(plain.as_bytes(), 0, PG).unwrap();
        assert_eq!(&plain[..end], "'a\\'");
        // `E'…'` enables backslash escapes → `\'` stays inside the string.
        let es = "E'a\\'b' rest";
        let end = super::skip_noncode(es.as_bytes(), 1, PG).unwrap();
        assert_eq!(&es[1..end], "'a\\'b'");
    }

    #[test]
    fn contains_write_classifies_by_head() {
        // Reads (any number) are allowed.
        assert!(!contains_write("SELECT * FROM t"));
        assert!(!contains_write("SELECT 1; SHOW TABLES; EXPLAIN SELECT 2"));
        assert!(!contains_write("WITH c AS (SELECT 1) SELECT * FROM c"));
        // A `where`/`update` hidden in a string or comment doesn't count.
        assert!(!contains_write("SELECT 'update me'; -- delete later"));
        // Writes / DDL are flagged.
        assert!(contains_write("UPDATE t SET a=1"));
        assert!(contains_write("DELETE FROM t"));
        assert!(contains_write("CREATE TABLE t (id INT)"));
        assert!(contains_write("DROP TABLE t"));
        // A write anywhere in a multi-statement batch trips it.
        assert!(contains_write("SELECT 1; DELETE FROM t"));
    }

    #[test]
    fn carries_credential_flags_the_statements_that_hold_a_secret() {
        // Every shape whose text contains the password the user typed.
        assert!(carries_credential("CREATE USER 'a'@'%' IDENTIFIED BY 'p'"));
        assert!(carries_credential("ALTER USER 'a'@'%' IDENTIFIED BY 'p'"));
        assert!(carries_credential(
            "GRANT ALL ON *.* TO 'a'@'%' IDENTIFIED BY 'p'"
        ));
        assert!(carries_credential("SET PASSWORD FOR 'a'@'%' = 'p'"));
        assert!(carries_credential("set password = 'p'")); // case-insensitive
        assert!(super::carries_credential(
            "CREATE ROLE app WITH LOGIN PASSWORD 'p'",
            PG
        ));
        assert!(super::carries_credential("ALTER ROLE app PASSWORD 'p'", PG));
        // Anywhere in a batch, not only at the head.
        assert!(carries_credential("SELECT 1; SET PASSWORD = 'p'"));
    }

    #[test]
    fn carries_credential_leaves_an_ordinary_password_column_alone() {
        // A column named `password` must not suppress the query that reads it —
        // this is why the check is on whole tokens rather than a substring scan.
        assert!(!carries_credential("SELECT password FROM users"));
        assert!(!carries_credential(
            "ALTER TABLE users ADD COLUMN password varchar(64)"
        ));
        assert!(!carries_credential(
            "CREATE TABLE users (id INT, password varchar(64))"
        ));
        assert!(!carries_credential("UPDATE users SET x = 1"));
        assert!(!carries_credential("SELECT * FROM t"));
        // Inside a string or a comment it isn't a token at all.
        assert!(!carries_credential("SELECT 'IDENTIFIED BY' AS note"));
        assert!(!carries_credential("SELECT 1 -- IDENTIFIED BY 'x'"));
        assert!(!carries_credential("SELECT `password` FROM `user`"));
    }

    #[test]
    fn statement_split_ignores_comment_and_backtick_semicolons() {
        // `;` inside a `#` comment must not split (H2).
        assert_eq!(statement_ranges("SELECT 1; # a;b").len(), 1);
        // `;` inside a backtick identifier must not split.
        assert_eq!(statement_ranges("SELECT * FROM `a;b`").len(), 1);
        // Two real statements do split.
        assert_eq!(statement_ranges("SELECT 1; SELECT 2").len(), 2);
        // `--2` is not a comment (no space) → one statement, not a split/comment.
        assert_eq!(statement_ranges("SELECT 1--2;").len(), 1);
    }

    #[test]
    fn where_guard_sees_through_comments_and_identifiers() {
        // `where` hidden in a `#` comment is NOT a real clause (H1).
        assert!(!has_top_level_where(
            "DELETE FROM logs # where did these go"
        ));
        // A backtick-quoted `where` column is not the clause.
        assert!(!has_top_level_where("DELETE FROM `where`"));
        // Real top-level WHERE.
        assert!(has_top_level_where("DELETE FROM t WHERE id = 1"));
        // WHERE only inside a subquery is not top-level.
        assert!(!has_top_level_where(
            "UPDATE t SET x = (SELECT y FROM u WHERE u.id = 1)"
        ));
        // Unbalanced ')' must not drive depth negative and hide a later WHERE.
        assert!(has_top_level_where("UPDATE t SET x=f()) WHERE id=1"));
    }

    #[test]
    fn unsafe_reason_covers_delete_update_truncate() {
        assert!(unsafe_reason("DELETE FROM t").is_some());
        assert!(unsafe_reason("DELETE FROM t WHERE id=1").is_none());
        assert!(unsafe_reason("UPDATE t SET a=1").is_some());
        assert!(unsafe_reason("TRUNCATE TABLE t").is_some());
        assert!(unsafe_reason("SELECT * FROM t").is_none());
        // A `#`-commented WHERE doesn't make a full-table DELETE look safe.
        assert!(unsafe_reason("DELETE FROM t # WHERE id=1").is_some());
    }

    #[test]
    fn read_only_gate_blocks_bypasses() {
        assert!(read_only_reason("SELECT * FROM t").is_ok());
        assert!(read_only_reason("WITH c AS (SELECT 1) SELECT * FROM c").is_ok());
        // CTE that hides a DELETE.
        assert!(read_only_reason("WITH c AS (SELECT 1) DELETE FROM t").is_err());
        // EXPLAIN ANALYZE actually executes the statement.
        assert!(read_only_reason("EXPLAIN ANALYZE DELETE FROM t").is_err());
        // SELECT … INTO OUTFILE writes files on the DB host.
        assert!(read_only_reason("SELECT * FROM t INTO OUTFILE '/tmp/x'").is_err());
        // SLEEP / locks.
        assert!(read_only_reason("SELECT SLEEP(10)").is_err());
        // Multi-statement.
        assert!(read_only_reason("SELECT 1; DROP TABLE t").is_err());
        // Dangerous words inside a string / identifier are fine.
        assert!(read_only_reason("SELECT 'delete from t'").is_ok());
        assert!(read_only_reason("SELECT `update` FROM t").is_ok());
    }

    #[test]
    fn edit_distance_basic_and_edges() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", ""), 3);
        // single substitution / insertion / deletion
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("flaw", "lawn"), 2);
        assert_eq!(edit_distance("SELECT", "SELET"), 1);
        // symmetric
        assert_eq!(edit_distance("abc", "yabd"), edit_distance("yabd", "abc"));
    }

    #[test]
    fn first_unsafe_finds_earliest_across_statements() {
        // First statement safe, second unsafe → reports the second.
        let r = first_unsafe("SELECT 1; DELETE FROM t");
        assert!(r.is_some());
        assert!(r.unwrap().contains("DELETE"));
        // All safe → None.
        assert!(first_unsafe("SELECT 1; SELECT 2").is_none());
        assert!(first_unsafe("DELETE FROM t WHERE id=1").is_none());
        // A comment-only trailing segment doesn't hide the earlier unsafe one.
        let r = first_unsafe("TRUNCATE TABLE t; # note");
        assert!(r.unwrap().contains("TRUNCATE"));
    }

    #[test]
    fn leading_keyword_skips_whitespace_and_comments() {
        assert_eq!(
            leading_keyword("select * from t"),
            Some("SELECT".to_string())
        );
        assert_eq!(
            leading_keyword("  \n /* c */ -- x\n update t"),
            Some("UPDATE".to_string())
        );
        // Starts with a digit / punctuation → no leading word.
        assert_eq!(leading_keyword("123 abc"), None);
        assert_eq!(leading_keyword("   "), None);
        assert_eq!(leading_keyword(""), None);
        // Underscore-led identifier is a word.
        assert_eq!(leading_keyword("_foo bar"), Some("_FOO".to_string()));
    }

    #[test]
    fn statement_range_locates_caret_and_falls_back_after_trailing_semicolon() {
        let sql = "SELECT 1; SELECT 2";
        // Caret in the first statement (range runs to the bound past the `;`).
        let (lo, hi) = statement_range(sql, 3);
        assert_eq!(&sql[lo..hi], "SELECT 1;");
        // Caret in the second statement.
        let (lo, hi) = statement_range(sql, 12);
        assert_eq!(&sql[lo..hi], "SELECT 2");
        // Caret past the final `;` (blank trailing segment) → previous statement
        // (its range runs to the bound past the `;`, so the `;` is included).
        let sql = "SELECT 1;";
        let (lo, hi) = statement_range(sql, sql.len());
        assert_eq!(&sql[lo..hi], "SELECT 1;");
        // Offset beyond the string length is clamped.
        let (lo, hi) = statement_range("SELECT 1", 9999);
        assert_eq!(&"SELECT 1"[lo..hi], "SELECT 1");
    }

    #[test]
    fn skip_noncode_handles_escapes_doubled_quotes_and_unterminated() {
        // Doubled '' stays inside the string.
        let s = "'a''b' rest";
        let end = skip_noncode(s.as_bytes(), 0).unwrap();
        assert_eq!(&s[..end], "'a''b'");
        // Backslash escape inside a string.
        let s = r"'a\'b' rest";
        let end = skip_noncode(s.as_bytes(), 0).unwrap();
        assert_eq!(&s[..end], r"'a\'b'");
        // Doubled backtick inside an identifier.
        let s = "`a``b` rest";
        let end = skip_noncode(s.as_bytes(), 0).unwrap();
        assert_eq!(&s[..end], "`a``b`");
        // Unterminated string runs to end.
        let s = "'no end";
        assert_eq!(skip_noncode(s.as_bytes(), 0), Some(s.len()));
        // Block comment.
        let s = "/* c */x";
        let end = skip_noncode(s.as_bytes(), 0).unwrap();
        assert_eq!(&s[..end], "/* c */");
        // Not a boundary char → None.
        assert_eq!(skip_noncode(b"abc", 0), None);
        // `--` without trailing whitespace is NOT a comment.
        assert_eq!(skip_noncode(b"--x", 0), None);
    }
}
