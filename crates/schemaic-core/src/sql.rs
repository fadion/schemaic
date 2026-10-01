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
    pub(crate) fn double_quote_is_ident(self) -> bool {
        !matches!(self, SqlDialect::MySql)
    }

    /// Are `` `…` `` identifiers accepted? MySQL's own syntax, which SQLite also
    /// takes for compatibility; PostgreSQL doesn't.
    ///
    /// `pub(crate)` because `core::pairs` was asking the same question and could
    /// not reach this, so it hand-spelled `dialect != SqlDialect::Postgres`
    /// twice. The two agreed for all three engines; what they could not survive
    /// is a fourth, or a change here, which would move one and not the other.
    pub(crate) fn backtick_ident(self) -> bool {
        matches!(self, SqlDialect::MySql | SqlDialect::Sqlite)
    }

    /// Are `[…]` identifiers accepted? SQL Server's own syntax, which SQLite
    /// also takes for compatibility. Whether a `]` can be written inside one is
    /// [`Self::bracket_doubles`].
    pub(crate) fn bracket_ident(self) -> bool {
        matches!(self, SqlDialect::Sqlite | SqlDialect::MsSql)
    }

    /// Does `]]` stand for a `]` inside a `[…]` name? SQL Server only. SQLite
    /// defines no escape, so there the span ends at the first `]`.
    ///
    /// `pub(crate)` for `filter`, which has to know when a name holds one.
    pub(crate) fn bracket_doubles(self) -> bool {
        matches!(self, SqlDialect::MsSql)
    }

    /// Does the server expand `/*! … */` (and MariaDB's `/*M! … */`) as **code**
    /// rather than skipping it as a comment?
    ///
    /// MySQL and MariaDB only. The marker may carry a minimum server version —
    /// `/*!50000`, `/*M!010000` — and a bare `/*!` fires unconditionally; this
    /// module treats every one of them as code, because a lexer that guessed the
    /// server's version would guess in the unsafe direction half the time.
    fn executable_comment(self) -> bool {
        matches!(self, SqlDialect::MySql)
    }

    /// Are `$tag$ … $tag$` strings accepted? PostgreSQL only.
    fn dollar_quoted(self) -> bool {
        matches!(self, SqlDialect::Postgres)
    }

    /// Do `/* … */` comments nest? PostgreSQL and SQL Server, as the SQL
    /// standard has it: `/* a /* b */ c */` is one comment there, and on MySQL
    /// and SQLite the first `*/` ends it.
    fn nested_block_comments(self) -> bool {
        matches!(self, SqlDialect::Postgres | SqlDialect::MsSql)
    }

    /// Does the client honour a `DELIMITER` directive? MySQL only — see
    /// [`delimiter_directive`] for why it exists there at all.
    fn delimiter_directive(self) -> bool {
        matches!(self, SqlDialect::MySql)
    }

    /// Does the client split a script into batches at `GO` lines, and does a
    /// routine body run to the end of its batch? SQL Server only — see
    /// [`go_directive`] and [`BodyScan`].
    ///
    /// `pub(crate)` for `ddl::join_scripts`, which writes a `GO` between the
    /// objects of a script. (`sqlfmt` asks [`go_directive`] instead.)
    pub(crate) fn batch_separator(self) -> bool {
        matches!(self, SqlDialect::MsSql)
    }

    /// Do names take a `@`/`@@`/`#`/`##` prefix — a variable, a system
    /// function, a temporary table? SQL Server only; see
    /// [`t_sql_name_prefix`].
    pub fn prefixed_names(self) -> bool {
        matches!(self, SqlDialect::MsSql)
    }

    /// Is a read-only connection's session **only a transaction that is rolled
    /// back**, rather than one the server refuses writes in? SQL Server's —
    /// it has no read-only session to ask for (`db::mssql::fetch_query`) — so
    /// what a rollback cannot undo, the text gate has to refuse, on every path
    /// that runs on such a connection ([`run_verdict`]).
    pub fn read_only_is_a_rollback(self) -> bool {
        matches!(self, SqlDialect::MsSql)
    }
}

/// If `b[i..]` starts a comment, return the index just past it. Handles `--`
/// (whitespace after it required per [`SqlDialect::dash_comment_needs_space`]),
/// `#` line comments (per [`SqlDialect::hash_line_comment`]), and `/* … */` block
/// comments (every dialect; nesting per [`SqlDialect::nested_block_comments`] —
/// ending a PostgreSQL comment at its first `*/` read the rest of it as the
/// statement, and a quote there as the start of a string that hid the next one).
fn skip_comment(b: &[u8], i: usize, dialect: SqlDialect) -> Option<usize> {
    let n = b.len();
    if i >= n {
        return None;
    }
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
        // **An executable comment is not a comment.** MySQL and MariaDB expand
        // `/*! … */` — and MariaDB `/*M! … */` — as *code*, optionally gated on
        // a version number that `/*!` alone omits and that `/*M!010000` clears
        // on every MariaDB this app supports. Returning the end of the whole run
        // hid the body from the one lexer, and with it from both security gates
        // built on it: `SELECT /*!LOAD_FILE('/etc/passwd')*/` tokenised as
        // `["SELECT"]` and passed the AI read-only gate, and
        // `SELECT * FROM t /*!INTO OUTFILE '/tmp/x'*/` was a plain read to
        // `contains_write`, so a connection marked read-only ran it with no
        // refusal and no confirmation.
        //
        // So the span ends **just past the marker**: the body is lexed as code
        // and the trailing `*/` falls out as punctuation. Over-blocking when the
        // server's version is below the marker's is the right direction for a
        // refusal gate, and it is one function, so every caller of
        // `skip_noncode` gets it at once.
        if let Some(after) = executable_marker(b, i, dialect) {
            return Some(after);
        }
        let nests = dialect.nested_block_comments();
        let mut depth = 1usize;
        let mut j = i + 2;
        while j + 1 < n {
            if b[j] == b'*' && b[j + 1] == b'/' {
                depth -= 1;
                j += 2;
                if depth == 0 {
                    return Some(j);
                }
            } else if nests && b[j] == b'/' && b[j + 1] == b'*' {
                depth += 1;
                j += 2;
            } else {
                j += 1;
            }
        }
        // Unterminated: the comment runs to the end.
        return Some(n);
    }
    None
}

/// The index just past a MySQL-family executable-comment marker opening at
/// `b[i]`, or `None` when this `/*` opens an ordinary comment.
///
/// The three spellings are `/*!`, `/*!nnnnn` and MariaDB's `/*M!` / `/*M!nnnnnn`;
/// the digits are a minimum server version and are consumed with the marker
/// because they are not part of the statement either. See [`skip_comment`].
fn executable_marker(b: &[u8], i: usize, dialect: SqlDialect) -> Option<usize> {
    if !dialect.executable_comment() {
        return None;
    }
    let mut j = i + 2;
    if b.get(j) == Some(&b'M') {
        j += 1;
    }
    if b.get(j) != Some(&b'!') {
        return None;
    }
    j += 1;
    while b.get(j).is_some_and(u8::is_ascii_digit) {
        j += 1;
    }
    Some(j)
}

/// The index just past a **whole** `/*! … */` executable-comment run opening at
/// `b[i]` — marker, body and closing `*/` — or `None` when this `/*` opens an
/// ordinary comment (or the dialect has no executable comments).
///
/// **The other question about the same bytes, and both answers are needed.**
/// [`skip_comment`] stops just past the marker because its callers are the
/// security gates, which must see the body as the code the server will run. A
/// *formatter* is asking something else — "what may I re-flow" — and the answer
/// there is the whole run, verbatim: once the body is ordinary tokens the
/// trailing `*/` falls out as two one-byte puncts, `ops(MySql)` has no `*/`
/// entry, and Format Code wrote `* /` into the user's buffer, leaving the
/// comment unterminated and swallowing everything after it.
///
/// Unterminated → end of input, matching [`skip_comment`]'s ordinary arm.
pub(crate) fn executable_comment_end(b: &[u8], i: usize, dialect: SqlDialect) -> Option<usize> {
    let n = b.len();
    if i + 1 >= n || b[i] != b'/' || b[i + 1] != b'*' {
        return None;
    }
    let mut j = executable_marker(b, i, dialect)?;
    while j + 1 < n && !(b[j] == b'*' && b[j + 1] == b'/') {
        j += 1;
    }
    Some((j + 2).min(n))
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

/// What the non-code span opening at `b[i]` **is** — the classification half of
/// [`skip_noncode`], for a caller that has to colour or label the span rather
/// than skip it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NonCode {
    /// `--`, `#` or `/* … */`.
    Comment,
    /// A string literal — including a dollar-quoted body, which is one.
    Literal,
    /// A **quoted identifier**: a name, not a value.
    Identifier,
}

/// See [`NonCode`]. `None` exactly where [`skip_noncode`] returns `None`.
///
/// **Here rather than at the caller, because the caller got it wrong.** The
/// syntax highlighter asked [`skip_noncode`] where a span was — dialect-aware —
/// and then classified it by its opening byte with no dialect at all, in a
/// `match` whose arms were `` ` ``, `'`/`"`, and everything else as a comment.
/// `skip_noncode` also answers `Some` for `$` on PostgreSQL and `[` on SQLite,
/// so both landed on the comment arm: **every dollar-quoted function body and
/// `DO` block in a PostgreSQL editor was greyed out as if commented**, and every
/// bracket-quoted identifier on SQLite was. `"Name"` painted as a string on the
/// two engines where it is an identifier. The highlighter's own field doc
/// promised the opposite ("so `#`-operators / `$tag$` bodies aren't coloured as
/// comments on a PostgreSQL connection") — the `#` half worked and the `$tag$`
/// half was exactly inverted.
///
/// The predicates it needs (`double_quote_is_ident`, `backtick_ident`,
/// `bracket_ident`, `dollar_quoted`) are this module's, and a second module
/// needing the same answer is the same privacy wall `comment_open` was exposed
/// for.
pub fn noncode_kind(b: &[u8], i: usize, dialect: SqlDialect) -> Option<NonCode> {
    if comment_open(b, i, dialect) {
        return Some(NonCode::Comment);
    }
    match *b.get(i)? {
        b'\'' => Some(NonCode::Literal),
        b'"' if dialect.double_quote_is_ident() => Some(NonCode::Identifier),
        b'"' => Some(NonCode::Literal),
        b'`' if dialect.backtick_ident() => Some(NonCode::Identifier),
        b'[' if dialect.bracket_ident() => Some(NonCode::Identifier),
        // A `$` that opens no valid tag is ordinary punctuation, which is what
        // `skip_noncode` says by returning `None` for it.
        b'$' if dialect.dollar_quoted() => scan_dollar(b, i).map(|_| NonCode::Literal),
        _ => None,
    }
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
///
/// The tag obeys PostgreSQL's own rule — *"the same rules as an unquoted
/// identifier, except that it cannot contain a dollar sign"* — so it is scanned
/// with [`is_word_start`]/[`is_word_byte`], **this module's third word scanner**
/// and the one the sweep that consolidated the other two did not reach. Written
/// ASCII-only it ended the tag at the `0xC3` of `$prüfung$`, answered `None`, and
/// the statement splitter then cut a PL/pgSQL body at its internal semicolons.
/// Asking [`is_word_start`] for the first byte also closes the over-read on the
/// other side: `$1$` is two positional parameters, not the tag `1`.
///
/// **A `$` that continues a name opens nothing.** PostgreSQL lets an unquoted
/// identifier carry `$` after its first character, so `a$$` is one name and a
/// dollar quote opens only where a token starts. Opening one after `a` read
/// `SELECT 1 AS a$$; SELECT pg_sleep(100); SELECT 1 AS b$$` as a single
/// statement holding a string, where the server runs three.
///
/// **Asked of the one byte before, not the run.** A `$` after a word byte or
/// another `$` opens nothing. That byte also ends a number (`1$$x$$`) or a
/// closing quote (`$a$x$a$$b$`) — but a quote opening straight after either
/// does not parse, so the difference can only over-block. Walking back to the
/// run's start instead made every `$` of `a$a$a$…` rescan the text before it.
fn scan_dollar(b: &[u8], i: usize) -> Option<usize> {
    if i > 0 && continues_dollar_name(b[i - 1]) {
        return None;
    }
    let n = b.len();
    let mut j = i + 1;
    if j < n && b[j] != b'$' && !is_word_start(b[j]) {
        return None; // `$1` / `$+` — no identifier can begin here
    }
    while j < n && is_word_byte(b[j]) {
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

/// Can this byte continue a PostgreSQL name, where `a$$` is one identifier?
///
/// [`is_word_byte`] plus `$` — a continuation only, since `$` begins no name
/// and `$1` is a parameter. One definition so the lexer's dollar quote
/// ([`scan_dollar`]) and the function gate's tokens ([`call_tokens`]) cannot
/// disagree about where a name ends, which is the disagreement the
/// `a$$ … b$$` smuggle lived in.
fn continues_dollar_name(b: u8) -> bool {
    is_word_byte(b) || b == b'$'
}

/// Can this byte continue a bare name on `dialect`, after its first byte?
///
/// [`is_word_byte`] plus each engine's extra continuation bytes: `$` on all
/// four ([`continues_dollar_name`]) — MySQL's bare identifier is
/// `[0-9a-zA-Z$_]`, SQLite's tokenizer continues one through `$`, and `a$b`
/// is one name on PostgreSQL — and T-SQL's `#` and `@` besides: `dbo.h#`,
/// `purge@now` and `v$as` are each one regular identifier there (measured on
/// SQL Server 2022). MySQL and SQLite were said to keep the plain word bytes,
/// and Format Code wrote a valid `a$b` back as the syntax error `a $ b`.
///
/// Only a *continuation*: `@x` is a T-SQL variable and `#t` a temporary
/// table, whose leading byte is a prefix rather than part of an ordinary
/// word — [`t_sql_name_prefix`] measures those. The one definition every T-SQL
/// scanner asks, so the read-only gate's call scan, the view header walk and
/// the formatter cannot disagree about where a name ends — that disagreement
/// is how `dbo.f@GETDATE()` reached the gate as a call to `GETDATE`.
pub fn continues_name(b: u8, dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::MsSql => continues_dollar_name(b) || matches!(b, b'#' | b'@'),
        SqlDialect::Postgres | SqlDialect::MySql | SqlDialect::Sqlite => continues_dollar_name(b),
    }
}

/// The length of a T-SQL name prefix at `b[i..]` — `@` (a variable), `@@` (a
/// system function), `#` (a temporary table) or `##` (a global one) — when a
/// name follows it; `0` otherwise, and always `0` on an engine whose names
/// take no such prefix.
///
/// A prefix with no name after it (`a @ b`, `#` alone) is not one, so an
/// operator a dialect spells with these bytes is left alone.
pub fn t_sql_name_prefix(b: &[u8], i: usize, dialect: SqlDialect) -> usize {
    if !dialect.prefixed_names() {
        return 0;
    }
    let Some(&c) = b.get(i).filter(|&&c| c == b'@' || c == b'#') else {
        return 0;
    };
    let len = if b.get(i + 1) == Some(&c) { 2 } else { 1 };
    match b.get(i + len) {
        Some(&n) if is_word_byte(n) => len,
        _ => 0,
    }
}

/// Is the `'` at `i` preceded by a standalone `E`/`e` prefix (not the tail of a
/// longer word), i.e. PostgreSQL's escape-string syntax?
fn e_prefixed(b: &[u8], i: usize) -> bool {
    i >= 1 && matches!(b[i - 1], b'e' | b'E') && (i < 2 || !is_word_byte(b[i - 2]))
}

/// Scan a `[…]` bracketed identifier to just past its `]`. On SQLite there is
/// no escape inside one, so the span ends at the first `]`; on SQL Server `]]`
/// is an escaped `]` ([`SqlDialect::bracket_doubles`]). Unterminated → end of
/// input, matching [`scan_quoted`]'s policy.
fn scan_bracket(b: &[u8], i: usize, dialect: SqlDialect) -> usize {
    if dialect.bracket_doubles() {
        return scan_quoted_until(b, i, b']');
    }
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

/// [`scan_quoted`] for a span whose closing byte differs from its opening one
/// (`[` … `]`), with the doubled closer as its only escape.
fn scan_quoted_until(b: &[u8], i: usize, close: u8) -> usize {
    let n = b.len();
    let mut j = i + 1;
    while j < n {
        if b[j] == close {
            if j + 1 < n && b[j + 1] == close {
                j += 2;
                continue;
            }
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
///
/// **`i` past the end is `None`, not a panic.** Most callers walk a slice they
/// have already bounded, but the ones that hand over "whatever is left after the
/// keyword" can arrive here with an empty slice — and the answer there is the
/// same as for any byte that starts no literal: there is nothing to skip.
pub fn skip_noncode(b: &[u8], i: usize, dialect: SqlDialect) -> Option<usize> {
    if let Some(j) = skip_comment(b, i, dialect) {
        return Some(j);
    }
    if i >= b.len() {
        return None;
    }
    match b[i] {
        b'\'' => {
            let backslash =
                dialect.backslash_escapes() || (dialect.e_string_backslash() && e_prefixed(b, i));
            Some(scan_quoted(b, i, b'\'', backslash))
        }
        b'"' => Some(scan_quoted(b, i, b'"', !dialect.double_quote_is_ident())),
        b'`' if dialect.backtick_ident() => Some(scan_quoted(b, i, b'`', false)),
        b'[' if dialect.bracket_ident() => Some(scan_bracket(b, i, dialect)),
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

/// `sql` with every `--` / `#` **line** comment rewritten as an equivalent
/// `/* … */` block comment — the form that survives being collapsed onto one
/// line.
///
/// **What this is for.** A preview folds a statement to one flowing line
/// ([`crate::history::preview`], [`crate::snippet::collapsed`]) and the panels
/// then syntax-colour that line. A line comment's terminator is the newline the
/// collapse just removed, so
///
/// ```text
/// -- daily revenue
/// SELECT id, name FROM orders
/// ```
///
/// arrived at the highlighter as one 76-byte comment token and the row rendered
/// as if the whole query were commented out. The hazard the collapse's own doc
/// had considered was a *block* comment spanning lines; this is the inverse, and
/// it only exists because the two correct functions are composed.
///
/// **Rewritten rather than dropped**, because `-- daily revenue` is often the
/// most informative thing in a saved query and a preview that silently loses it
/// is a different wrong answer. The body is escaped so it cannot close the block
/// it is now inside — a comment containing `*/` would otherwise end the block
/// early and leave the rest of the statement outside it, which is the same class
/// of bug one level down.
///
/// Everything else is copied byte for byte, strings and quoted identifiers
/// included, through the shared boundary lexer — so a `--` inside a literal is
/// still a literal.
pub fn inline_line_comments(sql: &str, dialect: SqlDialect) -> String {
    let b = sql.as_bytes();
    let mut out = String::with_capacity(sql.len() + 8);
    let mut i = 0usize;
    let mut copied = 0usize;
    while i < b.len() {
        let Some(j) = skip_noncode(b, i, dialect) else {
            i += 1;
            continue;
        };
        let j = j.max(i + 1).min(b.len());
        let region = &sql[i..j];
        if let Some(body) = line_comment_body(region) {
            out.push_str(&sql[copied..i]);
            // Spaced on both sides so `--daily` and `-- daily` come out the
            // same, and so the closing `*/` can never end up glued to the body.
            out.push_str("/* ");
            out.push_str(body.trim().replace("*/", "* /").replace("/*", "/ *").trim());
            out.push_str(" */");
            // The newline was the terminator, and it is also what a caller that
            // keeps the lines needs; only the *comment* changes shape here.
            if region.ends_with('\n') {
                out.push('\n');
            }
            copied = j;
        }
        i = j;
    }
    out.push_str(&sql[copied..]);
    out
}

/// A noncode region's text, if it is a line comment: everything after the opener
/// and before the newline that ended it.
fn line_comment_body(region: &str) -> Option<&str> {
    let body = region
        .strip_prefix("--")
        .or_else(|| region.strip_prefix('#'))?;
    Some(body.trim_end_matches('\n').trim_end_matches('\r'))
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

/// Is `sql[lo..hi]` a client directive rather than a statement — MySQL's
/// `DELIMITER`, or SQL Server's `GO` batch separator?
///
/// Exposed so the callers that *execute* ranges can drop it: the server would
/// answer a syntax error, since it is the client that owns the word. One
/// predicate for both, so no executing path can drop one and send the other.
pub fn is_delimiter_directive(sql: &str, lo: usize, hi: usize, dialect: SqlDialect) -> bool {
    delimiter_directive(sql, lo, dialect).is_some_and(|(end, _)| end >= hi)
        || go_directive(sql, lo, dialect).is_some_and(|end| end >= hi)
}

/// A `GO` batch separator starting at `i`: the offset just past its line.
///
/// **A client directive, not T-SQL** — `sqlcmd` and SSMS split a script into
/// batches at it and never send it — and SQL Server's alone. It stands on a
/// line of its own: `GO`, an optional repeat count, and an optional `--`
/// comment. The count is accepted and **not** honoured; a batch runs once.
///
/// The caller answers "is `i` at the start of a line"; this answers whether
/// the line is a directive.
pub(crate) fn go_directive(sql: &str, i: usize, dialect: SqlDialect) -> Option<usize> {
    if !dialect.batch_separator() {
        return None;
    }
    let b = sql.as_bytes();
    if !b.get(i..i + 2)?.eq_ignore_ascii_case(b"GO") {
        return None;
    }
    let mut j = i + 2;
    let blank = |c: u8| c == b' ' || c == b'\t' || c == b'\r';
    while b.get(j).is_some_and(|&c| blank(c)) {
        j += 1;
    }
    while b.get(j).is_some_and(u8::is_ascii_digit) {
        j += 1;
    }
    while b.get(j).is_some_and(|&c| blank(c)) {
        j += 1;
    }
    if b.get(j..j + 2) == Some(b"--") {
        while b.get(j).is_some_and(|&c| c != b'\n') {
            j += 1;
        }
    }
    match b.get(j) {
        None => Some(j),
        Some(b'\n') => Some(j + 1),
        Some(_) => None,
    }
}

/// Is `i` the first non-blank byte of its line?
pub(crate) fn at_line_start(b: &[u8], i: usize) -> bool {
    b[..i]
        .iter()
        .rev()
        .take_while(|&&c| c != b'\n')
        .all(|&c| c == b' ' || c == b'\t')
}

/// Where a scan through a SQL Server segment stands with respect to a routine
/// body.
///
/// `CREATE PROCEDURE`, `FUNCTION` and `TRIGGER` (and their `ALTER` and
/// `CREATE OR ALTER` forms) take **the rest of the batch** as their body, `;`s
/// and all, so only a `GO` line or the end of the input ends one. A view is a
/// single `SELECT` and needs nothing here.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyScan {
    /// At the start of a segment, still able to become a routine.
    Start,
    /// `CREATE`, `ALTER` or `CREATE OR ALTER` seen.
    Create,
    /// `CREATE OR` seen; `ALTER` must follow.
    Or,
    /// Inside a routine body, which runs to the batch's end.
    In,
    /// This segment is something else.
    No,
}

impl BodyScan {
    fn word(self, w: &str) -> BodyScan {
        let is = |k: &str| w.eq_ignore_ascii_case(k);
        match self {
            BodyScan::Start if is("CREATE") || is("ALTER") => BodyScan::Create,
            BodyScan::Create if is("OR") => BodyScan::Or,
            BodyScan::Or if is("ALTER") => BodyScan::Create,
            BodyScan::Create
                if is("PROC") || is("PROCEDURE") || is("FUNCTION") || is("TRIGGER") =>
            {
                BodyScan::In
            }
            BodyScan::In => BodyScan::In,
            _ => BodyScan::No,
        }
    }
}

/// Where a scan through a SQL Server batch stands with respect to what a `;`
/// must not cut.
///
/// **Each piece Run Everything or a `.sql` run sends is its own batch**, and a
/// batch is T-SQL's unit of scope: a variable lives in one, and a `BEGIN …
/// END` block, a `TRY … CATCH` and an `IF … ELSE` are each one statement to
/// the server. Cut at their `;`s, `DECLARE @id int; SELECT @id = 5` failed at
/// its second piece (Msg 137) and a block's first piece was a syntax error
/// (Msg 102) — after the pieces before it had committed. So a `;` does not
/// split inside a `BEGIN`/`CASE` block ([`Self::depth`]), nor directly before
/// an `ELSE` (the scan's lookahead), and from a `DECLARE`, a `RETURN`, a
/// `GOTO` or a label the rest of the batch is one piece ([`Self::whole`]), as
/// sqlcmd would send it. Statements that share nothing still go one by one,
/// each its own result.
#[derive(Clone, Copy, Default)]
struct BatchScope {
    /// Open `BEGIN … END` and `CASE … END` blocks.
    depth: u32,
    /// A `BEGIN` whose next word says whether it opens a block —
    /// `BEGIN TRAN` and its relatives do not.
    begin_pending: bool,
    /// An `END` just seen, and whether it closed a block — undone if the next
    /// word makes it `END CONVERSATION`, a statement.
    end_pending: Option<bool>,
    /// The rest of the batch is one piece.
    whole: bool,
}

impl BatchScope {
    fn word(mut self, w: &str) -> BatchScope {
        let is = |k: &str| w.eq_ignore_ascii_case(k);
        if std::mem::take(&mut self.begin_pending)
            && !(is("TRAN")
                || is("TRANSACTION")
                || is("DISTRIBUTED")
                || is("DIALOG")
                || is("CONVERSATION"))
        {
            self.depth += 1;
        }
        if let Some(closed) = self.end_pending.take()
            && closed
            && is("CONVERSATION")
        {
            self.depth += 1;
        }
        if is("BEGIN") {
            self.begin_pending = true;
        } else if is("CASE") {
            self.depth += 1;
        } else if is("END") {
            self.end_pending = Some(self.depth > 0);
            self.depth = self.depth.saturating_sub(1);
        } else if is("DECLARE") || is("RETURN") || is("GOTO") {
            self.whole = true;
        }
        self
    }

    /// May a `;` here end the statement? A `BEGIN;` is a block's own `BEGIN`.
    fn holds(self) -> bool {
        self.whole || self.depth > 0 || self.begin_pending
    }
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

/// Where a scan through one script stands between the chunks it arrives in.
///
/// A script runner reads a file a block at a time and cannot hold it all, so it
/// scans a *buffer* and drains the complete statements out of it. Almost nothing
/// has to survive that drain — the buffer always restarts at a statement
/// boundary, so [`TriggerScan`] is back at `Start` by construction — but the
/// **delimiter** does: `DELIMITER $$` is itself a statement, so by the time the
/// body it governs arrives, the directive that set it has been drained away.
#[derive(Clone, Debug)]
pub struct ScanState {
    delim: Vec<u8>,
}

impl Default for ScanState {
    fn default() -> Self {
        Self { delim: vec![b';'] }
    }
}

impl ScanState {
    /// A fresh scan, terminating statements at `;`.
    pub fn new() -> Self {
        Self::default()
    }
}

/// A statement boundary: the offset just past its terminator, and how many of
/// those bytes are the **client's** delimiter rather than the server's.
///
/// `strip` is `0` for `;` and for the boundary a `DELIMITER` directive itself
/// makes, and `delim.len()` when a custom terminator closed the statement.
/// **The distinction is not cosmetic**: `;` is a statement separator every
/// engine accepts at the end of what it is sent, while `$$` is a word the
/// *client* invented — MySQL lexes `END$$` as one identifier and answers a
/// syntax error. See [`executable_statements`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bound {
    pub at: usize,
    pub strip: usize,
}

/// Byte offsets bounding each **complete** statement in `sql`, resuming from
/// `state` and leaving it where the scan ended.
///
/// The difference from [`statement_bounds`] is the tail: no final `sql.len()` is
/// pushed, so the bytes after the last offset are an *unfinished* statement the
/// next chunk continues. That is what makes the answer safe to act on — a `;`
/// inside an unterminated string produces no boundary, so those bytes stay in
/// the caller's buffer rather than being executed as a statement of their own.
pub fn statement_bounds_open(sql: &str, dialect: SqlDialect, state: &mut ScanState) -> Vec<Bound> {
    scan_bounds(sql, dialect, &mut state.delim, false, None)
}

/// [`statement_bounds_open`] for the **last** chunk of a script: the end of
/// `sql` is the end of the input, so nothing waits for more — a final `GO`
/// with no newline is a separator, and a `;` with nothing after it ends its
/// statement — and the end itself closes the last statement.
pub fn statement_bounds_closing(
    sql: &str,
    dialect: SqlDialect,
    state: &mut ScanState,
) -> Vec<Bound> {
    let mut bounds = scan_bounds(sql, dialect, &mut state.delim, true, None);
    if bounds.last().is_none_or(|b| b.at < sql.len()) {
        bounds.push(Bound {
            at: sql.len(),
            strip: 0,
        });
    }
    bounds
}

/// Byte offsets bounding each top-level statement: `[0, after-`;`, …, len]`.
/// `;` inside strings / identifiers / comments does not split, on MySQL a
/// `DELIMITER` directive changes what does (see [`delimiter_directive`]), and on
/// SQLite a `;` inside a `CREATE TRIGGER`'s `BEGIN … END` block does not split
/// either (see [`TriggerScan`]).
pub fn statement_bounds(sql: &str, dialect: SqlDialect) -> Vec<usize> {
    let mut delim: Vec<u8> = vec![b';'];
    let mut bounds: Vec<usize> = scan_bounds(sql, dialect, &mut delim, true, None)
        .into_iter()
        .map(|b| b.at)
        .collect();
    bounds.push(sql.len());
    bounds
}

/// Every runnable statement of `sql`, **as the text to send to a server**.
///
/// [`statement_ranges`]' text with the client's `DELIMITER` token removed, and
/// the one form any path that *executes* should use. A range keeps the
/// terminator because the editor selects and highlights with it; a server must
/// not see it. `;` is left alone — every engine accepts a trailing one, and
/// removing it would change what tens of existing call sites send.
///
/// **The bug this exists to stop is the whole point of `DELIMITER`.** A dump
/// carrying a trigger or routine is written `DELIMITER $$` … `END$$`, which is
/// the only way a compound body holding its own semicolons can be split at all
/// — and `END$$` handed to MySQL lexes as a single identifier, so the body never
/// closes and the server answers a syntax error. Every such file therefore
/// failed at its first routine, half-loaded, on both the script runner and Run
/// Everything.
pub fn executable_statements(sql: &str, dialect: SqlDialect) -> Vec<String> {
    let mut delim: Vec<u8> = vec![b';'];
    let bounds = scan_bounds(sql, dialect, &mut delim, true, None);
    let mut out = Vec::new();
    let mut prev = 0usize;
    for bound in bounds.iter().skip(1).copied().chain(std::iter::once(Bound {
        at: sql.len(),
        strip: 0,
    })) {
        if let Some(text) = executable_range(sql, prev, bound, dialect) {
            out.push(text.to_string());
        }
        prev = bound.at;
    }
    out
}

/// One segment as the server should receive it, or `None` when it is not a
/// statement at all (blank, comment-only, or the `DELIMITER` directive itself).
///
/// Shared by [`executable_statements`] and `core::script`'s streaming splitter,
/// so the text a script runner sends and the text Run Everything sends cannot
/// come out different.
pub fn executable_range(sql: &str, from: usize, bound: Bound, dialect: SqlDialect) -> Option<&str> {
    let (lo, hi) = trim_range(sql, from, bound.at);
    if !is_runnable_segment(sql, lo, hi, dialect) {
        return None;
    }
    // Strip only when the trimmed end really is the terminator this bound
    // recorded — trailing whitespace between the two would mean it is not.
    let hi = if hi == bound.at {
        hi.saturating_sub(bound.strip)
    } else {
        hi
    };
    let (lo, hi) = trim_range(sql, lo, hi);
    (lo < hi).then(|| &sql[lo..hi])
}

/// The one scan both [`statement_bounds`] and [`statement_bounds_open`] are.
///
/// `delim` is in/out so a chunked caller can carry the terminator across a
/// drain; `at_eof` says whether the end of `sql` is the end of the *script*,
/// which is the only thing the two callers genuinely disagree about.
///
/// `until` stops the walk once a boundary **past** that offset has been
/// recorded — everything after it belongs to statements the caller has already
/// said it does not care about. It exists for [`statement_range`], which asks
/// "which statement is the caret in" on **every caret move**, undebounced,
/// through `update_signature_help`: over the whole document that measured 5.0
/// ms at 1 MiB and 80.7 ms at 16 MiB, to answer a question whose actual work
/// (`intel::signature_help` on the isolated statement) is 8 µs. `None` keeps
/// the full walk, and the boundaries produced before the stop are byte-for-byte
/// the ones the full walk produces — this is the same lexer, cut short, not a
/// second one.
///
/// `soft`, when given, collects the `;`s **only the batch scope held** —
/// [`BatchScope::whole`], outside any block and not before an `ELSE` — which
/// are where Run at the caret may stop inside a piece ([`run_current_range`]).
fn scan_bounds_with(
    sql: &str,
    dialect: SqlDialect,
    delim: &mut Vec<u8>,
    at_eof: bool,
    until: Option<usize>,
    mut soft: Option<&mut Vec<usize>>,
) -> Vec<Bound> {
    let b = sql.as_bytes();
    let n = b.len();
    let mut bounds = vec![Bound { at: 0, strip: 0 }];
    let mut i = 0;
    // Where the current segment begins, for recognising a directive only at the
    // start of one.
    let mut seg = 0usize;
    // SQLite alone. The other two engines keep exactly the boundaries they had —
    // MySQL's trigger bodies go behind `DELIMITER`, and changing that would
    // silently alter what Run Everything sends to a server.
    let track_triggers = dialect == SqlDialect::Sqlite;
    let mut scan = TriggerScan::Start;
    let track_bodies = dialect.batch_separator();
    let mut body = BodyScan::Start;
    let mut batch = BatchScope::default();
    while i < n {
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j;
            continue;
        }
        // SQL Server's `GO` line ends the statement before it — which need not
        // have a `;` — and is a segment of its own that `is_runnable_segment`
        // drops. Before the word walk below, which would step over it.
        if track_bodies
            && at_line_start(b, i)
            && let Some(end) = go_directive(sql, i, dialect)
        {
            // A `GO` line with no newline behind it mid-script may be the
            // front of a longer word (`GOTO`); wait for the rest.
            if !at_eof && b.get(end - 1) != Some(&b'\n') {
                break;
            }
            let line = b[..i]
                .iter()
                .rposition(|&c| c == b'\n')
                .map_or(0, |k| k + 1);
            if bounds.last().is_none_or(|last| last.at < line) {
                bounds.push(Bound { at: line, strip: 0 });
            }
            bounds.push(Bound { at: end, strip: 0 });
            if until.is_some_and(|u| end > u) {
                break;
            }
            i = end;
            seg = end;
            body = BodyScan::Start;
            batch = BatchScope::default();
            continue;
        }
        // `@x`, `@@ROWCOUNT`, `#t`: a name, never a keyword — `@declare`
        // declares nothing.
        if track_bodies && t_sql_name_prefix(b, i, dialect) > 0 {
            i += t_sql_name_prefix(b, i, dialect);
            while i < n && continues_name(b[i], dialect) {
                i += 1;
            }
            continue;
        }
        if track_bodies && is_word_start(b[i]) {
            let start = i;
            let mut end = i + 1;
            while end < n && continues_name(b[end], dialect) {
                end += 1;
            }
            let w = &sql[start..end];
            body = body.word(w);
            // A word after a `.` is a qualified name's part, never a keyword.
            if start == 0 || b[start - 1] != b'.' {
                batch = batch.word(w);
            }
            // A label — `again:` — is a `GOTO`'s target anywhere in the
            // batch, so the batch goes whole from here. **Wherever it stands**,
            // not only after a `;`: T-SQL needs no `;` before a statement, so
            // `SELECT 1\nagain:` opens one, and a word directly before a single
            // `:` is a label and nothing else outside a string — `::` is a
            // scope qualifier, and a qualified name's part is no label.
            if b.get(end) == Some(&b':')
                && b.get(end + 1) != Some(&b':')
                && (start == 0 || b[start - 1] != b'.')
            {
                batch.whole = true;
            }
            i = end;
            continue;
        }
        // Only at the start of a segment — `SELECT delimiter FROM t` is data.
        // Comments may precede it, as the `mysql` client reads a script: our
        // own dump opens its trigger section with a `-- Triggers` line right
        // above the `DELIMITER $$`.
        if only_comments_between(sql, seg, i, dialect)
            && let Some((end, token)) = delimiter_directive(sql, i, dialect)
        {
            // Mid-script, a directive running to the end of the buffer with no
            // newline behind it has not arrived in full, and acting on it is
            // worse than waiting: the token would be a *prefix* of the real one,
            // and a terminator of `$` never matches the `$$` the file meant.
            // Leave the whole thing for the next chunk.
            if !at_eof && b.get(end - 1) != Some(&b'\n') {
                break;
            }
            // The comments ahead of it are a segment of their own, which
            // `is_runnable_segment` drops as comment-only — left in front, they
            // would hide the directive from it and send `DELIMITER` along.
            if !(seg..i).all(|k| b[k].is_ascii_whitespace()) {
                bounds.push(Bound { at: i, strip: 0 });
            }
            // The directive's own segment is dropped whole by
            // `is_runnable_segment`, so there is nothing to strip off it.
            bounds.push(Bound { at: end, strip: 0 });
            if until.is_some_and(|u| end > u) {
                break;
            }
            *delim = token.into_bytes();
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
        if b[i..].starts_with(delim.as_slice()) {
            if scan.inside_body() || body == BodyScan::In {
                i += delim.len();
                continue;
            }
            if track_bodies {
                if batch.holds() {
                    if let Some(soft) = soft.as_deref_mut()
                        && batch.depth == 0
                        && !batch.begin_pending
                        && !matches!(
                            next_code_word(sql, i + delim.len(), dialect, at_eof),
                            Lookahead::Word(w) if w.eq_ignore_ascii_case("ELSE")
                        )
                    {
                        soft.push(i + delim.len());
                    }
                    i += delim.len();
                    continue;
                }
                // `IF … ; ELSE …` is one statement: look past the `;`.
                match next_code_word(sql, i + delim.len(), dialect, at_eof) {
                    Lookahead::Word(w) if w.eq_ignore_ascii_case("ELSE") => {
                        i += delim.len();
                        continue;
                    }
                    // Still arriving; the next chunk says.
                    Lookahead::Pending => break,
                    _ => {}
                }
            }
            // `;` is the server's own separator and stays; anything else is a
            // word the client invented and must not be sent.
            bounds.push(Bound {
                at: i + delim.len(),
                strip: if delim.as_slice() == b";" {
                    0
                } else {
                    delim.len()
                },
            });
            i += delim.len();
            // The caller's statement has now been closed; the rest of the
            // document is somebody else's.
            if until.is_some_and(|u| i > u) {
                break;
            }
            seg = i;
            scan = TriggerScan::Start;
            body = BodyScan::Start;
            batch = BatchScope::default();
            continue;
        }
        i += 1;
    }
    bounds
}

/// [`scan_bounds_with`] with no interest in where the batch scope held a `;`
/// — every caller but Run at the caret.
fn scan_bounds(
    sql: &str,
    dialect: SqlDialect,
    delim: &mut Vec<u8>,
    at_eof: bool,
    until: Option<usize>,
) -> Vec<Bound> {
    scan_bounds_with(sql, dialect, delim, at_eof, until, None)
}

/// What follows a point in the text, for [`scan_bounds`]' look past a `;`.
enum Lookahead<'a> {
    /// The next code token is this word.
    Word(&'a str),
    /// Something else, or the end of the script.
    Other,
    /// The buffer ends first, mid-script: the answer is in the next chunk.
    Pending,
}

/// The first code word at or after `from`, over whitespace, comments and
/// strings — or [`Lookahead::Pending`] when a chunked scan cannot tell yet.
fn next_code_word(sql: &str, from: usize, dialect: SqlDialect, at_eof: bool) -> Lookahead<'_> {
    let b = sql.as_bytes();
    let mut j = from;
    loop {
        while b.get(j).is_some_and(u8::is_ascii_whitespace) {
            j += 1;
        }
        if j >= b.len() {
            return if at_eof {
                Lookahead::Other
            } else {
                Lookahead::Pending
            };
        }
        match skip_noncode(b, j, dialect) {
            // An unterminated comment runs to the end of the buffer.
            Some(k) if k >= b.len() && !at_eof => return Lookahead::Pending,
            Some(k) if matches!(b[j], b'-' | b'/') => j = k,
            _ => break,
        }
    }
    if !is_word_start(b[j]) {
        return Lookahead::Other;
    }
    let mut k = j + 1;
    while k < b.len() && continues_name(b[k], dialect) {
        k += 1;
    }
    if k >= b.len() && !at_eof {
        return Lookahead::Pending;
    }
    Lookahead::Word(&sql[j..k])
}

/// Is `sql[from..to]` nothing but whitespace and comments — where a client
/// directive may still open a statement?
///
/// Comments are found by [`skip_noncode`], the one lexer; a string or a quoted
/// name is code, so `'x' DELIMITER $$` keeps the word as data.
fn only_comments_between(sql: &str, from: usize, to: usize, dialect: SqlDialect) -> bool {
    let b = sql.as_bytes();
    let mut j = from;
    while j < to {
        if b[j].is_ascii_whitespace() {
            j += 1;
            continue;
        }
        match skip_noncode(b, j, dialect) {
            Some(k) if matches!(b[j], b'-' | b'/' | b'#') && k <= to => j = k,
            _ => return false,
        }
    }
    true
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

/// The statement containing `offset`, **as the text to send to a server** — or
/// `None` when there is nothing runnable there.
///
/// [`statement_range`]'s segment put through [`executable_range`], which is the
/// third executing path and the one that was left behind. Run Everything and
/// the script runner were wired to `executable_statements`/`executable_range`;
/// **Run Current** still sliced `statement_range` and sent the result, so a
/// caret inside a trigger sent `…END$$` — which MySQL lexes as one identifier —
/// and a caret on the `DELIMITER $$` line sent the client directive itself.
///
/// Deliberately a *new* function rather than a change to `statement_range`:
/// that one has fifteen call sites and only this one may change behaviour. A
/// range keeps its terminator because the editor selects and highlights with
/// it; only the path that executes wants it off.
///
/// **And [`run_current_range`]'s piece, not [`statement_range`]'s**: inside a
/// T-SQL batch scope it stops at the caret's own statement.
pub fn executable_at(sql: &str, offset: usize, dialect: SqlDialect) -> Option<&str> {
    let (lo, hi) = run_current_bounds(sql, offset, dialect);
    executable_range(sql, lo, hi, dialect)
}

/// Where Run at the caret's piece starts, and the bound that ends it — see
/// [`run_current_range`].
fn run_current_bounds(sql: &str, offset: usize, dialect: SqlDialect) -> (usize, Bound) {
    let offset = offset.min(sql.len());
    let mut delim: Vec<u8> = vec![b';'];
    let mut soft = Vec::new();
    let mut bounds = scan_bounds_with(sql, dialect, &mut delim, true, None, Some(&mut soft));
    bounds.push(Bound {
        at: sql.len(),
        strip: 0,
    });
    let mut k = 0;
    for (w, b) in bounds.iter().enumerate().take(bounds.len() - 1) {
        if b.at <= offset {
            k = w;
        }
    }
    // The same fallback `statement_range` makes: a caret sitting after the
    // final `;` is in a blank segment and means the statement before it.
    let (lo, hi) = trim_range(sql, bounds[k].at, bounds[k + 1].at);
    let k = if lo == hi && k > 0 { k - 1 } else { k };
    let (lo, hi) = (bounds[k].at, bounds[k + 1]);
    // The first `;` past the caret that only the batch scope held ends it.
    match soft.iter().find(|&&s| s > offset && s > lo && s < hi.at) {
        Some(&at) => (lo, Bound { at, strip: 0 }),
        None => (lo, hi),
    }
}

/// The trimmed byte range of the statement containing `offset`.
///
/// **Scans as far as the caret's statement and stops**, not to the end of the
/// document: `update_signature_help` asks this on every caret move with no
/// debounce and no size cap, and `sqlfile::open_verdict` will open a 64 MiB
/// script. Over the whole buffer it measured 5.0 / 20.5 / 80.7 ms at
/// 1 / 4 / 16 MiB — an arrow key's worth of UI-thread work to locate a
/// statement whose signature help then costs 8 µs.
///
/// The boundaries are the same lexer's, cut short (see `scan_bounds`' `until`),
/// so the answer is unchanged; only the boundaries *after* the caret's
/// statement go unvisited, and nothing here ever read them.
pub fn statement_range(sql: &str, offset: usize, dialect: SqlDialect) -> (usize, usize) {
    let offset = offset.min(sql.len());
    let mut delim: Vec<u8> = vec![b';'];
    let mut bounds: Vec<usize> = scan_bounds(sql, dialect, &mut delim, true, Some(offset))
        .into_iter()
        .map(|b| b.at)
        .collect();
    // `statement_bounds` closes the list with the document's end. Here that is
    // right only when the walk ran out of document rather than stopping — if it
    // stopped, the last bound it recorded already closes the caret's statement.
    if bounds.last().is_none_or(|&last| last <= offset) {
        bounds.push(sql.len());
    }
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

/// The trimmed byte range **Run at the caret runs** — the one the editor
/// outlines before it does, and [`executable_at`]'s text.
///
/// [`statement_range`]'s, except inside a T-SQL batch scope. From a
/// `DECLARE`, a `RETURN`, a `GOTO` or a label the rest of the `GO` batch is
/// one piece ([`BatchScope::whole`]), which is right for Run Everything and a
/// `.sql` run and was wrong here: Ctrl+Enter on a `SELECT` below a `DECLARE`
/// ran every statement to the end of the batch, the `DELETE` after it
/// included, and when the piece was the whole buffer nothing outlined it. So
/// the piece is cut again at the first `;` past the caret that only the scope
/// held — never inside a block, nor before an `ELSE` — and what runs is the
/// scope's start through the caret's own statement: the variable it reads goes
/// with it, the statements below it do not.
///
/// The editor's statement for everything else — completion, signature help,
/// the AI actions — stays [`statement_range`]'s whole piece, which is where the
/// variable a statement reads is declared.
pub fn run_current_range(sql: &str, offset: usize, dialect: SqlDialect) -> (usize, usize) {
    let (lo, hi) = run_current_bounds(sql, offset, dialect);
    trim_range(sql, lo, hi.at)
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

/// The **first** top-level statement in `sql`, trimmed — or the whole string when
/// it holds one (or holds nothing a lexer can call a statement).
///
/// Borrowed from `sql`, so it costs a lexer pass and no allocation. The caller is
/// a reader that can only describe one result set and wants the statement that
/// produces it: PostgreSQL's `Parse` takes a single command, so a multi-statement
/// string cannot be prepared at all, and without a `PREPARE` there are no column
/// *types* — which is how a `bytea` came to be read as an unknown kind and
/// rendered as its whole hex encoding.
///
/// It is [`statement_ranges`]' first range verbatim, **terminator included**, and
/// not a second notion of where a statement ends. A trailing `;` is one range's
/// worth of statement to every other caller here — Run Everything hands
/// `sql[lo..hi]` straight to the same reader, and so to the same `PREPARE` — and
/// a server that parses that as one command is a server this already depends on.
pub fn first_statement(sql: &str, dialect: SqlDialect) -> &str {
    match statement_ranges(sql, dialect).first() {
        Some(&(lo, hi)) => &sql[lo..hi],
        None => sql,
    }
}

/// Every top-level statement's trimmed byte range that contains real SQL, in
/// order. Comment/whitespace-only segments (e.g. a trailing `# note` after the
/// last `;`) are dropped so Run Everything doesn't emit an "empty query" tab.
pub fn statement_ranges(sql: &str, dialect: SqlDialect) -> Vec<(usize, usize)> {
    statement_bounds(sql, dialect)
        .windows(2)
        .map(|w| trim_range(sql, w[0], w[1]))
        .filter(|&(lo, hi)| is_runnable_segment(sql, lo, hi, dialect))
        .collect()
}

/// Is the **trimmed** segment `sql[lo..hi]` something to send to a server?
///
/// Three conditions that always travel together: it holds something, that
/// something is code rather than comments and whitespace, and it is not the
/// client-side `DELIMITER` directive (which the server has never heard of and
/// would answer with a syntax error).
///
/// Public, and factored out of [`statement_ranges`], because a script runner
/// asks the same question of a segment it found in a *buffer* rather than in a
/// whole document — and two spellings of "is this a statement" is exactly how a
/// runner comes to send `DELIMITER $$` to MySQL.
pub fn is_runnable_segment(sql: &str, lo: usize, hi: usize, dialect: SqlDialect) -> bool {
    lo < hi
        && segment_has_code(sql, lo, hi, dialect)
        && !is_delimiter_directive(sql, lo, hi, dialect)
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

/// The first `n` upper-cased word tokens of `sql`, skipping string, quoted-
/// identifier and comment content.
///
/// [`leading_keyword`]'s plural, and **bounded**, which is the point: classifying
/// a statement needs its opening words and nothing else, while the private
/// `word_tokens` tokenises to the end and allocates a `String` per word. Asked of
/// a dump's extended `INSERT` that is sixteen megabytes of values to learn one
/// word, times every statement in the file.
///
/// The bound also has to be generous enough for the longest header anyone
/// actually writes, which is MySQL's view preamble:
/// `CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`localhost` SQL SECURITY DEFINER
/// VIEW` puts `VIEW` eighth, with the back-quoted parts skipped as identifiers.
pub fn leading_words(sql: &str, n: usize, dialect: SqlDialect) -> Vec<String> {
    let b = sql.as_bytes();
    let len = b.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < len && out.len() < n {
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j;
            continue;
        }
        if is_word_start(b[i]) {
            let start = i;
            i += 1;
            while i < len && is_word_byte(b[i]) {
                i += 1;
            }
            out.push(sql[start..i].to_ascii_uppercase());
            continue;
        }
        i += 1;
    }
    out
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
    // Invariant 11, not an ASCII rule: with the ASCII spelling
    // `leading_keyword("SELECTé 1")` was `Some("SELECT")`, because the
    // non-ASCII byte ended the word early. The statement is a syntax error
    // either way, so nothing downstream lost a guard — but a second wrong
    // spelling of the one definition is how the first one gets copied.
    if i < n && is_word_start(b[i]) {
        let s = i;
        let mut j = i + 1;
        while j < n && is_word_byte(b[j]) {
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
///
/// SQL Server has `USE` too, with the same effect on the statements after it
/// in one batch.
pub fn use_target(sql: &str, dialect: SqlDialect) -> Option<String> {
    let has_use = match dialect {
        SqlDialect::MySql | SqlDialect::MsSql => true,
        SqlDialect::Postgres | SqlDialect::Sqlite => false,
    };
    if !has_use || leading_keyword(sql, dialect)? != "USE" {
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
    let (name, mut i) = if i < n && crate::intel::ident_quote(dialect, b[i]).is_some() {
        // `` `a``b` `` and `[a]]b]` — a doubled closer is one literal one.
        // `ident_at` knows each dialect's quoting; it answers the end too.
        match ident_at(sql, i, dialect) {
            (Some(name), end) => (name, end),
            (None, _) => return None,
        }
    } else if i < n && is_word_start(b[i]) {
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
            // **`is_word_start`/`is_word_byte`, not an ASCII rule.** This
            // scanner is the missing-`WHERE` safety net, and with the ASCII
            // spelling a byte `>= 0x80` *ended* a word — so the ASCII tail of a
            // non-ASCII identifier was scanned as a word of its own and
            // `DELETE FROM éwhere` answered `true`, i.e. "this statement has a
            // WHERE". `unsafe_reason` then returned `None`, `run_verdict` never
            // reached its `Confirm` arm, and with `confirm_writes` off the
            // delete ran unasked. utf8 identifiers are ordinary on MySQL and
            // PostgreSQL, which is what invariant 11 is for.
            c if is_word_start(c) => {
                let s = i;
                let mut j = i + 1;
                while j < n && is_word_byte(b[j]) {
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

/// The run guard's warning for `stmt`, if it should be asked about first:
/// [`every_row_reason`], or [`drop_reason`] — a statement that destroys an
/// object holding stored rows.
///
/// **It asks only for what holds stored rows.** `DROP` ran unasked while the
/// less destructive `TRUNCATE` was held — found by a CLI test run, but the gap
/// was the editor's too, since this is the one guard both use. A `TEMPORARY`
/// table dies with the session, and a view, index, trigger, routine or user
/// holds no rows, so those run as before: a guard that fires on every DDL
/// statement is one people learn to click through. The schema tree's own Drop
/// goes through the DDL preview instead, so it is not asked twice.
pub fn unsafe_reason(stmt: &str, dialect: SqlDialect) -> Option<String> {
    every_row_reason(stmt, dialect).or_else(|| drop_reason(stmt, dialect))
}

/// The warning for a statement that deletes stored rows **with the object that
/// holds them**, rather than row by row.
///
/// `DROP TABLE`/`TABLES` (MySQL's grammar has both spellings, and the plural
/// once ran unasked beside a held singular), `DATABASE` and `SCHEMA`; and the
/// spellings that destroy the same thing under another name: MariaDB's `CREATE
/// OR REPLACE TABLE`/`DATABASE`, which drops what is there before creating it
/// empty; PostgreSQL's `DROP OWNED`, which drops everything a role owns; a
/// `DROP TYPE`/`DOMAIN`/`EXTENSION … CASCADE`, which drops every column of the
/// type with its values (without `CASCADE` the server refuses while anything
/// depends on it); and MySQL's `ALTER TABLE … TRUNCATE PARTITION` /
/// `DROP PARTITION`. An `ALTER … DROP COLUMN` stays out, deliberately — it is
/// the everyday schema edit this guard would teach people to click through.
///
/// `pub(crate)` for the `.sql` panel, which counts these as destruction
/// (`script::is_destructive`), apart from the every-row count.
///
/// Each T-SQL statement in `stmt` is judged, not its first alone — see
/// [`tsql_statements`].
pub(crate) fn drop_reason(stmt: &str, dialect: SqlDialect) -> Option<String> {
    tsql_statements(stmt, dialect)
        .into_iter()
        .find_map(|s| one_drop_reason(s, dialect))
}

/// [`drop_reason`] for one statement.
fn one_drop_reason(stmt: &str, dialect: SqlDialect) -> Option<String> {
    let stmt = analyzed_statement(stmt, dialect).unwrap_or(stmt);
    let words = leading_words(stmt, PARTITION_WORDS, dialect);
    let word = |i: usize| words.get(i).map(String::as_str);
    match (word(0)?, word(1)) {
        ("DROP", Some("TABLE" | "TABLES")) => {
            Some("DROP TABLE deletes the table and every row in it.".to_string())
        }
        ("DROP", Some(obj @ ("DATABASE" | "SCHEMA"))) => Some(format!(
            "DROP {obj} deletes the {} and every table in it.",
            obj.to_ascii_lowercase()
        )),
        ("DROP", Some("OWNED")) => Some(
            "DROP OWNED deletes everything the role owns, every table and its rows with it."
                .to_string(),
        ),
        ("DROP", Some(obj @ ("TYPE" | "DOMAIN" | "EXTENSION")))
            if word_tokens(stmt, dialect).0.iter().any(|w| w == "CASCADE") =>
        {
            Some(format!(
                "DROP {obj} … CASCADE deletes every column that depends on it, with its values."
            ))
        }
        ("CREATE", Some("OR")) if word(2) == Some("REPLACE") => match word(3)? {
            "TABLE" => Some(
                "CREATE OR REPLACE TABLE deletes the existing table and every row in it."
                    .to_string(),
            ),
            obj @ ("DATABASE" | "SCHEMA") => Some(format!(
                "CREATE OR REPLACE {obj} deletes the existing {} and every table in it.",
                obj.to_ascii_lowercase()
            )),
            _ => None,
        },
        ("ALTER", _) => {
            let at = words.windows(2).position(|w| {
                matches!(w[0].as_str(), "TRUNCATE" | "DROP") && w[1] == "PARTITION"
            })?;
            let why = match words[at].as_str() {
                "TRUNCATE" => "TRUNCATE PARTITION removes every row in the partitions it names.",
                _ => "DROP PARTITION deletes the partition and every row in it.",
            };
            Some(why.to_string())
        }
        _ => None,
    }
}

/// How far into an `ALTER TABLE` [`drop_reason`] looks for its partition
/// clause: `ALTER [ONLINE] [IGNORE] TABLE db . t TRUNCATE PARTITION` is seven
/// words, and a bound keeps the look at a statement's head a look at its head.
const PARTITION_WORDS: usize = 10;

/// The statement an `EXPLAIN ANALYZE` (PostgreSQL, MySQL 8) or MariaDB's bare
/// `ANALYZE` **runs**, if `stmt` is one: the text after the prefix.
///
/// Those prefixes execute what they explain — `EXPLAIN ANALYZE DELETE FROM t`
/// deletes every row of `t` — so the guard's head-based arms have to judge the
/// statement underneath, or the prefix is a way round every one of them. A
/// plain `EXPLAIN` only plans and is `None`, as is `ANALYZE TABLE t` / PG's
/// `ANALYZE t` (their remainder is no statement an arm matches). The options
/// PostgreSQL writes in parentheses count as analysing when `ANALYZE` is among
/// them — `ANALYZE false` included, since over-asking is the safe direction.
fn analyzed_statement(stmt: &str, dialect: SqlDialect) -> Option<&str> {
    let head = leading_keyword(stmt, dialect)?;
    let mut i = leading_keyword_end(stmt, dialect)?;
    let mut analyzed = head == "ANALYZE";
    if head != "EXPLAIN" && !analyzed {
        return None;
    }
    let b = stmt.as_bytes();
    loop {
        i = skip_blank(b, i, dialect);
        if head == "EXPLAIN" && b.get(i) == Some(&b'(') {
            let close = matching_paren(b, i, dialect)?;
            analyzed |= leading_words(&stmt[i + 1..close], usize::MAX, dialect)
                .iter()
                .any(|w| w == "ANALYZE");
            i = close + 1;
            continue;
        }
        let rest = &stmt[i..];
        match leading_keyword(rest, dialect).as_deref() {
            Some("ANALYZE") => analyzed = true,
            Some("VERBOSE" | "EXTENDED" | "PARTITIONS") => {}
            // `FORMAT=TREE` / `FORMAT = JSON`: the word, the `=`, the value.
            Some("FORMAT") => {
                let at = skip_blank(b, i + leading_keyword_end(rest, dialect)?, dialect);
                if b.get(at) != Some(&b'=') {
                    break;
                }
                let value = &stmt[at + 1..];
                i = at + 1 + leading_keyword_end(value, dialect)?;
                continue;
            }
            _ => break,
        }
        i += leading_keyword_end(rest, dialect)?;
    }
    analyzed.then(|| &stmt[i..])
}

/// `i` moved past whitespace and comments.
fn skip_blank(b: &[u8], mut i: usize, dialect: SqlDialect) -> usize {
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        match skip_comment(b, i, dialect) {
            Some(j) if i < b.len() => i = j,
            _ => return i,
        }
    }
}

/// The index of the `)` closing the `(` at `open`, skipping strings, quoted
/// identifiers and comments; `None` when it is never closed.
fn matching_paren(b: &[u8], open: usize, dialect: SqlDialect) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
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

/// If `stmt` would rewrite or erase every row of a table (DELETE/UPDATE
/// without a top-level WHERE, TRUNCATE, or a data-modifying CTE that names no
/// row), the warning to show the user; else `None`.
///
/// Its own question, apart from [`unsafe_reason`], because the `.sql` panel
/// counts exactly these (`script::Probe::unqualified`) under a line that says
/// so — and counts a `DROP` separately, as destruction.
///
/// Under an `EXPLAIN ANALYZE` (or MariaDB's `ANALYZE`) it judges the statement
/// the prefix runs ([`analyzed_statement`]); and MySQL's `ALTER TABLE …
/// TRUNCATE PARTITION ALL` is a `TRUNCATE` by another name.
///
/// **Each T-SQL statement in `stmt` is judged**, not its first alone: with no
/// `;` between them, a bare `DELETE` on the line below a scoped one borrowed
/// that one's `WHERE` — see [`tsql_statements`].
pub fn every_row_reason(stmt: &str, dialect: SqlDialect) -> Option<String> {
    tsql_statements(stmt, dialect)
        .into_iter()
        .find_map(|s| one_every_row_reason(s, dialect))
}

/// [`every_row_reason`] for one statement.
fn one_every_row_reason(stmt: &str, dialect: SqlDialect) -> Option<String> {
    let stmt = analyzed_statement(stmt, dialect).unwrap_or(stmt);
    match leading_keyword(stmt, dialect)?.as_str() {
        "ALTER" => {
            let words = leading_words(stmt, PARTITION_WORDS + 1, dialect);
            words
                .windows(3)
                .any(|w| w[0] == "TRUNCATE" && w[1] == "PARTITION" && w[2] == "ALL")
                .then(|| "TRUNCATE PARTITION ALL removes every row in the table.".to_string())
        }
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

impl GuardPolicy {
    /// Assemble the policy from the connection the run will go to.
    ///
    /// **The decision the write guard acts on, lifted out of the window.** It was
    /// the closure `guard_policy` inside `app_view` — a 9,600-line function, so
    /// nothing in it was nameable, callable or testable — while the invariant it
    /// serves says the *decision* (`run_verdict`) is pure and tested. The
    /// decision was; its three inputs were not, and that is what this ends.
    ///
    /// **And `script_view` assembles the same policy**, which is the other half
    /// of why this is a constructor rather than a struct literal at each site:
    /// two independent assemblies that happen to agree are not the same policy,
    /// they are two policies nobody is comparing.
    ///
    /// `conn` is `None` when the tab's connection is gone. The engine then falls
    /// back to [`SqlDialect::default`] and `read_only` to `false`, matching
    /// `connection::read_only_of`'s documented fail-open: the run that follows
    /// fails on the missing connection rather than on a flag nobody set.
    pub fn of(
        conn: Option<&crate::connection::Connection>,
        no_database: bool,
        confirm_writes: bool,
    ) -> GuardPolicy {
        GuardPolicy {
            read_only: crate::connection::read_only_ref(conn),
            confirm_writes,
            dialect: conn.map_or_else(SqlDialect::default, |c| {
                SqlDialect::from_db_type(&c.db_type)
            }),
            no_database,
        }
    }
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
///
/// **SQL Server asks too**, for the same reason: a connection naming no
/// database is in the login's default one, `master` unless someone changed it.
/// Its server-level objects are different ones — a `USER` lives in a database
/// there, a `LOGIN` does not — so it has its own arm.
pub fn needs_database(sql: &str, dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::Postgres => {}
        SqlDialect::MsSql => return mssql_needs_database(sql),
        SqlDialect::MySql | SqlDialect::Sqlite => return false,
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

/// [`needs_database`]'s SQL Server arm.
///
/// Reads, `USE` (which is how a batch picks its database), session or
/// transaction control and flow control leave nothing behind, and neither do
/// the statements about the server rather than a database in it — `KILL`,
/// `BACKUP`, `RESTORE`, `DBCC`, `RECONFIGURE`, `SHUTDOWN`. The server-level
/// objects are the ones that live in no database: databases, logins, server
/// roles, endpoints, credentials, server audits and availability groups.
/// `EXEC` needs one, since a procedure can create anything, and so does a
/// `SELECT … INTO`, which creates the table it names.
///
/// **Every statement in the range is asked, in order** ([`tsql_statements`]):
/// T-SQL needs no `;`, so `SET NOCOUNT ON` on the line above a `CREATE TABLE`
/// is one range, and its head alone sent the table to `master`. A `USE`
/// answers for everything after it.
fn mssql_needs_database(sql: &str) -> bool {
    let dialect = SqlDialect::MsSql;
    for stmt in tsql_statements(sql, dialect) {
        let Some(kw) = leading_keyword(stmt, dialect) else {
            continue;
        };
        match kw.as_str() {
            "USE" => return false,
            // A read — unless it writes a table, or its common table
            // expressions feed a statement that does.
            "SELECT" | "WITH" => {
                let words = word_tokens(stmt, dialect).0;
                let writes = |k: usize| match words[k].as_str() {
                    "INTO" | "INSERT" | "UPDATE" | "DELETE" => true,
                    // `INNER MERGE JOIN` is a join hint.
                    "MERGE" => words.get(k + 1).map(String::as_str) != Some("JOIN"),
                    _ => false,
                };
                if (0..words.len()).any(writes) {
                    return true;
                }
            }
            "BEGIN" | "COMMIT" | "ROLLBACK" | "SAVE" | "SET" | "DECLARE" | "PRINT"
            | "CHECKPOINT" | "WAITFOR" | "IF" | "WHILE" | "RETURN" | "THROW" | "RAISERROR"
            | "KILL" | "BACKUP" | "RESTORE" | "DBCC" | "RECONFIGURE" | "SHUTDOWN" => {}
            "CREATE" | "DROP" | "ALTER" => {
                let rest = leading_keyword_end(stmt, dialect)
                    .map(|e| &stmt[e..])
                    .unwrap_or("");
                let obj = leading_keyword(rest, dialect).unwrap_or_default();
                if !matches!(
                    obj.as_str(),
                    "DATABASE" | "LOGIN" | "SERVER" | "ENDPOINT" | "CREDENTIAL" | "AVAILABILITY"
                ) {
                    return true;
                }
                // `DATABASE SCOPED …`, `DATABASE CURRENT`, `DATABASE AUDIT
                // SPECIFICATION` and `DATABASE ENCRYPTION KEY` name the current
                // database or something inside it: on an unscoped tab, `master`.
                if obj == "DATABASE" {
                    let after = leading_keyword_end(rest, dialect)
                        .map(|e| &rest[e..])
                        .unwrap_or("");
                    let next = leading_keyword(after, dialect).unwrap_or_default();
                    if matches!(next.as_str(), "SCOPED" | "CURRENT" | "AUDIT" | "ENCRYPTION") {
                        return true;
                    }
                }
            }
            _ => return true,
        }
    }
    false
}

/// Does this piece hold more than one T-SQL statement — so that what follows
/// its first result, or its row cap, is statements the server will still run?
///
/// [`tsql_statements`]' count, and so it errs toward *several*: a cut in the
/// wrong place makes a reader drain a result it could have left, which costs
/// time and never an outcome. `false` on every other engine.
pub fn holds_several_statements(sql: &str, dialect: SqlDialect) -> bool {
    tsql_statements(sql, dialect).len() > 1
}

/// The statements one T-SQL range holds; any other dialect's range, whole.
///
/// **T-SQL needs no `;` between statements**, so a range that
/// [`statement_ranges`] cut at `;` and `GO` can hold several — `SET NOCOUNT
/// ON` and a `CREATE TABLE` on the next line, a scoped `DELETE` and a bare one
/// — and a guard that reads a range's head judges its first statement alone.
/// This cuts the range again before each top-level word that begins a
/// statement.
///
/// **Not a parser**, and it does not have to be: it finds where the
/// statements the guards ask about *begin*, and a cut it makes in the wrong
/// place produces a fragment no guard answers for. It does not cut where
/// the word belongs to the statement before it — a set operator's second
/// `SELECT`, `MERGE`'s `THEN UPDATE`, a cursor's `FOR UPDATE`, a foreign key's
/// `ON DELETE CASCADE` — nor after the head of a statement whose text runs on
/// to the end: a procedure, function, trigger or view is one statement
/// whatever it holds, and a `GRANT`'s privilege list is made of those words.
/// A leading `WITH` keeps the statement its common table expressions feed.
pub(crate) fn tsql_statements(stmt: &str, dialect: SqlDialect) -> Vec<&str> {
    if !dialect.batch_separator() {
        return vec![stmt];
    }
    // The top-level tokens, each a word (upper-cased, with where it starts)
    // or `None` for anything else — punctuation, a literal, a quoted name, a
    // variable, a whole parenthesised group.
    let b = stmt.as_bytes();
    let mut toks: Vec<Option<(usize, String)>> = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, dialect) {
            if depth == 0 && skip_comment(b, i, dialect).is_none() {
                toks.push(None);
            }
            i = j;
            continue;
        }
        let c = b[i];
        if is_word_start(c) {
            let s = i;
            i += 1;
            // To where `continues_name` ends the name, so `a$delete` and
            // `h#update` are one word each and no `DELETE`/`UPDATE`.
            while i < b.len() && continues_name(b[i], dialect) {
                i += 1;
            }
            // `@delete` is a variable, `#update` a temporary table and
            // `x.select` a qualified name: none is the keyword it spells.
            let named = s > 0 && matches!(b[s - 1], b'@' | b'#' | b'.');
            if depth == 0 {
                toks.push((!named).then(|| (s, stmt[s..i].to_ascii_uppercase())));
            }
            continue;
        }
        match c {
            b'(' => {
                if depth == 0 {
                    toks.push(None);
                }
                depth += 1;
            }
            b')' => depth = depth.saturating_sub(1),
            c if depth == 0 && !c.is_ascii_whitespace() => toks.push(None),
            _ => {}
        }
        i += 1;
    }
    let word = |k: usize| {
        toks.get(k)
            .and_then(|t| t.as_ref())
            .map(|(_, w)| w.as_str())
    };
    let module = |k: usize| {
        matches!(
            word(k),
            Some("PROC" | "PROCEDURE" | "FUNCTION" | "TRIGGER" | "VIEW")
        )
    };
    // Does the statement headed at `k` run to the end of the range?
    let runs_to_end = |k: usize| match word(k) {
        Some("CREATE") => {
            module(k + 1)
                || (word(k + 1) == Some("OR") && word(k + 2) == Some("ALTER") && module(k + 3))
        }
        Some("ALTER") => module(k + 1),
        Some("GRANT" | "REVOKE" | "DENY") => true,
        _ => false,
    };
    // `EXPLAIN` and `ANALYZE` are no T-SQL, and whole is how the guards'
    // `analyzed_statement` reads the prefixes other engines give them.
    let prefixed = matches!(word(0), Some("EXPLAIN" | "ANALYZE"));
    let mut cuts = Vec::new();
    let mut cte = word(0) == Some("WITH");
    if !runs_to_end(0) && !prefixed {
        for (k, tok) in toks.iter().enumerate().skip(1) {
            let Some((at, w)) = tok else { continue };
            let (prev, next) = (word(k - 1), word(k + 1));
            let continues = match w.as_str() {
                "SELECT" => matches!(
                    prev,
                    Some("UNION" | "ALL" | "EXCEPT" | "INTERSECT" | "FOR" | "AS")
                ),
                "UPDATE" | "DELETE" => {
                    matches!(prev, Some("THEN" | "FOR"))
                        || (prev == Some("ON")
                            && matches!(next, Some("CASCADE" | "SET" | "NO" | "RESTRICT")))
                }
                "INSERT" => prev == Some("THEN"),
                // `INNER MERGE JOIN` is a join hint, not a `MERGE`.
                "MERGE" => next == Some("JOIN"),
                // `DROP TABLE IF EXISTS t` is one statement.
                "IF" => prev.is_some() && next == Some("EXISTS"),
                // An `ALTER TABLE`'s partition clause, and its `DROP COLUMN`,
                // `DROP CONSTRAINT` and `DROP PERIOD`, none of them a
                // statement of their own.
                "TRUNCATE" => next == Some("PARTITION"),
                "DROP" => matches!(next, Some("PARTITION" | "COLUMN" | "CONSTRAINT" | "PERIOD")),
                "CREATE" | "ALTER" | "DECLARE" | "WHILE" | "PRINT" | "EXEC" | "EXECUTE"
                | "BEGIN" | "COMMIT" | "ROLLBACK" | "USE" | "RETURN" | "GRANT" | "REVOKE"
                | "DENY" => false,
                _ => continue,
            };
            if continues {
                continue;
            }
            // The first statement after a leading `WITH` is the one it feeds.
            if std::mem::take(&mut cte) {
                continue;
            }
            cuts.push(*at);
            if runs_to_end(k) {
                break;
            }
        }
    }
    let mut out = Vec::with_capacity(cuts.len() + 1);
    let mut from = 0;
    for at in cuts {
        out.push(stmt[from..at].trim());
        from = at;
    }
    out.push(stmt[from..].trim());
    out.retain(|s| !s.is_empty());
    if out.is_empty() {
        out.push(stmt);
    }
    out
}

/// How a statement failed for want of a database — see [`no_database_failure`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoDatabaseFailure {
    /// The server refused to run it anywhere.
    Refused,
    /// The server ran it — in a database nobody chose — and it failed there.
    RanElsewhere,
}

/// Did this server error happen **because the statement ran with no
/// database** — so naming one is the fix? And if so, did it run at all?
///
/// `message` is the error as the driver layer reported it (`DbError`'s text),
/// and only the caller knows the other half: that nothing was selected. Asked of
/// an error from a scoped run, a match here is still a missing table, not a
/// missing database.
///
/// Per engine, because each says it differently — and one does not say it:
///
/// - **MySQL/MariaDB** refuse outright: `ERROR 1046 (3D000): No database
///   selected`. Matched on the code and SQLSTATE, which the driver prints ahead
///   of the message and neither server localises.
/// - **PostgreSQL** never refuses. An unscoped connection lands in a
///   maintenance database (see [`GuardPolicy::no_database`]), so the failure is
///   the table not being *there*: `relation "company" does not exist`
///   (`42P01`). The driver layer carries the server's words without the code,
///   so this reads the English text; a server with a translated `lc_messages`
///   gets no match, which only loses the hint.
/// - **SQLite** has no database to leave out — the file is the database.
///
/// An exhaustive `match` rather than a comparison, so a fourth engine has to
/// say how it spells this rather than falling onto one side of a `!=`.
pub fn no_database_failure(message: &str, dialect: SqlDialect) -> Option<NoDatabaseFailure> {
    match dialect {
        SqlDialect::MySql => message
            .contains("ERROR 1046 (3D000)")
            .then_some(NoDatabaseFailure::Refused),
        // The message has to *be* about the relation — at the start, or after
        // `DbError`'s `query failed: ` — since `column "x" of relation "t" does
        // not exist` names one that is there.
        SqlDialect::Postgres => ((message.starts_with("relation \"")
            || message.contains(": relation \""))
            && message.contains("\" does not exist"))
        .then_some(NoDatabaseFailure::RanElsewhere),
        SqlDialect::Sqlite => None,
        // SQL Server never refuses either: an unscoped connection is in the
        // login's default database, and the failure is error 208 there. The
        // text is the server's, in English unless the login's language says
        // otherwise, which only loses the hint.
        SqlDialect::MsSql => message
            .contains("Invalid object name '")
            .then_some(NoDatabaseFailure::RanElsewhere),
    }
}

/// What the write guard says when a statement [`needs_database`] and none is
/// selected — the words MySQL's own ERROR 1046 uses. Named, so a front end that
/// adds a hint to this refusal recognises it by identity rather than by
/// re-typing it.
pub const NO_DATABASE_SELECTED: &str = "No database selected.";

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
    // **Where the read-only session is only a rollback, the rollback is not the
    // whole guard.** A `RAISERROR … WITH LOG`, or a function that calls an
    // extended procedure, writes nothing `contains_write` can see, and its
    // effect outside the database outlives the rolled-back transaction —
    // which is why the headless paths and Analyze ask `read_only_reason` on
    // such a connection. The editor runs the same statement against the same
    // guarantee, so it asks the same gate; anything laxer here would be a
    // second gate.
    if policy.read_only && policy.dialect.read_only_is_a_rollback() {
        for s in stmts {
            if let Err(reason) = read_only_reason(s, policy.dialect) {
                return RunVerdict::Block(format!("Read-only connection: {reason}"));
            }
        }
    }
    if policy.no_database && stmts.iter().any(|s| needs_database(s, policy.dialect)) {
        // MySQL's own message, deliberately: there it is the *server* that
        // refuses (ERROR 1046), because the connection simply carries no
        // database. PostgreSQL's carries a hidden one instead, so the refusal has
        // to come from here — and it should read the same either way.
        return RunVerdict::Block(NO_DATABASE_SELECTED.to_string());
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

/// May this statement be **re-run to stream an export**?
///
/// The whole-table export takes the statement a result came from and executes it
/// a second time, uncapped, on a fresh connection. That is a path executing user
/// SQL, so it answers to the write guard like every other one — but it answers a
/// *narrower* question than [`run_verdict`], and deliberately so.
///
/// `run_verdict` can say `Confirm`: on a writable connection a statement that
/// modifies data is allowed through once the user agrees, because they typed it
/// and pressed Run. **An export has no such moment.** The user picked a file name
/// from a Save dialog; nothing about that asks to run an `UPDATE … RETURNING` a
/// second time, and a confirmation raised from a file picker would be a question
/// about something they never requested. So this is a flat refusal rather than a
/// verdict with a `Confirm` arm — strictly stronger than the guard, never weaker,
/// which is what keeps it from being a second, laxer gate.
///
/// Built on [`contains_write`], the same predicate `run_verdict` uses, so there
/// is one answer to "does this statement write" and one place to grep. That
/// predicate is a **whitelist of read heads**, which is what makes it right here:
/// a `CALL proc()`, an `INSERT … RETURNING` and a data-modifying CTE are all
/// writes to it, and each of the three returns rows, so each could otherwise have
/// reached a truncated grid and been offered an "All rows" export.
pub fn rerunnable_for_export(sql: &str, dialect: SqlDialect) -> bool {
    !contains_write(sql, dialect)
}

/// May this **script file** be run, and what has to be agreed to first?
///
/// Running a `.sql` file is a path executing user SQL, so it answers to the write
/// guard like every other one — and, like [`rerunnable_for_export`], it answers a
/// question [`run_verdict`] cannot be asked. `run_verdict` takes the statements;
/// a script has tens of thousands of them, arriving a block at a time, and none
/// of them read yet at the moment the user presses the button. There is no
/// `&[String]` to hand it and no meaning to a confirmation raised at statement
/// 30,000.
///
/// So the gate is **one decision at launch, strictly stronger than the guard on
/// every axis** — never a second, laxer one:
///
/// - **A script is unconditionally a write.** `run_verdict` asks `contains_write`
///   and lets a pure read through; here there is nothing to ask, so a read-only
///   connection is refused *without* reading the file. A script that turned out
///   to hold nothing but `SELECT`s is refused too, which is the safe direction.
/// - **It never returns [`RunVerdict::Allow`]**, whatever `confirm_writes` says.
///   That setting is about a statement the user typed and can see; a file they
///   picked from a dialog is the case it was written for, not an exception to it.
///
/// **Who satisfies the `Confirm`.** Not a bar on screen: `script_view`'s panel
/// is the confirmation, and it is a better one than this message — it names the
/// statement counts and, in red, how many of them destroy or delete data, all
/// before a button marked Run. The caller matches this verdict exhaustively so
/// that is a decision written down rather than a fall-through; the `Confirm`
/// arm remains so that no caller can ever read this as "go ahead".
/// - **No database refuses outright**, rather than only when some statement
///   `needs_database` — see [`GuardPolicy::no_database`] for what an unscoped
///   `CREATE TABLE` does to a PostgreSQL server.
///
/// `file` names the file in the confirmation, because that is the only thing the
/// user can still recognise at this point — they have read no statements either.
pub fn script_verdict(policy: GuardPolicy, file: &str) -> RunVerdict {
    if policy.read_only {
        return RunVerdict::Block("Read-only connection.".to_string());
    }
    if policy.no_database {
        return RunVerdict::Block(NO_DATABASE_SELECTED.to_string());
    }
    RunVerdict::Confirm(format!(
        "Run every statement in {file}? A script can create, alter and drop \
         objects, and what it changes cannot be undone from here."
    ))
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

/// Keywords that make a statement non-read-only or dangerous on **every**
/// engine, matched as whole top-level tokens (outside strings / identifiers /
/// comments). The AI consumes untrusted result data, so it must not mutate,
/// lock, sleep, or touch the filesystem — this is a security boundary
/// (review C7).
///
/// The engine-specific half is [`deny_keywords_for`], and the split is not
/// cosmetic: this list *was* the whole gate, and every filesystem entry in it
/// was a MySQL spelling, so `SELECT pg_read_file('/etc/hostname')` passed on
/// PostgreSQL and handed a server-side file to the CLI vendor.
const DENY_ANY_ENGINE: &[&str] = &[
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
    "ANALYZE",
    "OPTIMIZE",
    "REPAIR",
];

/// The half of the AI deny list that only means something on `dialect`.
///
/// **Per dialect for exactly the reason [`read_only_heads`] is** — "the engines
/// don't have the same statements" — and the gate one hundred lines above it was
/// not, for a year. Each engine's filesystem, sleep and lock primitives have
/// nothing in common but their effect: `LOAD_FILE` is MySQL's, `pg_read_file` is
/// PostgreSQL's, `readfile` is a SQLite CLI extension, and a shared list that
/// held one engine's spellings silently passed the other two.
///
/// Two things to know before adding a name:
///
/// - **`_` is a word byte** to [`word_tokens`], so a name with an underscore
///   must be written out in full. `LOAD` does *not* cover `LOAD_FILE` — it
///   catches only `LOAD DATA INFILE`, which is how MySQL's own read primitive
///   stayed open while the list appeared to name it.
/// - **This list only ever over-blocks a column name**, and the correct
///   direction for a refusal gate is to over-block. But an entry on the wrong
///   engine is still a wrong answer, which is why
///   `the_deny_list_names_only_this_engines_spellings` pins that too.
fn deny_keywords_for(dialect: SqlDialect) -> &'static [&'static str] {
    match dialect {
        SqlDialect::MySql => &[
            "LOAD_FILE",
            "OUTFILE",
            "DUMPFILE",
            "SLEEP",
            "BENCHMARK",
            "GET_LOCK",
            "RELEASE_LOCK",
        ],
        // **No function is named here**: PostgreSQL's calls are answered by an
        // allowlist, [`PG_READ_FUNCTIONS`], because an enumeration of the
        // writers failed open with every release and every extension. What
        // is left is what is not a call.
        //
        // `COPY` because on PostgreSQL it reads *and writes* server-side files
        // (`COPY t FROM '/etc/passwd'`), while MySQL has no such statement and
        // SQLite's is a shell dot-command. `UESCAPE` because it puts a string
        // literal between a quoted name and its `(` — `U&"…" UESCAPE '!' (1)`
        // is one identifier and a call to the server — and rewrites the name
        // the allowlist would have compared.
        SqlDialect::Postgres => &["COPY", "UESCAPE"],
        // `ATTACH` is the SQLite hole that has no analogue: it opens *another*
        // database file by path, so a model that may `SELECT` can read any
        // SQLite file on the machine and — with a writable page — create one.
        SqlDialect::Sqlite => &[
            "ATTACH",
            "DETACH",
            "READFILE",
            "WRITEFILE",
            "LOAD_EXTENSION",
            "EDIT",
            "VACUUM",
            "REINDEX",
        ],
        // T-SQL's own. Its statements need no `;` between them, so each of
        // these may follow a `SELECT` as a second statement, and the scan over
        // every word is what finds it there. `EXEC` is `EXECUTE`'s short form,
        // and a procedure can do anything — `xp_cmdshell` included. `INTO` is
        // `SELECT … INTO`, which creates a table. `BEGIN` opens a transaction
        // or a Service Broker dialog. `OPENROWSET(BULK …)` reads a file on the
        // server; `OPENQUERY`/`OPENDATASOURCE` send text to another server,
        // which this gate cannot read. `WHILE` and `GOTO` are T-SQL's
        // `BENCHMARK`: a loop writes nothing and runs until it is stopped. The
        // table hints are named as locks by [`locking_clause`].
        SqlDialect::MsSql => &[
            "EXEC",
            "DECLARE",
            "WAITFOR",
            "WHILE",
            "GOTO",
            "BACKUP",
            "RESTORE",
            "DBCC",
            "BULK",
            "OPENROWSET",
            "OPENQUERY",
            "OPENDATASOURCE",
            "RECONFIGURE",
            "DENY",
            "REVERT",
            "BEGIN",
            "RECEIVE",
            "CONVERSATION",
            "INTO",
            "THROW",
            "RAISERROR",
            "CHECKPOINT",
            "SETUSER",
            "UPDATETEXT",
            "WRITETEXT",
            "ENABLE",
            "DISABLE",
            "OPEN",
            "CLOSE",
            "UPDLOCK",
            "XLOCK",
            "TABLOCK",
            "TABLOCKX",
            "HOLDLOCK",
            "SERIALIZABLE",
            "REPEATABLEREAD",
        ],
    }
}

/// Is `word` refused by the AI gate on `dialect`?
///
/// The composition of the two halves, in one place, so no caller can check one
/// and miss the other.
fn is_denied(word: &str, dialect: SqlDialect) -> bool {
    DENY_ANY_ENGINE.contains(&word) || deny_keywords_for(dialect).contains(&word)
}

/// The functions a read-only query may call on `dialect`, or `None` where the
/// engine's calls are answered by its deny list instead.
///
/// **An allowlist where the function set is open.** MySQL's and SQLite's
/// builtins are small closed sets and their deny lists name the few that act;
/// PostgreSQL's are neither, and a deny list of its writers — file readers,
/// sleeps, advisory locks, replication slots, statistics resets, the functions
/// that evaluate SQL text — failed open with every release and every extension,
/// and could not speak about a function the database's own owner had written.
///
/// **SQL Server is the second such engine.** Its builtins are closed, but a
/// user-defined function may call an extended stored procedure, and a CLR
/// function may do whatever its assembly is permitted to — so a name the text
/// gate cannot vouch for is refused there too.
fn read_call_policy(dialect: SqlDialect) -> Option<CallPolicy> {
    match dialect {
        SqlDialect::Postgres => Some(CallPolicy {
            functions: PG_READ_FUNCTIONS,
            catalog_functions: PG_READ_FUNCTIONS,
            catalog: "pg_catalog",
            paren_keywords: PG_PAREN_KEYWORDS,
            double_colon_is_cast: true,
            quoted_names_a_builtin: true,
        }),
        SqlDialect::MsSql => Some(CallPolicy {
            functions: MSSQL_READ_FUNCTIONS,
            catalog_functions: MSSQL_SYS_READ_FUNCTIONS,
            catalog: "sys",
            paren_keywords: MSSQL_PAREN_KEYWORDS,
            double_colon_is_cast: false,
            quoted_names_a_builtin: false,
        }),
        SqlDialect::MySql | SqlDialect::Sqlite => None,
    }
}

/// How [`unlisted_call`] reads one dialect's calls.
struct CallPolicy {
    /// The functions known only to read, lower-case, as an unqualified call
    /// names them.
    functions: &'static [&'static str],
    /// The same, as a call qualified with [`Self::catalog`] names them. One
    /// list on PostgreSQL, where `pg_catalog.lower` is `lower`. Two on SQL
    /// Server, whose builtins take no schema and whose `sys` functions must
    /// be named with one — so an owner's `dbo.fn_my_permissions` is not
    /// vouched for by the system function it shares a name with.
    catalog_functions: &'static [&'static str],
    /// The one schema a qualified call may name and still be listed — the
    /// system's own. Any other is whatever the owner defined under the name.
    catalog: &'static str,
    /// Words that cannot name a function, standing before a `(` that is the
    /// grammar's rather than a call's.
    paren_keywords: &'static [&'static str],
    /// Is `::` a cast, so the name after it a type? PostgreSQL's reading. On
    /// SQL Server `Type::Method(…)` calls a static method of a CLR type, which
    /// is a call like any other.
    double_colon_is_cast: bool,
    /// Can a quoted name call a builtin? PostgreSQL resolves one against the
    /// stored lower-case name; SQL Server resolves one as a user-defined object.
    quoted_names_a_builtin: bool,
}

/// PostgreSQL builtins known to only read, lower-case as `pg_catalog` stores
/// them.
///
/// **What earns a place**: every overload reads — no file, no sleep or wait, no
/// lock, no signal to another backend, no session state (`set_config`,
/// `setseed`), no sequence (`nextval`), no large object, no SQL text evaluated
/// (`query_to_xml`, `ts_stat`, `ts_rewrite`, `dblink`, `crosstab`) and no write
/// the read-only session cannot see (replication slots, `pg_stat_reset`, WAL
/// switches). A name that is not here is refused, which includes every
/// function an extension or the database's owner defined *under a name of its
/// own* — the text gate cannot read a body, and a function's `STABLE` label is
/// its author's claim.
///
/// **A name is not a function**, and this is a list of names. PostgreSQL picks
/// the overload whose argument types match best across the whole search path,
/// so an owner's `public.lower(integer)` answers `SELECT lower(1)` though
/// `lower` is listed — measured on PG 16.15 — and an owner's implicit cast
/// calls its function with no name in the text at all. Both rest on the
/// read-only session, as a view's body does.
///
/// **Adding a name** needs only that claim checked against the documentation;
/// `every_listed_function_is_a_postgres_builtin` catches a misspelling, and a
/// name the list lacks costs a refusal, never a write.
///
/// A few entries are grammar rather than `pg_proc` rows — a type written with
/// its modifier (`character(10)`), `ROLLUP (…)`, a `TABLESAMPLE` method — and
/// are here because they sit before a `(` exactly as a call does.
const PG_READ_FUNCTIONS: &[&str] = &[
    // Aggregates.
    "count",
    "sum",
    "avg",
    "min",
    "max",
    "array_agg",
    "string_agg",
    "json_agg",
    "jsonb_agg",
    "json_agg_strict",
    "jsonb_agg_strict",
    "json_object_agg",
    "jsonb_object_agg",
    "json_object_agg_strict",
    "json_object_agg_unique",
    "json_object_agg_unique_strict",
    "jsonb_object_agg_strict",
    "jsonb_object_agg_unique",
    "jsonb_object_agg_unique_strict",
    "bool_and",
    "bool_or",
    "every",
    "bit_and",
    "bit_or",
    "bit_xor",
    "stddev",
    "stddev_pop",
    "stddev_samp",
    "variance",
    "var_pop",
    "var_samp",
    "corr",
    "covar_pop",
    "covar_samp",
    "regr_avgx",
    "regr_avgy",
    "regr_count",
    "regr_intercept",
    "regr_r2",
    "regr_slope",
    "regr_sxx",
    "regr_sxy",
    "regr_syy",
    "percentile_cont",
    "percentile_disc",
    "mode",
    "rank",
    "dense_rank",
    "percent_rank",
    "cume_dist",
    "range_agg",
    "range_intersect_agg",
    "xmlagg",
    "any_value",
    // Window functions.
    "row_number",
    "ntile",
    "lag",
    "lead",
    "first_value",
    "last_value",
    "nth_value",
    // Grammar call forms.
    "cast",
    "treat",
    "coalesce",
    "nullif",
    "greatest",
    "least",
    "row",
    "grouping",
    "rollup",
    "cube",
    "extract",
    "overlay",
    "position",
    "substring",
    "trim",
    "normalize",
    "current_time",
    "current_timestamp",
    "localtime",
    "localtimestamp",
    // Types written with a modifier, or called as a cast.
    "numeric",
    "decimal",
    "dec",
    "float",
    "float4",
    "float8",
    "int2",
    "int4",
    "int8",
    "char",
    "character",
    "bpchar",
    "varchar",
    "text",
    "name",
    "bit",
    "varbit",
    "time",
    "timetz",
    "timestamp",
    "timestamptz",
    "interval",
    "date",
    "bool",
    "cidr",
    "regclass",
    "oid",
    // Operators with a function of the same name, which `LIKE (…)` reaches.
    "like",
    "overlaps",
    // `TABLESAMPLE` methods.
    "bernoulli",
    "system",
    // Strings.
    "ascii",
    "bit_length",
    "btrim",
    "char_length",
    "character_length",
    "chr",
    "concat",
    "concat_ws",
    "convert",
    "convert_from",
    "convert_to",
    "decode",
    "encode",
    "format",
    "initcap",
    "left",
    "length",
    "lower",
    "lpad",
    "ltrim",
    "md5",
    "octet_length",
    "parse_ident",
    "quote_ident",
    "quote_literal",
    "quote_nullable",
    "regexp_count",
    "regexp_instr",
    "regexp_like",
    "regexp_match",
    "regexp_matches",
    "regexp_replace",
    "regexp_split_to_array",
    "regexp_split_to_table",
    "regexp_substr",
    "repeat",
    "replace",
    "reverse",
    "right",
    "rpad",
    "rtrim",
    "split_part",
    "starts_with",
    "string_to_array",
    "string_to_table",
    "strpos",
    "substr",
    "to_ascii",
    "to_hex",
    "translate",
    "unistr",
    "upper",
    "sha224",
    "sha256",
    "sha384",
    "sha512",
    "get_bit",
    "get_byte",
    "bit_count",
    "to_char",
    "to_number",
    "to_date",
    "to_timestamp",
    // Numbers.
    "abs",
    "cbrt",
    "ceil",
    "ceiling",
    "degrees",
    "div",
    "exp",
    "factorial",
    "floor",
    "gcd",
    "lcm",
    "ln",
    "log",
    "log10",
    "min_scale",
    "mod",
    "pi",
    "power",
    "radians",
    "round",
    "scale",
    "sign",
    "sqrt",
    "trim_scale",
    "trunc",
    "width_bucket",
    "random",
    "random_normal",
    "acos",
    "acosd",
    "asin",
    "asind",
    "atan",
    "atand",
    "atan2",
    "atan2d",
    "cos",
    "cosd",
    "cot",
    "cotd",
    "sin",
    "sind",
    "tan",
    "tand",
    "sinh",
    "cosh",
    "tanh",
    "asinh",
    "acosh",
    "atanh",
    "erf",
    "erfc",
    // Dates and times.
    "age",
    "clock_timestamp",
    "date_add",
    "date_bin",
    "date_part",
    "date_subtract",
    "date_trunc",
    "isfinite",
    "justify_days",
    "justify_hours",
    "justify_interval",
    "make_date",
    "make_interval",
    "make_time",
    "make_timestamp",
    "make_timestamptz",
    "now",
    "statement_timestamp",
    "timeofday",
    "transaction_timestamp",
    "timezone",
    // Networks.
    "abbrev",
    "broadcast",
    "family",
    "host",
    "hostmask",
    "masklen",
    "netmask",
    "network",
    "set_masklen",
    // JSON.
    "to_json",
    "to_jsonb",
    "array_to_json",
    "row_to_json",
    "json_build_array",
    "json_build_object",
    "jsonb_build_array",
    "jsonb_build_object",
    "json_object",
    "jsonb_object",
    "json_array_elements",
    "json_array_elements_text",
    "jsonb_array_elements",
    "jsonb_array_elements_text",
    "json_array_length",
    "jsonb_array_length",
    "json_each",
    "json_each_text",
    "jsonb_each",
    "jsonb_each_text",
    "json_extract_path",
    "json_extract_path_text",
    "jsonb_extract_path",
    "jsonb_extract_path_text",
    "json_object_keys",
    "jsonb_object_keys",
    "json_populate_record",
    "jsonb_populate_record",
    "json_populate_recordset",
    "jsonb_populate_recordset",
    "json_to_record",
    "jsonb_to_record",
    "json_to_recordset",
    "jsonb_to_recordset",
    "json_strip_nulls",
    "jsonb_strip_nulls",
    "jsonb_set",
    "jsonb_set_lax",
    "jsonb_insert",
    "jsonb_pretty",
    "json_typeof",
    "jsonb_typeof",
    "jsonb_path_exists",
    "jsonb_path_match",
    "jsonb_path_query",
    "jsonb_path_query_array",
    "jsonb_path_query_first",
    "jsonb_path_exists_tz",
    "jsonb_path_match_tz",
    "jsonb_path_query_tz",
    "jsonb_path_query_array_tz",
    "jsonb_path_query_first_tz",
    "json",
    "json_array",
    "json_arrayagg",
    "json_objectagg",
    // Arrays and sets.
    "array_append",
    "array_cat",
    "array_dims",
    "array_fill",
    "array_length",
    "array_lower",
    "array_ndims",
    "array_position",
    "array_positions",
    "array_prepend",
    "array_remove",
    "array_replace",
    "array_sample",
    "array_shuffle",
    "array_to_string",
    "array_upper",
    "cardinality",
    "trim_array",
    "unnest",
    "generate_subscripts",
    "generate_series",
    // Ranges.
    "isempty",
    "lower_inc",
    "upper_inc",
    "lower_inf",
    "upper_inf",
    "range_merge",
    "int4range",
    "int8range",
    "numrange",
    "tsrange",
    "tstzrange",
    "daterange",
    "int4multirange",
    "int8multirange",
    "nummultirange",
    "tsmultirange",
    "tstzmultirange",
    "datemultirange",
    "multirange",
    // Text search — not `ts_stat` or `ts_rewrite`, which take SQL text.
    "to_tsvector",
    "to_tsquery",
    "plainto_tsquery",
    "phraseto_tsquery",
    "websearch_to_tsquery",
    "ts_rank",
    "ts_rank_cd",
    "ts_headline",
    "setweight",
    "strip",
    "numnode",
    "querytree",
    "tsvector_to_array",
    "array_to_tsvector",
    "ts_delete",
    "ts_filter",
    "get_current_ts_config",
    // XML — not `query_to_xml` and its siblings, which take SQL text.
    "xmlcomment",
    "xmlconcat",
    "xmlelement",
    "xmlforest",
    "xmlpi",
    "xmlroot",
    "xmlparse",
    "xmlserialize",
    "xmlexists",
    "xmltable",
    "xmlattributes",
    "xmlnamespaces",
    "xml_is_well_formed",
    "xml_is_well_formed_document",
    "xml_is_well_formed_content",
    "xpath",
    "xpath_exists",
    // Enums, UUIDs, nulls.
    "enum_first",
    "enum_last",
    "enum_range",
    "gen_random_uuid",
    "num_nulls",
    "num_nonnulls",
    // The session and server, read.
    "current_database",
    "current_schema",
    "current_schemas",
    "current_setting",
    "current_query",
    "version",
    "pg_backend_pid",
    "pg_blocking_pids",
    "pg_safe_snapshot_blocking_pids",
    "pg_conf_load_time",
    "pg_postmaster_start_time",
    "pg_is_in_recovery",
    "pg_is_wal_replay_paused",
    "pg_current_wal_lsn",
    "pg_current_wal_insert_lsn",
    "pg_current_wal_flush_lsn",
    "pg_last_wal_receive_lsn",
    "pg_last_wal_replay_lsn",
    "pg_last_xact_replay_timestamp",
    "pg_wal_lsn_diff",
    "pg_walfile_name",
    "pg_jit_available",
    "pg_trigger_depth",
    "inet_client_addr",
    "inet_client_port",
    "inet_server_addr",
    "inet_server_port",
    "pg_my_temp_schema",
    "pg_is_other_temp_schema",
    "pg_listening_channels",
    "pg_notification_queue_usage",
    "pg_client_encoding",
    "getdatabaseencoding",
    "pg_encoding_to_char",
    "pg_char_to_encoding",
    "pg_input_is_valid",
    "pg_input_error_info",
    "pg_current_snapshot",
    // The catalog, read. `pg_logical_slot_peek_changes` leaves the slot where it was.
    "pg_typeof",
    "pg_column_size",
    "pg_column_compression",
    "pg_collation_for",
    "format_type",
    "to_regclass",
    "to_regtype",
    "to_regproc",
    "to_regprocedure",
    "to_regnamespace",
    "to_regrole",
    "to_regoper",
    "to_regoperator",
    "to_regcollation",
    "pg_get_viewdef",
    "pg_get_functiondef",
    "pg_get_function_arguments",
    "pg_get_function_identity_arguments",
    "pg_get_function_result",
    "pg_get_indexdef",
    "pg_get_constraintdef",
    "pg_get_triggerdef",
    "pg_get_ruledef",
    "pg_get_expr",
    "pg_get_serial_sequence",
    "pg_get_statisticsobjdef",
    "pg_get_partkeydef",
    "pg_get_partition_constraintdef",
    "pg_get_keywords",
    "pg_get_userbyid",
    "pg_options_to_table",
    "pg_index_column_has_property",
    "pg_index_has_property",
    "pg_indexam_has_property",
    "obj_description",
    "col_description",
    "shobj_description",
    "pg_describe_object",
    "pg_identify_object",
    "pg_identify_object_as_address",
    "pg_table_is_visible",
    "pg_type_is_visible",
    "pg_function_is_visible",
    "has_table_privilege",
    "has_column_privilege",
    "has_any_column_privilege",
    "has_database_privilege",
    "has_schema_privilege",
    "has_function_privilege",
    "has_sequence_privilege",
    "has_type_privilege",
    "has_tablespace_privilege",
    "has_foreign_data_wrapper_privilege",
    "has_server_privilege",
    "has_language_privilege",
    "has_parameter_privilege",
    "pg_has_role",
    "row_security_active",
    "pg_database_size",
    "pg_indexes_size",
    "pg_relation_size",
    "pg_table_size",
    "pg_total_relation_size",
    "pg_tablespace_size",
    "pg_size_pretty",
    "pg_size_bytes",
    "pg_partition_tree",
    "pg_partition_root",
    "pg_partition_ancestors",
    "pg_logical_slot_peek_changes",
    "pg_logical_slot_peek_binary_changes",
];

/// Unquoted words PostgreSQL's grammar puts before a `(` that is not a call's.
///
/// **Only words that cannot name a function**: PostgreSQL's *reserved* and
/// *column-name* keywords, which its grammar never accepts as an unquoted
/// function name. Each one skipped here is a name the allowlist never sees, so
/// a word that can also name one — the unreserved `EXPLAIN`, the
/// `type_func_name` keywords `JOIN`, `ILIKE`, `SIMILAR`, `LEFT` — is not here:
/// a builtin of that name belongs in [`PG_READ_FUNCTIONS`], and the rest are
/// grammar only where [`paren_is_grammar`] finds them in place, since an
/// owner's `join(1)` is a call like any other.
/// `no_word_skipped_before_a_paren_names_an_unlisted_builtin` holds the builtin
/// half of that line; the keyword categories are PostgreSQL's appendix C.
const PG_PAREN_KEYWORDS: &[&str] = &[
    "ALL",
    "AND",
    "ANY",
    "ARRAY",
    "AS",
    "BETWEEN",
    "BOTH",
    "CASE",
    "DISTINCT",
    "ELSE",
    "EXCEPT",
    "EXISTS",
    "FOR",
    "FROM",
    "GROUP",
    "HAVING",
    "IN",
    "INTERSECT",
    "LATERAL",
    "LEADING",
    "LIMIT",
    "NOT",
    "OFFSET",
    "ON",
    "ONLY",
    "OR",
    "SELECT",
    "SOME",
    "SYMMETRIC",
    "ASYMMETRIC",
    "THEN",
    "TO",
    "TRAILING",
    "UNION",
    "USING",
    "VALUES",
    "VARIADIC",
    "WHEN",
    "WHERE",
];

/// SQL Server builtins known to only read, lower-case, as an unqualified call
/// names them.
///
/// **What earns a place** is the same test as [`PG_READ_FUNCTIONS`]: every
/// form reads — no file, no wait, no session state beyond the statement, no
/// sequence, no SQL text evaluated. So `OPENROWSET`, `OPENQUERY` and
/// `OPENDATASOURCE` are absent (and deny-listed besides). `NEWID` makes a
/// value and changes nothing, so it is here; `RAND` with a seed reseeds the
/// connection's generator, and the connection ends with the query, so it is
/// here too.
///
/// A few entries are grammar rather than functions — a type written with its
/// length where `CONVERT` takes one (`nvarchar(20)`), and `FOR XML`/`FOR JSON`'s
/// `PATH`, `RAW` and `ROOT` — and are here because they sit before a `(`
/// exactly as a call does.
///
/// **Checked against SQL Server 2022 (16.0.4295).** It publishes no catalogue
/// of its builtins, so each name was called with no arguments: a name that is
/// not a builtin answers error 195 (*"is not a recognized built-in function
/// name"*), and every entry here answered something else — an argument count,
/// a missing `OVER`, the syntax a special form needs — except the three rowset
/// functions (`OPENJSON`, `STRING_SPLIT`, `GENERATE_SERIES`), which were run
/// in a `FROM`, and the grammar entries, which were run in place.
const MSSQL_READ_FUNCTIONS: &[&str] = &[
    // Aggregates.
    "avg",
    "checksum_agg",
    "count",
    "count_big",
    "grouping",
    "grouping_id",
    "max",
    "min",
    "stdev",
    "stdevp",
    "string_agg",
    "sum",
    "var",
    "varp",
    "approx_count_distinct",
    "approx_percentile_cont",
    "approx_percentile_disc",
    // Ranking and analytic.
    "row_number",
    "rank",
    "dense_rank",
    "ntile",
    "cume_dist",
    "percent_rank",
    "first_value",
    "last_value",
    "lag",
    "lead",
    "percentile_cont",
    "percentile_disc",
    // Strings.
    "ascii",
    "char",
    "charindex",
    "concat",
    "concat_ws",
    "datalength",
    "difference",
    "format",
    "left",
    "len",
    "lower",
    "ltrim",
    "nchar",
    "patindex",
    "quotename",
    "replace",
    "replicate",
    "reverse",
    "right",
    "rtrim",
    "soundex",
    "space",
    "str",
    "string_escape",
    "string_split",
    "stuff",
    "substring",
    "translate",
    "trim",
    "unicode",
    "upper",
    // Mathematics.
    "abs",
    "acos",
    "asin",
    "atan",
    "atn2",
    "ceiling",
    "cos",
    "cot",
    "degrees",
    "exp",
    "floor",
    "greatest",
    "least",
    "log",
    "log10",
    "pi",
    "power",
    "radians",
    "rand",
    "round",
    "sign",
    "sin",
    "sqrt",
    "square",
    "tan",
    // Dates and times.
    "dateadd",
    "date_bucket",
    "datediff",
    "datediff_big",
    "datefromparts",
    "datename",
    "datepart",
    "datetrunc",
    "datetime2fromparts",
    "datetimefromparts",
    "datetimeoffsetfromparts",
    "day",
    "eomonth",
    "getdate",
    "getutcdate",
    "isdate",
    "month",
    "smalldatetimefromparts",
    "switchoffset",
    "sysdatetime",
    "sysdatetimeoffset",
    "sysutcdatetime",
    "timefromparts",
    "todatetimeoffset",
    "year",
    // Conversion and logic.
    "cast",
    "convert",
    "parse",
    "try_cast",
    "try_convert",
    "try_parse",
    "choose",
    "coalesce",
    "iif",
    "isnull",
    "isnumeric",
    "nullif",
    // JSON. `JSON_MODIFY` returns a modified *copy* of its argument.
    "isjson",
    "json_array",
    "json_modify",
    "json_object",
    "json_path_exists",
    "json_query",
    "json_value",
    "openjson",
    // Bits, hashing, compression and generated values.
    "binary_checksum",
    "bit_count",
    "checksum",
    "compress",
    "decompress",
    "get_bit",
    "hashbytes",
    "left_shift",
    "newid",
    "right_shift",
    "set_bit",
    // Full-text predicates and the rowsets beside them.
    "contains",
    "containstable",
    "freetext",
    "freetexttable",
    // Rowset builders over their own arguments.
    "generate_series",
    // Metadata: names, ids and properties of the catalogue and the session.
    "app_name",
    "col_length",
    "col_name",
    "collationproperty",
    "columnproperty",
    "connectionproperty",
    "databasepropertyex",
    "db_id",
    "db_name",
    "file_id",
    "file_idex",
    "file_name",
    "filegroup_id",
    "filegroup_name",
    "filegroupproperty",
    "fileproperty",
    "has_perms_by_name",
    "host_id",
    "host_name",
    "ident_current",
    "ident_incr",
    "ident_seed",
    "index_col",
    "indexkey_property",
    "indexproperty",
    "is_member",
    "is_rolemember",
    "is_srvrolemember",
    "object_definition",
    "object_id",
    "object_name",
    "object_schema_name",
    "objectproperty",
    "objectpropertyex",
    "original_db_name",
    "parsename",
    "schema_id",
    "schema_name",
    "scope_identity",
    "serverproperty",
    "session_context",
    "sql_variant_property",
    "stats_date",
    "suser_id",
    "suser_name",
    "suser_sid",
    "suser_sname",
    "type_id",
    "type_name",
    "typeproperty",
    "user_id",
    "user_name",
    "xact_state",
    // Grammar: a type's length where `CONVERT` takes the type, and `FOR XML`/
    // `FOR JSON` options.
    "binary",
    "datetime2",
    "datetimeoffset",
    "decimal",
    "float",
    "numeric",
    "nvarchar",
    "time",
    "varbinary",
    "varchar",
    "path",
    "raw",
    "root",
];

/// [`MSSQL_READ_FUNCTIONS`], for the live tier's oracle in `schemaic-db`,
/// which asks a real server whether every name is a builtin. Public rather
/// than copied there, so the test reads the list the gate reads.
pub fn mssql_read_functions_for_test() -> &'static [&'static str] {
    MSSQL_READ_FUNCTIONS
}

/// [`MSSQL_SYS_READ_FUNCTIONS`], for the same oracle.
pub fn mssql_sys_read_functions_for_test() -> &'static [&'static str] {
    MSSQL_SYS_READ_FUNCTIONS
}

/// SQL Server's `sys` functions known to only read, as `sys.<name>(…)` calls
/// them — and only so qualified: see [`CallPolicy::catalog_functions`]. The
/// file readers (`fn_get_audit_file`, `fn_xe_file_target_read_file`,
/// `fn_trace_gettable`) and the log reader (`fn_dblog`) are absent on purpose.
const MSSQL_SYS_READ_FUNCTIONS: &[&str] = &[
    "dm_db_index_physical_stats",
    "dm_db_stats_properties",
    "dm_exec_cursors",
    "dm_exec_plan_attributes",
    "dm_exec_query_plan",
    "dm_exec_sql_text",
    "dm_exec_text_query_plan",
    "dm_sql_referenced_entities",
    "dm_sql_referencing_entities",
    "fn_builtin_permissions",
    "fn_helpcollations",
    "fn_listextendedproperty",
    "fn_my_permissions",
];

/// Words T-SQL's grammar puts before a `(` that is not a call's — its reserved
/// keywords, which can never name a function. `LEFT` and `RIGHT` are absent
/// because they are also string functions (listed above); a `LEFT JOIN (` is
/// found by [`paren_is_grammar`]'s `JOIN` rule instead.
const MSSQL_PAREN_KEYWORDS: &[&str] = &[
    "ALL",
    "AND",
    "ANY",
    "APPLY",
    "AS",
    "BETWEEN",
    "CASE",
    "DISTINCT",
    "ELSE",
    "EXCEPT",
    "EXISTS",
    "FROM",
    "HAVING",
    "IN",
    // The table hint `WITH (INDEX(ix))`.
    "INDEX",
    "INTERSECT",
    "IS",
    "JOIN",
    "LIKE",
    "NOT",
    "ON",
    "OPTION",
    "OR",
    "OVER",
    "PIVOT",
    "SELECT",
    "SOME",
    // `TABLESAMPLE (10 PERCENT)`, which takes no method name as PostgreSQL's
    // does.
    "TABLESAMPLE",
    "THEN",
    "TOP",
    "UNION",
    "UNPIVOT",
    "VALUES",
    "WHEN",
    "WHERE",
    "WITH",
];

/// One code token of a statement, for [`unlisted_call`]: what the word scan
/// has, plus the punctuation it throws away.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CallTok<'a> {
    /// An unquoted word, as written.
    Word(&'a str),
    /// A quoted identifier's name — case kept, because PostgreSQL does.
    Quoted(String),
    /// One byte of punctuation.
    Punct(u8),
    /// A literal or a number.
    Other,
}

impl CallTok<'_> {
    fn is_word(&self, upper: &str) -> bool {
        matches!(self, CallTok::Word(w) if w.eq_ignore_ascii_case(upper))
    }
    fn is_name(&self) -> bool {
        matches!(self, CallTok::Word(_) | CallTok::Quoted(_))
    }
}

/// `sql` as [`CallTok`]s, comments dropped, on [`skip_noncode`]'s boundaries.
fn call_tokens(sql: &str, dialect: SqlDialect) -> Vec<CallTok<'_>> {
    let b = sql.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if let Some(j) = skip_noncode(b, i, dialect).filter(|&j| j > i) {
            match noncode_kind(b, i, dialect) {
                Some(NonCode::Comment) => {}
                Some(NonCode::Identifier) => toks.push(CallTok::Quoted(
                    ident_at(sql, i, dialect).0.unwrap_or_default(),
                )),
                _ => toks.push(CallTok::Other),
            }
            i = j;
        } else if is_word_byte(c) {
            // `$` continues a name — `lower$(1)` calls `lower$` — which is why
            // `scan_dollar` opens no quote there; a number does not take one.
            // On T-SQL `#` and `@` do too: `dbo.f@GETDATE()` calls
            // `f@GETDATE`, not the allowlisted `GETDATE`.
            let start = i;
            let word = is_word_start(c);
            while i < b.len()
                && if word {
                    continues_dollar_name(b[i]) || continues_name(b[i], dialect)
                } else {
                    is_word_byte(b[i])
                }
            {
                i += 1;
            }
            toks.push(if is_word_start(c) {
                CallTok::Word(&sql[start..i])
            } else {
                CallTok::Other
            });
        } else {
            toks.push(CallTok::Punct(c));
            i += 1;
        }
    }
    toks
}

/// The index of the `)` that closes the `(` at `open`, if the statement has one.
fn closing_paren(t: &[CallTok], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (k, tok) in t.iter().enumerate().skip(open) {
        match tok {
            CallTok::Punct(b'(') => depth += 1,
            CallTok::Punct(b')') => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// The indices of every CTE name declared with a column list — `x` in
/// `WITH x(a, b) AS (…)` — which is a `name(` that declares rather than calls.
///
/// **Marked only when the whole header is there**: name, column list, `AS`,
/// an optional `[NOT] MATERIALIZED`, and the body's `(`. `WITH ORDINALITY AS
/// u(v, i)` and `WITH TIME ZONE` fail it at the step after `AS`, and a CTE
/// named after a function marks that position alone — the name called
/// elsewhere in the statement is still asked about.
fn cte_column_list_names(t: &[CallTok]) -> Vec<usize> {
    let mut names = Vec::new();
    for k in (0..t.len()).filter(|&k| t[k].is_word("WITH")) {
        let mut j = k + 1;
        if t.get(j).is_some_and(|x| x.is_word("RECURSIVE")) {
            j += 1;
        }
        while t.get(j).is_some_and(CallTok::is_name) {
            let mut m = j + 1;
            let listed = t.get(m) == Some(&CallTok::Punct(b'('));
            if listed {
                let Some(close) = closing_paren(t, m) else {
                    break;
                };
                m = close + 1;
            }
            if !t.get(m).is_some_and(|x| x.is_word("AS")) {
                break;
            }
            m += 1;
            if t.get(m).is_some_and(|x| x.is_word("NOT")) {
                m += 1;
            }
            if t.get(m).is_some_and(|x| x.is_word("MATERIALIZED")) {
                m += 1;
            }
            if t.get(m) != Some(&CallTok::Punct(b'(')) {
                break;
            }
            if listed {
                names.push(j);
            }
            let Some(close) = closing_paren(t, m) else {
                break;
            };
            if t.get(close + 1) != Some(&CallTok::Punct(b',')) {
                break;
            }
            j = close + 2;
        }
    }
    names
}

/// Is the name at `t[k]`, which a `(` follows, grammar rather than a call?
///
/// Four positions declare or type rather than call, whatever the word: after
/// `AS` (an alias's column list, `CAST(x AS numeric(10,2))`), after `::` (a
/// type's modifier), after `TABLESAMPLE` (the method is a handler that only
/// samples), and straight after a value — a `)`, a `]`, a literal or a quoted
/// name. The grammar never juxtaposes a value and a call, so there it is an
/// alias's column list (`(VALUES (1)) v(n)`, `"t" x(a)`) or a clause on an
/// aggregate (`OVER (…)`, `FILTER (…)`) — **except `OPERATOR(s.op)`**, which
/// stands exactly there and reaches an operator in any schema. The rest are
/// keywords only where they stand — `BY` after `ORDER`, `EXPLAIN` at the head,
/// `JOIN (` before a relation — so asking the word alone would have let a
/// function of that name through everywhere else.
///
/// SQL Server reads through the same positions with its own keyword list
/// ([`CallPolicy::paren_keywords`]), and with `::` a method call rather than a
/// cast. `GROUP` after `WITHIN` is an ordered aggregate's clause.
fn paren_is_grammar(t: &[CallTok], k: usize, policy: &CallPolicy) -> bool {
    if t[k].is_word("OPERATOR") {
        return false;
    }
    let prev = k.checked_sub(1).map(|p| &t[p]);
    let prev2 = k.checked_sub(2).map(|p| &t[p]);
    let after = |w: &str| prev.is_some_and(|p| p.is_word(w));
    if after("AS")
        || after("TABLESAMPLE")
        || matches!(
            prev,
            Some(CallTok::Punct(b')' | b']') | CallTok::Other | CallTok::Quoted(_))
        )
        || (policy.double_colon_is_cast
            && prev == Some(&CallTok::Punct(b':'))
            && prev2 == Some(&CallTok::Punct(b':')))
    {
        return true;
    }
    let CallTok::Word(w) = &t[k] else {
        return false;
    };
    let w = w.to_ascii_uppercase();
    policy.paren_keywords.contains(&w.as_str())
        || match w.as_str() {
            "BY" => after("ORDER") || after("GROUP") || after("PARTITION"),
            "GROUP" => after("WITHIN"),
            "SETS" => after("GROUPING"),
            "VARYING" => after("CHARACTER") || after("CHAR") || after("BIT"),
            "FIRST" | "NEXT" => after("FETCH"),
            "MATERIALIZED" => after("NOT"),
            "EXPLAIN" => k == 0,
            "JOIN" => {
                [
                    "INNER", "OUTER", "LEFT", "RIGHT", "FULL", "CROSS", "NATURAL",
                ]
                .iter()
                .any(|j| after(j))
                    || opens_a_relation(t, k + 1)
            }
            _ => false,
        }
}

/// Does the `(` at `open` hold a relation — a subquery or a parenthesised join
/// — rather than an argument list?
///
/// What makes `JOIN (` grammar after a table's name, where `join(` after a
/// keyword would be a call. A subquery opens with a query's head, which no
/// argument list can (`f(SELECT 1)` does not parse), and a parenthesised join
/// holds a `JOIN` of its own at its top level, which an argument list cannot
/// either: the word names no column, and a nested `join(…)` there is asked
/// about in its own right.
fn opens_a_relation(t: &[CallTok], open: usize) -> bool {
    if t.get(open + 1).is_some_and(|x| {
        ["SELECT", "VALUES", "WITH", "TABLE"]
            .iter()
            .any(|h| x.is_word(h))
    }) {
        return true;
    }
    let Some(close) = closing_paren(t, open) else {
        return false;
    };
    let mut depth = 0usize;
    for tok in &t[open + 1..close] {
        match tok {
            CallTok::Punct(b'(') => depth += 1,
            CallTok::Punct(b')') => depth = depth.saturating_sub(1),
            tok if depth == 0 && tok.is_word("JOIN") => return true,
            _ => {}
        }
    }
    false
}

/// The first call in `sql` to a function `allowed` does not list, as the
/// refusal should show it — `None` when every call is listed.
///
/// A call is a name before a `(`, quoted or not, and — PostgreSQL's own — a
/// name after a closing `)` or `]` and a `.`: field selection on a value that
/// is not a row is a call with no parentheses, and `('1'::float8).pg_sleep`
/// slept on PG 16.15. A **qualified** call is listed only in `pg_catalog`,
/// since `public.lower` is whatever the owner defined under that name.
///
/// **What it cannot see** is a call the text does not spell: a view's body, a
/// trigger, an operator's function, an implicit cast's, an owner's overload of
/// a listed name (see [`PG_READ_FUNCTIONS`]), and `alias.f` — `f` applied to a
/// whole row, which reaches only a function taking a row, all of them
/// `pg_catalog` formatters. Those rest on the read-only session.
///
/// **A token scan, not `core::intel`'s AST**, against the rule that structure
/// goes through the parser — and deliberately. A refusal gate has to fail
/// closed: here anything *spelled* as a call is asked about unless a named
/// grammar position says otherwise, so an unrecognised construct is refused.
/// An AST visitor fails the other way — it collects calls from the node shapes
/// it knows (`Expr::Function`, a table function, `UNNEST`, …), and a shape it
/// misses is a call nobody asked about, which is the enumeration this list
/// replaced. The parser is also not PostgreSQL's: a statement it rejects would
/// be refused outright, and one it reads differently from the server is the
/// disagreement every text gate here has been bypassed through.
///
/// **A quoted name is never a SQL Server builtin.** T-SQL resolves `[len](x)`
/// as a user-defined object, not as `LEN`, so on that dialect a quoted call is
/// refused whatever it spells; PostgreSQL resolves a quoted name against the
/// stored lower-case `proname`, and compares it as written.
fn unlisted_call(sql: &str, dialect: SqlDialect, policy: &CallPolicy) -> Option<String> {
    let t = call_tokens(sql, dialect);
    let ctes = cte_column_list_names(&t);
    let listed_in = |allowed: &[&str], tok: &CallTok| match tok {
        CallTok::Word(w) => allowed.contains(&w.to_ascii_lowercase().as_str()),
        CallTok::Quoted(q) => policy.quoted_names_a_builtin && allowed.contains(&q.as_str()),
        _ => false,
    };
    let listed = |tok: &CallTok| listed_in(policy.functions, tok);
    let shown = |tok: &CallTok| match tok {
        CallTok::Word(w) => w.to_string(),
        CallTok::Quoted(q) => format!("\"{q}\""),
        _ => String::new(),
    };
    for k in 0..t.len() {
        if !t[k].is_name() {
            continue;
        }
        let called = t.get(k + 1) == Some(&CallTok::Punct(b'('));
        let dotted = k >= 1 && t[k - 1] == CallTok::Punct(b'.');
        let before_dot = k.checked_sub(2).map(|p| &t[p]);
        if dotted && !called {
            if matches!(before_dot, Some(CallTok::Punct(b')' | b']'))) && !listed(&t[k]) {
                return Some(format!("(…).{}", shown(&t[k])));
            }
            continue;
        }
        if !called {
            continue;
        }
        if dotted && let Some(schema) = before_dot.filter(|q| q.is_name()) {
            // A quoted schema keeps its case on PostgreSQL; on SQL Server the
            // catalog's name is compared as the server's collation would,
            // case-insensitively.
            let in_catalog = match schema {
                CallTok::Word(w) => w.eq_ignore_ascii_case(policy.catalog),
                CallTok::Quoted(q) if policy.quoted_names_a_builtin => q == policy.catalog,
                CallTok::Quoted(q) => q.eq_ignore_ascii_case(policy.catalog),
                _ => false,
            };
            if !in_catalog || !listed_in(policy.catalog_functions, &t[k]) {
                return Some(format!("{}.{}", shown(schema), shown(&t[k])));
            }
            continue;
        }
        if ctes.contains(&k) || (!dotted && paren_is_grammar(&t, k, policy)) || listed(&t[k]) {
            continue;
        }
        return Some(shown(&t[k]));
    }
    None
}

/// The first byte at or after `i` that is neither whitespace nor inside a
/// comment — `None` if the input runs out first.
///
/// **The separator between a name and its `(` is not just whitespace.** Every
/// dialect's parser treats a comment there as a gap, so a rule asking "is the
/// next thing a `(`" has to ask this and not `is_ascii_whitespace`. Built on
/// [`skip_noncode`] like everything else that reads SQL boundaries here, and on
/// [`noncode_kind`] so that a *string* or a quoted *identifier* — which are
/// code, not gaps — stop the scan rather than being stepped over.
fn next_code_byte(b: &[u8], mut i: usize, dialect: SqlDialect) -> Option<u8> {
    let n = b.len();
    while i < n {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if noncode_kind(b, i, dialect) == Some(NonCode::Comment)
            && let Some(j) = skip_noncode(b, i, dialect)
            && j > i
        {
            i = j;
            continue;
        }
        return Some(b[i]);
    }
    None
}

/// Split SQL into upper-cased word tokens, skipping string/identifier/comment
/// content. The bool is set once a top-level `;` is followed by more real
/// content (i.e. the input is multiple statements).
fn word_tokens(sql: &str, dialect: SqlDialect) -> (Vec<String>, bool) {
    let b = sql.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut words = Vec::new();
    let mut word: Vec<u8> = Vec::new();
    let mut ended = false;
    let mut multi = false;
    macro_rules! flush {
        () => {
            if !word.is_empty() {
                if ended {
                    multi = true;
                }
                words.push(String::from_utf8_lossy(&std::mem::take(&mut word)).into_owned());
            }
        };
    }
    while i < n {
        if let Some(j) = skip_noncode(b, i, dialect) {
            flush!();
            // **A quoted name followed by `(` is a function call, and the deny
            // scan has to see it.** PostgreSQL resolves a double-quoted
            // identifier against the stored lower-case `pg_proc.proname`, so
            // `SELECT "pg_read_file"('/etc/passwd')` is the same call as the
            // unquoted one — but the whole run was skipped as non-code, the
            // name never became a token, and `is_denied` was never asked about
            // it. Every PostgreSQL entry was one character from reachable,
            // including `lo_export`, which writes a server-side file.
            //
            // Only before a `(`. A quoted *column* of the same spelling is a
            // read, and `SELECT "delete" FROM t` staying `Ok` is the case this
            // gate was deliberately tuned for.
            //
            // **`next_code_byte`, not "the first non-whitespace byte".** The
            // first spelling of this rule tested `!c.is_ascii_whitespace()`,
            // and a comment is not whitespace — so `"pg_read_file"/*x*/('…')`
            // put the name straight back out of reach, on a gate the MCP
            // server's `run_query` is the sole consumer of. Every engine skips
            // a comment between a name and its argument list exactly as it
            // skips a space.
            if noncode_kind(b, i, dialect) == Some(NonCode::Identifier)
                && next_code_byte(b, j, dialect) == Some(b'(')
                && let (Some(name), _) = ident_at(sql, i, dialect)
            {
                words.push(name.to_ascii_uppercase());
            }
            i = j;
            continue;
        }
        let c = b[i];
        if c == b';' {
            flush!();
            ended = true;
        } else if is_word_byte(c) || (!word.is_empty() && continues_name(c, dialect)) {
            // A name runs on where `continues_name` says — through `$` on every
            // engine, `#`/`@` on T-SQL — so `a$delete` is one column and not a
            // `DELETE`. Only a continuation: an empty word does not start at
            // `$`, PostgreSQL's `$1`.
            //
            // Invariant 11's third site here. The ASCII rule flushed at every
            // byte `>= 0x80`, so `cafédelete` arrived as the two tokens `CAFÃ`
            // and `DELETE` and `contains_write` answered true for a `SELECT`.
            // That direction is safe for a refusal gate — over-blocking is the
            // correct way to be wrong — but it is still a wrong answer, and the
            // reason `is_word_byte` is the one definition.
            //
            // A byte, not a `char`: pushing a `u8 as char` re-encodes a
            // continuation byte as a Latin-1 code point. The bytes are a
            // `&str`'s, so a word cut at [`is_word_byte`]'s boundaries is whole
            // UTF-8 and `from_utf8` cannot fail — but `_lossy` rather than an
            // `unwrap`, because a token is not worth a panic.
            word.push(c.to_ascii_uppercase());
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
        // No `EXPLAIN`: a T-SQL plan is `SET SHOWPLAN_XML ON`, session state.
        SqlDialect::MsSql => &["SELECT", "WITH"],
    }
}

/// Is `sql` a single read-only statement we're willing to run unattended?
/// Returns the rejection reason on failure.
///
/// **Two front ends now, so the wording names neither.** This was the AI's gate
/// alone and its refusals said so; `schemaic query` shares it, and a person
/// typing a `SLEEP()` at a prompt being told it "is not permitted in an AI
/// query" is being answered about somebody else's session.
pub fn read_only_reason(sql: &str, dialect: SqlDialect) -> Result<(), String> {
    read_only_refusal(sql, dialect).map_err(|r| r.reason)
}

/// Why [`read_only_refusal`] refused a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadRefusal {
    /// The words [`read_only_reason`] answers with.
    pub reason: String,
    /// Whether the refusal is about a **write** — a statement that is not a
    /// read, or a write keyword inside one — so a front end with a write path
    /// may point at it. A lock, a sleep or a second statement is not one:
    /// `schemaic query` answered a `FOR UPDATE` with "use `exec` to write",
    /// which re-labelled the lock as the write its own message said it was not.
    pub writes: bool,
}

/// [`read_only_reason`], with the refusal's kind — see [`ReadRefusal`].
///
/// **A write is named before a lock, whatever order they come in.** The lock
/// message tells the caller to drop the clause, and for a statement that also
/// deletes, that retry is refused again as the `DELETE` — so a lock is named
/// only when nothing else in the statement is refused.
pub fn read_only_refusal(sql: &str, dialect: SqlDialect) -> Result<(), ReadRefusal> {
    let refuse = |reason: String, writes: bool| Err(ReadRefusal { reason, writes });
    let (words, multi) = word_tokens(sql, dialect);
    if multi {
        return refuse("only a single statement is allowed".to_string(), false);
    }
    let heads = read_only_heads(dialect);
    let head = words.first().map(|s| s.as_str()).unwrap_or("");
    if !heads.contains(&head) {
        // Naming this engine's heads, not a union of all three: a model told it
        // may `SHOW` on SQLite will keep trying.
        return refuse(
            format!("only read-only queries ({}) are allowed", heads.join("/")),
            true,
        );
    }
    let denied: Vec<usize> = (0..words.len())
        .filter(|&k| is_denied(&words[k], dialect))
        .collect();
    if let Some(&k) = denied
        .iter()
        .find(|&&k| locking_clause(&words, k).is_none())
    {
        return refuse(
            format!("`{}` is not permitted in a read-only query", words[k]),
            WRITE_KEYWORDS.contains(&words[k].as_str()),
        );
    }
    if advances_a_sequence(&words) {
        return refuse(
            "`NEXT VALUE FOR` advances a sequence, which a read-only query does not".to_string(),
            true,
        );
    }
    // After a write, so a deleting CTE is still named as the `DELETE`; before a
    // lock, whose "drop the clause" retry this would refuse again.
    if let Some(policy) = read_call_policy(dialect)
        && let Some(name) = unlisted_call(sql, dialect, &policy)
    {
        return refuse(
            format!(
                "`{name}` is not a function known to be read-only, so it is not permitted \
                 in a read-only query; functions from extensions or defined in the database \
                 are refused too"
            ),
            false,
        );
    }
    let lock = denied
        .iter()
        .filter_map(|&k| Some((k, locking_clause(&words, k)?)))
        .chain(shared_locks(&words))
        .min_by_key(|&(k, _)| k);
    if let Some((_, clause)) = lock {
        return refuse(
            format!(
                "`{clause}` locks the rows it reads, which a read-only query does not; \
                 drop the locking clause"
            ),
            false,
        );
    }
    Ok(())
}

/// The shared-lock clauses no deny-list word catches — `FOR SHARE` (MySQL 8,
/// PostgreSQL) and `FOR KEY SHARE` (PostgreSQL) — with where each starts.
/// `SHARE` itself is no denied word: a column may be called that.
fn shared_locks(words: &[String]) -> impl Iterator<Item = (usize, &'static str)> + '_ {
    let at = |i: usize| words.get(i).map(String::as_str);
    (0..words.len()).filter_map(move |i| match (at(i)?, at(i + 1), at(i + 2)) {
        ("FOR", Some("SHARE"), _) => Some((i, "FOR SHARE")),
        ("FOR", Some("KEY"), Some("SHARE")) => Some((i, "FOR KEY SHARE")),
        _ => None,
    })
}

/// The row-locking clause the denied word at `words[k]` belongs to, if it does.
///
/// **Refused either way, but named for what it is.** `SELECT … FOR UPDATE`
/// used to answer "`UPDATE` is not permitted", which reads as a write having
/// been found. The refusal is right — it takes row locks a read-only query must
/// not hold — but the reason is the lock, and the fix is dropping the clause.
fn locking_clause(words: &[String], k: usize) -> Option<&'static str> {
    let before = |n: usize| {
        k.checked_sub(n)
            .and_then(|i| words.get(i))
            .map(String::as_str)
    };
    let after = |n: usize| words.get(k + n).map(String::as_str);
    match words[k].as_str() {
        "UPDATE" if before(1) == Some("FOR") => Some("FOR UPDATE"),
        "UPDATE" if (before(3), before(2), before(1)) == (Some("FOR"), Some("NO"), Some("KEY")) => {
            Some("FOR NO KEY UPDATE")
        }
        "LOCK" if (after(1), after(2), after(3)) == (Some("IN"), Some("SHARE"), Some("MODE")) => {
            Some("LOCK IN SHARE MODE")
        }
        // SQL Server's table hints. Only ever deny-listed on that dialect, so
        // on the others these words never reach here.
        "UPDLOCK" => Some("UPDLOCK"),
        "XLOCK" => Some("XLOCK"),
        "TABLOCK" => Some("TABLOCK"),
        "TABLOCKX" => Some("TABLOCKX"),
        "HOLDLOCK" => Some("HOLDLOCK"),
        "SERIALIZABLE" => Some("SERIALIZABLE"),
        "REPEATABLEREAD" => Some("REPEATABLEREAD"),
        _ => None,
    }
}

/// Does the statement advance a sequence with `NEXT VALUE FOR` — SQL Server's
/// spelling and MariaDB's, and the standard's — which no rollback undoes? A
/// phrase rather than a denied word, because `value` and `next` are both
/// ordinary column names.
///
/// **Every dialect**, because the question is the phrase's: MariaDB 10.3+
/// takes it too, so `SELECT NEXT VALUE FOR s` passed the gate there as a read.
/// On an engine without it the phrase is a syntax error, and refusing that is
/// the safe direction.
fn advances_a_sequence(words: &[String]) -> bool {
    words
        .windows(3)
        .any(|w| w[0] == "NEXT" && w[1] == "VALUE" && w[2] == "FOR")
}

/// The `sql_mode` names under which a MySQL/MariaDB server reads a quote
/// differently from [`skip_noncode`], which assumes `\` escapes inside every
/// quote.
///
/// **A text gate is only as good as its agreement with the server about where a
/// statement ends.** Under `NO_BACKSLASH_ESCAPES` the server closes
/// `'a\'` at the second quote, and under `ANSI_QUOTES` it reads `"a\"` as an
/// identifier with no escapes at all — so `SELECT 'a\'; DELETE FROM t; -- '` is
/// one string and one `SELECT` to the gate and three statements to the server,
/// and the driver sends multi-statement text. The combination modes are here
/// because each one *implies* `ANSI_QUOTES`: dropping the flag and keeping
/// `ANSI` would have the server put it straight back.
const MYSQL_MODES_THAT_MOVE_A_QUOTE: &[&str] = &[
    "NO_BACKSLASH_ESCAPES",
    "ANSI_QUOTES",
    "ANSI",
    "DB2",
    "MAXDB",
    "MSSQL",
    "ORACLE",
    "POSTGRESQL",
];

/// `mode` (a server's `@@SESSION.sql_mode`) with every name that would make the
/// server lex a quote differently from the gate taken out, and everything else
/// kept.
///
/// **It removes names rather than overwriting the mode**, for the reason
/// `export::MYSQL_LITERAL_MODE_SQL` does: the strictness the server was
/// configured with is not this function's business. A combination mode is
/// listed by the server beside the flags it expands to, so what it implied
/// apart from the quote survives as those flags.
pub fn mysql_mode_lexed_like_the_gate(mode: &str) -> String {
    mode.split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .filter(|m| {
            !MYSQL_MODES_THAT_MOVE_A_QUOTE
                .iter()
                .any(|bad| m.eq_ignore_ascii_case(bad))
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Does a server in `mode` lex quotes the way [`skip_noncode`] does?
///
/// The check a session makes **after** pinning its mode, reading it back rather
/// than trusting the `SET` — a server that puts a flag back (a combination
/// mode this list does not know) must fail the statement, not run it.
pub fn mysql_mode_is_lexed_like_the_gate(mode: &str) -> bool {
    mode.split(',').map(str::trim).all(|m| {
        !MYSQL_MODES_THAT_MOVE_A_QUOTE
            .iter()
            .any(|bad| m.eq_ignore_ascii_case(bad))
    })
}

/// Keywords that make a statement a write *wherever* they appear in it, not just
/// at its head — the set that survives the read-head allowlist below.
///
/// Deliberately narrower than [`DENY_ANY_ENGINE`]: this gate allows several read
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
                if words
                    .iter()
                    .any(|w| unterminated_write_words(dialect).contains(&w.as_str()))
                    || advances_a_sequence(&words)
                {
                    return true;
                }
            }
        }
    }
    false
}

/// The words that make a statement a write **anywhere** in it on an engine
/// whose statements need no `;` between them — SQL Server's.
///
/// There, the text after a `SELECT` can be a second statement, so a head test
/// cannot see it: `SELECT 1 COMMIT EXEC('DELETE FROM t')` read as one read,
/// and on a read-only connection its `COMMIT` ended the rollback-only
/// transaction the session is guarded by (`db::mssql::fetch_query`) and the
/// procedure's writes stuck. So every word that begins a statement other than
/// a read, or does something a rollback cannot undo, counts wherever it
/// stands. Over-blocking a column of that name is the safe direction on a
/// connection the user marked read-only.
fn unterminated_write_words(dialect: SqlDialect) -> &'static [&'static str] {
    if !dialect.batch_separator() {
        return &[];
    }
    &[
        "EXEC",
        "EXECUTE",
        "COMMIT",
        "ROLLBACK",
        "SAVE",
        "BEGIN",
        "SET",
        "DECLARE",
        "USE",
        "DENY",
        "BACKUP",
        "RESTORE",
        "DBCC",
        "BULK",
        "KILL",
        "RECONFIGURE",
        "SHUTDOWN",
        "OPENROWSET",
        "OPENQUERY",
        "OPENDATASOURCE",
        "INTO",
        "UPDATETEXT",
        "WRITETEXT",
        "ENABLE",
        "DISABLE",
        "RECEIVE",
        "CONVERSATION",
        "CHECKPOINT",
        "SETUSER",
        "REVERT",
        // `ADD SIGNATURE`, `ADD COUNTER SIGNATURE`, `ADD SENSITIVITY
        // CLASSIFICATION`. A reserved word in T-SQL, so an unbracketed `add`
        // is never a name.
        "ADD",
    ]
}

/// `SELECT {projection} FROM {rest}` capped at `limit` rows, in `dialect` —
/// **the** spelling of a generated row cap.
///
/// `rest` is everything after `FROM`: the table, and any `WHERE`/`ORDER BY`.
/// Three engines write the cap as a trailing `LIMIT n`; T-SQL has no `LIMIT`
/// and writes `TOP (n)` after `SELECT`. The generators each wrote the
/// trailing form into a `format!`, which is a comparison no census finds.
pub fn limited_select(dialect: SqlDialect, projection: &str, rest: &str, limit: usize) -> String {
    match dialect {
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::Sqlite => {
            format!("SELECT {projection} FROM {rest} LIMIT {limit}")
        }
        SqlDialect::MsSql => format!("SELECT TOP ({limit}) {projection} FROM {rest}"),
    }
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
/// Per statement — on SQL Server each statement [`tsql_statements`] finds in a
/// range, since a range runs whole from a `DECLARE` and through a `BEGIN …
/// END` — on whole tokens outside strings / quoted identifiers / comments
/// (so `SELECT password FROM users` is *not* a credential statement — it names a
/// column):
///
/// - anything containing `IDENTIFIED` — `CREATE`/`ALTER USER … IDENTIFIED BY`,
///   and `GRANT … IDENTIFIED BY`;
/// - a `SET` statement containing `PASSWORD` — `SET PASSWORD FOR … = …`;
/// - a `CREATE`/`ALTER`/`DROP`/`GRANT`/`REVOKE` naming a `USER` or `ROLE` *and*
///   `PASSWORD` — PostgreSQL's `CREATE ROLE … WITH PASSWORD '…'` — or, as
///   its second word, T-SQL's `LOGIN` (`ALTER LOGIN sa WITH PASSWORD = …`);
/// - `BY PASSWORD` anywhere — T-SQL's `CREATE MASTER KEY ENCRYPTION BY
///   PASSWORD`, `OPEN … DECRYPTION BY PASSWORD`, a certificate's or a
///   symmetric key's;
/// - a `CREATE`/`ALTER` naming a `CREDENTIAL` *and* a `SECRET` — T-SQL's
///   `CREATE DATABASE SCOPED CREDENTIAL … SECRET = '…'`;
/// - a `CHANGE`/`START`/`EXEC`/`EXECUTE` holding `PASSWORD` or a word ending
///   `_PASSWORD` — MySQL's `CHANGE REPLICATION SOURCE TO SOURCE_PASSWORD = …`
///   and `START REPLICA … PASSWORD = …`, a T-SQL procedure's `@password` or
///   `@subscriber_password`;
/// - a call to one of [`SECRET_CALLS`], the system procedures and functions
///   that take a password as an argument (`EXEC sp_addlogin 'n', 'pw'`), or a
///   connection string holding one (`OPENROWSET('…', 'Server=h;PWD=…', …)`,
///   `dblink('… password=…', …)`), with no `PASSWORD` word for the rules above
///   to find.
///
/// Not per dialect: none of these words means anything else as a statement's
/// shape on any engine, and a rule asked of one engine only is the one a new
/// engine arrives without — this function was never taught T-SQL when SQL
/// Server came, and `CREATE LOGIN … WITH PASSWORD` went to `history.json`.
///
/// Where it is imprecise it is imprecise toward omitting: a dropped history entry
/// costs the user a scroll, a kept one writes their secret to disk.
pub fn carries_credential(sql: &str, dialect: SqlDialect) -> bool {
    statement_ranges(sql, dialect).into_iter().any(|(lo, hi)| {
        // **Each T-SQL statement where it begins**, not the range's head:
        // `scan_bounds` keeps a batch whole from a `DECLARE` on and never cuts
        // inside `BEGIN … END`, so `DECLARE …; CREATE LOGIN … PASSWORD` and
        // `IF NOT EXISTS (…) BEGIN CREATE LOGIN … END` head no range with the
        // statement that carries the password. The identity on every other
        // engine.
        tsql_statements(&sql[lo..hi], dialect)
            .into_iter()
            .any(|stmt| statement_carries_credential(stmt, dialect))
    })
}

/// [`carries_credential`]'s rules, for one statement.
fn statement_carries_credential(stmt: &str, dialect: SqlDialect) -> bool {
    let (words, _) = word_tokens(stmt, dialect);
    if words.iter().any(|w| w == "IDENTIFIED") {
        return true;
    }
    // A call to a routine that takes a password as an argument — except
    // `OPENROWSET(BULK …)`, which reads a file and takes none.
    if words.iter().enumerate().any(|(k, w)| {
        SECRET_CALLS.contains(&w.as_str())
            && !(w == "OPENROWSET" && words.get(k + 1).is_some_and(|n| n == "BULK"))
    }) {
        return true;
    }
    if words.windows(2).any(|p| p[0] == "BY" && p[1] == "PASSWORD") {
        return true;
    }
    let head = words.first().map(|s| s.as_str());
    if matches!(head, Some("CREATE" | "ALTER"))
        && words.iter().any(|w| w == "CREDENTIAL")
        && words.iter().any(|w| w == "SECRET")
    {
        return true;
    }
    // A password set by a word rather than a principal statement: MySQL's
    // `CHANGE REPLICATION SOURCE TO SOURCE_PASSWORD = …`, `START REPLICA …
    // PASSWORD = …`, and a T-SQL procedure's `@password`/`@…_password`
    // parameter (`@` is not a word byte, so the token is the bare word).
    let passwordish = |w: &String| w == "PASSWORD" || w.ends_with("_PASSWORD");
    if matches!(head, Some("CHANGE" | "START" | "EXEC" | "EXECUTE"))
        && words.iter().any(passwordish)
    {
        return true;
    }
    if !words.iter().any(|w| w == "PASSWORD") {
        return false;
    }
    let names_a_principal = words.iter().any(|w| w == "USER" || w == "ROLE")
        || words.get(1).is_some_and(|w| w == "LOGIN");
    match head {
        Some("SET") => true,
        Some("CREATE" | "ALTER" | "DROP" | "GRANT" | "REVOKE") => names_a_principal,
        _ => false,
    }
}

/// The routines whose arguments include a password in the clear, with no
/// `PASSWORD` keyword in the statement — SQL Server's system procedures for
/// logins, linked-server logins, orphaned users, application roles and
/// replication agents, its passphrase and password-hash functions, the rowset
/// functions and linked-server definition that take a connection string
/// (`PWD=…` inside a literal), and PostgreSQL's `dblink` family, whose first
/// argument is one. Upper case, as [`word_tokens`] gives words.
const SECRET_CALLS: &[&str] = &[
    "SP_ADDLOGIN",
    "SP_PASSWORD",
    "SP_ADDLINKEDSRVLOGIN",
    "SP_ADDLINKEDSERVER",
    "SP_CHANGE_USERS_LOGIN",
    "SP_ADDAPPROLE",
    "SP_SETAPPROLE",
    "SP_APPROLEPASSWORD",
    "SP_CONTROL_DBMASTERKEY_PASSWORD",
    "SP_ADDDISTRIBUTOR",
    "SP_ADDDISTPUBLISHER",
    "SP_CHANGEDISTRIBUTOR_PASSWORD",
    "SP_ADDSUBSCRIPTION",
    "SP_ADDPUSHSUBSCRIPTION_AGENT",
    "SP_ADDPULLSUBSCRIPTION_AGENT",
    "SP_ADDMERGEPUSHSUBSCRIPTION_AGENT",
    "SP_ADDMERGEPULLSUBSCRIPTION_AGENT",
    "SP_ADDLOGREADER_AGENT",
    "SP_ADDPUBLICATION_SNAPSHOT",
    "ENCRYPTBYPASSPHRASE",
    "DECRYPTBYPASSPHRASE",
    "PWDENCRYPT",
    "PWDCOMPARE",
    "OPENROWSET",
    "OPENDATASOURCE",
    "DBLINK",
    "DBLINK_CONNECT",
    "DBLINK_CONNECT_U",
    "DBLINK_EXEC",
];

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

    /// Every engine this build ships — see [`SqlDialect::ALL`].
    const EVERY_DIALECT: [SqlDialect; 4] = SqlDialect::ALL;

    /// **The test A2-L3-01 names, and it could not be compiled before.**
    ///
    /// The write guard's policy assembly was the closure `guard_policy` inside
    /// `app_view` — 9,600 lines, so nothing in it is nameable, callable or
    /// testable. The invariant says the *decision* is pure and tested, and
    /// `run_verdict` is; its three inputs were assembled where no test could
    /// reach them, which is the whole of the finding.
    ///
    /// The case that matters most is `no_database` on **PostgreSQL**: there it
    /// does not mean "nowhere to run", it means the connection lands in a hidden
    /// maintenance database, so an unscoped `CREATE TABLE` succeeds into a
    /// database Schemaic can never show again — and one landing in `template1` is
    /// inherited by every database made afterwards.
    #[test]
    fn a_tab_with_no_database_says_so_on_every_engine() {
        use crate::connection::Connection;
        for (db_type, expect) in [
            ("PostgreSQL", SqlDialect::Postgres),
            ("SQLite", SqlDialect::Sqlite),
            ("MySQL", SqlDialect::MySql),
        ] {
            let c = Connection {
                id: 1,
                name: "c".into(),
                db_type: db_type.into(),
                host: String::new(),
                port: 0,
                user: String::new(),
                password: String::new(),
                file: String::new(),
                database: String::new(),
                ssh: Default::default(),
                tls: Default::default(),
                color: None,
                prominent_color: false,
                read_only: false,
                cli_access: false,
                environment: crate::connection::Environment::None,
                ai_data: None,
                folder: String::new(),
                auth: crate::connection::AuthMode::Password,
            };
            let p = GuardPolicy::of(Some(&c), true, false);
            assert!(p.no_database, "{db_type}");
            assert_eq!(p.dialect, expect, "{db_type}");
            assert!(!p.read_only, "{db_type}");

            // And a bound database is the other answer, so this cannot pass by
            // always saying yes.
            assert!(!GuardPolicy::of(Some(&c), false, false).no_database);
        }
    }

    /// **The two spellings of "you ran this nowhere", as the driver layer
    /// reports them.** The MySQL line is the text `schemaic query` printed
    /// against a MySQL 8.4 connection with no database (issue #2), `DbError`'s
    /// prefix included.
    #[test]
    fn a_failure_for_want_of_a_database_is_recognised_per_engine() {
        assert_eq!(
            no_database_failure(
                "query failed: Server error: `ERROR 1046 (3D000): No database selected'",
                SqlDialect::MySql
            ),
            Some(NoDatabaseFailure::Refused)
        );
        assert_eq!(
            no_database_failure(
                "query failed: relation \"company\" does not exist",
                SqlDialect::Postgres
            ),
            Some(NoDatabaseFailure::RanElsewhere)
        );
        // PostgreSQL appends DETAIL/HINT after the message; still a match.
        assert_eq!(
            no_database_failure(
                "query failed: relation \"public.company\" does not exist — Perhaps you meant …",
                SqlDialect::Postgres
            ),
            Some(NoDatabaseFailure::RanElsewhere)
        );
    }

    /// **Any other failure is not this one**, or every missing table on a
    /// scoped run would be told to pass `-d`.
    #[test]
    fn other_failures_are_not_mistaken_for_a_missing_database() {
        for (message, dialect) in [
            (
                "query failed: Server error: `ERROR 1146 (42S02): Table 'app.company' doesn't exist'",
                SqlDialect::MySql,
            ),
            (
                "query failed: Server error: `ERROR 1064 (42000): You have an error in your SQL syntax'",
                SqlDialect::MySql,
            ),
            (
                "query failed: column \"company\" does not exist",
                SqlDialect::Postgres,
            ),
            (
                "query failed: database \"app\" does not exist",
                SqlDialect::Postgres,
            ),
            // The relation is there; its column is not. Naming a relation is
            // not the same as saying it is missing.
            (
                "query failed: column \"x\" of relation \"company\" does not exist",
                SqlDialect::Postgres,
            ),
            ("connection failed: timed out", SqlDialect::MySql),
            ("connection failed: timed out", SqlDialect::Postgres),
            ("", SqlDialect::MySql),
            ("", SqlDialect::Postgres),
        ] {
            assert_eq!(no_database_failure(message, dialect), None, "{message:?}");
        }
    }

    /// **One engine's spelling is not another's.** SQLite has no database to
    /// leave out, so nothing it says means that; and MySQL's code in a
    /// PostgreSQL error is a coincidence of text, not the failure.
    #[test]
    fn the_match_is_the_connections_own_engine_only() {
        let mysql = "query failed: Server error: `ERROR 1046 (3D000): No database selected'";
        let pg = "query failed: relation \"company\" does not exist";
        assert_eq!(no_database_failure(mysql, SqlDialect::Postgres), None);
        assert_eq!(no_database_failure(pg, SqlDialect::MySql), None);
        for message in [mysql, pg, "query failed: no such table: company"] {
            assert_eq!(no_database_failure(message, SqlDialect::Sqlite), None);
        }
    }

    /// The guard's refusal and the constant a front end compares against are
    /// the same words, on both paths that refuse for it.
    #[test]
    fn the_no_database_refusal_is_the_named_constant() {
        let policy = GuardPolicy {
            read_only: false,
            confirm_writes: false,
            dialect: SqlDialect::Postgres,
            no_database: true,
        };
        assert_eq!(
            run_verdict(&["CREATE TABLE t (id int)".to_string()], policy),
            RunVerdict::Block(NO_DATABASE_SELECTED.to_string())
        );
        assert_eq!(
            script_verdict(policy, "dump.sql"),
            RunVerdict::Block(NO_DATABASE_SELECTED.to_string())
        );
    }

    /// A connection the registry has lost falls back to the default engine and to
    /// *writable*, matching `connection::read_only_of` — the same documented
    /// fail-open, so the two cannot drift.
    #[test]
    fn a_policy_for_a_vanished_connection_is_the_documented_fallback() {
        let p = GuardPolicy::of(None, false, true);
        assert!(!p.read_only);
        assert_eq!(p.dialect, SqlDialect::default());
        assert!(p.confirm_writes, "the user's setting is still the user's");
    }

    /// A read-only connection produces a read-only policy, which is the term
    /// `run_verdict` turns into the one refusal with no "Run anyway".
    #[test]
    fn a_read_only_connection_produces_a_blocking_policy() {
        use crate::connection::Connection;
        let mut c = Connection {
            id: 1,
            name: "c".into(),
            db_type: "MySQL".into(),
            host: String::new(),
            port: 0,
            user: String::new(),
            password: String::new(),
            file: String::new(),
            database: String::new(),
            ssh: Default::default(),
            tls: Default::default(),
            color: None,
            prominent_color: false,
            read_only: true,
            cli_access: false,
            environment: crate::connection::Environment::None,
            ai_data: None,
            folder: String::new(),
            auth: crate::connection::AuthMode::Password,
        };
        assert!(GuardPolicy::of(Some(&c), false, false).read_only);
        assert!(matches!(
            run_verdict(
                &["DELETE FROM t WHERE id = 1".to_string()],
                GuardPolicy::of(Some(&c), false, false)
            ),
            RunVerdict::Block(_)
        ));
        c.read_only = false;
        assert!(!GuardPolicy::of(Some(&c), false, false).read_only);
    }

    /// The composition the two correct functions got wrong: a preview folds the
    /// statement to one line, which removes the newline a `--` comment ends on,
    /// and the panel then lexes that line — so the whole query was one comment
    /// token and rendered as if it were commented out.
    #[test]
    fn a_line_comment_survives_being_folded_onto_one_line() {
        let folded = |sql: &str, d: SqlDialect| {
            super::inline_line_comments(sql, d)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        for d in EVERY_DIALECT {
            let one = folded("-- daily revenue\nSELECT id FROM orders", d);
            assert_eq!(one, "/* daily revenue */ SELECT id FROM orders", "{d:?}");
            // The claim, stated the way the failure was measured: the comment
            // must not reach the `SELECT`.
            let end = super::skip_noncode(one.as_bytes(), 0, d).expect("a comment opens it");
            assert!(end < one.find("SELECT").unwrap(), "{d:?}: {one}");
            // A trailing comment does the same to everything after it.
            let two = folded("SELECT 1 -- why\nFROM t", d);
            assert_eq!(two, "SELECT 1 /* why */ FROM t", "{d:?}");
        }
        // MySQL's `#`, which PostgreSQL and SQLite do not have.
        assert_eq!(
            folded("SELECT 1 # note\nFROM t", SqlDialect::MySql),
            "SELECT 1 /* note */ FROM t"
        );
    }

    /// Everything that is not a line comment is copied byte for byte — including
    /// a `--` inside a string, which is not a comment at all.
    #[test]
    fn inline_line_comments_leaves_everything_else_alone() {
        for d in EVERY_DIALECT {
            for sql in [
                "SELECT '-- not a comment' FROM t",
                "SELECT /* block */ 1 FROM t",
                "SELECT 1 FROM t",
                "",
            ] {
                assert_eq!(super::inline_line_comments(sql, d), sql, "{d:?}: {sql}");
            }
        }
        // A quoted identifier holding the opener is a name, not a comment.
        assert_eq!(
            super::inline_line_comments("SELECT `a--b` FROM t", SqlDialect::MySql),
            "SELECT `a--b` FROM t"
        );
    }

    /// A comment carrying `*/` would close the block it is now inside and leave
    /// the rest of the statement outside it — the same bug one level down.
    #[test]
    fn a_comment_cannot_close_the_block_it_is_rewritten_into() {
        let out = super::inline_line_comments("-- see /* x */ here\nSELECT 1", SqlDialect::MySql);
        assert_eq!(out, "/* see / * x * / here */\nSELECT 1");
        let folded = out.split_whitespace().collect::<Vec<_>>().join(" ");
        let end = super::skip_noncode(folded.as_bytes(), 0, SqlDialect::MySql).unwrap();
        assert!(end < folded.find("SELECT").unwrap(), "{folded}");
    }

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

    /// **The statement whose columns describe what comes back.** PostgreSQL's
    /// `Parse` takes one command, so a multi-statement string cannot be prepared
    /// and the reader gets no column *types* — and an unknown type is what turns
    /// a `bytea` into a cell holding its whole hex encoding. Since that reader
    /// reports only the first result set, the first statement is the one to
    /// describe.
    #[test]
    fn the_first_statement_is_the_one_a_reader_can_describe() {
        let pg = SqlDialect::Postgres;
        // One statement: handed back whole, so the single-statement path prepares
        // exactly the string it always did.
        assert_eq!(
            super::first_statement("SELECT id FROM a", pg),
            "SELECT id FROM a"
        );
        // The terminator is part of the range, and stays — Run Everything already
        // hands `sql[lo..hi]` to the same `PREPARE`, so one command plus its `;`
        // is a shape this reader has always been given.
        assert_eq!(
            super::first_statement("SELECT id FROM a;", pg),
            "SELECT id FROM a;"
        );
        // Two statements: only the first, and the second is not in the slice.
        assert_eq!(
            super::first_statement("SELECT id FROM a; SELECT photo FROM b", pg),
            "SELECT id FROM a;"
        );
        // A leading comment belongs to the statement it introduces.
        assert_eq!(
            super::first_statement("/* note */ SELECT 1; SELECT 2", pg),
            "/* note */ SELECT 1;"
        );
        // A semicolon inside a string or a dollar-quoted body is not a boundary,
        // which is the whole reason this goes through `statement_ranges` rather
        // than `split(';')`.
        assert_eq!(
            super::first_statement("SELECT 'a;b' AS s; SELECT 2", pg),
            "SELECT 'a;b' AS s;"
        );
        assert_eq!(
            super::first_statement("SELECT $$a;b$$; SELECT 2", pg),
            "SELECT $$a;b$$;"
        );
        // Nothing a lexer can call a statement: hand back the input rather than an
        // empty string, so the caller's `prepare` fails on the real text and its
        // error is the server's own.
        for empty in ["", "   ", ";", "-- just a comment"] {
            assert_eq!(super::first_statement(empty, pg), empty, "{empty:?}");
        }
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

    /// Brackets are SQLite's and SQL Server's: on the other engines `[` is
    /// ordinary code, so the same text splits where its semicolons are.
    #[test]
    fn brackets_are_not_identifiers_on_the_other_engines() {
        let sql = "SELECT * FROM [my; tbl]; SELECT 2;";
        assert_eq!(super::statement_ranges(sql, SqlDialect::MySql).len(), 3);
        assert_eq!(super::statement_ranges(sql, SqlDialect::Postgres).len(), 3);
        assert_eq!(super::statement_ranges(sql, SqlDialect::MsSql).len(), 2);
    }

    /// **T-SQL escapes `]` inside a bracketed name by doubling it**, where
    /// SQLite has no escape at all. Ending the span at the first `]` would read
    /// `[a]]; DROP TABLE t; --]` as a name, then a real `DROP`, where SQL
    /// Server sees one identifier — or, the other way round, hide a statement
    /// the server runs.
    #[test]
    fn a_doubled_bracket_stays_inside_a_sql_server_name() {
        let s = b"[a]]b] x";
        assert_eq!(super::skip_noncode(s, 0, SqlDialect::MsSql), Some(6));
        assert_eq!(super::skip_noncode(s, 0, SqlDialect::Sqlite), Some(3));
        let sql = "SELECT [a]]; DROP TABLE t; --] FROM t; SELECT 2";
        let r = super::statement_ranges(sql, SqlDialect::MsSql);
        assert_eq!(r.len(), 2, "{r:?}");
        assert_eq!(
            super::ident_at("[a]]b]", 0, SqlDialect::MsSql).0.as_deref(),
            Some("a]b")
        );
    }

    /// **A read-only SQL Server connection cannot be written through a
    /// statement that needs no `;`.** `contains_write` split at `;` and knew
    /// neither `COMMIT` nor `EXEC`, so `SELECT 1 COMMIT EXEC('DELETE …')` was a
    /// read: the `COMMIT` ended the guard's own transaction and the procedure's
    /// writes stuck.
    #[test]
    fn a_sql_server_write_hidden_behind_a_read_is_still_a_write() {
        let ms = SqlDialect::MsSql;
        for sql in [
            "SELECT 1 COMMIT EXEC('DELETE FROM t')",
            "SELECT 1 EXEC sp_executesql N'DELETE FROM t'",
            "SELECT 1 EXECUTE dbo.purge",
            "SELECT 1 ROLLBACK",
            "SELECT * INTO copy FROM t",
            "SELECT NEXT VALUE FOR s",
            "SELECT 1 DECLARE @x int SET @x = 1",
            "SELECT 1 DBCC FREEPROCCACHE",
            "SELECT * FROM OPENQUERY(srv, 'DELETE FROM t')",
            // T-SQL's `ADD` statements, headless behind a read (measured on
            // 2022: the batch classified the column).
            "SELECT id FROM dbo.t\nADD SENSITIVITY CLASSIFICATION TO dbo.t.email WITH (LABEL = 'PII')",
            "SELECT 1 ADD SIGNATURE TO dbo.p BY CERTIFICATE c",
            "SELECT 1 ADD COUNTER SIGNATURE TO dbo.p BY CERTIFICATE c",
        ] {
            assert!(super::contains_write(sql, ms), "{sql}");
        }
        // Reads stay reads — `add` bracketed as a name is a name.
        for sql in [
            "SELECT a FROM t WHERE a = N'EXEC'",
            "SELECT [add] FROM t",
            "SELECT TOP (5) [commit] FROM t",
            "WITH c AS (SELECT 1 AS a) SELECT * FROM c",
        ] {
            assert!(!super::contains_write(sql, ms), "{sql}");
        }
    }

    #[test]
    fn a_row_cap_is_a_trailing_limit_except_on_sql_server() {
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert_eq!(
                super::limited_select(d, "*", "t ORDER BY id", 5),
                "SELECT * FROM t ORDER BY id LIMIT 5"
            );
        }
        assert_eq!(
            super::limited_select(SqlDialect::MsSql, "*", "t ORDER BY id", 5),
            "SELECT TOP (5) * FROM t ORDER BY id"
        );
    }

    /// **`GO` is the client's batch separator**, like MySQL's `DELIMITER`: SQL
    /// Server has never heard of it, so it ends a statement and is not sent.
    #[test]
    fn a_go_line_ends_a_sql_server_batch_and_is_not_sent() {
        let d = SqlDialect::MsSql;
        let sql = "SELECT 1\nGO\nSELECT 2\ngo 3\nSELECT 3";
        assert_eq!(
            super::executable_statements(sql, d),
            vec!["SELECT 1", "SELECT 2", "SELECT 3"]
        );
        assert_eq!(super::statement_ranges(sql, d).len(), 3);
        // Only a line of its own: a column called `go`, and `GO` in a string or
        // comment, are data.
        let data = "SELECT go FROM t\nSELECT 'x\nGO\n' /*\nGO\n*/";
        assert_eq!(super::executable_statements(data, d).len(), 1);
        // A `GO` after a trailing comment on the line is still the separator.
        assert_eq!(
            super::executable_statements("SELECT 1\nGO -- next\nSELECT 2", d).len(),
            2
        );
        // Not a directive on any other engine.
        assert_eq!(
            super::executable_statements(sql, SqlDialect::Postgres).len(),
            1
        );
    }

    /// **A routine body holds its own `;`s and runs to the end of the batch.**
    /// `CREATE PROCEDURE … AS BEGIN …; …; END` cut at its semicolons is a
    /// procedure that stops at its first statement and a stray `END`.
    #[test]
    fn a_sql_server_routine_body_is_not_cut_at_its_semicolons() {
        let d = SqlDialect::MsSql;
        let sql = "CREATE PROCEDURE p AS BEGIN SELECT 1; SELECT 2; END;\nGO\nSELECT 3; SELECT 4";
        let stmts = super::executable_statements(sql, d);
        assert_eq!(stmts.len(), 3, "{stmts:?}");
        assert!(stmts[0].ends_with("END;"), "{}", stmts[0]);
        for head in [
            "CREATE OR ALTER PROCEDURE p AS SELECT 1; SELECT 2",
            "ALTER PROC p AS SELECT 1; SELECT 2",
            "CREATE FUNCTION f() RETURNS INT AS BEGIN DECLARE @x INT; RETURN 1; END",
            "CREATE TRIGGER tr ON t AFTER INSERT AS SELECT 1; SELECT 2",
        ] {
            assert_eq!(super::executable_statements(head, d).len(), 1, "{head}");
        }
        // A view is one `SELECT`, and a table is not a body.
        assert_eq!(
            super::executable_statements("CREATE TABLE t (a INT); SELECT 1", d).len(),
            2
        );
        // The chunked scanner agrees, cut anywhere.
        for chunk in [1, 3, 7, 64] {
            assert_eq!(chunked(sql, d, chunk).len(), 3, "chunk {chunk}");
        }
    }

    /// **What lives in a T-SQL batch is never cut at a `;`.** A variable is
    /// batch-scoped, a `BEGIN … END` block, a `TRY … CATCH` and an
    /// `IF … ELSE` are one statement to the server, and each piece Run
    /// Everything or a `.sql` run sends is its own batch — so `DECLARE @id
    /// int; SELECT @id = 5; SELECT @id` failed at its second piece (Msg 137)
    /// and the block's first piece was a syntax error (Msg 102), after the
    /// pieces before had committed (measured on 2022). From a `DECLARE`, a
    /// `RETURN`, a `GOTO` or a label, the rest of the batch goes whole, as
    /// sqlcmd sends it; statements that share nothing still go one by one.
    #[test]
    fn a_t_sql_batch_scope_is_never_cut_at_a_semicolon() {
        let d = SqlDialect::MsSql;
        let one = |sql: &str| {
            let stmts = super::executable_statements(sql, d);
            assert_eq!(stmts.len(), 1, "{sql}\n{stmts:#?}");
            for chunk in [1, 2, 5, 13, 64] {
                assert_eq!(chunked(sql, d, chunk).len(), 1, "chunk {chunk}: {sql}");
            }
        };
        one("DECLARE @id int;\nSELECT @id = 5;\nSELECT @id AS v;");
        // A label opens a statement whether or not a `;` closed the one before
        // it — T-SQL needs none — so its `GOTO` stays in its batch. Cut there,
        // `GOTO again` went alone (Msg 133) after the write before it had
        // committed.
        one("SELECT 1\nagain:\nPRINT 2;\nGOTO again");
        one("UPDATE t SET a = 1 WHERE id = 1\nretry:\nPRINT 2;\nGOTO retry;");
        one(
            "IF 1 = 1\nBEGIN\n  UPDATE t SET a = 1 WHERE id = 1;\n  UPDATE t SET a = 2 WHERE id = 2;\nEND",
        );
        one("BEGIN TRY SELECT 1/0; END TRY BEGIN CATCH SELECT ERROR_MESSAGE(); END CATCH");
        one("IF 1 = 1 SELECT 1; ELSE SELECT 2;");
        one("WHILE 1 = 0 BEGIN SELECT CASE WHEN 1 = 1 THEN 1 END; SELECT 2; END");
        one("again: PRINT 1; GOTO again;");
        one("IF NOT EXISTS (SELECT 1 FROM t) RETURN; DELETE FROM u;");
        // Statements before the batch-scoped one still go on their own, and
        // a `GO` ends the scope.
        let stmts = super::executable_statements(
            "SELECT 1; DECLARE @x int; SET @x = 1; SELECT @x;\nGO\nSELECT 2; SELECT 3;",
            d,
        );
        assert_eq!(stmts.len(), 4, "{stmts:#?}");
        assert!(
            stmts[1].starts_with("DECLARE") && stmts[1].ends_with("SELECT @x;"),
            "{stmts:#?}"
        );
        // A transaction's `BEGIN` opens no block, and neither does `END
        // CONVERSATION`, so what follows them still splits.
        for sql in [
            "BEGIN TRAN; UPDATE t SET a = 1; COMMIT;",
            "BEGIN TRANSACTION; UPDATE t SET a = 1; COMMIT TRANSACTION;",
            "BEGIN DISTRIBUTED TRANSACTION; UPDATE t SET a = 1; COMMIT;",
        ] {
            assert_eq!(super::executable_statements(sql, d).len(), 3, "{sql}");
        }
        // A `@@` system function and a `#` temp table are no variables, and
        // the words only count as code: in a string or a comment they are
        // nothing.
        assert_eq!(
            super::executable_statements(
                "SELECT @@ROWCOUNT; SELECT 'DECLARE'; -- BEGIN\nSELECT 3;",
                d
            )
            .len(),
            3
        );
        // A qualified name's part is no keyword, however it is spelled.
        assert_eq!(
            super::executable_statements("SELECT x.begin, x.declare FROM x; SELECT 2;", d).len(),
            2
        );
        // A `::` scope qualifier and a time in a string are no label.
        assert_eq!(
            super::executable_statements(
                "GRANT SELECT ON SCHEMA::dbo TO u; SELECT '10:30'; SELECT 2;",
                d
            )
            .len(),
            3
        );
        // Run at the caret takes the whole block.
        let block = "IF 1 = 1\nBEGIN\n  UPDATE t SET a = 1;\n  UPDATE t SET a = 2;\nEND";
        let caret = block.find("a = 2").unwrap();
        assert_eq!(super::executable_at(block, caret, d), Some(block));
        // Nothing changes on the other engines.
        assert_eq!(
            super::executable_statements("BEGIN; SELECT 1; END;", SqlDialect::Postgres).len(),
            3
        );
    }

    /// **Run at the caret runs up to the caret's statement, never past it.**
    /// A `DECLARE` makes the rest of its batch one piece, and Run Current took
    /// that piece whole: Ctrl+Enter on a `SELECT` below a `DECLARE` ran the
    /// `DELETE` after it too. The piece is still cut where the batch scope
    /// alone held a `;` — never inside a block or before an `ELSE` — so what
    /// the caret's statement needs above it goes with it, and nothing below.
    #[test]
    fn run_at_the_caret_stops_at_the_carets_statement_in_a_batch_scope() {
        let d = SqlDialect::MsSql;
        fn at<'a>(sql: &'a str, needle: &str) -> &'a str {
            let d = SqlDialect::MsSql;
            let caret = sql.find(needle).unwrap() + 1;
            let run = super::executable_at(sql, caret, d);
            let (lo, hi) = super::run_current_range(sql, caret, d);
            assert_eq!(run, Some(&sql[lo..hi]), "{sql}");
            run.unwrap()
        }
        let sql = "DECLARE @id int = 42;\nSELECT * FROM orders WHERE customer = @id;\n\n\
                   DELETE FROM orders WHERE customer = @id;";
        assert_eq!(
            at(sql, "SELECT"),
            "DECLARE @id int = 42;\nSELECT * FROM orders WHERE customer = @id;"
        );
        assert_eq!(at(sql, "DECLARE"), "DECLARE @id int = 42;");
        assert_eq!(at(sql, "DELETE"), sql);
        // The editor's statement is still the whole piece: completion and
        // signature help see the variable.
        let caret = sql.find("SELECT").unwrap();
        assert_eq!(super::statement_range(sql, caret, d), (0, sql.len()));
        // A block and an `IF … ELSE` stay whole inside the scope.
        let block = "DECLARE @i int = 0;\nWHILE @i < 2\nBEGIN\n  SET @i += 1;\n  PRINT @i;\nEND;\n\
                     SELECT @i;";
        assert_eq!(
            at(block, "PRINT"),
            &block[..block.find("END;").unwrap() + 4]
        );
        let branch = "DECLARE @x int = 1;\nIF @x = 1 SELECT 1;\nELSE SELECT 2;\nDELETE FROM t;";
        assert_eq!(
            at(branch, "SELECT 1"),
            "DECLARE @x int = 1;\nIF @x = 1 SELECT 1;\nELSE SELECT 2;"
        );
        // A caret in the blank after the last statement still means it.
        let tail = "DECLARE @x int = 1;\nSELECT @x;\n\n";
        assert_eq!(
            super::executable_at(tail, tail.len(), d),
            Some("DECLARE @x int = 1;\nSELECT @x;")
        );
        // Outside a batch scope, and on the other engines, nothing changes.
        let plain = "SELECT 1; DELETE FROM t;";
        assert_eq!(at(plain, "SELECT"), "SELECT 1;");
        assert_eq!(
            super::run_current_range(sql, 30, SqlDialect::MySql),
            super::statement_range(sql, 30, SqlDialect::MySql)
        );
    }

    /// SQL Server's lexical rules otherwise: standard `--` comments, no `#`
    /// comment (`#t` is a temporary table), no backslash escape, `"…"` spans a
    /// name, no backticks, no dollar quotes.
    #[test]
    fn sql_server_lexes_by_its_own_rules() {
        let d = SqlDialect::MsSql;
        assert_eq!(super::skip_noncode(b"--x\ny", 0, d), Some(3));
        assert_eq!(super::skip_noncode(b"1--2", 1, d), Some(4));
        assert_eq!(super::skip_noncode(b"#t", 0, d), None);
        assert_eq!(super::skip_noncode(br"'C:\' x", 0, d), Some(5));
        assert_eq!(super::skip_noncode(br#""a""b" x"#, 0, d), Some(6));
        assert_eq!(super::skip_noncode(b"`a` x", 0, d), None);
        assert_eq!(super::skip_noncode(b"$a$ x $a$", 0, d), None);
        // `N'…'` is a word and a string, and the string is what is skipped.
        assert_eq!(super::skip_noncode(b"N'a;b'", 1, d), Some(6));
        assert_eq!(
            super::statement_ranges("SELECT N'a;b'; SELECT 2", d).len(),
            2
        );
    }

    /// `$` is a parameter sigil in SQLite, not a dollar-quote: `$tag$` must stay
    /// ordinary code, or everything after it is swallowed as a string.
    #[test]
    fn sqlite_has_no_dollar_quoting() {
        let s = b"$tag$ x $tag$";
        assert_eq!(super::skip_noncode(s, 0, SqlDialect::Sqlite), None);
        assert_eq!(super::skip_noncode(s, 0, SqlDialect::Postgres), Some(13));
    }

    /// Drive [`super::statement_bounds_open`] over `sql` cut into `chunk`-byte
    /// pieces, the way a script runner reads a file, and return the statements
    /// it emits.
    ///
    /// **This is the caller, not the function** — which is the whole point of
    /// having it. The scan is only half the answer; the other half is the
    /// buffer discipline (drain to the last boundary, carry the rest, and treat
    /// whatever is left at EOF as the final statement), and every bug worth
    /// catching here lives in the seam between the two.
    fn chunked(sql: &str, dialect: SqlDialect, chunk: usize) -> Vec<String> {
        let mut state = super::ScanState::new();
        let mut buf = String::new();
        let mut out: Vec<String> = Vec::new();
        let emit = |buf: &str, lo: usize, hi: usize, out: &mut Vec<String>| {
            let (lo, hi) = super::trim_range(buf, lo, hi);
            if lo < hi
                && super::segment_has_code(buf, lo, hi, dialect)
                && !super::is_delimiter_directive(buf, lo, hi, dialect)
            {
                out.push(buf[lo..hi].to_string());
            }
        };
        for piece in sql.as_bytes().chunks(chunk) {
            buf.push_str(std::str::from_utf8(piece).expect("ASCII fixture"));
            let bounds = super::statement_bounds_open(&buf, dialect, &mut state);
            let cut = bounds.last().expect("a scan always starts at 0").at;
            for w in bounds.windows(2) {
                emit(&buf, w[0].at, w[1].at, &mut out);
            }
            buf.drain(..cut);
        }
        // End of file: whatever is still held is the last statement, terminator
        // or no terminator.
        emit(&buf, 0, buf.len(), &mut out);
        out
    }

    /// The fixture the chunked tests run on: every construct that can hide a
    /// `;` from a naive split, plus a `DELIMITER` block, in the shape our own
    /// dump writes.
    const SCRIPT: &str = "-- a comment holding a ; semicolon\n\
                          SELECT 'a;b' AS x;\n\
                          INSERT INTO t VALUES (1), (2);\n\
                          -- Triggers\n\
                          DELIMITER $$\n\
                          CREATE TRIGGER tr BEFORE INSERT ON t FOR EACH ROW\n\
                          BEGIN\n\
                            SET NEW.a = 1;\n\
                            SET NEW.b = 2;\n\
                          END$$\n\
                          DELIMITER ;\n\
                          UPDATE t SET a = 3 WHERE id = 1;\n";

    /// **The composition test.** Reading the script in blocks must produce
    /// exactly the statements reading it whole does — at *every* block size, so
    /// the boundary lands inside a comment, a string, a directive and a trigger
    /// body in turn.
    ///
    /// A block size of 1 is not a silly edge here: it is the cheapest way to
    /// assert that no construct in the fixture depends on arriving intact.
    #[test]
    fn a_chunked_scan_matches_the_whole_file_answer() {
        for dialect in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            let whole: Vec<String> = super::statement_ranges(SCRIPT, dialect)
                .iter()
                .map(|&(lo, hi)| SCRIPT[lo..hi].to_string())
                .collect();
            for chunk in 1..=SCRIPT.len() {
                assert_eq!(
                    chunked(SCRIPT, dialect, chunk),
                    whole,
                    "{dialect:?} disagrees with itself at a {chunk}-byte block"
                );
            }
        }
    }

    /// The delimiter is set by one statement and governs the *next* ones, so a
    /// runner that drained the directive away before its body arrived would
    /// terminate the trigger on the `;` inside it.
    ///
    /// Pinned on its own, and not left to the round-trip above, because it is
    /// the one piece of state that has to survive the drain: the round-trip
    /// says the answers agree, this says *why* they can.
    ///
    /// **`END$$` is right here and wrong to send.** This asserts the *bounds* —
    /// a range carries its terminator, which is what the editor selects and
    /// highlights. What a server receives is
    /// [`super::executable_statements`]' answer, and
    /// `a_client_delimiter_is_never_sent_to_the_server` is where that is pinned.
    /// Confusing the two is precisely how `END$$` reached MySQL.
    #[test]
    fn a_delimiter_survives_the_chunk_that_drained_it() {
        // A block that ends exactly on the directive's newline — the drain that
        // takes the directive and leaves the body for the next read.
        let cut = SCRIPT.find("CREATE TRIGGER").expect("the fixture has one");
        let stmts = chunked(SCRIPT, SqlDialect::MySql, cut);
        assert!(
            stmts
                .iter()
                .any(|s| s.starts_with("CREATE TRIGGER") && s.ends_with("END$$")),
            "the trigger came out in pieces: {stmts:?}"
        );
    }

    /// **A `DELIMITER` a comment line precedes is still a directive**, as the
    /// `mysql` client reads it. Our own dump opens its trigger and routine
    /// sections with a `-- Triggers` line directly above the first
    /// `DELIMITER $$`, and the directive was taken only when nothing but
    /// whitespace preceded it in the segment — so the comment, the directive
    /// and the trigger's first statement went to the server as one, the body
    /// was cut at its first `;`, and every MySQL dump holding a compound
    /// trigger stopped there with its tables already replaced.
    #[test]
    fn a_delimiter_after_a_comment_line_is_still_honoured() {
        let s = "SELECT 1;\n\n-- Triggers\n\nDELIMITER $$\n\n\
                 CREATE TRIGGER t BEFORE INSERT ON o FOR EACH ROW\nBEGIN\n  \
                 SET NEW.a = 1;\n  SET NEW.b = 2;\nEND$$\n\n\
                 /* routines */ -- and more\n# and a hash one\nDELIMITER ;\n\nSELECT 2;";
        let stmts = super::executable_statements(s, SqlDialect::MySql);
        assert_eq!(
            stmts,
            vec![
                "SELECT 1;".to_string(),
                "CREATE TRIGGER t BEFORE INSERT ON o FOR EACH ROW\nBEGIN\n  \
                 SET NEW.a = 1;\n  SET NEW.b = 2;\nEND"
                    .to_string(),
                "SELECT 2;".to_string(),
            ]
        );
        // The script runner's splitter reads it the same at every block size.
        for chunk in 1..=s.len() {
            let got = chunked(s, SqlDialect::MySql, chunk);
            assert_eq!(got.len(), 3, "at a {chunk}-byte block: {got:?}");
            assert!(got[1].starts_with("CREATE TRIGGER") && got[1].ends_with("END$$"));
        }
        // A comment is not a statement head that makes `delimiter` code:
        // the word inside a statement is still data.
        assert_eq!(
            super::executable_statements("SELECT /* c */ delimiter FROM t;", SqlDialect::MySql),
            vec!["SELECT /* c */ delimiter FROM t;".to_string()]
        );
    }

    /// A block boundary inside a string literal must not end a statement, and
    /// must not end one at the `;` the string is hiding either.
    #[test]
    fn a_semicolon_inside_a_string_split_across_blocks_does_not_split() {
        let sql = "SELECT 'a;b' AS x; SELECT 2;";
        // Every cut through the literal.
        for chunk in 8..=12 {
            let stmts = chunked(sql, SqlDialect::MySql, chunk);
            assert_eq!(stmts.len(), 2, "at a {chunk}-byte block: {stmts:?}");
            assert_eq!(stmts[0], "SELECT 'a;b' AS x;");
        }
    }

    /// A directive that runs to the end of the buffer has not been read yet —
    /// its token ends at the newline, and there is no newline. Acting on the
    /// truncation would set the terminator to a *prefix* of the real one
    /// (`DELIMITER $` for `DELIMITER $$`), which then never matches.
    #[test]
    fn a_directive_cut_by_the_buffer_end_is_not_read_yet() {
        let mut state = super::ScanState::new();
        let bounds =
            super::statement_bounds_open("SELECT 1;\nDELIMITER $", SqlDialect::MySql, &mut state);
        assert_eq!(
            bounds.iter().map(|b| b.at).collect::<Vec<_>>(),
            vec![0, 9],
            "only the complete statement"
        );
        // And the proof it was not acted on: the next chunk completes the
        // directive, and `$$` then terminates.
        let bounds =
            super::statement_bounds_open("DELIMITER $$\nSELECT 2$$", SqlDialect::MySql, &mut state);
        assert_eq!(
            bounds.len(),
            3,
            "the directive, then the statement it governs"
        );
    }

    /// The whole-file answer is the open scan plus the final segment, so the
    /// refactor that introduced [`super::statement_bounds_open`] cannot have
    /// moved any boundary the rest of the app already depends on.
    #[test]
    fn statement_bounds_is_the_open_scan_plus_the_tail() {
        for dialect in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            for sql in [
                SCRIPT,
                "SELECT 1;",
                "SELECT 1",
                "",
                "DELIMITER $$\n",
                "-- just a comment",
            ] {
                let mut state = super::ScanState::new();
                let mut open: Vec<usize> = super::statement_bounds_open(sql, dialect, &mut state)
                    .iter()
                    .map(|b| b.at)
                    .collect();
                open.push(sql.len());
                assert_eq!(
                    open,
                    super::statement_bounds(sql, dialect),
                    "{dialect:?} on {sql:?}"
                );
            }
        }
    }

    /// **The bug the whole `DELIMITER` machinery exists to avoid, reintroduced
    /// at the last step.** A statement is split correctly and then handed to the
    /// server with the *client's* terminator still on it: `END$$` lexes as one
    /// identifier in MySQL, the compound body never closes, and every dump
    /// carrying a trigger or routine fails at its first one — half-loaded.
    ///
    /// Confirmed against MariaDB before this was written:
    /// `mariadb -e "SELECT 1$$"` → `ERROR 1054: Unknown column '1$$'`.
    #[test]
    fn a_client_delimiter_is_never_sent_to_the_server() {
        let s = "DELIMITER $$\n\
                 CREATE TRIGGER tr BEFORE INSERT ON t FOR EACH ROW\n\
                 BEGIN\n  SET NEW.a = 1;\n  SET NEW.b = 2;\nEND$$\n\
                 DELIMITER ;\n\
                 UPDATE t SET a = 3;\n";
        let stmts = super::executable_statements(s, SqlDialect::MySql);
        assert_eq!(stmts.len(), 2, "{stmts:?}");
        assert!(stmts[0].starts_with("CREATE TRIGGER"), "{}", stmts[0]);
        assert!(
            stmts[0].ends_with("END"),
            "the client's delimiter reached the server: {:?}",
            stmts[0]
        );
        // And the body's own semicolons are untouched — the reason the
        // directive was used in the first place.
        assert!(stmts[0].contains("SET NEW.a = 1;"), "{}", stmts[0]);
    }

    /// **`;` stays.** Every engine accepts a trailing one, and stripping it
    /// would change what every existing call site sends — the blast radius this
    /// fix deliberately does not have.
    #[test]
    fn an_ordinary_semicolon_is_left_on_the_statement() {
        let stmts = super::executable_statements("SELECT 1; SELECT 2;", SqlDialect::MySql);
        assert_eq!(
            stmts,
            vec!["SELECT 1;".to_string(), "SELECT 2;".to_string()]
        );
    }

    /// A terminator with whitespace in front of it still comes off, and takes
    /// the whitespace with it.
    #[test]
    fn a_delimiter_is_stripped_along_with_the_space_before_it() {
        let stmts = super::executable_statements("DELIMITER $$\nSELECT 1 $$\n", SqlDialect::MySql);
        assert_eq!(stmts, vec!["SELECT 1".to_string()]);
    }

    /// **The third executing path, which the fix above missed.** Run Everything
    /// and the script runner were wired to `executable_statements`; *Run
    /// Current* still sliced `statement_range` and sent that, so a caret inside
    /// a trigger sent `…END$$` and a caret on the directive line sent
    /// `DELIMITER $$` itself.
    ///
    /// Asserted against `executable_statements` for the same file, so the two
    /// paths cannot answer differently — which is the property that was broken,
    /// not either function on its own.
    #[test]
    fn running_the_statement_under_the_caret_sends_what_run_everything_would() {
        let s = "DELIMITER $$\n\
                 CREATE TRIGGER tr BEFORE INSERT ON t FOR EACH ROW\n\
                 BEGIN\n  SET NEW.a = 1;\nEND$$\n\
                 DELIMITER ;\n\
                 UPDATE t SET a = 3;\n";
        let whole = super::executable_statements(s, SqlDialect::MySql);
        assert_eq!(whole.len(), 2, "{whole:?}");

        // A caret anywhere in the trigger body sends the trigger, without the
        // client's terminator on it.
        let body = s.find("SET NEW.a").expect("fixture");
        assert_eq!(
            super::executable_at(s, body, SqlDialect::MySql),
            Some(whole[0].as_str())
        );

        // A caret on the `DELIMITER` line itself has nothing to run — the
        // directive is the client's, and it used to be sent to the server.
        assert_eq!(super::executable_at(s, 2, SqlDialect::MySql), None);

        // **What the old spelling did**, pinned so that "simplifying"
        // `executable_at` back to a slice of `statement_range` fails here
        // rather than at a server. This is the assertion that would have been
        // red: it is exactly what *Run Current* sent.
        let (lo, hi) = super::statement_range(s, body, SqlDialect::MySql);
        assert!(
            s[lo..hi].ends_with("END$$"),
            "the range is right to highlight and wrong to send: {:?}",
            &s[lo..hi]
        );
        let (lo, hi) = super::statement_range(s, 2, SqlDialect::MySql);
        assert_eq!(
            &s[lo..hi],
            "DELIMITER $$",
            "and on the directive line it is the directive"
        );

        // And every caret position in the file agrees with one of the two, or
        // with nothing.
        for at in 0..=s.len() {
            if !s.is_char_boundary(at) {
                continue;
            }
            match super::executable_at(s, at, SqlDialect::MySql) {
                None => {}
                Some(got) => assert!(
                    whole.iter().any(|w| w == got),
                    "at {at}: Run Current would send {got:?}, which Run Everything never sends"
                ),
            }
        }
    }

    /// The ordinary case is unchanged: one statement in, the same statement
    /// out, terminator included — and a caret past the last `;` still means the
    /// statement before it, as `statement_range` has always answered.
    #[test]
    fn the_caret_picks_the_statement_it_is_in_and_falls_back_to_the_one_before() {
        let s = "SELECT 1;\nSELECT 2;\n";
        assert_eq!(
            super::executable_at(s, 3, SqlDialect::MySql),
            Some("SELECT 1;")
        );
        assert_eq!(
            super::executable_at(s, 13, SqlDialect::MySql),
            Some("SELECT 2;")
        );
        assert_eq!(
            super::executable_at(s, s.len(), SqlDialect::MySql),
            Some("SELECT 2;"),
            "a caret after the final terminator means the statement before it"
        );
        assert_eq!(super::executable_at("   \n", 0, SqlDialect::MySql), None);
        assert_eq!(
            super::executable_at("-- just a comment\n", 4, SqlDialect::MySql),
            None
        );
    }

    /// The other two engines have no client delimiter at all, so nothing is ever
    /// stripped there — a `$$` in PostgreSQL is dollar-quoting, which is data.
    #[test]
    fn only_the_engine_with_the_directive_strips_anything() {
        for dialect in [SqlDialect::Postgres, SqlDialect::Sqlite] {
            let stmts = super::executable_statements("SELECT 1;", dialect);
            assert_eq!(stmts, vec!["SELECT 1;".to_string()], "{dialect:?}");
        }
    }

    /// How much a verdict refuses, so two of them can be compared. The whole
    /// point of [`super::script_verdict`] is that it is never *below*
    /// [`super::run_verdict`] on this scale.
    fn strength(v: &super::RunVerdict) -> u8 {
        match v {
            super::RunVerdict::Allow => 0,
            super::RunVerdict::Confirm(_) => 1,
            super::RunVerdict::Block(_) => 2,
        }
    }

    /// Every policy a connection can be in, for the exhaustive tests below.
    fn policies() -> Vec<super::GuardPolicy> {
        let mut out = Vec::new();
        for read_only in [false, true] {
            for confirm_writes in [false, true] {
                for no_database in [false, true] {
                    for dialect in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
                        out.push(super::GuardPolicy {
                            read_only,
                            confirm_writes,
                            no_database,
                            dialect,
                        });
                    }
                }
            }
        }
        out
    }

    /// **The invariant, as a test.** A script gate that ever came out *weaker*
    /// than the run guard would be the second, laxer gate the architecture
    /// forbids — so compare the two over every policy and a set of statement
    /// lists that reaches each of `run_verdict`'s arms.
    ///
    /// The statements are what `run_verdict` gets; `script_verdict` never sees
    /// them, which is exactly the asymmetry under test.
    #[test]
    fn the_script_gate_is_never_weaker_than_the_run_guard() {
        let lists: Vec<Vec<String>> = vec![
            vec![],
            vec!["SELECT 1".to_string()],
            vec!["INSERT INTO t VALUES (1)".to_string()],
            vec!["DELETE FROM t".to_string()],
            vec!["CREATE TABLE t (a int)".to_string()],
            vec!["SELECT 1".to_string(), "DROP TABLE t".to_string()],
        ];
        for policy in policies() {
            let script = super::script_verdict(policy, "dump.sql");
            for stmts in &lists {
                let run = super::run_verdict(stmts, policy);
                assert!(
                    strength(&script) >= strength(&run),
                    "script {script:?} is weaker than run {run:?} for {policy:?} on {stmts:?}"
                );
            }
        }
    }

    /// A script never simply runs. `confirm_writes` off is the axis that makes
    /// this worth pinning: there `run_verdict` returns `Allow` for a write, and
    /// a script gate that merely delegated would inherit that.
    #[test]
    fn a_script_never_allows_whatever_the_settings_say() {
        for policy in policies() {
            assert_ne!(
                super::script_verdict(policy, "dump.sql"),
                super::RunVerdict::Allow,
                "{policy:?}"
            );
        }
    }

    /// The file has not been read, so "does it write?" cannot be asked — and the
    /// safe answer to a question you cannot ask is yes. A read-only connection
    /// refuses every script, including one that holds nothing but `SELECT`s.
    #[test]
    fn a_read_only_connection_refuses_a_script_without_reading_it() {
        let policy = super::GuardPolicy {
            read_only: true,
            confirm_writes: false,
            no_database: false,
            dialect: SqlDialect::MySql,
        };
        assert!(matches!(
            super::script_verdict(policy, "reads-only.sql"),
            super::RunVerdict::Block(_)
        ));
    }

    /// Blocked, not confirmed: an unscoped script on PostgreSQL builds into a
    /// maintenance database the schema tree can never show again.
    ///
    /// **No caller can produce this policy today** — `script_view::policy`
    /// hard-codes `no_database: false`, because the modal is only reachable from
    /// a database node. The arm is kept, and tested, because the property that
    /// matters is the *relation*: this gate must be no weaker than `run_verdict`
    /// for **every** policy, including the ones a second caller might one day
    /// hand it. Deleting it would leave that relation true only by accident.
    #[test]
    fn a_script_with_no_database_is_blocked_outright() {
        let policy = super::GuardPolicy {
            read_only: false,
            confirm_writes: false,
            no_database: true,
            dialect: SqlDialect::Postgres,
        };
        assert!(matches!(
            super::script_verdict(policy, "dump.sql"),
            super::RunVerdict::Block(_)
        ));
    }

    /// An ordinary policy yields `Confirm` — never `Allow`.
    ///
    /// **What this does *not* claim is that a bar appears on screen.** The
    /// script modal treats its own panel as the confirmation: the user picked
    /// the file, and the panel names the statement counts and, in red, how many
    /// of them destroy or delete data, above a button marked Run. So the arm is
    /// satisfied there rather than rendered, and `script_view::run_script`
    /// matches it out exhaustively so that is a decision on the page instead of
    /// a fall-through.
    ///
    /// This test used to assert the *wording* of a message nothing displays —
    /// which read as coverage of a confirmation flow that did not exist. What
    /// is worth pinning is the arm.
    #[test]
    fn an_ordinary_policy_asks_rather_than_allowing() {
        let policy = super::GuardPolicy {
            read_only: false,
            confirm_writes: false,
            no_database: false,
            dialect: SqlDialect::MySql,
        };
        assert!(matches!(
            super::script_verdict(policy, "sakila-dump.sql"),
            super::RunVerdict::Confirm(_)
        ));
    }

    /// The one place the two scans deliberately disagree, stated so a later
    /// edit can't quietly flatten it: a directive with no newline behind it is
    /// complete when the file ends there, and unfinished when a chunk does.
    ///
    /// This is why `"DELIMITER $$"` is not in the equivalence list above.
    #[test]
    fn only_the_end_of_a_file_can_complete_a_trailing_directive() {
        let sql = "DELIMITER $$";
        assert_eq!(
            super::statement_bounds(sql, SqlDialect::MySql),
            vec![0, 12, 12],
            "at EOF there is nothing more coming, so the directive is whole"
        );
        let mut state = super::ScanState::new();
        assert_eq!(
            super::statement_bounds_open(sql, SqlDialect::MySql, &mut state)
                .iter()
                .map(|b| b.at)
                .collect::<Vec<_>>(),
            vec![0],
            "mid-script the token may still be growing"
        );
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
    fn pg_dollar_tag_may_carry_a_non_ascii_letter() {
        // PostgreSQL's rule for the tag is "the same rules as an unquoted
        // identifier, except that it cannot contain a dollar sign", and an
        // unquoted identifier may carry diacritics and non-Latin letters —
        // architecture invariant 11. The tag scan was the third word scanner in
        // this module and was not swept with the other two.
        let s = "$café$ a;b $café$ rest";
        let body_end = s.rfind("$café$").unwrap() + "$café$".len();
        let end = super::skip_noncode(s.as_bytes(), 0, PG).unwrap();
        assert_eq!(end, body_end);
        assert_eq!(&s[..end], "$café$ a;b $café$");

        // The composition that is the actual damage: the splitter must not see
        // the body's own semicolons.
        assert_eq!(super::statement_ranges("SELECT $ü$ a; b $ü$", PG).len(), 1);
        assert_eq!(
            super::statement_ranges("SELECT $ü$ a; b $ü$; SELECT 2", PG).len(),
            2
        );
    }

    #[test]
    fn pg_dollar_tag_cannot_begin_with_a_digit() {
        // `$1$` is two positional params with nothing between them, not a tag:
        // a PostgreSQL identifier cannot begin with a digit.
        assert_eq!(super::skip_noncode(b"$1$ a;b $1$", 0, PG), None);
        // …and a digit still *continues* one.
        let s = "$t1$a;b$t1$";
        let end = super::skip_noncode(s.as_bytes(), 0, PG).unwrap();
        assert_eq!(&s[..end], s);
    }

    /// **Asked of one byte, so a `$`-dense statement lexes in one pass.** The
    /// first spelling walked back over the whole name before every `$`, and
    /// `a$a$a$…` rescanned everything before each one — a statement a model
    /// can send to `run_query`, and text the editor splits on every keystroke.
    #[test]
    fn pg_a_dollar_dense_name_lexes_in_one_pass() {
        let sql = format!("SELECT {}1", "a$".repeat(200_000));
        assert_eq!(super::statement_ranges(&sql, PG).len(), 1);
        assert!(super::read_only_reason(&sql, PG).is_ok());
    }

    /// **A `$` inside a name continues the name**, and the lexer opened a
    /// dollar quote there. PostgreSQL's identifiers may carry `$` after their
    /// first character, so `a$$` is one name — measured on PG 16.15, where
    /// `SELECT 1 AS a$$; SELECT 2; SELECT 3 AS b$$` returned three result sets
    /// with columns `a$$` and `b$$`. The lexer read `$$; SELECT 2; SELECT 3 AS
    /// b$$` as one string, so the read-only gate saw a single `SELECT` and the
    /// server ran whatever sat between the two names — through
    /// `simple_query_raw`, which takes several statements.
    #[test]
    fn pg_a_dollar_inside_a_name_opens_no_quote() {
        let smuggle = "SELECT 1 AS a$$; SELECT pg_sleep(100); SELECT 1 AS b$$";
        let err = super::read_only_reason(smuggle, PG).expect_err("three statements");
        assert!(err.contains("single statement"), "{err}");
        assert_eq!(super::statement_ranges(smuggle, PG).len(), 3);
        assert!(super::contains_write(
            "SELECT 1 AS a$$; DELETE FROM t; SELECT 1 AS b$$",
            PG
        ));
        // A name is a name however many dollars it carries, and a keyword is
        // lexed as one too: `SELECT$$x$$` is the identifier `select$$x$$`.
        assert_eq!(super::skip_noncode(b"a$$$$", 1, PG), None);
        assert_eq!(super::skip_noncode(b"SELECT$$x$$", 6, PG), None);
        // A dollar quote still opens wherever a token starts.
        for sql in [
            "SELECT $$a;b$$",
            "SELECT f($$a;b$$)",
            "SELECT x=$$a;b$$",
            "SELECT 'q'$$a;b$$",
        ] {
            assert_eq!(super::statement_ranges(sql, PG).len(), 1, "{sql}");
        }
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

    /// **The export's gate is the write guard's, only stricter.** These are the
    /// row-returning writes — each would otherwise reach a truncated grid, be
    /// offered an "All rows" export, and be executed a second time from a Save
    /// dialog with nothing asked.
    #[test]
    fn a_row_returning_write_is_never_rerunnable_for_an_export() {
        for d in EVERY_DIALECT {
            assert!(
                !super::rerunnable_for_export("UPDATE orders SET n = n + 1 RETURNING *", d),
                "{d:?}: an UPDATE … RETURNING returns rows and must not be re-run"
            );
            assert!(
                !super::rerunnable_for_export("INSERT INTO t (a) VALUES (1) RETURNING id", d),
                "{d:?}: an INSERT … RETURNING returns rows and must not be re-run"
            );
            assert!(
                !super::rerunnable_for_export("DELETE FROM t WHERE id = 1 RETURNING *", d),
                "{d:?}: a DELETE … RETURNING returns rows and must not be re-run"
            );
            // A data-modifying CTE reads as a `WITH`, which *is* a read head —
            // `contains_write`'s keyword sweep is what catches it, and this is
            // the case that would slip a head-only check.
            assert!(
                !super::rerunnable_for_export(
                    "WITH moved AS (DELETE FROM a RETURNING *) SELECT * FROM moved",
                    d
                ),
                "{d:?}: a data-modifying CTE must not be re-run"
            );
        }
        // A stored procedure returns rows and may write anything at all.
        assert!(!super::rerunnable_for_export(
            "CALL restock()",
            SqlDialect::MySql
        ));
    }

    /// And the other half: the reads an export exists for are all allowed, so
    /// the gate has not simply refused everything.
    #[test]
    fn an_ordinary_read_is_rerunnable_for_an_export() {
        for d in EVERY_DIALECT {
            for sql in [
                "SELECT * FROM rental",
                "SELECT a, b FROM t WHERE a > 1 ORDER BY b LIMIT 10",
                "WITH c AS (SELECT 1) SELECT * FROM c",
                // A write word inside a string or a comment is not a write.
                "SELECT 'update me' FROM t -- delete later",
            ] {
                assert!(
                    super::rerunnable_for_export(sql, d),
                    "{d:?}: refused a plain read: {sql}"
                );
            }
        }
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

    /// **T-SQL keeps its secrets in words the MySQL/PostgreSQL rules never
    /// named**: its principal is a `LOGIN`, a credential's secret is `SECRET =`,
    /// a key or certificate is protected `BY PASSWORD`, and the system
    /// procedures take the password as a bare argument with no `PASSWORD` word
    /// at all. Every one of these went into `history.json` in the clear.
    #[test]
    fn carries_credential_knows_t_sql_credential_statements() {
        let ms = |s: &str| super::carries_credential(s, SqlDialect::MsSql);
        for s in [
            "CREATE LOGIN app WITH PASSWORD = N'hunter2'",
            "ALTER LOGIN sa WITH PASSWORD = N'new' OLD_PASSWORD = N'old'",
            "create login [app] with password = 'x' must_change, check_policy = on",
            "CREATE DATABASE SCOPED CREDENTIAL c WITH IDENTITY = 'x', SECRET = 'p'",
            "ALTER CREDENTIAL c WITH IDENTITY = 'x', SECRET = 'p'",
            "CREATE MASTER KEY ENCRYPTION BY PASSWORD = 'p'",
            "OPEN MASTER KEY DECRYPTION BY PASSWORD = 'p'",
            "CREATE CERTIFICATE c ENCRYPTION BY PASSWORD = 'p' WITH SUBJECT = 's'",
            "CREATE SYMMETRIC KEY k WITH ALGORITHM = AES_256 ENCRYPTION BY PASSWORD = 'p'",
            "EXEC sp_addlogin 'n', 'pw'",
            "EXECUTE master.dbo.sp_password 'old', 'new', 'n'",
            "EXEC sp_addlinkedsrvlogin 'srv', 'false', NULL, 'u', 'pw'",
            "EXEC sp_setapprole 'r', 'pw'",
            "sp_addapprole 'r', 'pw'",
            "SELECT ENCRYPTBYPASSPHRASE('pw', 'data')",
            // Anywhere in a batch.
            "SELECT 1; CREATE LOGIN app WITH PASSWORD = 'x'",
        ] {
            assert!(ms(s), "{s}");
        }
        // A column or a table that merely shares a word is not a credential.
        for s in [
            "SELECT password FROM logins",
            "SELECT login, password FROM dbo.accounts",
            "CREATE TABLE t (login varchar(20), password varchar(64))",
            "UPDATE t SET secret = 1",
            "SELECT 'BY PASSWORD' AS note",
            "SELECT name FROM sys.credentials",
            "SELECT 1 -- CREATE LOGIN a WITH PASSWORD = 'x'",
        ] {
            assert!(!ms(s), "{s}");
        }
    }

    /// **A T-SQL credential statement is judged where it begins, not only at
    /// its range's head.** `scan_bounds` keeps a batch whole from a `DECLARE`
    /// on, and no `;` cuts inside `BEGIN … END`, so the password statement in
    /// each of these was the head of no range — and the head-only rule wrote
    /// it to `history.json`. The idempotent `IF NOT EXISTS … BEGIN CREATE
    /// LOGIN …` is the shape every provisioning script has.
    #[test]
    fn carries_credential_finds_a_t_sql_password_statement_inside_a_batch() {
        let ms = |s: &str| super::carries_credential(s, SqlDialect::MsSql);
        for s in [
            "DECLARE @n int = 1; CREATE USER app WITH PASSWORD = 'S3cret!x';",
            "IF NOT EXISTS (SELECT 1 FROM sys.server_principals WHERE name = N'bob') \
             BEGIN CREATE LOGIN bob WITH PASSWORD = 'S3cret!x'; END",
            "BEGIN TRY CREATE USER app WITH PASSWORD = 'S3cret!x'; END TRY \
             BEGIN CATCH THROW; END CATCH",
            "DECLARE @n int = 1; ALTER LOGIN bob WITH PASSWORD = 'S3cret!x';",
            "SET NOCOUNT ON\nCREATE LOGIN bob WITH PASSWORD = 'S3cret!x'",
        ] {
            assert!(ms(s), "{s}");
        }
        // Still only a statement that sets one: a batch reading a column of
        // that name is recorded.
        assert!(!ms(
            "DECLARE @n int = 1; SELECT login, password FROM dbo.accounts;"
        ));
        assert!(!ms("IF 1 = 1 BEGIN SELECT password FROM dbo.accounts; END"));
    }

    /// **A password inside a connection-string argument, a replication
    /// setting or a procedure's `@…password` parameter is a credential too.**
    /// Each of these is a password typed into the statement, and none has the
    /// `IDENTIFIED`/`BY PASSWORD`/principal shape the rules above look for —
    /// the password sits in a string the tokens skip, under a word like
    /// `SOURCE_PASSWORD`, or in a positional argument.
    #[test]
    fn carries_credential_knows_connection_strings_and_password_arguments() {
        let ms = |s: &str| super::carries_credential(s, SqlDialect::MsSql);
        for s in [
            "SELECT * FROM OPENROWSET('MSOLEDBSQL', 'Server=h;UID=sa;PWD=S3cret!;', 'SELECT 1 AS x')",
            "SELECT * FROM OPENROWSET('SQLNCLI', 'h'; 'sa'; 'S3cret!', 'SELECT 1')",
            "SELECT * FROM OPENDATASOURCE('MSOLEDBSQL', 'Data Source=h;User ID=sa;Password=S3cret!').db.dbo.t",
            "EXEC sp_addlinkedserver @server = N'L', @srvproduct = N'', \
             @provider = N'MSOLEDBSQL', @provstr = N'Server=h;Uid=sa;Pwd=S3cret!'",
            "EXEC sp_change_users_login 'Auto_Fix', 'appuser', NULL, 'S3cret!'",
            "EXEC sp_change_users_login @Action = 'Auto_Fix', \
             @UserNamePattern = 'appuser', @Password = 'S3cret!'",
            "EXEC sp_adddistributor @distributor = 'srv', @password = 'S3cret!'",
            "EXEC sp_addpushsubscription_agent @publication = 'p', \
             @subscriber_password = 'S3cret!'",
            "EXEC sp_addlogreader_agent @job_login = 'x', @job_password = 'S3cret!'",
            "DECLARE @n int = 1; EXEC sp_adddistributor 'srv', 'S3cret!'",
        ] {
            assert!(ms(s), "{s}");
        }
        for s in [
            "CHANGE REPLICATION SOURCE TO SOURCE_HOST = 'h', SOURCE_USER = 'r', \
             SOURCE_PASSWORD = 'S3cret!'",
            "CHANGE MASTER TO MASTER_HOST = 'h', MASTER_PASSWORD = 'S3cret!'",
            "START REPLICA USER = 'r' PASSWORD = 'S3cret!'",
            "START SLAVE USER = 'r' PASSWORD = 'S3cret!'",
        ] {
            assert!(carries_credential(s), "{s}");
        }
        for s in [
            "SELECT * FROM dblink('host=h dbname=d user=u password=S3cret!', \
             'SELECT 1') AS t(x int)",
            "SELECT dblink_connect('c', 'host=h password=S3cret!')",
            "SELECT dblink_connect_u('host=h password=S3cret!')",
            "SELECT dblink_exec('host=h password=S3cret!', 'DELETE FROM t')",
        ] {
            assert!(super::carries_credential(s, PG), "{s}");
        }
        // Not every read of a remote source or a password-named column.
        for s in [
            "SELECT * FROM OPENROWSET(BULK 'C:\\data\\f.json', SINGLE_CLOB) AS j",
            "EXEC sp_helplogins",
            "SELECT user_password FROM dbo.accounts",
            "UPDATE dbo.accounts SET last_password_change = GETDATE() WHERE id = 1",
        ] {
            assert!(!ms(s), "{s}");
        }
        assert!(!carries_credential("SELECT master_password FROM t"));
        assert!(!carries_credential("START TRANSACTION"));
    }

    /// **A client terminator glued to `END` is not a misspelled keyword.**
    /// With `$` continuing a MySQL name, a range closed by `DELIMITER $`'s
    /// terminator ends in the one word `END$` — which the typo check, one edit
    /// from `END`, flagged. No keyword holds a `$`, so a word that does is a
    /// name.
    #[test]
    fn a_dollar_terminator_after_end_draws_no_typo_warning() {
        let cat = crate::intel::Catalog::build(&[], None);
        for s in [
            "DELIMITER $\nCREATE PROCEDURE p() BEGIN SELECT 1; END$\nDELIMITER ;",
            "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; END$$\nDELIMITER ;",
            "SELECT a$b FROM t$x",
        ] {
            let d = crate::intel::diagnostics(s, &cat, SqlDialect::MySql);
            assert!(
                !d.iter().any(|d| d.message.contains("misspelled")),
                "{s}: {d:?}"
            );
        }
    }

    /// **Where a name ends is `continues_name`'s answer, for every engine.**
    /// MySQL (`[0-9a-zA-Z$_]`), SQLite, PostgreSQL and SQL Server all continue
    /// a bare name through `$`, so `a$delete` is one column — but MySQL and
    /// SQLite were said to stop at it.
    #[test]
    fn a_dollar_continues_a_name_on_every_engine() {
        for d in [
            SqlDialect::MySql,
            SqlDialect::Sqlite,
            SqlDialect::Postgres,
            SqlDialect::MsSql,
        ] {
            assert!(super::continues_name(b'$', d), "{d:?}");
            assert!(super::continues_name(b'a', d), "{d:?}");
            assert!(!super::continues_name(b' ', d), "{d:?}");
        }
        // A continuation only: `$` begins no name.
        assert!(!super::is_word_start(b'$'));
    }

    /// **The guards' word scanners end a name where `continues_name` says.**
    /// `word_tokens` and `tsql_statements` stopped at `$` on every engine, so
    /// `SELECT a$delete FROM t` — one column — read as a `DELETE`: refused by
    /// the read-only gate on all four, blocked on a read-only connection, and
    /// on SQL Server cut into `a$` | `delete FROM t` for a "DELETE without
    /// WHERE" confirm over a read.
    #[test]
    fn a_name_holding_a_dollar_is_not_the_keyword_after_it() {
        for d in [
            SqlDialect::MySql,
            SqlDialect::Sqlite,
            SqlDialect::Postgres,
            SqlDialect::MsSql,
        ] {
            for s in ["SELECT a$delete FROM t", "SELECT v$truncate FROM t"] {
                assert_eq!(super::read_only_reason(s, d), Ok(()), "{d:?}: {s}");
                assert!(!super::contains_write(s, d), "{d:?}: {s}");
                assert_eq!(super::first_unsafe(s, d), None, "{d:?}: {s}");
            }
        }
        let ms = SqlDialect::MsSql;
        assert_eq!(
            super::first_unsafe("DELETE FROM dbo.a$update WHERE id = 1", ms),
            None
        );
        // The keyword itself is still seen.
        assert!(super::first_unsafe("SELECT a$ FROM t; DELETE FROM t", ms).is_some());
        assert!(super::read_only_reason("SELECT 1; DELETE FROM t", SqlDialect::MySql).is_err());
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

    /// **Dropping what holds rows asks first, like `TRUNCATE`** — a CLI test
    /// run found `DROP TABLE` running with no `--yes` while the less
    /// destructive `TRUNCATE` was held. The warning names what goes.
    ///
    /// Every dialect: the verdict does not depend on the engine, and it reads
    /// two `leading_keyword`s, which do (`#` comments, `[ident]`, `$$`).
    #[test]
    fn dropping_a_table_database_or_schema_asks_first() {
        for d in EVERY_DIALECT {
            for (sql, noun) in [
                ("DROP TABLE t", "table"),
                ("drop table if exists a, b", "table"),
                // MySQL's grammar is `DROP [TEMPORARY] {TABLE | TABLES}`, and
                // the plural drops the table just the same.
                ("DROP TABLES t", "table"),
                ("drop tables if exists a, b", "table"),
                ("DROP DATABASE app", "database"),
                ("DROP SCHEMA IF EXISTS app CASCADE", "schema"),
                ("/* tidy */ DROP TABLE t", "table"),
                ("DROP TABLE \"t\" CASCADE", "table"),
            ] {
                let why =
                    super::unsafe_reason(sql, d).unwrap_or_else(|| panic!("{d:?}: {sql} must ask"));
                assert!(why.contains(noun), "{d:?}: {sql}: {why}");
            }
        }
        assert!(super::unsafe_reason("drop table [t]", SqlDialect::Sqlite).is_some());
    }

    /// **What destroys stored rows without being spelled `DROP TABLE`.**
    /// MariaDB's `CREATE OR REPLACE TABLE` drops the table before creating the
    /// empty one (and `… DATABASE` every table in it); PostgreSQL's `DROP OWNED
    /// BY` drops everything a role owns, and a `DROP TYPE`/`DOMAIN`/`EXTENSION
    /// … CASCADE` every column of that type, with its values. And MySQL's
    /// partition surgery: `TRUNCATE PARTITION` and `DROP PARTITION` erase the
    /// rows they name.
    #[test]
    fn a_statement_that_destroys_stored_rows_by_another_name_asks_first() {
        for d in EVERY_DIALECT {
            for (sql, noun) in [
                ("CREATE OR REPLACE TABLE orders (id int)", "table"),
                ("create or replace database app", "database"),
                ("DROP OWNED BY app_owner", "owns"),
                ("DROP OWNED BY r CASCADE", "owns"),
                ("DROP TYPE mood CASCADE", "column"),
                ("DROP DOMAIN d CASCADE", "column"),
                ("DROP EXTENSION IF EXISTS hstore CASCADE", "column"),
                ("ALTER TABLE t TRUNCATE PARTITION p0, p1", "partition"),
                ("ALTER TABLE t DROP PARTITION p0", "partition"),
                ("ALTER TABLE t TRUNCATE PARTITION ALL", "every row"),
            ] {
                let why =
                    super::unsafe_reason(sql, d).unwrap_or_else(|| panic!("{d:?}: {sql} must ask"));
                assert!(why.contains(noun), "{d:?}: {sql}: {why}");
            }
        }
    }

    /// **Only what holds data.** A temporary table dies with the session, and
    /// a view, index, trigger or function holds no rows, so none of them is
    /// held back — a guard that fires on every DDL statement is one users learn
    /// to click through. A type or domain with no `CASCADE` is refused by the
    /// server while anything depends on it, so it takes no rows either; and a
    /// `CREATE OR REPLACE` of what holds no rows is how MariaDB and PostgreSQL
    /// users write every view and routine.
    #[test]
    fn dropping_what_holds_no_stored_rows_does_not_ask() {
        for d in EVERY_DIALECT {
            for sql in [
                "DROP VIEW v",
                "DROP INDEX i ON t",
                "DROP TRIGGER tr",
                "DROP FUNCTION f",
                "DROP USER u",
                "DROP TYPE mood",
                "DROP DOMAIN d",
                "DROP VIEW v CASCADE",
                "CREATE OR REPLACE VIEW v AS SELECT 1",
                "CREATE OR REPLACE FUNCTION f() RETURNS int AS $$ SELECT 1 $$ LANGUAGE sql",
                "CREATE OR REPLACE TRIGGER tr BEFORE INSERT ON t FOR EACH ROW SET @x = 1",
                "CREATE TABLE t (id int)",
                "ALTER TABLE t ADD PARTITION (PARTITION p9 VALUES LESS THAN (9))",
                "ALTER TABLE t DROP COLUMN c",
            ] {
                assert_eq!(super::unsafe_reason(sql, d), None, "{d:?}: {sql}");
            }
        }
        for sql in [
            "DROP TEMPORARY TABLE t",
            "DROP TEMPORARY TABLES t",
            "CREATE OR REPLACE TEMPORARY TABLE t (id int)",
        ] {
            assert_eq!(unsafe_reason(sql), None, "{sql}");
        }
    }

    /// **PostgreSQL's block comments nest, and the one lexer has to know.**
    /// It ended every `/*` at the first `*/`, so on PostgreSQL the head after
    /// a nested comment was read from inside it: `/* a /* b */ c */ DELETE
    /// FROM t` had head `C` to every arm, and ran the every-row DELETE with no
    /// ask — the ordinary way it arises is commenting out a block that already
    /// holds a comment. Worse, a quote inside the comment's tail was read as a
    /// string opening, so a real second statement after it was invisible to
    /// every gate. MySQL and SQLite do not nest, and keep today's reading.
    #[test]
    fn a_nested_block_comment_ends_where_its_dialect_ends_it() {
        let nested = "/* old: /* note */ still comment */ DELETE FROM t";
        let pg = SqlDialect::Postgres;
        assert_eq!(
            super::leading_keyword(nested, pg).as_deref(),
            Some("DELETE")
        );
        assert!(super::unsafe_reason(nested, pg).is_some());
        let block = "/* disabled\n  SELECT 1; /* keep */\n*/\nDROP TABLE t";
        assert!(super::unsafe_reason(block, pg).is_some());
        // The quote is inside the comment on PostgreSQL, so what follows is a
        // second statement — refused by the read gate, and a write.
        let hidden = "SELECT 1 /* /* */ ' */; DELETE FROM t; --'";
        assert!(super::read_only_reason(hidden, pg).is_err());
        assert!(super::contains_write(hidden, pg));
        // An unterminated nest runs to the end, as an unterminated comment does.
        assert_eq!(super::leading_keyword("/* /* */ DELETE FROM t", pg), None);
        // SQL Server nests them too.
        let ms = SqlDialect::MsSql;
        assert_eq!(
            super::leading_keyword(nested, ms).as_deref(),
            Some("DELETE")
        );
        assert!(super::contains_write(hidden, ms));
        for d in [SqlDialect::MySql, SqlDialect::Sqlite] {
            assert_eq!(
                super::leading_keyword(nested, d).as_deref(),
                Some("STILL"),
                "{d:?} does not nest"
            );
        }
    }

    /// **An `EXPLAIN ANALYZE` runs the statement it explains** — PostgreSQL's
    /// and MySQL 8's, and MariaDB's bare `ANALYZE` — so an every-row `DELETE`
    /// under one is asked about exactly as it is without it. A plain `EXPLAIN`
    /// only plans, so it is not; nor is a scoped statement under `ANALYZE`, nor
    /// `ANALYZE TABLE`, which is maintenance.
    #[test]
    fn an_analyzed_statement_is_judged_as_the_statement_it_runs() {
        for d in EVERY_DIALECT {
            for sql in [
                "EXPLAIN ANALYZE DELETE FROM t",
                "explain analyze verbose update t set a = 1",
                "EXPLAIN (ANALYZE, BUFFERS) DELETE FROM t",
                "EXPLAIN (FORMAT JSON, ANALYZE true) DELETE FROM t",
                "EXPLAIN ANALYZE FORMAT=TREE DELETE FROM t",
                "ANALYZE DELETE FROM t",
                "ANALYZE FORMAT=JSON UPDATE t SET a = 1",
                "/* why */ EXPLAIN ANALYZE TRUNCATE t",
            ] {
                assert!(super::every_row_reason(sql, d).is_some(), "{d:?}: {sql}");
                assert!(super::unsafe_reason(sql, d).is_some(), "{d:?}: {sql}");
            }
            for sql in [
                "EXPLAIN DELETE FROM t",
                "EXPLAIN (FORMAT JSON) DELETE FROM t",
                "EXPLAIN ANALYZE DELETE FROM t WHERE id = 1",
                "ANALYZE TABLE t",
                "ANALYZE t",
                "EXPLAIN ANALYZE SELECT * FROM t",
            ] {
                assert_eq!(super::unsafe_reason(sql, d), None, "{d:?}: {sql}");
            }
        }
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
            // Multi-statement.
            assert!(gate("SELECT 1; DROP TABLE t").is_err(), "{d:?}");
            // A dangerous word inside a *standard* string is inert everywhere.
            assert!(gate("SELECT 'delete from t'").is_ok(), "{d:?}");
        }
        // `INTO OUTFILE`, `SLEEP` and the `GET_LOCK` pair left this sweep when
        // the deny list became per-dialect: they are MySQL syntax, and asserting
        // that PostgreSQL refuses a statement it cannot parse proved nothing
        // about PostgreSQL while hiding that it had no `pg_read_file` entry at
        // all. Each engine's own filesystem / sleep / lock primitives are pinned
        // by `the_gate_refuses_each_engines_own_filesystem_primitive` instead —
        // which is a strictly larger set than this line ever covered.
        assert!(read_only_reason("SELECT * FROM t INTO OUTFILE '/tmp/x'").is_err());
        assert!(read_only_reason("SELECT SLEEP(10)").is_err());
        assert!(read_only_reason("SELECT GET_LOCK('a', 1)").is_err());
        // The backtick is not standard, so this one is deliberately not in the
        // sweep — see `the_gate_reads_this_engines_identifier_quoting`.
        assert!(read_only_reason("SELECT `update` FROM t").is_ok());
    }

    /// **A locking read is refused as a lock, not as a write.** `SELECT … FOR
    /// UPDATE` was refused with "`UPDATE` is not permitted", which reads as a
    /// write having been detected; the refusal is right — it takes row locks —
    /// and the message should say that and name the clause.
    #[test]
    fn a_locking_read_is_refused_by_naming_its_locking_clause() {
        for (d, sql, clause) in [
            (
                SqlDialect::MySql,
                "SELECT * FROM t FOR UPDATE",
                "FOR UPDATE",
            ),
            (
                SqlDialect::Postgres,
                "SELECT * FROM t WHERE id = 1 FOR UPDATE SKIP LOCKED",
                "FOR UPDATE",
            ),
            (
                SqlDialect::Postgres,
                "select * from t for no key update",
                "FOR NO KEY UPDATE",
            ),
            (
                SqlDialect::MySql,
                "SELECT * FROM t LOCK IN SHARE MODE",
                "LOCK IN SHARE MODE",
            ),
            // The shared locks no deny-list word catches: a MySQL 8 read-only
            // session runs these and holds the locks for the statement.
            (SqlDialect::MySql, "SELECT * FROM t FOR SHARE", "FOR SHARE"),
            (
                SqlDialect::MySql,
                "SELECT * FROM t FOR SHARE OF t NOWAIT",
                "FOR SHARE",
            ),
            (
                SqlDialect::Postgres,
                "SELECT * FROM t FOR SHARE",
                "FOR SHARE",
            ),
            (
                SqlDialect::Postgres,
                "select * from t for key share",
                "FOR KEY SHARE",
            ),
        ] {
            let refusal = super::read_only_refusal(sql, d).expect_err(sql);
            let why = &refusal.reason;
            assert!(why.contains(clause), "{sql}: {why}");
            assert!(why.contains("lock"), "{sql}: {why}");
            assert!(!why.contains("not permitted"), "{sql}: {why}");
            assert!(!refusal.writes, "{sql}: a lock is no write");
            assert_eq!(super::read_only_reason(sql, d), Err(refusal.reason));
        }
        // A column that happens to be called `share` is still a read.
        assert!(super::read_only_reason("SELECT share, key FROM t", SqlDialect::MySql).is_ok());
    }

    /// And a real `UPDATE` inside a read is still named as the write it is —
    /// **whichever comes first**. A locking CTE ahead of a deleting one was
    /// told to drop the locking clause, and the retry was refused as `DELETE`.
    #[test]
    fn a_write_hidden_in_a_read_keeps_its_own_refusal() {
        for sql in [
            "WITH d AS (UPDATE t SET a = 1 RETURNING *) SELECT * FROM d",
            "WITH l AS (SELECT * FROM a FOR UPDATE), d AS (UPDATE t SET a = 1 RETURNING *) \
             SELECT * FROM d",
            "WITH l AS (SELECT * FROM a FOR SHARE), d AS (UPDATE t SET a = 1 RETURNING *) \
             SELECT * FROM d",
        ] {
            let refusal = super::read_only_refusal(sql, SqlDialect::Postgres).unwrap_err();
            assert_eq!(
                refusal.reason, "`UPDATE` is not permitted in a read-only query",
                "{sql}"
            );
            assert!(refusal.writes, "{sql}");
        }
    }

    /// **`writes` says whether the fix is a write path**, which is what a
    /// front end with one (`schemaic exec`) needs to know before pointing at
    /// it: a statement that is not a read, or one carrying a write, is; a
    /// sleep, a second `SELECT` or a lock is not.
    #[test]
    fn a_refusal_says_whether_it_was_a_write() {
        let writes = |s: &str| {
            super::read_only_refusal(s, SqlDialect::MySql)
                .unwrap_err()
                .writes
        };
        assert!(writes("DELETE FROM t"));
        assert!(writes("INSERT INTO t VALUES (1)"));
        assert!(writes("SELECT * INTO OUTFILE '/tmp/x' FROM t"));
        assert!(!writes("SELECT SLEEP(5)"));
        assert!(!writes("SELECT 1; SELECT 2"));
        assert!(!writes("SELECT * FROM t FOR UPDATE"));
    }

    /// **The deny list is per-engine for the same reason the heads are, and it
    /// was not.** Every entry was a MySQL spelling, so
    /// `SELECT pg_read_file('/etc/hostname')` passed the gate and shipped a
    /// server-side file to the CLI vendor — measured live against PG 16.15
    /// through the shipped `schemaic --mcp-serve` binary, where the same batch's
    /// `SLEEP(1)` *was* refused, which is what showed the gate was running and
    /// simply did not know these names. `pg_hba.conf`, `~/.ssh/id_rsa` and
    /// Schemaic's own `connections.json` are the same call.
    ///
    /// Asserted through `read_only_reason` rather than against the constant,
    /// because the tokeniser is where the second half of the bug lived: `_` is a
    /// word byte, so `LOAD_FILE` tokenises as one word and the `LOAD` entry —
    /// which covers only `LOAD DATA INFILE` — never matched it. MySQL's own read
    /// primitive was as open as PostgreSQL's.
    #[test]
    fn the_gate_refuses_each_engines_own_filesystem_primitive() {
        use super::read_only_reason as gate;
        let cases: &[(SqlDialect, &str)] = &[
            (SqlDialect::Postgres, "SELECT pg_read_file('/etc/passwd')"),
            (
                SqlDialect::Postgres,
                "SELECT pg_read_binary_file('/etc/passwd')",
            ),
            (SqlDialect::Postgres, "SELECT pg_ls_dir('/')"),
            (SqlDialect::Postgres, "SELECT pg_stat_file('/etc/passwd')"),
            (SqlDialect::Postgres, "SELECT lo_import('/etc/passwd')"),
            (SqlDialect::Postgres, "SELECT pg_sleep(10)"),
            (SqlDialect::Postgres, "SELECT pg_advisory_lock(1)"),
            (SqlDialect::Postgres, "SELECT pg_terminate_backend(1)"),
            (SqlDialect::MySql, "SELECT LOAD_FILE('/etc/passwd')"),
            (SqlDialect::Sqlite, "SELECT readfile('/etc/passwd')"),
            (SqlDialect::Sqlite, "SELECT writefile('/tmp/x', 'y')"),
            (SqlDialect::Sqlite, "SELECT load_extension('/tmp/x.so')"),
        ];
        for (d, sql) in cases {
            assert!(gate(sql, *d).is_err(), "{d:?} passed `{sql}`");
        }
    }

    /// **A read-only transaction does not refuse a non-transactional write.**
    /// Replication slots, statistics resets, WAL switches and backup markers
    /// never call `PreventCommandIfReadOnly`, so the session half of the gate
    /// waves them through — measured live on PG 16.15 under exactly the
    /// `SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY` the headless read
    /// path runs, where `pg_drop_replication_slot` removed a slot a CDC consumer
    /// would have lost its place with. Only the text half can stop them.
    #[test]
    fn the_gate_refuses_postgres_admin_writes_a_read_only_session_allows() {
        use super::read_only_reason as gate;
        for sql in [
            "SELECT pg_drop_replication_slot('debezium')",
            "SELECT pg_create_physical_replication_slot('s')",
            "SELECT pg_create_logical_replication_slot('s', 'pgoutput')",
            "SELECT pg_copy_logical_replication_slot('a', 'b')",
            "SELECT pg_replication_slot_advance('s', '0/0')",
            "SELECT * FROM pg_logical_slot_get_changes('s', NULL, NULL)",
            "SELECT pg_replication_origin_create('o')",
            "SELECT pg_stat_reset()",
            "SELECT pg_stat_reset_shared('bgwriter')",
            "SELECT pg_stat_statements_reset()",
            "SELECT pg_switch_wal()",
            "SELECT pg_create_restore_point('r')",
            "SELECT pg_backup_start('b')",
            "SELECT pg_promote()",
            "SELECT pg_wal_replay_pause()",
            "SELECT pg_logical_emit_message(false, 'p', 'm')",
            "SELECT pg_file_write('f', 'x', false)",
            "SELECT pg_ls_waldir()",
        ] {
            assert!(gate(sql, SqlDialect::Postgres).is_err(), "passed `{sql}`");
        }
        // The peek is a read: it leaves the slot where it was.
        assert!(
            gate(
                "SELECT * FROM pg_logical_slot_peek_changes('s', NULL, NULL)",
                SqlDialect::Postgres
            )
            .is_ok()
        );
    }

    /// **A function that takes SQL text is an eval, and the deny scan cannot see
    /// inside a string.** `query_to_xml('select pg_read_file(…)', …)` returned
    /// a server file's contents live on PG 16.15 while the bare
    /// `pg_read_file` was refused — every name on the list was one string
    /// literal from reachable. `dblink_exec` is worse: dblink's own connection
    /// is not the read-only session, so it writes.
    #[test]
    fn a_function_that_evaluates_sql_text_is_refused() {
        use super::read_only_reason as gate;
        for sql in [
            "SELECT query_to_xml('select pg_read_file(''PG_VERSION'')', true, true, '')",
            "SELECT query_to_xmlschema('select 1', true, true, '')",
            "SELECT query_to_xml_and_xmlschema('select 1', true, true, '')",
            "SELECT * FROM ts_stat('select pg_sleep(30)')",
            "SELECT ts_rewrite('a'::tsquery, 'select 1')",
            "SELECT dblink_exec('dbname=x', 'DELETE FROM t')",
            "SELECT * FROM dblink('dbname=x', 'select 1') AS t(a int)",
            "SELECT dblink_send_query('c', 'select 1')",
            "SELECT * FROM crosstab('select 1, 2, 3') AS t(a int, b int)",
        ] {
            assert!(gate(sql, SqlDialect::Postgres).is_err(), "passed `{sql}`");
        }
    }

    // ── PostgreSQL's function allowlist ──────────────────────────────────────

    /// **A function off the read list is refused, whoever wrote it.** The deny
    /// list this replaced named the writers it knew of, so a function added by
    /// a newer server, an extension or the database's owner passed unless
    /// somebody had thought to list it. The allowlist fails the other way.
    #[test]
    fn a_postgres_function_off_the_read_list_is_refused() {
        use super::read_only_reason as gate;
        for sql in [
            "SELECT my_func(1)",
            "SELECT * FROM t WHERE audit_and_return(id) > 0",
            "SELECT * FROM some_srf(1) AS s(a int)",
            "SELECT \"MyFunc\"(1)",
            // The case the enumeration could not see coming: a name it never had.
            "SELECT pg_brand_new_admin_function()",
            // Quoted exactly: `"LOWER"` is a different function from `lower`.
            "SELECT \"LOWER\"('a')",
            // `$` continues a name, so this is `lower$`, not `lower` and a `$`.
            "SELECT lower$(1)",
            "SELECT my$func(1)",
        ] {
            assert!(gate(sql, SqlDialect::Postgres).is_err(), "passed `{sql}`");
        }
        // Not a write: the fix is not `schemaic exec`.
        let refusal = super::read_only_refusal("SELECT my_func(1)", SqlDialect::Postgres)
            .expect_err("unlisted");
        assert!(!refusal.writes, "{refusal:?}");
        assert!(refusal.reason.contains("my_func"), "{}", refusal.reason);
        // The other two engines keep their deny lists; this is PostgreSQL's alone.
        assert!(gate("SELECT my_func(1)", SqlDialect::MySql).is_ok());
        assert!(gate("SELECT my_func(1)", SqlDialect::Sqlite).is_ok());
    }

    /// **A read the model writes every day must still pass.** The allowlist is
    /// only as usable as the grammar around it is understood: every keyword
    /// PostgreSQL puts before a `(` — `IN`, `OVER`, `FILTER`, a type's
    /// modifier, a CTE's column list — is a `name(` that is not a call.
    #[test]
    fn ordinary_postgres_reads_pass_the_function_allowlist() {
        use super::read_only_reason as gate;
        for sql in [
            "SELECT count(*), sum(x), avg(x), min(x), max(x) FROM t",
            "SELECT count(*) FILTER (WHERE x > 0) OVER (PARTITION BY (y) ORDER BY z) FROM t",
            "SELECT row_number() OVER (ORDER BY (a)), lag(a, 1) OVER w FROM t WINDOW w AS (ORDER BY a)",
            "SELECT mode() WITHIN GROUP (ORDER BY x), percentile_cont(0.5) WITHIN GROUP (ORDER BY x) FROM t",
            "SELECT string_agg(x, ',' ORDER BY x), array_agg(DISTINCT x) FROM t",
            "SELECT CAST(x AS numeric(10,2)), x::varchar(20), y::character varying(5) FROM t",
            "SELECT coalesce(a, b), nullif(a, 0), greatest(a, b), least(a, b) FROM t",
            "SELECT EXTRACT(YEAR FROM d), date_trunc('month', d), to_char(now(), 'YYYY') FROM t",
            "SELECT SUBSTRING(x FROM 1 FOR 2), TRIM(BOTH ' ' FROM x), POSITION('a' IN x) FROM t",
            "SELECT lower(x), upper(x), length(x), translate(x, 'a', 'b'), split_part(x, ',', 1) FROM t",
            "SELECT jsonb_build_object('a', 1), doc->>'k', jsonb_array_length(doc) FROM t",
            "SELECT * FROM jsonb_each('{}'::jsonb)",
            "SELECT (jsonb_each(doc)).* FROM t",
            "SELECT * FROM t WHERE id IN (SELECT id FROM u) AND EXISTS (SELECT 1)",
            "SELECT * FROM t WHERE x = ANY(ARRAY[1, 2]) AND NOT (a AND (b OR c))",
            "SELECT CASE WHEN (x) THEN (y) ELSE (z) END FROM t",
            "SELECT DISTINCT ON (a) a, b FROM t ORDER BY a",
            "SELECT * FROM ONLY (t) LIMIT (5) OFFSET (1)",
            "SELECT * FROM t JOIN (SELECT 1 AS id) s USING (id)",
            "SELECT * FROM t, LATERAL (SELECT 1) l",
            "SELECT * FROM (VALUES (1), (2)) v(n)",
            "SELECT ARRAY(SELECT 1), ROW(1, 2)",
            "SELECT * FROM generate_series(1, 3) AS g(n)",
            "SELECT * FROM unnest(ARRAY[1]) WITH ORDINALITY AS u(v, i)",
            "SELECT * FROM t TABLESAMPLE BERNOULLI (10) REPEATABLE (1)",
            "SELECT a, b, count(*) FROM t GROUP BY GROUPING SETS ((a), (b))",
            "SELECT a, count(*) FROM t GROUP BY ROLLUP (a)",
            "SELECT * FROM t ORDER BY a FETCH FIRST (5) ROWS ONLY",
            "SELECT * FROM t WHERE x BETWEEN (1) AND (2) OR y LIKE (z)",
            "SELECT 1 UNION (SELECT 2)",
            "WITH t(a, b) AS (SELECT 1, 2) SELECT * FROM t",
            "WITH RECURSIVE r(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM r WHERE n < 3) SELECT * FROM r",
            "WITH a AS (SELECT 1), b(x) AS NOT MATERIALIZED (SELECT 2), c AS MATERIALIZED (SELECT 3) SELECT 1",
            "EXPLAIN (FORMAT JSON) SELECT 1",
            "SELECT pg_size_pretty(pg_total_relation_size('t')), current_setting('search_path')",
            "SELECT has_table_privilege('t', 'SELECT'), pg_get_viewdef('v'::regclass), version()",
            "SELECT pg_catalog.lower('A'), \"pg_catalog\".\"lower\"('A')",
            "SELECT ('abc'::text).upper",
            "SELECT xmlelement(name a, xmlattributes(id AS x)) FROM t",
            "SELECT * FROM XMLTABLE(XMLNAMESPACES('u' AS n), '/r' PASSING d COLUMNS a numeric(10,2) PATH 'a')",
        ] {
            assert!(
                gate(sql, SqlDialect::Postgres).is_ok(),
                "refused `{sql}`: {:?}",
                gate(sql, SqlDialect::Postgres)
            );
        }
    }

    /// **PostgreSQL calls a function with no parentheses at all.** `(x).f` is
    /// field selection, and on a value that is not a row it is `f(x)` —
    /// measured on PG 16.15, where `SELECT ('1'::float8).pg_sleep` slept. A scan
    /// that only asks about `name(` never sees it, so a word after a closing
    /// `)` or `]` and a `.` is asked about as a call.
    #[test]
    fn a_field_selection_on_a_parenthesised_value_is_a_call() {
        use super::read_only_reason as gate;
        for sql in [
            "SELECT ('1'::float8).pg_sleep",
            "SELECT (ARRAY['1'::float8])[1].pg_sleep",
            "SELECT ('1'::float8) . \"pg_sleep\"",
            "SELECT ('1'::float8)./*c*/pg_sleep",
            "SELECT ('/etc/passwd'::text).pg_read_file",
            "SELECT (123).pg_terminate_backend",
        ] {
            assert!(gate(sql, SqlDialect::Postgres).is_err(), "passed `{sql}`");
        }
        // A column reference through an alias is not one: `t.pg_sleep` cannot
        // reach `pg_sleep(t)`, which takes no row (measured: "column does not
        // exist").
        assert!(gate("SELECT t.pg_sleep FROM t", SqlDialect::Postgres).is_ok());
    }

    /// **A name followed by `(` after `WITH` or `AS` is being declared, not
    /// called** — a CTE's column list, an alias's — and a CTE that borrows an
    /// unlisted function's name does not make the function callable.
    #[test]
    fn a_declared_name_is_not_a_call_but_does_not_open_the_function() {
        use super::read_only_reason as gate;
        assert!(
            gate(
                "WITH pg_sleep(x) AS (SELECT 1) SELECT pg_sleep(5)",
                SqlDialect::Postgres
            )
            .is_err()
        );
        assert!(
            gate(
                "WITH a AS (SELECT 1), my_func(x) AS (SELECT 2) SELECT my_func(1)",
                SqlDialect::Postgres
            )
            .is_err()
        );
        assert!(
            gate(
                "WITH a AS (SELECT 1), my_func(x) AS (SELECT 2) SELECT * FROM my_func",
                SqlDialect::Postgres
            )
            .is_ok()
        );
    }

    /// **A keyword PostgreSQL also accepts as a function's name is not skipped
    /// as a keyword.** `EXPLAIN` is unreserved and `JOIN`, `ILIKE` and `SIMILAR`
    /// may name a function, so a word list that skipped them before any `(`
    /// passed an owner's `join(1)` untouched. Each is grammar only where the
    /// grammar puts it: `EXPLAIN` at the head, `JOIN (` before a subquery, a
    /// parenthesised join, or after a join's own keywords.
    #[test]
    fn a_keyword_that_can_name_a_function_is_asked_about_as_a_call() {
        use super::read_only_reason as gate;
        for sql in [
            "SELECT explain(1)",
            "SELECT join(1)",
            "SELECT * FROM t WHERE join(t.id) > 0",
            "SELECT ilike('a', 'b')",
            "SELECT similar(1)",
            "SELECT * FROM t WHERE x = 1 AND join((SELECT 1)) > 0",
        ] {
            assert!(gate(sql, SqlDialect::Postgres).is_err(), "passed `{sql}`");
        }
        for sql in [
            "EXPLAIN (FORMAT JSON) SELECT 1",
            "SELECT * FROM t JOIN (SELECT 1 AS id) s USING (id)",
            "SELECT * FROM t a JOIN (VALUES (1)) v(id) ON true",
            "SELECT * FROM t LEFT JOIN (SELECT 1) s ON true",
            "SELECT * FROM t JOIN (u JOIN w ON true) ON true",
            "SELECT * FROM \"t\" JOIN (SELECT 1) s ON true",
            "SELECT * FROM \"t\" x(a, b)",
        ] {
            assert!(
                gate(sql, SqlDialect::Postgres).is_ok(),
                "refused `{sql}`: {:?}",
                gate(sql, SqlDialect::Postgres)
            );
        }
    }

    /// **`OPERATOR(schema.op)` reaches an operator in any schema**, and the
    /// rule that a name straight after a `)` is an alias waved it through
    /// there while refusing it after a literal.
    #[test]
    fn a_qualified_operator_is_refused_wherever_it_stands() {
        use super::read_only_reason as gate;
        for sql in [
            "SELECT (1) OPERATOR(app.===) 2",
            "SELECT 1 OPERATOR(app.===) 2",
            "SELECT 'a' OPERATOR(app.===) 'b'",
        ] {
            assert!(gate(sql, SqlDialect::Postgres).is_err(), "passed `{sql}`");
        }
    }

    /// **A qualified name is listed only in `pg_catalog`.** `public.lower` is
    /// whatever the database's owner defined under that name.
    #[test]
    fn a_schema_qualified_call_is_listed_only_in_pg_catalog() {
        use super::read_only_reason as gate;
        assert!(gate("SELECT pg_catalog.lower('A')", SqlDialect::Postgres).is_ok());
        for sql in [
            "SELECT public.lower('A')",
            "SELECT \"public\".lower('A')",
            "SELECT pg_catalog.pg_sleep(1)",
            "SELECT \"pg_catalog\".\"pg_sleep\"(1)",
            "SELECT app.pg_catalog.pg_sleep(1)",
        ] {
            assert!(gate(sql, SqlDialect::Postgres).is_err(), "passed `{sql}`");
        }
    }

    /// **`UESCAPE` puts a string between a quoted name and its `(`.**
    /// `U&"…" UESCAPE '!'` is one identifier to the server, so the call's `(`
    /// follows a literal the scan cannot see through — and the escape character
    /// rewrites the name the allowlist would have compared.
    #[test]
    fn a_unicode_escaped_name_is_refused_on_postgres() {
        use super::read_only_reason as gate;
        for sql in [
            "SELECT U&\"pg_sleep\" UESCAPE '!' (1)",
            "SELECT U&\"\\0070g_sleep\"(1)",
        ] {
            assert!(gate(sql, SqlDialect::Postgres).is_err(), "passed `{sql}`");
        }
    }

    /// **A misspelt entry is a function nobody can call**, so every name on the
    /// list must be one PostgreSQL 16.15 reports — or one of the grammar forms
    /// the list documents as sitting before a `(` without a `pg_proc` row.
    #[test]
    fn every_listed_function_is_a_postgres_builtin() {
        const GRAMMAR_ONLY: &[&str] = &[
            "rollup",
            "cube",
            "decimal",
            "dec",
            "float",
            "character",
            "bernoulli",
            "system",
            "xmltable",
            "xmlattributes",
            "xmlnamespaces",
        ];
        let unknown: Vec<_> = super::PG_READ_FUNCTIONS
            .iter()
            .filter(|name| {
                !crate::pg_builtins::PG_FUNCTIONS
                    .iter()
                    .any(|f| f.name == **name)
                    && !GRAMMAR_ONLY.contains(name)
            })
            .collect();
        assert!(unknown.is_empty(), "not PostgreSQL builtins: {unknown:?}");
        for name in super::PG_READ_FUNCTIONS {
            assert_eq!(*name, name.to_ascii_lowercase(), "stored lower-case");
        }
        let mut sorted = super::PG_READ_FUNCTIONS.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(before, sorted.len(), "a name is listed twice");
    }

    /// **A skipped word is one the allowlist never sees**, so none may be the
    /// name of a builtin the list does not hold — `left(x, 1)` is a call, and a
    /// `LEFT` in the keyword list would have waved every function of that name
    /// through.
    #[test]
    fn no_word_skipped_before_a_paren_names_an_unlisted_builtin() {
        let contextual = ["BY", "SETS", "VARYING", "FIRST", "NEXT", "MATERIALIZED"];
        let builtins: Vec<_> = super::PG_PAREN_KEYWORDS
            .iter()
            .chain(contextual.iter())
            .filter(|w| {
                let lower = w.to_ascii_lowercase();
                crate::pg_builtins::PG_FUNCTIONS
                    .iter()
                    .any(|f| f.name == lower)
                    && !super::PG_READ_FUNCTIONS.contains(&lower.as_str())
            })
            .collect();
        assert!(
            builtins.is_empty(),
            "skipped before `(` but builtins: {builtins:?}"
        );
    }

    /// **Every name the retired deny list held is still refused** — by the
    /// allowlist now, and quoted as well as bare. The list was the record of
    /// what had been found to act, each entry measured or read off the source;
    /// replacing it must not quietly re-admit one.
    #[test]
    fn every_function_the_deny_list_named_is_still_refused() {
        for name in [
            "pg_read_file",
            "pg_read_binary_file",
            "pg_ls_dir",
            "pg_stat_file",
            "lo_import",
            "lo_export",
            "pg_sleep",
            "pg_sleep_for",
            "pg_sleep_until",
            "pg_advisory_lock",
            "pg_advisory_lock_shared",
            "pg_advisory_xact_lock",
            "pg_advisory_xact_lock_shared",
            "pg_terminate_backend",
            "pg_cancel_backend",
            "pg_reload_conf",
            "pg_rotate_logfile",
            "pg_ls_logdir",
            "pg_ls_waldir",
            "pg_ls_tmpdir",
            "pg_ls_archive_statusdir",
            "pg_file_write",
            "pg_file_rename",
            "pg_file_unlink",
            "pg_file_sync",
            "pg_create_physical_replication_slot",
            "pg_create_logical_replication_slot",
            "pg_drop_replication_slot",
            "pg_copy_physical_replication_slot",
            "pg_copy_logical_replication_slot",
            "pg_replication_slot_advance",
            "pg_logical_slot_get_changes",
            "pg_logical_slot_get_binary_changes",
            "pg_replication_origin_create",
            "pg_replication_origin_drop",
            "pg_replication_origin_advance",
            "pg_replication_origin_session_setup",
            "pg_replication_origin_session_reset",
            "pg_replication_origin_xact_setup",
            "pg_replication_origin_xact_reset",
            "pg_logical_emit_message",
            "pg_stat_reset",
            "pg_stat_reset_shared",
            "pg_stat_reset_single_table_counters",
            "pg_stat_reset_single_function_counters",
            "pg_stat_reset_slru",
            "pg_stat_reset_replication_slot",
            "pg_stat_reset_subscription_stats",
            "pg_stat_statements_reset",
            "pg_switch_wal",
            "pg_switch_xlog",
            "pg_create_restore_point",
            "pg_backup_start",
            "pg_backup_stop",
            "pg_start_backup",
            "pg_stop_backup",
            "pg_promote",
            "pg_wal_replay_pause",
            "pg_wal_replay_resume",
            "pg_log_backend_memory_contexts",
            "query_to_xml",
            "query_to_xmlschema",
            "query_to_xml_and_xmlschema",
            "ts_stat",
            "ts_rewrite",
            "dblink",
            "dblink_exec",
            "dblink_open",
            "dblink_send_query",
            "dblink_connect",
            "dblink_connect_u",
            "crosstab",
            "crosstab2",
            "crosstab3",
            "crosstab4",
            "connectby",
            // Never on it, and as much a write: the session state and sequences.
            "set_config",
            "setseed",
            "nextval",
            "setval",
            "pg_notify",
            "txid_current",
        ] {
            assert!(
                !super::PG_READ_FUNCTIONS.contains(&name),
                "`{name}` is listed"
            );
            for sql in [format!("SELECT {name}(1)"), format!("SELECT \"{name}\"(1)")] {
                assert!(
                    super::read_only_reason(&sql, SqlDialect::Postgres).is_err(),
                    "passed `{sql}`"
                );
            }
        }
    }

    /// **The smuggle the mode pin exists for.** Under the gate's lexer this is
    /// one `SELECT` and one string; under `NO_BACKSLASH_ESCAPES` it is three
    /// statements. The gate cannot know the server's mode, which is why the
    /// session has to be put in the one the gate assumed.
    #[test]
    fn a_backslash_quote_hides_a_second_statement_from_the_gate() {
        use super::read_only_reason as gate;
        assert!(gate("SELECT 'a\\'; DELETE FROM t; -- '", SqlDialect::MySql).is_ok());
        assert!(gate("SELECT \"a\\\"; DELETE FROM t; -- \"", SqlDialect::MySql).is_ok());
    }

    #[test]
    fn the_pinned_mode_drops_the_flags_that_move_a_quote() {
        use super::mysql_mode_lexed_like_the_gate as pin;
        assert_eq!(
            pin("STRICT_TRANS_TABLES,NO_BACKSLASH_ESCAPES,NO_ENGINE_SUBSTITUTION"),
            "STRICT_TRANS_TABLES,NO_ENGINE_SUBSTITUTION"
        );
        assert_eq!(pin("ANSI_QUOTES"), "");
        assert_eq!(pin(""), "");
    }

    /// A combination mode implies `ANSI_QUOTES`; keeping it would have the
    /// server put the flag straight back. What it implied *apart* from the quote
    /// is listed beside it and survives.
    #[test]
    fn a_combination_mode_that_implies_ansi_quotes_goes_too() {
        use super::mysql_mode_lexed_like_the_gate as pin;
        assert_eq!(
            pin("REAL_AS_FLOAT,PIPES_AS_CONCAT,ANSI_QUOTES,IGNORE_SPACE,ONLY_FULL_GROUP_BY,ANSI"),
            "REAL_AS_FLOAT,PIPES_AS_CONCAT,IGNORE_SPACE,ONLY_FULL_GROUP_BY"
        );
        assert_eq!(
            pin("PIPES_AS_CONCAT,ANSI_QUOTES,IGNORE_SPACE,ORACLE,NO_KEY_OPTIONS"),
            "PIPES_AS_CONCAT,IGNORE_SPACE,NO_KEY_OPTIONS"
        );
    }

    /// A name that merely *contains* a hazard is not one.
    #[test]
    fn a_mode_name_is_matched_whole_not_by_substring() {
        use super::{
            mysql_mode_is_lexed_like_the_gate as ok, mysql_mode_lexed_like_the_gate as pin,
        };
        assert_eq!(pin("NO_BACKSLASH_ESCAPES_X"), "NO_BACKSLASH_ESCAPES_X");
        assert!(ok("NO_BACKSLASH_ESCAPES_X,STRICT_ALL_TABLES"));
    }

    /// The read-back check: a mode still carrying a hazard fails the session.
    #[test]
    fn a_mode_is_lexed_like_the_gate_only_without_every_hazard() {
        use super::mysql_mode_is_lexed_like_the_gate as ok;
        assert!(ok(""));
        assert!(ok("STRICT_TRANS_TABLES,NO_ENGINE_SUBSTITUTION"));
        assert!(!ok("STRICT_TRANS_TABLES,NO_BACKSLASH_ESCAPES"));
        assert!(!ok("ansi_quotes"));
        assert!(!ok("MSSQL"));
    }

    /// The other engine's spelling is *not* refused — over-blocking is the safe
    /// direction on a refusal gate, but it is still a wrong answer, and a shared
    /// list is how the gate came to give one in the first place.
    #[test]
    fn the_deny_list_names_only_this_engines_spellings() {
        use super::read_only_reason as gate;
        // `pg_read_file` is not a MySQL function; a column called that is a read.
        assert!(gate("SELECT pg_read_file FROM t", SqlDialect::MySql).is_ok());
        // `LOAD_FILE` likewise means nothing on PostgreSQL.
        assert!(gate("SELECT load_file FROM t", SqlDialect::Postgres).is_ok());
        // But the shared half holds everywhere.
        for d in EVERY_DIALECT {
            assert!(
                gate("SELECT 1 FROM t WHERE x = (DELETE)", d).is_err(),
                "{d:?}"
            );
        }
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
        // T-SQL has no `EXPLAIN`, `SHOW` or `DESCRIBE`: a plan is asked for
        // with `SET SHOWPLAN_XML ON`, which is session state.
        let ms = SqlDialect::MsSql;
        assert!(gate("SELECT 1", ms).is_ok());
        assert!(gate("WITH c AS (SELECT 1 AS a) SELECT * FROM c", ms).is_ok());
        for other in ["EXPLAIN SELECT 1", "SHOW TABLES", "DESCRIBE t"] {
            assert!(gate(other, ms).is_err(), "{other}");
        }
    }

    /// **T-SQL needs no `;` between statements**, so the text after a read
    /// can be a second statement that writes. The deny list reads every word,
    /// not just the head, and that is what refuses it.
    #[test]
    fn a_sql_server_write_without_a_semicolon_is_still_refused() {
        use super::read_only_reason as gate;
        let ms = SqlDialect::MsSql;
        for sql in [
            "SELECT 1 DELETE FROM t",
            "SELECT 1\nUPDATE t SET a = 1",
            "SELECT 1 INSERT t VALUES (1)",
            "SELECT 1 DROP TABLE t",
            "SELECT 1 TRUNCATE TABLE t",
            "SELECT 1 MERGE t USING s ON 1 = 1 WHEN MATCHED THEN DELETE;",
        ] {
            assert!(gate(sql, ms).is_err(), "{sql}");
        }
    }

    /// The T-SQL statements and clauses that act with no `INSERT`/`UPDATE`/
    /// `DELETE` in sight: a procedure call (which can do anything, including
    /// through `xp_cmdshell`), a sleep, a backup, a server-side file read,
    /// a table created by `SELECT … INTO`, a sequence advanced, a permission
    /// denied, a Service Broker message consumed, a transaction opened.
    #[test]
    fn the_gate_refuses_sql_servers_own_side_effects() {
        use super::read_only_reason as gate;
        let ms = SqlDialect::MsSql;
        for sql in [
            "EXEC sp_who",
            "SELECT 1 EXEC xp_cmdshell 'dir'",
            "SELECT 1 EXECUTE ('DELETE FROM t')",
            "SELECT 1 WAITFOR DELAY '00:00:10'",
            "SELECT 1 BACKUP DATABASE d TO DISK = 'c:\\x.bak'",
            "SELECT * FROM OPENROWSET(BULK 'C:\\secret.txt', SINGLE_CLOB) AS x",
            "SELECT * FROM OPENQUERY(linked, 'DELETE FROM t')",
            "SELECT * FROM OPENDATASOURCE('SQLNCLI', 'x').db.dbo.t",
            "SELECT * INTO copy_of_t FROM t",
            "SELECT * INTO #t FROM t",
            "SELECT NEXT VALUE FOR dbo.seq",
            "SELECT 1 DENY SELECT ON t TO u",
            "SELECT 1 DBCC FREEPROCCACHE",
            "SELECT 1 DECLARE @x INT",
            "SELECT 1 BEGIN TRAN",
            "SELECT 1 RECEIVE TOP (1) * FROM q",
            "SELECT 1 RECONFIGURE",
            "SELECT 1 CHECKPOINT",
        ] {
            assert!(gate(sql, ms).is_err(), "{sql}");
        }
    }

    /// **`NEXT VALUE FOR` advances a sequence on MariaDB too**, which the gate
    /// passed there as a read until SQL Server's arm asked the question.
    #[test]
    fn advancing_a_sequence_is_refused_on_every_engine() {
        for d in EVERY_DIALECT {
            assert!(
                super::read_only_reason("SELECT NEXT VALUE FOR s", d).is_err(),
                "{d:?}"
            );
            // A column called `next` or `value` is still a read.
            assert!(
                super::read_only_reason("SELECT next, value FROM t", d).is_ok(),
                "{d:?}"
            );
        }
    }

    /// A table hint that takes an update or exclusive lock, or holds a shared
    /// one to the end of the transaction, is refused and named as the lock.
    #[test]
    fn a_sql_server_locking_hint_is_named_as_a_lock() {
        use super::read_only_reason as gate;
        let ms = SqlDialect::MsSql;
        for hint in ["UPDLOCK", "XLOCK", "TABLOCKX", "HOLDLOCK", "TABLOCK"] {
            let sql = format!("SELECT * FROM t WITH ({hint})");
            let err = gate(&sql, ms).expect_err(&sql);
            assert!(err.contains("lock"), "{sql}: {err}");
        }
        // `NOLOCK` takes no lock at all, and `READPAST` skips locked rows.
        assert!(gate("SELECT * FROM t WITH (NOLOCK)", ms).is_ok());
        assert!(gate("SELECT * FROM t WITH (READPAST)", ms).is_ok());
    }

    /// **A function call is answered by an allowlist, as on PostgreSQL.** A
    /// user-defined function can call an extended stored procedure, and a CLR
    /// function can do anything its assembly can, so a name that is not a
    /// known builtin is refused — qualified or not, except in `sys`.
    #[test]
    fn a_sql_server_call_must_be_a_known_read_only_builtin() {
        use super::read_only_reason as gate;
        let ms = SqlDialect::MsSql;
        for ok in [
            "SELECT COUNT(*), SUM(a), MAX(b) FROM t",
            "SELECT LEN(name), UPPER(name), SUBSTRING(name, 1, 2) FROM t",
            "SELECT CAST(a AS varchar(10)), CONVERT(nvarchar(20), b, 120) FROM t",
            "SELECT TRY_CAST(a AS decimal(10, 2)) FROM t",
            "SELECT GETDATE(), SYSDATETIME(), DATEADD(day, 1, d), DATEDIFF(day, a, b) FROM t",
            "SELECT ISNULL(a, 0), COALESCE(a, b), IIF(a > 1, 'x', 'y'), NULLIF(a, 0) FROM t",
            "SELECT ROW_NUMBER() OVER (PARTITION BY a ORDER BY b) FROM t",
            "SELECT TOP (10) * FROM t WHERE a IN (1, 2) AND EXISTS (SELECT 1)",
            "SELECT * FROM t ORDER BY a OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY",
            "SELECT JSON_VALUE(doc, '$.a'), OBJECT_ID('dbo.t'), DB_NAME() FROM t",
            "SELECT STRING_AGG(name, ',') WITHIN GROUP (ORDER BY name) FROM t",
            "SELECT * FROM sys.dm_exec_sessions",
            "SELECT * FROM sys.fn_helpcollations()",
            "SELECT * FROM t FOR XML PATH('row'), ROOT('rows')",
            "SELECT * FROM OPENJSON(@j) WITH (a int '$.a')",
        ] {
            assert!(gate(ok, ms).is_ok(), "{ok}: {:?}", gate(ok, ms));
        }
        for refused in [
            "SELECT dbo.my_function(1)",
            "SELECT * FROM dbo.tvf(1)",
            "SELECT * FROM tvf(1)",
            "SELECT [dbo].[f](1)",
            "SELECT otherdb.dbo.f(1)",
            "SELECT * FROM sys.fn_get_audit_file('c:\\x', DEFAULT, DEFAULT)",
            "SELECT * FROM sys.fn_xe_file_target_read_file('c:\\x', NULL, NULL, NULL)",
            "SELECT geography::Point(1, 2, 4326)",
        ] {
            assert!(gate(refused, ms).is_err(), "{refused}");
        }
    }

    /// **A reserved word before a `(` is grammar, not a call** — T-SQL cannot
    /// name a function `INDEX` or `TABLESAMPLE` — so a table hint and a sample
    /// clause are reads like any other, rather than calls to unlisted
    /// functions.
    #[test]
    fn a_sql_server_hint_or_sample_clause_is_not_a_call() {
        use super::read_only_reason as gate;
        let ms = SqlDialect::MsSql;
        for ok in [
            "SELECT * FROM t WITH (INDEX(ix_a))",
            "SELECT * FROM t WITH (NOLOCK, INDEX(0))",
            "SELECT * FROM t TABLESAMPLE (10 PERCENT)",
        ] {
            assert!(gate(ok, ms).is_ok(), "{ok}: {:?}", gate(ok, ms));
        }
    }

    /// **A loop is T-SQL's `BENCHMARK`**: it writes nothing and runs until
    /// someone stops it, which on the unattended paths is nobody.
    #[test]
    fn a_sql_server_loop_is_not_a_read() {
        for sql in [
            "SELECT 1 WHILE 1 = 1 PRINT 'x'",
            "SELECT 1 again: PRINT 1 GOTO again",
        ] {
            assert!(
                super::read_only_reason(sql, SqlDialect::MsSql).is_err(),
                "{sql}"
            );
        }
    }

    /// **`#`, `@` and `$` continue a T-SQL name**, so `dbo.h#()` calls `h#`
    /// and `dbo.f@GETDATE()` calls `f@GETDATE` — neither of them the
    /// allowlisted name the scan used to stop at. Both are functions a server
    /// creates (measured on 2022), and a call to one is what the allowlist is
    /// there to refuse.
    #[test]
    fn a_t_sql_name_holding_hash_at_or_dollar_is_one_call() {
        use super::read_only_reason as gate;
        let ms = SqlDialect::MsSql;
        for refused in [
            "SELECT dbo.h#()",
            "SELECT dbo.purge@now()",
            "SELECT dbo.f@GETDATE()",
            "SELECT dbo.g#GETDATE()",
            "SELECT dbo.m$GETDATE()",
            "SELECT h#(1)",
        ] {
            assert!(gate(refused, ms).is_err(), "{refused}");
        }
        // A variable and a temp table still read: `@` and `#` *begin* those
        // names, and neither is followed by a call.
        for ok in [
            "SELECT @@ROWCOUNT, GETDATE()",
            "SELECT a FROM #t WHERE a = @x",
            "SELECT $1.50 + 1",
        ] {
            assert!(gate(ok, ms).is_ok(), "{ok}: {:?}", gate(ok, ms));
        }
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

    /// **A MySQL/MariaDB executable comment is code, and the one lexer read it
    /// as a comment.**
    ///
    /// `/*! … */` and MariaDB's `/*M! … */` are expanded by the server whenever
    /// its version is at least the number in the marker — `/*M!010000` fires on
    /// every MariaDB this app supports, and a bare `/*!` fires always. This
    /// repository already states that as live-measured fact in
    /// `intel::is_single_expression`'s doc, and `propose.rs` carries fixtures
    /// for it; the class was closed at the proposal surface and the two
    /// **security** gates built on the same lexer were never swept with it.
    ///
    /// So `SELECT /*!LOAD_FILE('/etc/passwd')*/` tokenised as `["SELECT"]`: the
    /// head is on the read allowlist, no deny keyword is ever a token, the AI
    /// gate returns `Ok`, and the server ships the file back as a cell. And
    /// `SELECT * FROM t /*!INTO OUTFILE '/tmp/x'*/` is a plain read to
    /// `contains_write`, so on a connection the user marked **read-only** it is
    /// `Allow` — no refusal and no confirmation — while the server writes the
    /// table to a file on the DB host.
    ///
    /// The lexer now stops at the marker rather than at `*/`, so the body is
    /// lexed as code. That over-blocks when the server's version is below the
    /// marker's, which is the right direction for both consumers.
    #[test]
    fn an_executable_comment_is_lexed_as_the_code_the_server_runs() {
        for sql in [
            "SELECT /*!LOAD_FILE('/etc/passwd')*/",
            "SELECT /*!50000 LOAD_FILE('/etc/passwd')*/",
            "SELECT /*M!100000 SLEEP(60)*/",
            "SELECT /*M!GET_LOCK('x',600)*/",
        ] {
            assert!(
                super::read_only_reason(sql, SqlDialect::MySql).is_err(),
                "{sql}"
            );
        }
        // The write guard, asserted through the composition it protects — the
        // invariant is about `run_verdict`, not about the predicate.
        let outfile = "SELECT * FROM orders /*!INTO OUTFILE '/tmp/orders.csv'*/";
        assert!(
            super::contains_write(outfile, SqlDialect::MySql),
            "{outfile}"
        );
        assert!(matches!(
            run_verdict(
                &[outfile.to_string()],
                GuardPolicy {
                    read_only: true,
                    confirm_writes: false,
                    dialect: SqlDialect::MySql,
                    no_database: false,
                },
            ),
            RunVerdict::Block(_)
        ));

        // An ordinary comment is still inert, on every engine…
        assert!(super::read_only_reason("SELECT /* delete from t */ 1", SqlDialect::MySql).is_ok());
        assert!(!super::contains_write(
            "SELECT 1 /* INTO OUTFILE 'x' */",
            SqlDialect::MySql
        ));
        // …and neither marker is special where the server does not expand it.
        for d in [SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert!(
                super::read_only_reason("SELECT /*!LOAD_FILE('/etc/passwd')*/ 1", d).is_ok(),
                "{d:?}"
            );
        }
    }

    /// **A quoted function name takes the token out of the deny scan's reach.**
    ///
    /// PostgreSQL resolves a double-quoted identifier against the stored
    /// lower-case `pg_proc.proname`, so `SELECT "pg_read_file"('/etc/passwd')`
    /// is the same call as the unquoted one — but the lexer skipped the quoted
    /// run whole, `PG_READ_FILE` never became a token, and `is_denied` was never
    /// asked. Every entry in the PostgreSQL list is reachable that way,
    /// including the two that act: `lo_export` writes a server-side file and
    /// `pg_terminate_backend` kills other sessions.
    ///
    /// The rule has to separate a quoted **call** from a quoted **column**, or
    /// it breaks the case the gate was deliberately tuned for — which is why the
    /// token is emitted only when the next non-space byte is `(`.
    #[test]
    fn a_quoted_function_name_is_still_the_function() {
        for sql in [
            "SELECT \"pg_read_file\"('/etc/passwd')",
            "SELECT \"pg_read_file\" ('/etc/passwd')",
            "SELECT \"lo_export\"(1,'/tmp/x')",
            "SELECT \"pg_terminate_backend\"(123)",
        ] {
            assert!(
                super::read_only_reason(sql, SqlDialect::Postgres).is_err(),
                "{sql}"
            );
        }
        // And a quoted *column* of the same spelling stays a read — the case
        // `the_gate_reads_this_engines_identifier_quoting` exists for.
        assert!(super::read_only_reason("SELECT \"delete\" FROM t", SqlDialect::Postgres).is_ok());
        assert!(super::read_only_reason("SELECT \"update\" FROM t", SqlDialect::Sqlite).is_ok());
        assert!(super::read_only_reason("SELECT `update` FROM t", SqlDialect::MySql).is_ok());
        // The same door on the write guard, which is a narrower list — it names
        // the tokens that change data, so a quoted `DELETE(` is a write and a
        // quoted `delete` column is still a read.
        assert!(super::contains_write(
            "SELECT \"delete\"(1)",
            SqlDialect::Postgres
        ));
        assert!(!super::contains_write(
            "SELECT \"delete\" FROM t",
            SqlDialect::Postgres
        ));
    }

    /// **A comment is a separator too, and the quoted-call rule only knew about
    /// whitespace.**
    ///
    /// `0557365` tested the first *non-whitespace* byte after the closing quote
    /// for `(`. A comment is not whitespace, so the first non-whitespace byte is
    /// `/` or `#`, the token was never pushed, and
    /// `SELECT "pg_read_file"/*x*/('/etc/passwd')` went back through the gate
    /// that `a_quoted_function_name_is_still_the_function` closes — reaching the
    /// MCP server's `run_query`, which is gated on this predicate alone.
    /// Every engine's parser skips a comment between a function name and its
    /// argument list exactly as it skips a space.
    #[test]
    fn a_comment_between_a_quoted_name_and_its_paren_is_a_separator() {
        // Every comment spelling each dialect has, in the separator slot.
        for sep in [
            "/*x*/",
            "/**/",
            "/*!*/",
            "-- c
",
            "/* multi
line */",
        ] {
            let sql = format!("SELECT \"pg_read_file\"{sep}('/etc/passwd')");
            assert!(
                super::read_only_reason(&sql, SqlDialect::Postgres).is_err(),
                "{sql:?}"
            );
            let sql = format!("SELECT \"lo_export\"{sep}(1,'/tmp/x')");
            assert!(
                super::read_only_reason(&sql, SqlDialect::Postgres).is_err(),
                "{sql:?}"
            );
            // The write guard has the identical door.
            let sql = format!("SELECT \"delete\"{sep}(1)");
            assert!(super::contains_write(&sql, SqlDialect::Postgres), "{sql:?}");
        }
        // MySQL's `#` comment, on the engine that has it.
        assert!(super::contains_write(
            "SELECT `delete`# c
(1)",
            SqlDialect::MySql
        ));
        // Mixed whitespace and comments, in both orders.
        for sep in [
            " /*x*/ ",
            "	--c
	",
            "/*a*/ /*b*/",
        ] {
            let sql = format!("SELECT \"pg_read_file\"{sep}('/etc/passwd')");
            assert!(
                super::read_only_reason(&sql, SqlDialect::Postgres).is_err(),
                "{sql:?}"
            );
        }
        // A quoted *column* followed by a comment is still a read — the case the
        // rule is tuned for must survive the widening.
        assert!(
            super::read_only_reason("SELECT \"delete\"/*x*/ FROM t", SqlDialect::Postgres).is_ok()
        );
        assert!(
            super::read_only_reason(
                "SELECT \"delete\" -- c
 FROM t",
                SqlDialect::Postgres
            )
            .is_ok()
        );
        assert!(!super::contains_write(
            "SELECT \"delete\"/*x*/ FROM t",
            SqlDialect::Postgres
        ));
        // An unterminated comment runs to end of input and reaches no `(`.
        assert!(super::read_only_reason("SELECT \"delete\"/* x", SqlDialect::Postgres).is_ok());
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

    /// **Stopping at the caret's statement must not change the answer.**
    ///
    /// `statement_range` now cuts the boundary walk short instead of scanning
    /// the whole document. The reference here is the *old* spelling — the full
    /// `statement_bounds` list, and the same selection over it — so this is a
    /// migration equivalence check rather than a restatement of the function
    /// under test: if the two ever disagree, the caret's statement has been
    /// mis-located and signature help, Run Current and the completion scope go
    /// with it.
    ///
    /// The corpus is the shapes where an early stop can go wrong: the very
    /// first and very last offsets, a `;` inside a string or comment (no
    /// boundary), MySQL's `DELIMITER` (a boundary the directive itself makes,
    /// and a terminator that changes mid-document), SQLite's `BEGIN … END`
    /// trigger body (semicolons that do not split), and blank trailing
    /// segments, which are the one case that reads a boundary *behind* the
    /// caret.
    #[test]
    fn stopping_at_the_caret_gives_the_same_range_as_scanning_the_whole_buffer() {
        fn full_scan(sql: &str, offset: usize, dialect: SqlDialect) -> (usize, usize) {
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
                return trim_range(sql, bounds[k - 1], bounds[k]);
            }
            (lo, hi)
        }

        let corpora = [
            "",
            "SELECT 1",
            "SELECT 1; SELECT 2; SELECT 3",
            "SELECT 1;   ;  ; SELECT 2;   ",
            "SELECT ';' ; SELECT 2",
            "SELECT 1 -- ; not a bound\n; SELECT 2",
            "SELECT /* ; */ 1; SELECT 2",
            "DELIMITER $$\nCREATE TRIGGER t BEGIN SELECT 1; END$$\nDELIMITER ;\nSELECT 2;",
            "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE b SET x=1; END; SELECT 2;",
            "SELECT 1;",
        ];
        for sql in corpora {
            for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
                for offset in 0..=sql.len() + 2 {
                    assert_eq!(
                        super::statement_range(sql, offset, d),
                        full_scan(sql, offset, d),
                        "{d:?} {sql:?} at {offset}"
                    );
                }
            }
        }
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

    /// A caller that hands over "whatever follows the keyword" can arrive with
    /// nothing left. That is `None` — the same answer as for a byte that starts
    /// no literal — and not a panic on `b[0]` of an empty slice, which is how
    /// `CREATE PROCEDURE \`p\`() COMMENT` used to take the process down through
    /// the MySQL routine header reader.
    #[test]
    fn skip_noncode_on_an_empty_slice_is_none() {
        assert_eq!(skip_noncode(b"", 0), None);
        assert_eq!(skip_noncode(b"abc", 3), None);
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert_eq!(super::skip_noncode(b"", 0, d), None);
        }
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

    /// **On an engine whose read-only session is only a rollback, the editor
    /// refuses what the rollback cannot undo** — as the headless gate and the
    /// plan modal's Analyze already did on the same connection. A `RAISERROR …
    /// WITH LOG` and a function calling an extended procedure are no writes to
    /// `contains_write`, and their effect outside the database survived the
    /// rolled-back transaction (measured on 2022).
    #[test]
    fn a_read_only_rollback_session_blocks_what_the_rollback_cannot_undo() {
        let ms = GuardPolicy {
            read_only: true,
            dialect: SqlDialect::MsSql,
            ..open_policy()
        };
        for sql in [
            "SELECT 1 AS a RAISERROR('x', 1, 1) WITH LOG",
            "SELECT dbo.note()",
        ] {
            assert!(
                matches!(v(&[sql], ms), RunVerdict::Block(ref m) if m.starts_with("Read-only connection")),
                "{sql}: {:?}",
                v(&[sql], ms)
            );
        }
        assert_eq!(v(&["SELECT * FROM t WHERE a = 1"], ms), RunVerdict::Allow);
        assert_eq!(v(&["SELECT GETDATE()"], ms), RunVerdict::Allow);
        // Writable, or on an engine with a real read-only session, nothing moves.
        assert_eq!(
            v(
                &["SELECT dbo.note()"],
                GuardPolicy {
                    read_only: false,
                    ..ms
                }
            ),
            RunVerdict::Allow
        );
        for d in [SqlDialect::Postgres, SqlDialect::MySql] {
            let p = GuardPolicy {
                read_only: true,
                dialect: d,
                ..open_policy()
            };
            assert_eq!(v(&["SELECT my_func()"], p), RunVerdict::Allow, "{d:?}");
        }
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

    /// **SQL Server has PostgreSQL's hazard.** A connection that names no
    /// database is in the login's default one — `master`, unless someone
    /// changed it — so an unscoped `CREATE TABLE` builds a table there.
    #[test]
    fn a_sql_server_statement_that_would_land_in_master_needs_a_database() {
        let ms = SqlDialect::MsSql;
        for sql in [
            "CREATE TABLE users (id int)",
            "CREATE INDEX ix ON users (id)",
            "CREATE SCHEMA sales",
            "CREATE USER bob FOR LOGIN bob",
            "INSERT INTO users VALUES (1)",
            "UPDATE users SET id = 2",
            "DELETE FROM users",
            "TRUNCATE TABLE users",
            "EXEC sp_rename 'a', 'b'",
            // `DATABASE` followed by these names the *current* database, or
            // something inside it — on an unscoped tab, `master`.
            "ALTER DATABASE SCOPED CONFIGURATION SET MAXDOP = 1",
            "ALTER DATABASE CURRENT SET RECOVERY SIMPLE",
            "CREATE DATABASE SCOPED CREDENTIAL c WITH IDENTITY = 'x', SECRET = 'y'",
            "CREATE DATABASE AUDIT SPECIFICATION a FOR SERVER AUDIT s",
            "CREATE DATABASE ENCRYPTION KEY WITH ALGORITHM = AES_256 \
             ENCRYPTION BY SERVER CERTIFICATE c",
            "DROP DATABASE SCOPED CREDENTIAL c",
        ] {
            assert!(needs_database(sql, ms), "{sql}");
        }
        for sql in [
            "SELECT name FROM sys.databases",
            "CREATE DATABASE app",
            "DROP DATABASE app",
            "ALTER DATABASE app SET RECOVERY SIMPLE",
            "CREATE LOGIN bob WITH PASSWORD = 'x'",
            "USE app",
            "BEGIN TRAN",
            "COMMIT",
            "SET NOCOUNT ON",
            "DECLARE @x INT",
        ] {
            assert!(!needs_database(sql, ms), "{sql}");
        }
    }

    /// **T-SQL's `;` is optional, so a harmless head can front a statement
    /// that is not.** `SET NOCOUNT ON` on the line above a `CREATE TABLE` is how
    /// a great many scripts begin, and the range's head alone said the table
    /// could go anywhere — into `master`, on an unscoped tab.
    #[test]
    fn a_sql_server_statement_behind_a_harmless_one_still_needs_a_database() {
        let ms = SqlDialect::MsSql;
        for sql in [
            "SET NOCOUNT ON\nCREATE TABLE t (id int)",
            "DECLARE @x int = 1\nINSERT INTO t VALUES (@x)",
            "SELECT 1 CREATE TABLE t (id int)",
            "BEGIN TRAN\nUPDATE t SET a = 1\nCOMMIT",
            "SELECT * INTO copy_of_t FROM app.dbo.t",
            "IF OBJECT_ID('t') IS NULL CREATE TABLE t (id int)",
            "DROP TABLE IF EXISTS t",
            "WITH c AS (SELECT 1 AS a) INSERT INTO t SELECT a FROM c",
        ] {
            assert!(needs_database(sql, ms), "{sql}");
        }
        for sql in [
            // `USE` moves the rest of the batch to the database it names.
            "USE app\nCREATE TABLE t (id int)",
            // Server-level: nothing is left behind in the current database.
            "KILL 55",
            "BACKUP DATABASE app TO DISK = N'/var/opt/mssql/app.bak'",
            "RESTORE DATABASE app FROM DISK = N'/var/opt/mssql/app.bak'",
            "DBCC SQLPERF(LOGSPACE)",
            "RECONFIGURE",
            "SET NOCOUNT ON\nSELECT name FROM sys.databases",
            "IF DB_ID('app') IS NULL CREATE DATABASE app",
            "DROP DATABASE IF EXISTS app",
            // A join hint, not a `MERGE`.
            "SELECT * FROM sys.objects o INNER MERGE JOIN sys.columns c \
             ON c.object_id = o.object_id",
        ] {
            assert!(!needs_database(sql, ms), "{sql}");
        }
    }

    /// **The missing-`WHERE` net judges every T-SQL statement in a range**,
    /// not the range's head: with no `;` between them, a bare `DELETE` on the
    /// line below a scoped one borrowed the first one's `WHERE`, and ran with
    /// no confirmation.
    #[test]
    fn every_sql_server_statement_in_a_range_is_judged_for_a_missing_where() {
        let ms = SqlDialect::MsSql;
        for sql in [
            "DELETE FROM a WHERE id = 1\nDELETE FROM b",
            "UPDATE t SET a = 1 WHERE id = 5\nUPDATE t SET b = 2",
            "SET NOCOUNT ON\nDELETE FROM t",
            "SET XACT_ABORT ON DELETE FROM t",
            "UPDATE t SET a = 1 SELECT * FROM t WHERE id = 1",
            "IF @x = 1 DELETE FROM t",
            "BEGIN TRAN DELETE FROM t COMMIT",
            "SELECT 1 TRUNCATE TABLE t",
            "WITH c AS (SELECT a FROM t) SELECT * FROM c DELETE FROM u",
            "SELECT 1 DROP TABLE t",
        ] {
            assert!(super::first_unsafe(sql, ms).is_some(), "{sql}");
        }
        // What is one statement is judged as one — or, as `INSERT INTO t
        // SELECT`, cut only where neither piece is a statement a guard asks
        // about.
        for sql in [
            "DELETE FROM t WHERE id IN (SELECT id FROM u)",
            "UPDATE t SET a = CASE WHEN b = 1 THEN 2 END WHERE c = 3",
            "MERGE t USING s ON t.id = s.id WHEN MATCHED THEN UPDATE SET a = s.a \
             WHEN NOT MATCHED BY SOURCE THEN DELETE;",
            "INSERT INTO t SELECT * FROM u",
            "SELECT a FROM t UNION ALL SELECT a FROM u",
            "DECLARE c CURSOR FOR SELECT a FROM t FOR UPDATE OF a",
            "ALTER TABLE t ADD CONSTRAINT fk FOREIGN KEY (a) REFERENCES u (id) \
             ON DELETE CASCADE ON UPDATE NO ACTION",
            "CREATE PROCEDURE p AS DELETE FROM t",
            "GRANT SELECT, UPDATE, DELETE ON t TO u",
            "DECLARE @delete int SELECT @delete = 1",
            "DELETE FROM t OUTPUT deleted.id WHERE id = 1",
            "WITH c AS (SELECT TOP (10) * FROM t ORDER BY id) DELETE FROM c WHERE a = 1",
        ] {
            assert_eq!(super::first_unsafe(sql, ms), None, "{sql}");
        }
        // The other engines end a statement at `;` alone, as before.
        assert_eq!(
            super::first_unsafe(
                "DELETE FROM a WHERE id = 1\nDELETE FROM b",
                SqlDialect::MySql
            ),
            None
        );
    }

    /// An `ALTER TABLE`'s `DROP COLUMN`/`CONSTRAINT`/`PERIOD` is a clause, not
    /// a statement — cut there, the `.sql` panel counted the `ALTER` and its
    /// `DROP` as two — while a `DROP` that names an object still begins one.
    #[test]
    fn an_alter_tables_drop_clause_is_no_statement_of_its_own() {
        let ms = SqlDialect::MsSql;
        for one in [
            "ALTER TABLE t DROP COLUMN c",
            "ALTER TABLE t DROP CONSTRAINT pk_t",
            "ALTER TABLE t DROP PERIOD FOR SYSTEM_TIME",
        ] {
            assert_eq!(super::tsql_statements(one, ms), vec![one]);
        }
        assert_eq!(
            super::tsql_statements("ALTER TABLE t ADD c int\nDROP TABLE u", ms),
            vec!["ALTER TABLE t ADD c int", "DROP TABLE u"]
        );
    }

    /// And the error it answers with, when it ran in `master` and the table
    /// was not there, is error 208's.
    #[test]
    fn sql_server_says_invalid_object_name_when_it_ran_elsewhere() {
        let ms = SqlDialect::MsSql;
        assert_eq!(
            no_database_failure("query failed: Invalid object name 'company'.", ms),
            Some(NoDatabaseFailure::RanElsewhere)
        );
        assert_eq!(
            no_database_failure("query failed: Invalid column name 'x'.", ms),
            None
        );
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

    /// **The invariant asked of a *scanner*, which is the half that was
    /// missing.** The three tests above assert the two predicates and nothing
    /// that uses them, so `sql.rs`' own three hand-rolled ASCII word scanners
    /// were green — and one of them is the missing-`WHERE` safety net.
    ///
    /// `has_top_level_where` ended a word at every byte `>= 0x80`, so the ASCII
    /// tail of a non-ASCII identifier scanned as a word of its own:
    /// `DELETE FROM éwhere` answered "this statement has a WHERE",
    /// `unsafe_reason` returned `None`, and `run_verdict` never reached its
    /// `Confirm` arm — with `confirm_writes` off the delete ran unasked.
    /// Measured on the unfixed tree for all three spellings below.
    #[test]
    fn a_non_ascii_table_name_does_not_hide_a_missing_where() {
        for dialect in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            // The control: the same statement with an ASCII name is guarded,
            // and always was.
            assert!(!super::has_top_level_where(
                "DELETE FROM plainwhere",
                dialect
            ));
            for sql in [
                "DELETE FROM éwhere",
                "DELETE FROM sıwhere",
                "UPDATE caféwhere SET a = 1",
                // The lead byte immediately before the keyword, and a name that
                // *is* the keyword with a non-ASCII prefix.
                "DELETE FROM日本where",
            ] {
                assert!(
                    !super::has_top_level_where(sql, dialect),
                    "{sql} on {dialect:?} claims a WHERE it does not have"
                );
                assert!(
                    super::unsafe_reason(sql, dialect).is_some(),
                    "{sql} on {dialect:?} would run with no confirmation"
                );
            }
            // And a real `WHERE` after a non-ASCII name is still found, or the
            // fix would trade a missed guard for a spurious one.
            assert!(super::has_top_level_where(
                "DELETE FROM café WHERE a = 1",
                dialect
            ));
            assert!(super::unsafe_reason("DELETE FROM café WHERE a = 1", dialect).is_none());
        }
    }

    /// The same invariant at this module's other two scanners.
    ///
    /// Neither loses a guard — a statement `leading_keyword` mis-splits is a
    /// syntax error anyway, and `word_tokens` splitting only makes
    /// `contains_write` over-block, which is the correct direction for a
    /// refusal gate. They are pinned because a wrong copy of the one definition
    /// is what the next scanner gets copied from.
    #[test]
    fn a_non_ascii_byte_does_not_end_a_word_for_the_other_two_scanners() {
        // `SELECTé 1` is one word, so the leading keyword is that whole word —
        // not `SELECT`, which is what the ASCII scanner answered and what every
        // `read_only_heads` comparison downstream would have matched.
        assert_eq!(
            super::leading_keyword("SELECTé 1", SqlDialect::MySql).as_deref(),
            Some("SELECTé")
        );
        assert_eq!(
            super::leading_keyword("SELECT 1", SqlDialect::MySql),
            Some("SELECT".to_string())
        );
        // `cafédelete` is a column name, not a `DELETE`.
        assert!(!super::contains_write(
            "SELECT * FROM cafédelete",
            SqlDialect::MySql
        ));
        assert!(super::contains_write("DELETE FROM t", SqlDialect::MySql));
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

    /// SQL Server's `USE` carries across a batch as MySQL's does, and its
    /// quoted form is a bracket with the closer doubled.
    #[test]
    fn a_sql_server_use_switches_the_database_by_its_bracketed_name() {
        let ms = SqlDialect::MsSql;
        assert_eq!(use_target("USE app", ms).as_deref(), Some("app"));
        assert_eq!(use_target("USE [my db];", ms).as_deref(), Some("my db"));
        assert_eq!(use_target("USE [a]]b]", ms).as_deref(), Some("a]b"));
        assert_eq!(use_target("USE app SELECT 1", ms), None);
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
