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

/// The lexical boundary rules, one predicate per divergence — the table
/// [`skip_noncode`] and [`skip_comment`] read instead of comparing against one
/// engine.
///
/// **They are predicates because the question stopped being binary.** The scanner
/// used to ask `dialect == SqlDialect::Postgres` and `!= SqlDialect::MySql`, which
/// silently sorts any *third* engine onto whichever side each comparison happens
/// to put it — with nothing failing to compile, because `!=` is exhaustive over
/// any number of variants. Three of those defaults would have been wrong for
/// SQLite and two of them dangerously: it has no `\` escape inside a string, so
/// `'C:\'` under MySQL's rule latches the scanner into a literal that never ends
/// and swallows the rest of the statement — which is precisely how a `WHERE` gets
/// hidden from the unsafe-statement guard (the bug this module was consolidated
/// to kill). Naming the capability makes each site say what it means, and adding
/// an engine fills in a table rather than hoping a `!=` falls the right way.
impl SqlDialect {
    /// Does `--` need whitespace (or EOL) after it to open a comment?
    ///
    /// MySQL alone requires it, so there `1--2` is `1 - -2`. PostgreSQL and SQLite
    /// follow the standard: `--` opens a comment wherever it appears.
    fn dash_comment_needs_space(self) -> bool {
        matches!(self, SqlDialect::MySql)
    }

    /// Is `#` a line comment? MySQL only — PostgreSQL spells operators with it
    /// (`#>`/`#>>`/`#-`), and SQLite doesn't accept the character at all, so
    /// treating it as a comment there would swallow a line the server would have
    /// rejected outright.
    fn hash_line_comment(self) -> bool {
        matches!(self, SqlDialect::MySql)
    }

    /// Does `\` escape inside an ordinary `'…'` string?
    ///
    /// MySQL only. PostgreSQL confines it to `E'…'` ([`Self::e_string_backslash`]),
    /// and SQLite has no backslash escape whatsoever — `'a\'` there is a complete
    /// string whose last character is a backslash, where MySQL reads the quote as
    /// escaped and keeps scanning.
    fn backslash_escapes(self) -> bool {
        matches!(self, SqlDialect::MySql)
    }

    /// Does an `E'…'` prefix turn `\` escapes on? PostgreSQL only.
    fn e_string_backslash(self) -> bool {
        matches!(self, SqlDialect::Postgres)
    }

    /// Is `"…"` a quoted *identifier* rather than a string literal?
    ///
    /// MySQL reads it as a string and `\`-escapes it; PostgreSQL and SQLite read it
    /// as an identifier, escaped only by doubling. (SQLite additionally *falls back*
    /// to reading one as a string when it resolves to no identifier, but that is a
    /// name-resolution rule, not a lexical one — the span is the same either way.)
    fn double_quote_is_ident(self) -> bool {
        !matches!(self, SqlDialect::MySql)
    }

    /// Are `` `…` `` identifiers accepted? MySQL's own syntax, which SQLite also
    /// takes for compatibility; PostgreSQL doesn't.
    fn backtick_ident(self) -> bool {
        matches!(self, SqlDialect::MySql | SqlDialect::Sqlite)
    }

    /// Are `[…]` identifiers accepted? SQLite only (taken for SQL-Server/Access
    /// compatibility). There is **no escape inside one** — the span ends at the
    /// first `]`, because SQLite defines no way to write one.
    fn bracket_ident(self) -> bool {
        matches!(self, SqlDialect::Sqlite)
    }

    /// Are `$tag$ … $tag$` strings accepted? PostgreSQL only.
    fn dollar_quoted(self) -> bool {
        matches!(self, SqlDialect::Postgres)
    }

    /// Does the client honour a `DELIMITER` directive? MySQL only — see
    /// [`delimiter_directive`] for why it exists there at all.
    fn delimiter_directive(self) -> bool {
        matches!(self, SqlDialect::MySql)
    }
}

/// If `b[i..]` starts a comment, return the index just past it. Handles `--`
/// (whitespace after it required per [`SqlDialect::dash_comment_needs_space`]),
/// `#` line comments (per [`SqlDialect::hash_line_comment`]), and `/* … */` block
/// comments (every dialect; non-nesting).
fn skip_comment(b: &[u8], i: usize, dialect: SqlDialect) -> Option<usize> {
    let n = b.len();
    if b[i] == b'-'
        && i + 1 < n
        && b[i + 1] == b'-'
        && (!dialect.dash_comment_needs_space() || i + 2 >= n || b[i + 2].is_ascii_whitespace())
    {
        let mut j = i + 2;
        while j < n && b[j] != b'\n' {
            j += 1;
        }
        return Some(j);
    }
    if b[i] == b'#' && dialect.hash_line_comment() {
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

/// Does a comment open at `b[i]`?
///
/// The classification half of [`skip_comment`], exposed because
/// [`crate::pairs::region_at`] has to tell a comment span from a string span
/// *after* the lexer has found one, and was answering it with its own byte test —
/// a second copy of the rule, which said `#` opened a comment on every dialect
/// but Postgres. That was right until it wasn't: SQLite has no `#` comment, and a
/// duplicated rule is one that gets a new engine wrong in exactly one place.
pub(crate) fn comment_open(b: &[u8], i: usize, dialect: SqlDialect) -> bool {
    i < b.len() && skip_comment(b, i, dialect).is_some()
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

/// Is this byte part of an identifier word?
///
/// **The one definition of architecture invariant 11** — *"identifier scanning
/// treats bytes `>= 0x80` as word bytes so Unicode identifiers tokenize whole"*.
/// It lives beside [`skip_noncode`] because it answers the other half of the same
/// question: that one says where a token *can't* start, this one says how far a
/// word runs.
///
/// It is one function rather than four because the invariant is stated in
/// `docs/architecture.md` and was upheld by four private copies across two crates, each with
/// its own comment restating the rule and no test comparing them — a fifth
/// scanner written without the `>= 0x80` clause, or one of the four edited in
/// isolation, would have reverted a documented invariant silently. The crate had
/// already regressed and repaired it once before this was consolidated.
///
/// `>= 0x80` covers both UTF-8 lead *and* continuation bytes, so a name splits at
/// no point inside a multi-byte character.
pub fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Can a word *start* here?
///
/// Deliberately not [`is_word_byte`]: a digit continues an identifier but can't
/// begin one, or `1e5` and `2024_01` would scan as names. The `>= 0x80` half is
/// the same invariant, and was hand-copied at four scanner sites — the reason
/// this is a function is that the two rules differ by exactly one word
/// (`alphanumeric` vs `alphabetic`), which is invisible when you are reading a
/// copy rather than comparing two.
pub fn is_word_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

/// Is the `'` at `i` preceded by a standalone `E`/`e` prefix (not the tail of a
/// longer word), i.e. PostgreSQL's escape-string syntax?
fn e_prefixed(b: &[u8], i: usize) -> bool {
    i >= 1
        && matches!(b[i - 1], b'e' | b'E')
        && (i < 2 || !(b[i - 2].is_ascii_alphanumeric() || b[i - 2] == b'_'))
}

/// Scan a SQLite `[…]` bracketed identifier to just past its `]`. There is no
/// escape inside one, so the span simply ends at the first `]`; unterminated →
/// end of input, matching [`scan_quoted`]'s policy.
fn scan_bracket(b: &[u8], i: usize) -> usize {
    let n = b.len();
    let mut j = i + 1;
    while j < n {
        if b[j] == b']' {
            return j + 1;
        }
        j += 1;
    }
    n
}

/// If `b[i..]` starts a string literal, quoted/backtick/bracketed identifier,
/// dollar-quoted string, or comment, return the index just past it; otherwise
/// `None`. Every boundary rule comes off the [`SqlDialect`] capability table
/// above, so no engine is the implicit default: MySQL `\`-escapes every quote and
/// reads `"` as a string; PostgreSQL uses `"` identifiers, `$tag$` strings and
/// `\`-escapes only in `E'…'`; SQLite reads `"`, `` ` `` *and* `[…]` as
/// identifiers and has no backslash escape at all.
pub fn skip_noncode(b: &[u8], i: usize, dialect: SqlDialect) -> Option<usize> {
    if let Some(j) = skip_comment(b, i, dialect) {
        return Some(j);
    }
    match b[i] {
        b'\'' => {
            let backslash =
                dialect.backslash_escapes() || (dialect.e_string_backslash() && e_prefixed(b, i));
            Some(scan_quoted(b, i, b'\'', backslash))
        }
        b'"' => Some(scan_quoted(b, i, b'"', !dialect.double_quote_is_ident())),
        b'`' if dialect.backtick_ident() => Some(scan_quoted(b, i, b'`', false)),
        b'[' if dialect.bracket_ident() => Some(scan_bracket(b, i)),
        b'$' if dialect.dollar_quoted() => scan_dollar(b, i),
        _ => None,
    }
}

/// The identifier at or after `at`, **unquoted**, and the offset just past it —
/// or `(None, offset)` when there isn't one.
///
/// **The one reader for a name in raw SQL text.** A backend that scans a stored
/// `CREATE` statement needs this constantly (SQLite's `CONSTRAINT <name>`,
/// `COLLATE <name>`) and re-spelling it is how the four-quoting rule drifts:
/// which bytes quote a name is [`crate::intel::ident_quote`]'s answer, per
/// dialect and **per byte**, because SQLite's `[x]` does not close with the byte
/// it opened with and only two of its three quotings double to escape.
///
/// The bare arm asks [`is_word_start`] as well as [`is_word_byte`], which the
/// hand-rolled copy this replaces did not: a digit cannot begin a name, so
/// `CONSTRAINT 3way` there read back a constraint called `3way`.
///
/// A quoted name that is never closed runs to the end of the input, matching
/// [`skip_noncode`]'s policy — a truncated statement should not silently produce
/// a shorter name.
pub fn ident_at(sql: &str, at: usize, dialect: SqlDialect) -> (Option<String>, usize) {
    let b = sql.as_bytes();
    let mut i = at;
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    let Some(&open) = b.get(i) else {
        return (None, i);
    };
    let Some((close, doubled)) = crate::intel::ident_quote(dialect, open) else {
        // A **string** literal is accepted as a name too, because SQLite does:
        // `CONSTRAINT 'x'` is legal there and means the identifier `x`.
        if open == b'\'' && dialect == SqlDialect::Sqlite {
            return quoted_ident(sql, i, b'\'', true);
        }
        if !is_word_start(open) {
            return (None, i);
        }
        let start = i;
        while i < b.len() && is_word_byte(b[i]) {
            i += 1;
        }
        return (Some(sql[start..i].to_string()), i);
    };
    quoted_ident(sql, i, close, doubled)
}

/// The body of a quoted identifier opening at `i`, with `close` doubled to
/// escape when `doubled`.
fn quoted_ident(sql: &str, i: usize, close: u8, doubled: bool) -> (Option<String>, usize) {
    let b = sql.as_bytes();
    let mut name = String::new();
    let mut i = i + 1;
    while i < b.len() {
        if b[i] == close {
            if doubled && b.get(i + 1) == Some(&close) {
                name.push(close as char);
                i += 2;
                continue;
            }
            return (Some(name), i + 1);
        }
        let ch = sql[i..].chars().next().map_or(1, char::len_utf8);
        name.push_str(&sql[i..i + ch]);
        i += ch;
    }
    (Some(name), i)
}

/// `sql` with a statement terminator on the end — put where a terminator will
/// actually terminate.
///
/// **A `;` appended to trimmed text can land inside a comment.** SQLite stores a
/// statement without its terminator and keeps the author's own trailing comment
/// with it, so `CREATE INDEX ia ON t(a) -- why this index exists` trims to end
/// *inside* the comment and `…exists;` is a script the engine rejects at the
/// next statement. The same is true of an unclosed `/*`. So the tail is walked
/// through [`skip_noncode`], and a `;` that would be swallowed goes on a line of
/// its own instead.
///
/// A statement that already ends in a `;` at a code position is returned as it
/// is, so this is idempotent.
pub fn terminated(sql: &str, dialect: SqlDialect) -> String {
    let t = sql.trim_end();
    let b = t.as_bytes();
    // Walk to the end, recording whether the last thing the lexer skipped ran
    // off the end of the input — which is what an unclosed comment does.
    let mut i = 0usize;
    let mut open_comment = false;
    let mut last_code: Option<u8> = None;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, dialect) {
            // Would a `;` written after this text land inside the region? Only
            // for a comment, and only one that is still open at the end: a line
            // comment nothing terminated, or a `/*` with no `*/`. A closed block
            // comment that happens to sit last is not a hazard.
            open_comment = skip_comment(b, i, dialect).is_some()
                && j >= b.len()
                && !(b[i] == b'/' && j >= i + 4 && b[j - 2] == b'*' && b[j - 1] == b'/');
            if j < b.len() {
                last_code = None;
            }
            i = j.max(i + 1);
            continue;
        }
        if !b[i].is_ascii_whitespace() {
            last_code = Some(b[i]);
        }
        open_comment = false;
        i += 1;
    }
    if open_comment {
        // The `;` cannot follow on this line — the comment runs to the end.
        return format!("{t}\n;");
    }
    match last_code {
        Some(b';') => t.to_string(),
        _ => format!("{t};"),
    }
}

/// The index of the `)` that closes the `(` at `start`, or `None` when it is
/// never closed (or `start` isn't an open paren at all).
///
/// Nesting counts; a paren inside a string, quoted identifier or comment does
/// not, because every step goes through [`skip_noncode`] — `name <> ')'` carries
/// a close-paren in a literal, and a raw byte scan reads it as the end of the
/// group.
///
/// This is the shared form of a scan that had grown three copies: `ddl`'s
/// `peel_parens` (correct — it went through this lexer), and `pg`'s
/// `pg_trigger_when`/`pg_trigger_args` (hand-rolled, aware of `'` only, so a
/// `"it's"` identifier latched the scanner into a string that never ended).
/// Returning the index rather than the slice is what lets a caller keep the
/// offsets it needs into its own buffer.
pub fn balanced_paren_span(b: &[u8], start: usize, dialect: SqlDialect) -> Option<usize> {
    if b.get(start) != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut i = start;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j;
            continue;
        }
        match b[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The index of the first occurrence of `needle` at a *code* position — outside
/// every string, quoted identifier and comment.
///
/// The plain `str::find`/`rfind` this replaces cannot tell the keyword it is
/// looking for from the same bytes inside a literal argument, which is how
/// `EXECUTE FUNCTION audit_fn('EXECUTE FUNCTION x(', 'b')` came apart.
pub fn find_code(hay: &str, needle: &str, dialect: SqlDialect) -> Option<usize> {
    let b = hay.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j;
            continue;
        }
        // Byte comparison, not `hay[i..].starts_with` — `i` walks bytes and a
        // multi-byte character elsewhere in the input would make that slice
        // panic on a char boundary.
        if b[i..].starts_with(needle.as_bytes()) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// A `DELIMITER <token>` directive starting at `i`: the offset just past its
/// line, and the token it sets.
///
/// **A client directive, not SQL** — the server has never heard of it — and
/// MySQL's alone. It exists because a compound body (`BEGIN … END`) holds its own
/// semicolons, so `mysqldump` and every hand-written trigger script switch the
/// terminator around them; a splitter that doesn't know the word cuts such a
/// script into fragments that are each a syntax error. It is recognised only at
/// the start of a statement, so `SELECT delimiter FROM t` is untouched.
fn delimiter_directive(sql: &str, i: usize, dialect: SqlDialect) -> Option<(usize, String)> {
    if !dialect.delimiter_directive() {
        return None;
    }
    let b = sql.as_bytes();
    const KW: &[u8] = b"DELIMITER";
    if b.len() - i < KW.len() + 1
        || !b[i..i + KW.len()]
            .iter()
            .zip(KW)
            .all(|(x, y)| x.eq_ignore_ascii_case(y))
        || !b[i + KW.len()].is_ascii_whitespace()
    {
        return None;
    }
    let mut j = i + KW.len();
    while j < b.len() && b[j] != b'\n' && b[j].is_ascii_whitespace() {
        j += 1;
    }
    let start = j;
    while j < b.len() && !b[j].is_ascii_whitespace() {
        j += 1;
    }
    if start == j {
        return None;
    }
    let token = sql[start..j].to_string();
    // The rest of the line belongs to the directive, whatever it is.
    let end = sql[j..]
        .find('\n')
        .map(|k| j + k + 1)
        .unwrap_or_else(|| sql.len());
    Some((end, token))
}

/// Is `sql[lo..hi]` a `DELIMITER` directive rather than a statement?
///
/// Exposed so the callers that *execute* ranges can drop it: the server would
/// answer a syntax error, since it is the client that owns the word.
pub fn is_delimiter_directive(sql: &str, lo: usize, hi: usize, dialect: SqlDialect) -> bool {
    delimiter_directive(sql, lo, dialect).is_some_and(|(end, _)| end >= hi)
}

/// Where a scan through a SQLite statement stands with respect to a
/// `CREATE TRIGGER` body.
///
/// SQLite is the one engine whose statements can *contain* `;` with no way to
/// say so: a trigger body is a `BEGIN … END` block of whole statements, and
/// SQLite has no `DELIMITER` directive to hide them behind. Its own shell solves
/// this in `sqlite3_complete()` by tracking the block, and so does this.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TriggerScan {
    /// At the start of a segment, still able to become a trigger.
    Start,
    /// `CREATE` seen; `TEMP`/`TEMPORARY` may follow before `TRIGGER` does.
    Create,
    /// Inside a `CREATE TRIGGER`, `depth` block openers deep. A `;` splits only
    /// at depth zero — which is the `;` after the body's own `END`.
    In { depth: u32 },
    /// This segment is something else; stop looking until the next one.
    No,
}

impl TriggerScan {
    /// Advance on one **code** word. `BEGIN` and `CASE` both open a block that
    /// `END` closes — counting openers rather than stopping at the first `END`
    /// is what keeps a `CASE … END` inside the body from ending the statement.
    fn word(self, w: &str) -> TriggerScan {
        let is = |k: &str| w.eq_ignore_ascii_case(k);
        match self {
            TriggerScan::Start if is("CREATE") => TriggerScan::Create,
            // `TEMP`/`TEMPORARY` sit between `CREATE` and `TRIGGER`; `IF NOT
            // EXISTS` sits after it and needs nothing here.
            TriggerScan::Create if is("TEMP") || is("TEMPORARY") => TriggerScan::Create,
            TriggerScan::Create if is("TRIGGER") => TriggerScan::In { depth: 0 },
            TriggerScan::In { depth } if is("BEGIN") || is("CASE") => {
                TriggerScan::In { depth: depth + 1 }
            }
            TriggerScan::In { depth } => TriggerScan::In {
                depth: if is("END") {
                    depth.saturating_sub(1)
                } else {
                    depth
                },
            },
            TriggerScan::No => TriggerScan::No,
            // Any other leading word: not a trigger, and nothing later in the
            // segment can make it one.
            _ => TriggerScan::No,
        }
    }

    /// Is a `;` here inside a trigger body rather than the end of a statement?
    fn inside_body(self) -> bool {
        matches!(self, TriggerScan::In { depth } if depth > 0)
    }
}

/// Byte offsets bounding each top-level statement: `[0, after-`;`, …, len]`.
/// `;` inside strings / identifiers / comments does not split, on MySQL a
/// `DELIMITER` directive changes what does (see [`delimiter_directive`]), and on
/// SQLite a `;` inside a `CREATE TRIGGER`'s `BEGIN … END` block does not split
/// either (see [`TriggerScan`]).
pub fn statement_bounds(sql: &str, dialect: SqlDialect) -> Vec<usize> {
    let b = sql.as_bytes();
    let n = b.len();
    let mut bounds = vec![0usize];
    let mut delim: Vec<u8> = vec![b';'];
    let mut i = 0;
    // Where the current segment begins, for recognising a directive only at the
    // start of one.
    let mut seg = 0usize;
    // SQLite alone. The other two engines keep exactly the boundaries they had —
    // MySQL's trigger bodies go behind `DELIMITER`, and changing that would
    // silently alter what Run Everything sends to a server.
    let track_triggers = dialect == SqlDialect::Sqlite;
    let mut scan = TriggerScan::Start;
    while i < n {
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j;
            continue;
        }
        // Only at the start of a segment — `SELECT delimiter FROM t` is data.
        if (seg..i).all(|k| b[k].is_ascii_whitespace())
            && let Some((end, token)) = delimiter_directive(sql, i, dialect)
        {
            bounds.push(end);
            delim = token.into_bytes();
            i = end;
            seg = end;
            scan = TriggerScan::Start;
            continue;
        }
        // The word walk is SQLite's only, and it is safe to skip a whole word
        // here because no word can contain the delimiter.
        if track_triggers && is_word_start(b[i]) {
            let start = i;
            let mut end = i + 1;
            while end < n && is_word_byte(b[end]) {
                end += 1;
            }
            scan = scan.word(&sql[start..end]);
            i = end;
            continue;
        }
        if b[i..].starts_with(&delim) {
            if scan.inside_body() {
                i += delim.len();
                continue;
            }
            bounds.push(i + delim.len());
            i += delim.len();
            seg = i;
            scan = TriggerScan::Start;
            continue;
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
        .filter(|&(lo, hi)| {
            lo < hi
                && segment_has_code(sql, lo, hi, dialect)
                // The directive is the client's, not the server's.
                && !is_delimiter_directive(sql, lo, hi, dialect)
        })
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

/// The database a `USE db` statement switches to, or `None` for anything else.
///
/// MySQL only — PostgreSQL has no `USE`, and a `USE` there is a syntax error the
/// server refuses, so nothing to track.
///
/// Why this exists: `run_batch` computes the scope **once** before the loop and
/// stamps it on every result, on a method whose own doc advertises that a `USE`
/// carries across statements. So Run Everything on `USE sakila; SELECT * FROM
/// actor;` from a tab scoped to `world` ran statement 2 in `sakila` and labelled
/// its result `world` — the stats line lying in exactly the case the label
/// exists to catch. `Session::fetch_query` has the same shape against an
/// immutable pinned name.
///
/// Deliberately conservative. It reads the **one** identifier after the keyword
/// and refuses anything else, so `USE` with a variable, an expression, or
/// trailing junk answers `None` — and the caller drops the label rather than
/// printing a name it isn't sure of. A missing label says nothing; a wrong one
/// is a new class of wrong, which is the whole defect being fixed.
///
/// The identifier goes through [`skip_noncode`], so a backtick-quoted name is
/// lifted out whole and unquoted (`` USE `my db` `` → `my db`), and a comment
/// between the keyword and the name is skipped.
pub fn use_target(sql: &str, dialect: SqlDialect) -> Option<String> {
    if dialect != SqlDialect::MySql || leading_keyword(sql, dialect)? != "USE" {
        return None;
    }
    let b = sql.as_bytes();
    let n = b.len();
    let mut i = leading_keyword_end(sql, dialect)?;
    loop {
        while i < n && b[i].is_ascii_whitespace() {
            i += 1;
        }
        // `skip_comment` indexes `b[i]` unguarded, so the end of input has to be
        // checked here.
        match (i < n).then(|| skip_comment(b, i, dialect)).flatten() {
            Some(j) => i = j,
            None => break,
        }
    }
    let (name, mut i) = if i < n && b[i] == b'`' {
        // `` `a``b` `` — a doubled backtick is one literal backtick.
        let end = skip_noncode(b, i, dialect)?;
        (sql[i + 1..end - 1].replace("``", "`"), end)
    } else if i < n && (b[i].is_ascii_alphabetic() || b[i] == b'_' || b[i] >= 0x80) {
        let s = i;
        let mut j = i + 1;
        while j < n && is_word_byte(b[j]) {
            j += 1;
        }
        (sql[s..j].to_string(), j)
    } else {
        return None;
    };
    // Nothing may follow but whitespace, a comment, and a terminating `;`.
    loop {
        while i < n && b[i].is_ascii_whitespace() {
            i += 1;
        }
        // `skip_comment` indexes `b[i]` unguarded, so the end of input has to be
        // checked here.
        match (i < n).then(|| skip_comment(b, i, dialect)).flatten() {
            Some(j) => i = j,
            None => break,
        }
    }
    if i < n && b[i] == b';' {
        i += 1;
        while i < n && b[i].is_ascii_whitespace() {
            i += 1;
        }
    }
    (i == n && !name.is_empty()).then_some(name)
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

/// The connection + settings state the write guards read.
#[derive(Clone, Copy, Debug)]
pub struct GuardPolicy {
    /// The connection is marked read-only: a write is blocked outright.
    pub read_only: bool,
    /// "Confirm before running writes" — a soft confirmation on any write/DDL.
    pub confirm_writes: bool,
    pub dialect: SqlDialect,
    /// The tab has no database selected. On PostgreSQL that is not the same as
    /// "nowhere to run": the connection falls back to a *maintenance* database
    /// (`postgres`, then the user's own, then `template1`), which is hidden from
    /// the schema tree — so an unscoped `CREATE TABLE` succeeds into a database
    /// Schemaic can never show again, and one landing in `template1` is inherited
    /// by every database created afterwards. See [`needs_database`].
    pub no_database: bool,
}

/// What the write guards say about a run request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunVerdict {
    /// Nothing stands in the way — run it.
    Allow,
    /// Refused, with **no override**. The read-only connection block: the user
    /// marked this connection read-only, and the product deliberately offers no
    /// "Run anyway" for it.
    Block(String),
    /// Held back with an override available ("Run anyway") — the missing-`WHERE`
    /// warning, or `confirm_writes`.
    Confirm(String),
}

/// Does `sql` need a database to run *in*, as opposed to being a server-level or
/// read-only statement that is fine without one?
///
/// Only PostgreSQL asks. On MySQL a connection with no database has none, and the
/// server answers with ERROR 1046; on PostgreSQL every connection is inside some
/// database, so with none selected the statement silently lands in the hidden
/// maintenance one. The natural first-run sequence on a fresh server —
/// `CREATE DATABASE app;` then `CREATE TABLE …` — is exactly the shape that
/// breaks: the first statement is correct and *needs* the maintenance
/// connection, the second creates a table nothing in Schemaic can reach again.
///
/// So: server-level `CREATE`/`DROP`/`ALTER DATABASE` (and the other cluster-wide
/// objects, which don't live in a database either) are allowed, as is anything
/// that can't leave a persistent object behind. Everything else is refused.
pub fn needs_database(sql: &str, dialect: SqlDialect) -> bool {
    if dialect != SqlDialect::Postgres {
        return false;
    }
    let Some(kw) = leading_keyword(sql, dialect) else {
        return false; // empty, or comments only — nothing will run
    };
    // Reads and session/transaction control can't create anything that outlives
    // the connection, so where they run doesn't matter. A read of a table that
    // isn't there fails on its own, which is a clearer error than ours.
    if matches!(
        kw.as_str(),
        "SELECT"
            | "SHOW"
            | "EXPLAIN"
            | "VALUES"
            | "TABLE"
            | "BEGIN"
            | "START"
            | "COMMIT"
            | "END"
            | "ROLLBACK"
            | "ABORT"
            | "SAVEPOINT"
            | "RELEASE"
            | "SET"
            | "RESET"
            | "DISCARD"
            | "LISTEN"
            | "UNLISTEN"
            | "NOTIFY"
            | "CHECKPOINT"
    ) {
        return false;
    }
    if matches!(kw.as_str(), "CREATE" | "DROP" | "ALTER") {
        let rest = leading_keyword_end(sql, dialect)
            .map(|e| &sql[e..])
            .unwrap_or("");
        let obj = leading_keyword(rest, dialect).unwrap_or_default();
        // Cluster-wide objects: they don't live in a database, so they are the
        // statements a database-less tab exists to run.
        return !matches!(
            obj.as_str(),
            "DATABASE" | "ROLE" | "USER" | "GROUP" | "TABLESPACE"
        );
    }
    true
}

/// The write guard, as one decision over the statements about to run.
///
/// This is *the* answer to "may this run", and it exists as a function because
/// it used to be two closures inside the editor pane's view body — which meant
/// every other way of running SQL silently had no guard at all. The command
/// palette's `>run` and the AI chat's Insert & Run each reached the raw run
/// action and executed writes past all three protections, including the
/// read-only block that has no override by design. A guard living in one caller
/// of a shared path is a guard the next caller opts out of by omission.
///
/// `stmts` is what would execute: one element for a single statement, one per
/// statement for a batch (each element may itself hold several, which
/// [`first_unsafe`] and [`contains_write`] both handle). Order matters — the
/// hard block is checked before either soft one, so a read-only connection never
/// offers "Run anyway", and an unsafe missing-`WHERE` statement reports *that*
/// rather than the generic "modifies data".
pub fn run_verdict(stmts: &[String], policy: GuardPolicy) -> RunVerdict {
    let writes = || stmts.iter().any(|s| contains_write(s, policy.dialect));
    if policy.read_only && writes() {
        return RunVerdict::Block("Read-only connection.".to_string());
    }
    if policy.no_database && stmts.iter().any(|s| needs_database(s, policy.dialect)) {
        // MySQL's own message, deliberately: there it is the *server* that
        // refuses (ERROR 1046), because the connection simply carries no
        // database. PostgreSQL's carries a hidden one instead, so the refusal has
        // to come from here — and it should read the same either way.
        return RunVerdict::Block("No database selected.".to_string());
    }
    if let Some(message) = stmts.iter().find_map(|s| first_unsafe(s, policy.dialect)) {
        return RunVerdict::Confirm(message);
    }
    if policy.confirm_writes && writes() {
        return RunVerdict::Confirm(if stmts.len() == 1 {
            "This statement modifies data.".to_string()
        } else {
            "These statements modify data.".to_string()
        });
    }
    RunVerdict::Allow
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

/// The statement heads this dialect will accept as read-only, in the order they
/// should be listed to a reader.
///
/// **Per dialect, because the engines don't have the same statements.** This was
/// one shared list, and a third engine made it wrong in both directions at once:
/// it advertised `SHOW` and `DESCRIBE` to SQLite, which has neither, so the gate
/// passed a statement the engine then rejected with a raw parser error, and the
/// rejection message named heads the connection couldn't use. `DESCRIBE`/`DESC`
/// are MySQL's alone — PostgreSQL's equivalent is psql's `\d`, a client command
/// rather than SQL — while `SHOW` is real on PostgreSQL (`SHOW search_path`).
///
/// This is the **one** definition: the MCP server builds `run_query`'s advertised
/// description from it too, so what the model is told and what the gate enforces
/// cannot drift.
pub fn read_only_heads(dialect: SqlDialect) -> &'static [&'static str] {
    match dialect {
        SqlDialect::MySql => &["SELECT", "SHOW", "DESCRIBE", "DESC", "EXPLAIN", "WITH"],
        SqlDialect::Postgres => &["SELECT", "SHOW", "EXPLAIN", "WITH"],
        SqlDialect::Sqlite => &["SELECT", "EXPLAIN", "WITH"],
    }
}

/// Is `sql` a single read-only statement we're willing to run on the AI's
/// behalf? Returns the rejection reason on failure.
pub fn read_only_reason(sql: &str, dialect: SqlDialect) -> Result<(), String> {
    let (words, multi) = word_tokens(sql, dialect);
    if multi {
        return Err("only a single statement is allowed".to_string());
    }
    let heads = read_only_heads(dialect);
    let head = words.first().map(|s| s.as_str()).unwrap_or("");
    if !heads.contains(&head) {
        // Naming this engine's heads, not a union of all three: a model told it
        // may `SHOW` on SQLite will keep trying.
        return Err(format!(
            "only read-only queries ({}) are allowed",
            heads.join("/")
        ));
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
mod ident_at_tests {
    use super::ident_at;
    use crate::intel::SqlDialect::{MySql, Postgres, Sqlite};

    fn name(sql: &str) -> Option<String> {
        ident_at(sql, 0, Sqlite).0
    }

    /// All four spellings SQLite accepts, and the doubling rule for the three
    /// that have one.
    #[test]
    fn every_sqlite_quoting_reads_back_unquoted() {
        assert_eq!(name(r#""x""#).as_deref(), Some("x"));
        assert_eq!(name("`x`").as_deref(), Some("x"));
        assert_eq!(name("[x]").as_deref(), Some("x"));
        assert_eq!(name("'x'").as_deref(), Some("x"));
        assert_eq!(name(r#""a""b""#).as_deref(), Some("a\"b"));
        assert_eq!(name("`a``b`").as_deref(), Some("a`b"));
        assert_eq!(name("'a''b'").as_deref(), Some("a'b"));
        // `[…]` has no escape at all: the content runs to the first `]`.
        assert_eq!(name("[a]]b]").as_deref(), Some("a"));
    }

    /// **A digit cannot begin a name**, which the copy this replaced did not
    /// check: `CONSTRAINT 3way` read back a constraint called `3way`.
    #[test]
    fn a_bare_name_cannot_start_with_a_digit() {
        assert_eq!(name("3way"), None);
        assert_eq!(name("way3").as_deref(), Some("way3"));
        assert_eq!(name("_x").as_deref(), Some("_x"));
        // A byte >= 0x80 is a word byte and a word start — the identifier rule
        // this project states.
        assert_eq!(name("é").as_deref(), Some("é"));
    }

    #[test]
    fn leading_space_is_skipped_and_nothing_is_nothing() {
        assert_eq!(ident_at("   x", 0, Sqlite).0.as_deref(), Some("x"));
        assert_eq!(ident_at("", 0, Sqlite), (None, 0));
        assert_eq!(ident_at("   ", 0, Sqlite).0, None);
        assert_eq!(name("("), None);
    }

    /// The offset is just past the name, so a scanner can carry on from it.
    #[test]
    fn the_offset_lands_past_the_name() {
        assert_eq!(ident_at("CONSTRAINT ck CHECK", 10, Sqlite).1, 13);
        assert_eq!(ident_at(r#" "ck" CHECK"#, 0, Sqlite).1, 5);
    }

    /// **Which byte quotes a name is the dialect's answer, not this function's.**
    /// MySQL reads `"` as a *string* and PostgreSQL has no backtick, so neither
    /// may take the other's quoting as a name.
    #[test]
    fn the_quoting_is_the_dialects() {
        assert_eq!(ident_at("`x`", 0, MySql).0.as_deref(), Some("x"));
        assert_eq!(ident_at(r#""x""#, 0, MySql).0, None, "a string on MySQL");
        assert_eq!(ident_at(r#""x""#, 0, Postgres).0.as_deref(), Some("x"));
        assert_eq!(ident_at("`x`", 0, Postgres).0, None);
        assert_eq!(ident_at("[x]", 0, Postgres).0, None);
    }
}

#[cfg(test)]
mod terminated_tests {
    use super::terminated;
    use crate::intel::SqlDialect::{MySql, Sqlite};

    #[test]
    fn an_ordinary_statement_gets_one_semicolon() {
        assert_eq!(terminated("SELECT 1", Sqlite), "SELECT 1;");
        assert_eq!(terminated("SELECT 1  \n", Sqlite), "SELECT 1;");
        assert_eq!(terminated("", Sqlite), ";");
    }

    /// Idempotent, because callers string statements together and a double `;`
    /// is an empty statement some clients refuse.
    #[test]
    fn a_terminated_statement_is_returned_as_it_is() {
        assert_eq!(terminated("SELECT 1;", Sqlite), "SELECT 1;");
        assert_eq!(terminated("SELECT 1; \n", Sqlite), "SELECT 1;");
        assert_eq!(
            terminated(&terminated("SELECT 1", Sqlite), Sqlite),
            "SELECT 1;"
        );
    }

    /// **The one this exists for.** SQLite keeps the author's own trailing
    /// comment in `sqlite_master.sql`, so trimming and appending put the `;`
    /// *inside* it — and the next statement in the script joined the comment.
    #[test]
    fn a_semicolon_never_lands_inside_a_trailing_comment() {
        assert_eq!(
            terminated("CREATE INDEX ia ON t(a) -- why this index exists", Sqlite),
            "CREATE INDEX ia ON t(a) -- why this index exists\n;"
        );
        // An unclosed block comment behaves the same way and used to be missed
        // by a `--`-only fix.
        assert_eq!(
            terminated("CREATE INDEX ia ON t(a) /* unclosed", Sqlite),
            "CREATE INDEX ia ON t(a) /* unclosed\n;"
        );
        // A *closed* comment is not a hazard: code follows it, or nothing does.
        assert_eq!(
            terminated("CREATE INDEX ia ON t(a) /* why */", Sqlite),
            "CREATE INDEX ia ON t(a) /* why */;"
        );
        // A comment that already had its terminator after it is untouched.
        assert_eq!(
            terminated("CREATE INDEX ia ON t(a) -- why\n;", Sqlite),
            "CREATE INDEX ia ON t(a) -- why\n;"
        );
    }

    /// The `;` and the comment marker have to be read as *code*, not as bytes:
    /// both can sit inside a literal.
    #[test]
    fn a_semicolon_or_a_dash_inside_a_literal_is_data() {
        assert_eq!(
            terminated("INSERT INTO t VALUES ('a;')", Sqlite),
            "INSERT INTO t VALUES ('a;');"
        );
        assert_eq!(
            terminated("INSERT INTO t VALUES ('-- not a comment')", Sqlite),
            "INSERT INTO t VALUES ('-- not a comment');"
        );
        // MySQL's `#` comment is the dialect's business, not this function's.
        assert_eq!(terminated("SELECT 1 # why", MySql), "SELECT 1 # why\n;");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intel::SqlDialect;

    /// Every engine this build ships. A test whose expected answer does not
    /// depend on the dialect should loop over this rather than pick one, so a
    /// fourth engine is added to the suite by adding it here.
    const EVERY_DIALECT: [SqlDialect; 3] =
        [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite];

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

    // ── SQLite boundaries ────────────────────────────────────────────────────
    // Every one of these passed *the wrong way* before the capability table
    // existed, and none of them would have failed to compile: `!= Postgres` and
    // `== Postgres` each sort a third engine silently. They are written as
    // separate named tests rather than one sweep so a regression says which rule
    // broke.

    /// SQLite has **no** backslash escape in a string, so `'a\'` ends at its own
    /// quote. Under MySQL's rule the scanner reads `\'` as escaped, runs past the
    /// end of the literal and swallows the rest of the statement — which is how a
    /// `WHERE` disappears from the unsafe-statement guard. This is the dangerous
    /// one.
    #[test]
    fn sqlite_has_no_backslash_escape_in_a_string() {
        let s = br"'C:\' , x";
        let end = super::skip_noncode(s, 0, SqlDialect::Sqlite).expect("a string");
        assert_eq!(&s[..end], br"'C:\'", "the literal ends at its own quote");
        // MySQL genuinely differs here — the contrast is the point.
        let my = super::skip_noncode(s, 0, SqlDialect::MySql).expect("a string");
        assert!(my > end, "MySQL keeps scanning past the escaped quote");
    }

    /// The consequence, at the level the guard actually works on: a `DELETE`
    /// whose `WHERE` follows a path literal is only safe if the literal ended.
    #[test]
    fn a_windows_path_literal_does_not_hide_a_sqlite_where() {
        let sql = r"DELETE FROM files WHERE dir = 'C:\' AND id > 0";
        assert!(super::has_top_level_where(sql, SqlDialect::Sqlite));
        assert_eq!(super::unsafe_reason(sql, SqlDialect::Sqlite), None);
    }

    /// SQLite follows the standard: `--` opens a comment with no whitespace after
    /// it, where MySQL needs some and reads `1--2` as arithmetic.
    #[test]
    fn sqlite_dash_comment_needs_no_whitespace() {
        let s = b"1--2\nx";
        assert_eq!(super::skip_noncode(s, 1, SqlDialect::Sqlite), Some(4));
        assert_eq!(super::skip_noncode(s, 1, SqlDialect::MySql), None);
    }

    /// `#` is not a comment in SQLite — it isn't even valid there — so treating it
    /// as one would swallow a line the engine would have rejected.
    #[test]
    fn sqlite_hash_is_not_a_comment() {
        let s = b"#nope\nx";
        assert_eq!(super::skip_noncode(s, 0, SqlDialect::Sqlite), None);
        assert_eq!(super::skip_noncode(s, 0, SqlDialect::MySql), Some(5));
    }

    /// SQLite accepts all three identifier quotings, which no other engine here
    /// does: `"x"` (standard), `` `x` `` (MySQL compatibility) and `[x]`
    /// (SQL-Server compatibility).
    #[test]
    fn sqlite_takes_all_three_identifier_quotings() {
        // Each closer sits at index 7, so the span ends at 8.
        for (src, end) in [
            (&br#""my tbl" x"#[..], 8),
            (&b"`my tbl` x"[..], 8),
            (&b"[my tbl] x"[..], 8),
        ] {
            assert_eq!(
                super::skip_noncode(src, 0, SqlDialect::Sqlite),
                Some(end),
                "{}",
                String::from_utf8_lossy(src)
            );
        }
    }

    /// A bracketed identifier has no escape, so the span ends at the first `]` —
    /// and, more to the point, the space inside it must not split a statement or
    /// end a word.
    #[test]
    fn a_bracketed_identifier_does_not_split_a_statement() {
        let sql = "SELECT * FROM [my; tbl]; SELECT 2;";
        let r = super::statement_ranges(sql, SqlDialect::Sqlite);
        assert_eq!(r.len(), 2, "{:?}", r);
        assert_eq!(&sql[r[0].0..r[0].1], "SELECT * FROM [my; tbl];");
    }

    /// Brackets are SQLite's alone: on the other engines `[` is ordinary code, so
    /// the same text splits where its semicolons are.
    #[test]
    fn brackets_are_not_identifiers_on_the_other_engines() {
        let sql = "SELECT * FROM [my; tbl]; SELECT 2;";
        assert_eq!(super::statement_ranges(sql, SqlDialect::MySql).len(), 3);
        assert_eq!(super::statement_ranges(sql, SqlDialect::Postgres).len(), 3);
    }

    /// `$` is a parameter sigil in SQLite, not a dollar-quote: `$tag$` must stay
    /// ordinary code, or everything after it is swallowed as a string.
    #[test]
    fn sqlite_has_no_dollar_quoting() {
        let s = b"$tag$ x $tag$";
        assert_eq!(super::skip_noncode(s, 0, SqlDialect::Sqlite), None);
        assert_eq!(super::skip_noncode(s, 0, SqlDialect::Postgres), Some(13));
    }

    /// `DELIMITER` is MySQL's client-side word. SQLite has no such directive, so
    /// the line is just the head of the statement that follows — the call
    /// PostgreSQL already gets.
    ///
    /// It asserts the *terminator actually moves*, not just a statement count:
    /// the obvious `DELIMITER $$\nSELECT 1;` splits into one range either way (with
    /// the terminator changed there is simply no `$$` to split on), so a count
    /// there is vacuous — which a flip of the capability proved.
    #[test]
    fn sqlite_has_no_delimiter_directive() {
        let sql = "DELIMITER $$\nSELECT 1;";
        assert!(super::is_delimiter_directive(sql, 0, 13, SqlDialect::MySql));
        assert!(!super::is_delimiter_directive(
            sql,
            0,
            13,
            SqlDialect::Sqlite
        ));
        // And the consequence: `$$` terminates a statement only where the
        // directive was honoured.
        let script = "DELIMITER $$\nSELECT 1$$ SELECT 2$$";
        assert_eq!(super::statement_ranges(script, SqlDialect::MySql).len(), 2);
        assert_eq!(super::statement_ranges(script, SqlDialect::Sqlite).len(), 1);
    }

    /// A MySQL compound trigger body holds its own semicolons, so a script
    /// carrying one is only splittable with `DELIMITER` — the form `mysqldump`
    /// writes, and the form the DDL preview now hands to "Open in editor".
    #[test]
    fn a_delimiter_directive_moves_the_statement_terminator() {
        let s = "DELIMITER $$\n\nCREATE TRIGGER t BEFORE INSERT ON o FOR EACH ROW\nBEGIN\n  \
                 SET NEW.a = 1;\n  SET NEW.b = 2;\nEND$$\n\nDELIMITER ;";
        let r = statement_ranges(s);
        assert_eq!(
            r.len(),
            1,
            "{:?}",
            r.iter().map(|&(a, b)| &s[a..b]).collect::<Vec<_>>()
        );
        assert!(s[r[0].0..r[0].1].starts_with("CREATE TRIGGER"));
        assert!(s[r[0].0..r[0].1].ends_with("END$$"));
    }

    /// Without it, the same body is cut into fragments — which is the bug, and
    /// is also why the directive can't just be ignored.
    #[test]
    fn the_same_body_without_a_delimiter_is_still_split_on_semicolons() {
        let s = "CREATE TRIGGER t BEFORE INSERT ON o FOR EACH ROW\nBEGIN\n  SET NEW.a = 1;\n  \
                 SET NEW.b = 2;\nEND;";
        assert_eq!(statement_ranges(s).len(), 3);
    }

    #[test]
    fn delimiter_is_only_a_directive_at_the_start_of_a_statement() {
        // A column called `delimiter`, and the word inside a statement, are data.
        let s = "SELECT delimiter FROM t; SELECT 2;";
        assert_eq!(statement_ranges(s).len(), 2);
        // PostgreSQL has no such directive at all, so the line is just the head
        // of the one statement that follows it.
        let pg = "DELIMITER $$\nSELECT 1;";
        assert_eq!(super::statement_ranges(pg, SqlDialect::Postgres).len(), 1);
        // Restoring `;` puts the ordinary terminator back.
        let s = "DELIMITER $$\nSELECT 1$$\nDELIMITER ;\nSELECT 2;\nSELECT 3;";
        let r = statement_ranges(s);
        assert_eq!(r.len(), 3, "{:?}", r);
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

    /// Every dialect, because a verdict that does *not* depend on the engine must
    /// not be proved on one: the gate is the only guard on AI-issued SQL and it
    /// is dialect-parameterised, so a fourth engine should inherit this suite
    /// rather than the single dialect the helper above happens to bind.
    #[test]
    fn read_only_gate_blocks_bypasses() {
        for d in EVERY_DIALECT {
            let gate = |s: &str| super::read_only_reason(s, d);
            assert!(gate("SELECT * FROM t").is_ok(), "{d:?}");
            assert!(
                gate("WITH c AS (SELECT 1) SELECT * FROM c").is_ok(),
                "{d:?}"
            );
            // CTE that hides a DELETE.
            assert!(gate("WITH c AS (SELECT 1) DELETE FROM t").is_err(), "{d:?}");
            // EXPLAIN ANALYZE actually executes the statement.
            assert!(gate("EXPLAIN ANALYZE DELETE FROM t").is_err(), "{d:?}");
            // SELECT … INTO OUTFILE writes files on the DB host.
            assert!(
                gate("SELECT * FROM t INTO OUTFILE '/tmp/x'").is_err(),
                "{d:?}"
            );
            // SLEEP / locks.
            assert!(gate("SELECT SLEEP(10)").is_err(), "{d:?}");
            // Multi-statement.
            assert!(gate("SELECT 1; DROP TABLE t").is_err(), "{d:?}");
            // A dangerous word inside a *standard* string is inert everywhere.
            assert!(gate("SELECT 'delete from t'").is_ok(), "{d:?}");
        }
        // The backtick is not standard, so this one is deliberately not in the
        // sweep — see `the_gate_reads_this_engines_identifier_quoting`.
        assert!(read_only_reason("SELECT `update` FROM t").is_ok());
    }

    /// The allowed heads are the ones the *engine* has. `SHOW` and `DESCRIBE`
    /// were allowed on all three, which let the gate wave through a statement
    /// SQLite has no syntax for at all — the model then got a raw parser error
    /// instead of being told the engine has no such thing.
    #[test]
    fn the_read_only_heads_are_the_ones_the_engine_actually_has() {
        use super::read_only_reason as gate;
        // Every engine reads with these.
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert!(gate("SELECT 1", d).is_ok(), "{d:?} SELECT");
            assert!(
                gate("WITH c AS (SELECT 1) SELECT * FROM c", d).is_ok(),
                "{d:?} WITH"
            );
            assert!(gate("EXPLAIN SELECT 1", d).is_ok(), "{d:?} EXPLAIN");
        }
        // `SHOW` is MySQL's and PostgreSQL's (`SHOW search_path`); SQLite has none.
        assert!(gate("SHOW TABLES", SqlDialect::MySql).is_ok());
        assert!(gate("SHOW search_path", SqlDialect::Postgres).is_ok());
        assert!(gate("SHOW TABLES", SqlDialect::Sqlite).is_err());
        // `DESCRIBE`/`DESC` are MySQL's alone — psql's `\d` is a client command,
        // not SQL, and SQLite has nothing of the kind.
        assert!(gate("DESCRIBE t", SqlDialect::MySql).is_ok());
        assert!(gate("DESC t", SqlDialect::MySql).is_ok());
        assert!(gate("DESCRIBE t", SqlDialect::Postgres).is_err());
        assert!(gate("DESCRIBE t", SqlDialect::Sqlite).is_err());
    }

    // ── The gate's lexer half, per dialect ───────────────────────────────────
    // `c6c5dae` fixed a real bypass — a SQLite connection was gated with
    // `SqlDialect::MySql` — and its test asserts only that `dialect_of` returns
    // the right enum. These pin the behaviour that made the mis-pairing matter:
    // the gate's answer changes with the dialect *before* it reaches the head
    // list, because where a statement ends is a dialect question.

    /// **The payload the bypass used.** MySQL has a backslash escape in a string
    /// and the other two do not, so `'a\'` ends the literal everywhere except
    /// MySQL — where the scanner runs on and the `; DELETE` is swallowed into it.
    /// Gated as MySQL, a SQLite connection was handed a statement that runs both
    /// halves; measured against a live SQLite, the DELETE emptied the table.
    #[test]
    fn the_gate_reads_this_engines_string_escape() {
        let payload = r"SELECT 'a\' ; DELETE FROM s; --'";
        for d in [SqlDialect::Sqlite, SqlDialect::Postgres] {
            let err = super::read_only_reason(payload, d)
                .expect_err("two statements, because the literal ends at its own quote");
            assert!(err.contains("single statement"), "{d:?}: {err}");
        }
        // MySQL's answer is different *and correct there*: it really is one
        // statement, because the engine reads `\'` as an escaped quote.
        assert!(super::read_only_reason(payload, SqlDialect::MySql).is_ok());
    }

    /// `#` opens a comment on MySQL alone. Hiding a `DELETE` behind one is a read
    /// on MySQL and a rejected write everywhere else — the same text, two honest
    /// answers, and the wrong dialect picks the wrong one.
    #[test]
    fn the_gate_reads_this_engines_comment_rule() {
        let hidden = "SELECT 1 # DELETE FROM t";
        assert!(super::read_only_reason(hidden, SqlDialect::MySql).is_ok());
        for d in [SqlDialect::Sqlite, SqlDialect::Postgres] {
            let err = super::read_only_reason(hidden, d).expect_err("`#` is not a comment here");
            assert!(err.contains("DELETE"), "{d:?}: {err}");
        }
        // A `--` comment is every engine's, and the newline ends it on all three,
        // so what follows is code and the deny scan sees it.
        for d in EVERY_DIALECT {
            assert!(
                super::read_only_reason("SELECT 1 -- x\n, (SELECT DROP)", d).is_err(),
                "{d:?}"
            );
        }
    }

    /// Which quotings make a keyword inert is the engine's business too:
    /// backticks are MySQL's and SQLite's, brackets are SQLite's alone, and a
    /// `;` inside one is not a statement break where the quoting is real.
    #[test]
    fn the_gate_reads_this_engines_identifier_quoting() {
        // A column literally called `update`.
        let backticked = "SELECT `update` FROM t";
        assert!(super::read_only_reason(backticked, SqlDialect::MySql).is_ok());
        assert!(super::read_only_reason(backticked, SqlDialect::Sqlite).is_ok());
        assert!(
            super::read_only_reason(backticked, SqlDialect::Postgres).is_err(),
            "PostgreSQL has no backtick, so the word is code"
        );
        // A bracketed name carrying a `;`: one statement on SQLite, two anywhere
        // the bracket is ordinary code.
        let bracketed = "SELECT * FROM [odd; name]";
        assert!(super::read_only_reason(bracketed, SqlDialect::Sqlite).is_ok());
        for d in [SqlDialect::MySql, SqlDialect::Postgres] {
            let err = super::read_only_reason(bracketed, d).expect_err("`[` is not a quote here");
            assert!(err.contains("single statement"), "{d:?}: {err}");
        }
    }

    /// The rejection names what *this* engine allows, so the model can retry with
    /// something that exists rather than re-reading a list that includes `SHOW`.
    #[test]
    fn the_rejection_lists_only_this_engines_heads() {
        let err = super::read_only_reason("DELETE FROM t", SqlDialect::Sqlite).unwrap_err();
        assert!(err.contains("SELECT"), "{err}");
        assert!(!err.contains("SHOW"), "SQLite has no SHOW: {err}");
        assert!(!err.contains("DESCRIBE"), "SQLite has no DESCRIBE: {err}");
        let err = super::read_only_reason("DELETE FROM t", SqlDialect::MySql).unwrap_err();
        assert!(err.contains("SHOW") && err.contains("DESCRIBE"), "{err}");
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

    // ── The write guard (`run_verdict`) ──
    //
    // These three protections used to live as two closures inside the editor
    // pane's view body, so the command palette's `>run` and the AI chat's
    // Insert & Run — both of which reached the raw run action — executed writes
    // past all of them. The point of the function is that there is now one
    // answer to "may this run", and it can be asserted without a GUI.

    /// Read-only off, confirm-writes off — nothing but the unsafe-WHERE net.
    fn open_policy() -> GuardPolicy {
        GuardPolicy {
            read_only: false,
            confirm_writes: false,
            dialect: SqlDialect::MySql,
            no_database: false,
        }
    }

    fn v(stmts: &[&str], policy: GuardPolicy) -> RunVerdict {
        let owned: Vec<String> = stmts.iter().map(|s| s.to_string()).collect();
        run_verdict(&owned, policy)
    }

    #[test]
    fn a_plain_select_runs_under_every_policy() {
        let sql = &["SELECT * FROM orders"];
        assert_eq!(v(sql, open_policy()), RunVerdict::Allow);
        assert_eq!(
            v(
                sql,
                GuardPolicy {
                    read_only: true,
                    confirm_writes: true,
                    ..open_policy()
                }
            ),
            RunVerdict::Allow,
            "read-only and confirm-writes are about writes; a read is neither"
        );
    }

    #[test]
    fn a_read_only_connection_blocks_a_write_with_no_override() {
        // The repro: `DELETE FROM orders;` on a connection marked read-only.
        // `Block` carries no pending run — the product offers no "Run anyway"
        // here on purpose, which is what made the palette's bypass worse than
        // the others.
        let p = GuardPolicy {
            read_only: true,
            ..open_policy()
        };
        assert_eq!(
            v(&["DELETE FROM orders WHERE id = 1"], p),
            RunVerdict::Block("Read-only connection.".to_string())
        );
        // Even one write among reads blocks the batch.
        assert_eq!(
            v(
                &["SELECT 1", "UPDATE t SET a = 1 WHERE id = 2", "SELECT 2"],
                p
            ),
            RunVerdict::Block("Read-only connection.".to_string())
        );
    }

    // ── No database selected (PostgreSQL's hidden maintenance database) ──

    /// The sequence a fresh server invites: create the database, then create a
    /// table in it. The first statement *needs* the maintenance connection; the
    /// second used to run on it too, creating a table inside `postgres` — which
    /// is filtered out of the schema tree, so it could never be reached again.
    #[test]
    fn the_first_run_sequence_on_an_empty_postgres_server() {
        assert!(!needs_database("CREATE DATABASE app", SqlDialect::Postgres));
        assert!(needs_database(
            "CREATE TABLE users (id serial primary key)",
            SqlDialect::Postgres
        ));
    }

    #[test]
    fn cluster_wide_objects_do_not_need_a_database() {
        for sql in [
            "CREATE DATABASE app",
            "DROP DATABASE IF EXISTS app",
            "ALTER DATABASE app OWNER TO bob",
            "CREATE ROLE app_rw LOGIN",
            "CREATE USER bob",
            "DROP TABLESPACE fast",
        ] {
            assert!(!needs_database(sql, SqlDialect::Postgres), "{sql}");
        }
    }

    #[test]
    fn anything_that_would_land_in_the_maintenance_database_needs_one() {
        for sql in [
            "CREATE TABLE users (id int)",
            "CREATE INDEX ix ON users (id)",
            "CREATE SCHEMA sales",
            "INSERT INTO users VALUES (1)",
            "UPDATE users SET id = 2",
            "DELETE FROM users",
            "TRUNCATE users",
            "GRANT SELECT ON users TO bob",
        ] {
            assert!(needs_database(sql, SqlDialect::Postgres), "{sql}");
        }
    }

    /// A read leaves nothing behind, and one against a table that isn't there
    /// fails on its own with a clearer message than ours.
    #[test]
    fn reads_and_session_control_run_without_a_database() {
        for sql in [
            "SELECT datname FROM pg_database",
            "  -- which server is this?\n SHOW server_version",
            "EXPLAIN SELECT 1",
            "BEGIN",
            "COMMIT",
            "SET search_path TO public",
        ] {
            assert!(!needs_database(sql, SqlDialect::Postgres), "{sql}");
        }
    }

    /// MySQL's connection genuinely has no database, and the server says so
    /// (ERROR 1046). Answering first would only add a second voice.
    #[test]
    fn mysql_leaves_the_refusal_to_its_server() {
        assert!(!needs_database(
            "CREATE TABLE users (id int)",
            SqlDialect::MySql
        ));
    }

    #[test]
    fn the_guard_blocks_a_database_less_run_with_no_override() {
        let p = GuardPolicy {
            dialect: SqlDialect::Postgres,
            no_database: true,
            ..open_policy()
        };
        assert_eq!(
            v(&["CREATE TABLE users (id int)"], p),
            RunVerdict::Block("No database selected.".to_string()),
            "the same message MySQL's server gives"
        );
        // The statement that fixes the situation still runs.
        assert_eq!(v(&["CREATE DATABASE app"], p), RunVerdict::Allow);
        // One offender in a batch stops the batch: the rest would run in the
        // maintenance database too.
        assert!(matches!(
            v(&["SELECT 1", "CREATE TABLE t (id int)"], p),
            RunVerdict::Block(_)
        ));
    }

    #[test]
    fn a_bound_database_gates_nothing() {
        let p = GuardPolicy {
            dialect: SqlDialect::Postgres,
            ..open_policy()
        };
        assert_eq!(v(&["CREATE TABLE users (id int)"], p), RunVerdict::Allow);
    }

    /// Read-only is the harder refusal and must be the one reported.
    #[test]
    fn a_read_only_connection_still_wins_over_the_missing_database() {
        let p = GuardPolicy {
            read_only: true,
            dialect: SqlDialect::Postgres,
            no_database: true,
            ..open_policy()
        };
        assert_eq!(
            v(&["CREATE TABLE users (id int)"], p),
            RunVerdict::Block("Read-only connection.".to_string())
        );
    }

    #[test]
    fn the_hard_block_wins_over_both_soft_ones() {
        // A read-only connection must not be offered "Run anyway" just because
        // the statement also trips the missing-WHERE net.
        let p = GuardPolicy {
            read_only: true,
            confirm_writes: true,
            ..open_policy()
        };
        assert!(matches!(
            v(&["DELETE FROM orders"], p),
            RunVerdict::Block(_)
        ));
    }

    #[test]
    fn a_missing_where_is_reported_ahead_of_the_generic_write_confirm() {
        // Both would fire; the specific message is the useful one.
        let p = GuardPolicy {
            confirm_writes: true,
            ..open_policy()
        };
        let RunVerdict::Confirm(msg) = v(&["DELETE FROM orders"], p) else {
            panic!("expected a confirm");
        };
        assert!(msg.contains("without WHERE"), "{msg}");
    }

    #[test]
    fn the_missing_where_net_fires_even_with_confirm_writes_off() {
        // It is not a setting — it is the net that catches the mistake.
        let RunVerdict::Confirm(msg) = v(&["UPDATE t SET a = 1"], open_policy()) else {
            panic!("expected a confirm");
        };
        assert!(msg.contains("without WHERE"), "{msg}");
        assert!(matches!(
            v(&["TRUNCATE TABLE t"], open_policy()),
            RunVerdict::Confirm(_)
        ));
    }

    #[test]
    fn confirm_writes_holds_back_an_otherwise_safe_write() {
        let p = GuardPolicy {
            confirm_writes: true,
            ..open_policy()
        };
        assert_eq!(
            v(&["INSERT INTO t VALUES (1)"], p),
            RunVerdict::Confirm("This statement modifies data.".to_string())
        );
        // …and says so in the plural for a batch.
        assert_eq!(
            v(&["SELECT 1", "INSERT INTO t VALUES (1)"], p),
            RunVerdict::Confirm("These statements modify data.".to_string())
        );
        // With the setting off, the same write is allowed straight through.
        assert_eq!(
            v(&["INSERT INTO t VALUES (1)"], open_policy()),
            RunVerdict::Allow
        );
    }

    #[test]
    fn a_data_modifying_cte_is_a_write_to_every_guard() {
        // The statement reads like a SELECT and writes; all three protections
        // have to see through it (this is A3-L5-01's statement).
        let cte = "WITH d AS (DELETE FROM orders RETURNING *) SELECT * FROM d";
        let pg = SqlDialect::Postgres;
        assert!(matches!(
            v(
                &[cte],
                GuardPolicy {
                    read_only: true,
                    dialect: pg,
                    ..open_policy()
                }
            ),
            RunVerdict::Block(_)
        ));
        assert!(matches!(
            v(
                &[cte],
                GuardPolicy {
                    confirm_writes: true,
                    dialect: pg,
                    ..open_policy()
                }
            ),
            RunVerdict::Confirm(_)
        ));
    }

    #[test]
    fn a_write_hidden_in_a_multi_statement_element_is_still_seen() {
        // A single "statement" handed to the guard may hold several — the
        // editor's Ctrl+Enter passes the text under the caret, and a paste can
        // be anything.
        let p = GuardPolicy {
            confirm_writes: true,
            ..open_policy()
        };
        assert!(matches!(
            v(&["SELECT 1; DELETE FROM orders WHERE id = 1"], p),
            RunVerdict::Confirm(_)
        ));
        // And the missing-WHERE net reaches into it too.
        assert!(matches!(
            v(&["SELECT 1; DELETE FROM orders"], open_policy()),
            RunVerdict::Confirm(_)
        ));
    }

    #[test]
    fn nothing_to_run_is_allowed_rather_than_guarded() {
        assert_eq!(v(&[], open_policy()), RunVerdict::Allow);
        assert_eq!(
            v(&["", "   ", "-- just a note"], open_policy()),
            RunVerdict::Allow
        );
    }

    // ── The word-byte rule (architecture invariant 11) ────────────────────

    #[test]
    fn word_byte_rule_holds_over_every_byte_value() {
        // Enumerated rather than sampled, because the invariant is about the
        // whole byte range and the clause that gets dropped is always the same
        // one: `>= 0x80`. Four copies of this predicate used to exist, none
        // testing the others, and the crate has already regressed and repaired
        // it once (the note in the completion layer says so).
        for b in 0u8..=255 {
            let expected = b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80;
            assert_eq!(is_word_byte(b), expected, "byte {b:#04x}");
        }
    }

    #[test]
    fn a_digit_continues_a_word_but_cannot_start_one() {
        // The one word of difference between the two predicates, pinned: without
        // it `1e5` and `2024_01` scan as identifiers.
        for b in b'0'..=b'9' {
            assert!(is_word_byte(b), "{b:#04x}");
            assert!(!is_word_start(b), "{b:#04x}");
        }
        // Everything else agrees, over the whole byte range.
        for b in 0u8..=255 {
            if !b.is_ascii_digit() {
                assert_eq!(is_word_byte(b), is_word_start(b), "byte {b:#04x}");
            }
        }
    }

    #[test]
    fn a_unicode_identifier_is_one_word_not_several() {
        // What the `>= 0x80` clause is *for*. Every byte of a multi-byte
        // character — lead and continuation alike — must be a word byte, or the
        // name splits at the first non-ASCII byte and the halves match nothing
        // in the catalog.
        for name in ["café", "日本語", "Ω", "naïve_column"] {
            assert!(
                name.as_bytes().iter().all(|&b| is_word_byte(b)),
                "{name} split"
            );
        }
        // …and the rule still stops at real boundaries.
        for &b in b" .,()'`\"-;" {
            assert!(!is_word_byte(b), "{:?} treated as a word byte", b as char);
        }
    }

    #[test]
    fn balanced_paren_span_matches_the_outer_pair() {
        let s = "((a > 0) AND (b < 1)) trailing";
        assert_eq!(
            balanced_paren_span(s.as_bytes(), 0, SqlDialect::Postgres),
            Some(20)
        );
        assert_eq!(&s[..=20], "((a > 0) AND (b < 1))");
    }

    #[test]
    fn balanced_paren_span_ignores_parens_inside_literals() {
        // The reason this goes through `skip_noncode` rather than counting
        // bytes: both of these carry a close-paren that is data, not structure.
        for (s, dialect) in [
            ("(name <> ')')", SqlDialect::Postgres),
            ("(name <> ')')", SqlDialect::MySql),
        ] {
            let end = balanced_paren_span(s.as_bytes(), 0, dialect);
            assert_eq!(end, Some(s.len() - 1), "{s}");
        }
        // A quoted identifier holding a quote is the case the hand-rolled
        // scanner latched on: after `"it's"` it believed it was inside a string
        // for the rest of the input.
        let s = r#"(new."it's" > 0)"#;
        assert_eq!(
            balanced_paren_span(s.as_bytes(), 0, SqlDialect::Postgres),
            Some(s.len() - 1)
        );
    }

    #[test]
    fn balanced_paren_span_rejects_a_non_paren_start_and_an_unclosed_group() {
        assert_eq!(balanced_paren_span(b"a > 0", 0, SqlDialect::Postgres), None);
        assert_eq!(
            balanced_paren_span(b"(a > 0", 0, SqlDialect::Postgres),
            None
        );
        assert_eq!(balanced_paren_span(b"", 0, SqlDialect::Postgres), None);
    }

    #[test]
    fn find_code_skips_a_match_inside_a_literal() {
        let s = "EXECUTE FUNCTION f('EXECUTE FUNCTION x(', 'b')";
        // The real keyword is at 0; the one in the argument must not be found.
        assert_eq!(
            find_code(s, "EXECUTE FUNCTION ", SqlDialect::Postgres),
            Some(0)
        );
        let s = "CREATE TRIGGER t ... f('EXECUTE FUNCTION x(')";
        assert_eq!(
            find_code(s, "EXECUTE FUNCTION ", SqlDialect::Postgres),
            None
        );
    }

    fn used(sql: &str) -> Option<String> {
        use_target(sql, SqlDialect::MySql)
    }

    #[test]
    fn a_use_statement_names_the_database_it_switches_to() {
        assert_eq!(used("USE sakila"), Some("sakila".into()));
        assert_eq!(used("use sakila;"), Some("sakila".into()));
        assert_eq!(used("  USE   sakila  ;  "), Some("sakila".into()));
        assert_eq!(used("USE my_db2"), Some("my_db2".into()));
    }

    /// Through `skip_noncode`, so the name comes back **unquoted** — the label is
    /// prose, not SQL — and a doubled backtick is one literal backtick.
    #[test]
    fn a_backticked_database_name_is_lifted_out_whole() {
        assert_eq!(used("USE `my db`"), Some("my db".into()));
        assert_eq!(used("USE `a``b`;"), Some("a`b".into()));
    }

    #[test]
    fn a_comment_between_the_keyword_and_the_name_is_skipped() {
        assert_eq!(used("USE /* x */ sakila"), Some("sakila".into()));
        assert_eq!(used("/* lead */ USE sakila -- tail"), Some("sakila".into()));
    }

    /// **Anything it can't read plainly is `None`**, and the caller then drops
    /// the label rather than printing a name it isn't sure of. A missing label
    /// says nothing; a wrong one is the defect this function exists to fix.
    #[test]
    fn anything_but_a_plain_use_is_refused() {
        for s in [
            "SELECT 1",
            "USE",
            "USE ;",
            "USE sakila world",
            "USE @db",
            "USE 'sakila'",
            "USEsakila",
            "",
        ] {
            assert_eq!(used(s), None, "{s:?}");
        }
    }

    /// PostgreSQL has no `USE` — the server refuses it, so there is nothing to
    /// track and no label to change.
    #[test]
    fn postgres_has_no_use_statement() {
        assert_eq!(use_target("USE sakila", SqlDialect::Postgres), None);
    }

    // ── SQLite trigger bodies ────────────────────────────────────────────────

    fn sqlite_stmts(sql: &str) -> Vec<&str> {
        super::statement_ranges(sql, SqlDialect::Sqlite)
            .into_iter()
            .map(|(lo, hi)| &sql[lo..hi])
            .collect()
    }

    /// **A SQLite trigger body is full of `;` and none of them ends the
    /// statement.** MySQL solves this with `DELIMITER`, which SQLite has no form
    /// of — so the boundary rule has to know that a `CREATE TRIGGER` runs to the
    /// `;` after its `END`, exactly as `sqlite3_complete()` does for SQLite's own
    /// shell.
    ///
    /// Without this the splitter cuts the trigger in half: Run Everything sends
    /// `… BEGIN UPDATE log SET n = n + 1;` as one statement and `END;` as
    /// another, which is the application handing the user a script it cannot run
    /// itself.
    #[test]
    fn a_sqlite_trigger_body_is_one_statement() {
        let sql = "CREATE TRIGGER t AFTER INSERT ON emp BEGIN \
                   UPDATE log SET n = n + 1; DELETE FROM tmp; END;\nSELECT 1;";
        assert_eq!(
            sqlite_stmts(sql),
            [
                "CREATE TRIGGER t AFTER INSERT ON emp BEGIN \
                 UPDATE log SET n = n + 1; DELETE FROM tmp; END;",
                "SELECT 1;"
            ]
        );
    }

    /// A `CASE … END` inside the body must not be mistaken for the block's own
    /// `END`. This is why the rule counts openers rather than looking for the
    /// first `END;` — the naive version ends the statement in the middle of an
    /// expression.
    #[test]
    fn a_case_expression_does_not_end_the_block() {
        let sql = "CREATE TRIGGER t AFTER UPDATE ON emp BEGIN \
                   UPDATE log SET n = CASE WHEN NEW.a > 1 THEN 1 ELSE 2 END; END;\nSELECT 2;";
        assert_eq!(
            sqlite_stmts(sql),
            [
                "CREATE TRIGGER t AFTER UPDATE ON emp BEGIN \
                 UPDATE log SET n = CASE WHEN NEW.a > 1 THEN 1 ELSE 2 END; END;",
                "SELECT 2;"
            ]
        );
    }

    /// The words only count as code: `BEGIN`/`END` inside a string, a quoted
    /// identifier or a comment belong to the data, and the shared lexer is what
    /// sees through them.
    #[test]
    fn begin_and_end_inside_literals_do_not_count() {
        let sql = "CREATE TRIGGER t AFTER INSERT ON emp BEGIN \
                   INSERT INTO log VALUES ('END; BEGIN'); -- END;\n END;\nSELECT 3;";
        assert_eq!(sqlite_stmts(sql).len(), 2, "{:#?}", sqlite_stmts(sql));
        assert!(sqlite_stmts(sql)[1].starts_with("SELECT 3"));
    }

    /// `TEMP`/`TEMPORARY` and `IF NOT EXISTS` sit between `CREATE` and the
    /// trigger's name, and the rule has to reach past them — as does a plain
    /// `CREATE TABLE`, which must keep splitting on its own `;`.
    #[test]
    fn the_rule_reaches_past_the_optional_header_words_and_no_further() {
        let sql = "CREATE TEMP TRIGGER IF NOT EXISTS t BEFORE DELETE ON emp \
                   BEGIN SELECT 1; END;\nSELECT 4;";
        assert_eq!(sqlite_stmts(sql).len(), 2);
        // Not a trigger: every `;` still splits, including inside parentheses.
        let plain = "CREATE TABLE t (a INT); SELECT 5;";
        assert_eq!(
            sqlite_stmts(plain),
            ["CREATE TABLE t (a INT);", "SELECT 5;"]
        );
    }

    /// An unterminated trigger is one (incomplete) statement, not a pile of
    /// fragments — the same answer the splitter gives any unterminated tail.
    #[test]
    fn an_unfinished_trigger_stays_one_statement() {
        let sql = "CREATE TRIGGER t AFTER INSERT ON emp BEGIN UPDATE log SET n = 1;";
        assert_eq!(sqlite_stmts(sql), [sql]);
    }

    /// The rule is SQLite's alone. MySQL keeps `DELIMITER`, and a MySQL trigger
    /// body written without one still splits the way it always did — changing
    /// that would silently alter what Run Everything sends to a MySQL server.
    #[test]
    fn other_engines_are_untouched() {
        let sql = "CREATE TRIGGER t AFTER INSERT ON emp FOR EACH ROW \
                   BEGIN SET NEW.a = 1; END;";
        assert!(
            super::statement_ranges(sql, SqlDialect::MySql).len() > 1,
            "MySQL's boundary rule must not change"
        );
        assert!(
            super::statement_ranges(sql, SqlDialect::Postgres).len() > 1,
            "PostgreSQL's boundary rule must not change"
        );
    }
}
