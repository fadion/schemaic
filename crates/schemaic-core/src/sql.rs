//! Shared SQL lexing/analysis — pure `&str` → data, no UI or DB dependency.
//!
//! Everything here is built on one boundary primitive, [`skip_noncode`], so the
//! statement splitter, the unsafe-statement guard, and the AI read-only gate all
//! agree on where strings / identifiers / comments begin and end. (Previously
//! these were hand-rolled separately and disagreed — review §3.4: a `#` comment
//! or a backtick identifier could hide a `WHERE` from the guard, etc.)

/// If `b[i..]` starts a comment, return the index just past it. Handles `--`
/// (MySQL requires whitespace/EOL after it, so `1--2` is `1 - -2`, not a
/// comment), `#` line comments, and `/* … */` block comments.
fn skip_comment(b: &[u8], i: usize) -> Option<usize> {
    let n = b.len();
    if b[i] == b'-'
        && i + 1 < n
        && b[i + 1] == b'-'
        && (i + 2 >= n || b[i + 2].is_ascii_whitespace())
    {
        let mut j = i + 2;
        while j < n && b[j] != b'\n' {
            j += 1;
        }
        return Some(j);
    }
    if b[i] == b'#' {
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

/// If `b[i..]` starts a string literal, backtick identifier, or comment, return
/// the index just past it; otherwise `None`. String literals honor both `\`
/// and doubled-quote (`''`) escapes; backtick identifiers honor `` `` ``.
pub fn skip_noncode(b: &[u8], i: usize) -> Option<usize> {
    if let Some(j) = skip_comment(b, i) {
        return Some(j);
    }
    let n = b.len();
    match b[i] {
        q @ (b'\'' | b'"') => {
            let mut j = i + 1;
            while j < n {
                if b[j] == b'\\' && j + 1 < n {
                    j += 2;
                    continue;
                }
                if b[j] == q {
                    if j + 1 < n && b[j + 1] == q {
                        j += 2; // doubled quote → escaped, stay in the string
                        continue;
                    }
                    return Some(j + 1);
                }
                j += 1;
            }
            Some(n) // unterminated → to end
        }
        b'`' => {
            let mut j = i + 1;
            while j < n {
                if b[j] == b'`' {
                    if j + 1 < n && b[j + 1] == b'`' {
                        j += 2;
                        continue;
                    }
                    return Some(j + 1);
                }
                j += 1;
            }
            Some(n)
        }
        _ => None,
    }
}

/// Byte offsets bounding each top-level statement: `[0, after-`;`, …, len]`.
/// `;` inside strings / identifiers / comments does not split.
pub fn statement_bounds(sql: &str) -> Vec<usize> {
    let b = sql.as_bytes();
    let n = b.len();
    let mut bounds = vec![0usize];
    let mut i = 0;
    while i < n {
        if let Some(j) = skip_noncode(b, i) {
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
pub fn statement_range(sql: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(sql.len());
    let bounds = statement_bounds(sql);
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
fn segment_has_code(sql: &str, lo: usize, hi: usize) -> bool {
    let b = sql.as_bytes();
    let mut i = lo;
    while i < hi {
        if b[i].is_ascii_whitespace() {
            i += 1;
        } else if let Some(j) = skip_comment(b, i) {
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
pub fn statement_ranges(sql: &str) -> Vec<(usize, usize)> {
    statement_bounds(sql)
        .windows(2)
        .map(|w| trim_range(sql, w[0], w[1]))
        .filter(|&(lo, hi)| lo < hi && segment_has_code(sql, lo, hi))
        .collect()
}

/// The uppercased first keyword of `sql` (skipping leading whitespace and
/// comments), or `None` if it doesn't start with a word.
pub fn leading_keyword(sql: &str) -> Option<String> {
    let b = sql.as_bytes();
    let n = b.len();
    let mut i = 0;
    loop {
        while i < n && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < n
            && let Some(j) = skip_comment(b, i)
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
        return Some(sql[s..j].to_ascii_uppercase());
    }
    None
}

/// Does `sql` contain a `WHERE` keyword at paren depth 0 (not inside a
/// subquery, string, identifier, or comment)?
pub fn has_top_level_where(sql: &str) -> bool {
    let b = sql.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut depth: i32 = 0;
    while i < n {
        if let Some(j) = skip_noncode(b, i) {
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
pub fn unsafe_reason(stmt: &str) -> Option<String> {
    match leading_keyword(stmt)?.as_str() {
        "TRUNCATE" => Some("TRUNCATE removes every row in the table.".to_string()),
        kind @ ("DELETE" | "UPDATE") => {
            if has_top_level_where(stmt) {
                None
            } else {
                Some(format!("{kind} statement without WHERE clause detected."))
            }
        }
        _ => None,
    }
}

/// The first unsafe statement's warning across all statements in `sql`.
pub fn first_unsafe(sql: &str) -> Option<String> {
    statement_ranges(sql)
        .into_iter()
        .find_map(|(lo, hi)| sql.get(lo..hi).and_then(unsafe_reason))
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
fn word_tokens(sql: &str) -> (Vec<String>, bool) {
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
        if let Some(j) = skip_noncode(b, i) {
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
pub fn read_only_reason(sql: &str) -> Result<(), String> {
    let (words, multi) = word_tokens(sql);
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

/// Does `sql` contain any statement that isn't a plain read? Classifies by each
/// statement's head keyword (skipping strings/identifiers/comments): a read is
/// `SELECT`/`SHOW`/`DESCRIBE`/`DESC`/`EXPLAIN`/`WITH`/`VALUES`/`TABLE`; anything
/// else (UPDATE/DELETE/INSERT/CREATE/DROP/…, or a stored-proc CALL/DO/SET/USE) is
/// treated as a write. Used to block mutations on a read-only connection. Unlike
/// the single-statement AI gate (`read_only_reason`), this allows several read
/// statements and only flags the actual writes.
pub fn contains_write(sql: &str) -> bool {
    for (lo, hi) in statement_ranges(sql) {
        let (words, _) = word_tokens(&sql[lo..hi]);
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
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

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
