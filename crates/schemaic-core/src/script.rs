//! Reading a `.sql` script as a stream of statements.
//!
//! The counterpart to [`crate::dump`]. That module decides what a dump *file*
//! says and hands `schemaic-app` a plan to write it; this one takes such a file
//! back apart, a block at a time, so a 2 GB dump can be replayed without ever
//! being held in memory. Between them a database can make the round trip that
//! export alone could not: Schemaic wrote files only another tool could read.
//!
//! **Everything here is pure.** [`Splitter`] is fed bytes and hands back
//! statements; it opens no file, holds no connection and knows nothing about
//! transactions. The driver in `schemaic-app` does the I/O, and the write guard
//! it must pass first is [`crate::sql::script_verdict`].
//!
//! The splitting itself is [`crate::sql::statement_bounds_open`] — the one SQL
//! boundary lexer, resumable. What this module adds is the discipline around it:
//! a buffer that keeps an unfinished statement until the rest of it arrives, a
//! drain that never cuts one in half, and the file position each statement came
//! from so a failure at statement 30,000 can be named by its line.

use crate::intel::SqlDialect;
use crate::sql::{self, ScanState};

/// What a driver should refuse to hold for one statement.
///
/// The splitter itself is happy to buffer without limit — it cannot know whether
/// the next byte closes the statement — so the ceiling belongs to the caller,
/// which is the half that can stop reading. 256 MB is far past any statement a
/// dump writes (an extended `INSERT` is capped by the server's own
/// `max_allowed_packet`, a megabyte or sixteen) and comfortably short of the
/// point where holding it is the user's whole machine.
///
/// A file that reaches it is not a script with a long statement; it is a file
/// with no statement terminator in it at all — a `.sql` that is really a CSV, or
/// a dump truncated inside a string literal.
pub const MAX_PENDING_BYTES: usize = 256 << 20;

/// One statement, with where in the file it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    /// The statement, trimmed, terminator included — exactly the bytes
    /// [`sql::statement_ranges`] would hand over for the same text.
    pub sql: String,
    /// 1-based line the statement starts on, for naming a failure.
    pub line: u64,
    /// Byte offset of the statement's first byte in the file, for a progress
    /// bar that can be honest about a file whose statement count is unknown.
    pub offset: u64,
}

/// Turns the blocks of a script file into statements.
///
/// Feed it with [`push`](Self::push) (or [`push_str`](Self::push_str)) and drain
/// what it returns; call [`finish`](Self::finish) once the file is exhausted,
/// because a script's last statement need not carry a terminator.
///
/// **Why a buffer rather than a scan per block.** A block boundary falls
/// wherever the read size puts it — inside a string, a comment, a dollar-quoted
/// body, a `DELIMITER` directive. The splitter never has to reason about that,
/// because it only ever *drains* up to a boundary the scan actually found: bytes
/// in an unterminated construct produce no boundary, so they stay put and are
/// re-scanned, from a real statement start, when more of the file arrives.
pub struct Splitter {
    dialect: SqlDialect,
    /// The scan's carry-over — the `DELIMITER` in force, which the directive
    /// that set it was drained away with.
    state: ScanState,
    /// Bytes read but not yet handed back as a statement.
    buf: String,
    /// An incomplete UTF-8 sequence cut by a block boundary, waiting for its
    /// remaining bytes. See [`push`](Self::push).
    carry: Vec<u8>,
    /// Decoded-byte offset of `buf[0]`, for [`Statement::offset`].
    offset: u64,
    /// **File** bytes handed to this splitter so far. Distinct from `offset`
    /// because lossy decoding is not length-preserving; see
    /// [`consumed`](Self::consumed).
    raw: u64,
    /// 1-based line of `buf[0]`.
    line: u64,
    /// Nothing has been decoded yet, so the next non-empty text may open with a
    /// BOM. See [`split`](Self::split).
    at_start: bool,
}

impl Splitter {
    /// A splitter for a script in `dialect`, positioned at the start of a file.
    pub fn new(dialect: SqlDialect) -> Self {
        Self {
            dialect,
            state: ScanState::new(),
            buf: String::new(),
            carry: Vec::new(),
            offset: 0,
            raw: 0,
            line: 1,
            at_start: true,
        }
    }

    /// Feed the next block of the file; returns the statements it completed.
    ///
    /// **Bytes, not `&str`, and that is the point.** A block boundary lands on a
    /// byte offset, which on a UTF-8 file may be the middle of a character — and
    /// a caller left to handle that itself is a caller that will one day call
    /// `from_utf8_lossy` per block and turn a `ä` straddling the boundary into
    /// two replacement characters, silently, inside a string literal on its way
    /// to the server. So the split sequence is held here and completed by the
    /// next block.
    ///
    /// A sequence that is genuinely invalid rather than merely incomplete
    /// becomes `U+FFFD`, which is [`crate::sqlfile::decode`]'s choice for the
    /// same problem: a mis-encoded byte should cost one character, not the file.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Statement> {
        // The common case — no character was cut — must not copy the block.
        let joined: Vec<u8>;
        let mut rest: &[u8] = if self.carry.is_empty() {
            bytes
        } else {
            joined = std::mem::take(&mut self.carry)
                .into_iter()
                .chain(bytes.iter().copied())
                .collect();
            &joined
        };
        let mut text = String::new();
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    text.push_str(s);
                    break;
                }
                Err(e) => {
                    text.push_str(
                        std::str::from_utf8(&rest[..e.valid_up_to()]).expect("valid by definition"),
                    );
                    match e.error_len() {
                        // Truncated by the block boundary — wait for the rest.
                        None => {
                            self.carry.extend_from_slice(&rest[e.valid_up_to()..]);
                            break;
                        }
                        // Genuinely invalid: one replacement character, carry on.
                        Some(n) => {
                            text.push('\u{FFFD}');
                            rest = &rest[e.valid_up_to() + n..];
                        }
                    }
                }
            }
        }
        // **Counted in the file's bytes, not the decoded text's.** An invalid
        // byte becomes a three-byte `U+FFFD`, so counting the decoded form made
        // a latin-1 file report more progress than it had — "5.2 MB of 4.9 MB".
        self.raw += bytes.len() as u64;
        self.split(&text)
    }

    /// [`push`](Self::push) for text that is already decoded.
    ///
    /// The whole of the splitting logic; `push` is this plus the UTF-8 carry.
    pub fn push_str(&mut self, text: &str) -> Vec<Statement> {
        self.raw += text.len() as u64;
        self.split(text)
    }

    /// The splitting itself, with no byte accounting — the half both
    /// [`push`](Self::push) and [`push_str`](Self::push_str) share, so the file
    /// position is counted exactly once whichever door was used.
    ///
    /// **Cost.** The buffer is re-scanned from its start on every block, so a
    /// statement spanning *m* blocks costs O(m²·block). That is fine for a dump,
    /// whose statements are far smaller than one block, and is why the driver
    /// grows its reads with [`pending`](Self::pending): a file with no
    /// terminator in it at all would otherwise re-scan a quarter of a gigabyte
    /// a thousand times before [`MAX_PENDING_BYTES`] refused it. Resuming the
    /// scan mid-buffer instead is not free — the scanner would have to carry
    /// "inside an unterminated string that began at *s*" across the call, and
    /// getting that wrong mis-splits a file rather than merely slowing it down.
    fn split(&mut self, text: &str) -> Vec<Statement> {
        // **The BOM comes off here, not at the caller.** Every Windows editor
        // writes one, U+FEFF is valid UTF-8 so `push` keeps it, and
        // `sql::trim_range` trims ASCII only — so it merged into the first word
        // (`is_word_start(0xEF)` is true by invariant) and statement 1 went to
        // the server as `\u{feff}DROP …`. Worse than the syntax error: `probe`
        // then read the kind as `\u{feff}DROP`, which `is_destructive` does not
        // match, so the red confirmation did not render for a file whose first
        // statement was `DROP DATABASE`. Doing it inside the splitter is what
        // makes the probe and the runner unable to disagree.
        //
        // `offset` moves on by the bytes dropped so a statement's offset still
        // points into the file. `raw` is untouched: it counts what was handed
        // in, and the BOM really was handed in.
        let text = if self.at_start && !text.is_empty() {
            self.at_start = false;
            match text.strip_prefix('\u{feff}') {
                Some(rest) => {
                    self.offset += '\u{feff}'.len_utf8() as u64;
                    rest
                }
                None => text,
            }
        } else {
            text
        };
        self.buf.push_str(text);
        let bounds = sql::statement_bounds_open(&self.buf, self.dialect, &mut self.state);
        let cut = bounds.last().expect("a scan always starts at 0").at;
        let mut out = Vec::new();
        // Newlines are counted once, walking forward with the windows, rather
        // than re-counted from the buffer start per statement — which would be
        // quadratic in a block holding thousands of tiny `INSERT`s.
        let mut walked = 0usize;
        let mut line = self.line;
        for w in bounds.windows(2) {
            let lo = sql::trim_range(&self.buf, w[0].at, w[1].at).0;
            line += newlines(&self.buf[walked..lo]) as u64;
            walked = lo;
            // **Through `sql::executable_range`, not by slicing the bounds.**
            // The client's `DELIMITER` token has to come off, and a script
            // runner sending a different string from Run Everything is exactly
            // the drift that function exists to prevent.
            if let Some(text) = sql::executable_range(&self.buf, w[0].at, w[1], self.dialect) {
                out.push(Statement {
                    sql: text.to_string(),
                    line,
                    offset: self.offset + lo as u64,
                });
            }
        }
        self.line += newlines(&self.buf[..cut]) as u64;
        self.offset += cut as u64;
        self.buf.drain(..cut);
        out
    }

    /// The end of the file: whatever is still held is the last statement, whether
    /// or not it carries a terminator.
    ///
    /// Also flushes a trailing incomplete UTF-8 sequence as `U+FFFD` — a file
    /// that ends mid-character is truncated, and losing the last character is a
    /// better report than losing the last statement.
    pub fn finish(&mut self) -> Option<Statement> {
        if !self.carry.is_empty() {
            self.carry.clear();
            self.buf.push('\u{FFFD}');
        }
        let lo = sql::trim_range(&self.buf, 0, self.buf.len()).0;
        // No terminator was found, so there is none to strip — but the same
        // function decides what counts as a statement.
        let end = sql::Bound {
            at: self.buf.len(),
            strip: 0,
        };
        let out = sql::executable_range(&self.buf, 0, end, self.dialect).map(|text| Statement {
            sql: text.to_string(),
            line: self.line + newlines(&self.buf[..lo]) as u64,
            offset: self.offset + lo as u64,
        });
        self.offset += self.buf.len() as u64;
        self.buf.clear();
        out
    }

    /// Bytes read but not yet handed back — an unfinished statement.
    ///
    /// The driver's backpressure signal: past [`MAX_PENDING_BYTES`] the file has
    /// no terminator in it and reading on will not help.
    pub fn pending(&self) -> usize {
        self.buf.len() + self.carry.len()
    }

    /// Bytes **of the file** turned into statements so far, for a progress
    /// reading against the file's length.
    ///
    /// Counted from what was handed in rather than from the decoded buffer,
    /// because lossy decoding is not length-preserving: an invalid byte becomes
    /// a three-byte `U+FFFD`, and the decoded count therefore ran *ahead* of the
    /// file on a mis-encoded dump. Subtracting what is still held can only make
    /// this an under-estimate, which is the safe direction for a progress bar —
    /// and it is exact at the end, where nothing is held.
    pub fn consumed(&self) -> u64 {
        self.raw.saturating_sub(self.pending() as u64)
    }
}

/// `\n` count — the line accounting, in one place.
fn newlines(s: &str) -> usize {
    s.as_bytes().iter().filter(|&&b| b == b'\n').count()
}

/// Heads whose *object* matters as much as the verb: `CREATE TABLE` and
/// `DROP TABLE` are not the same news, and neither is `CREATE INDEX`.
const QUALIFIED_HEADS: &[&str] = &["CREATE", "DROP", "ALTER"];

/// The object words worth naming after one of [`QUALIFIED_HEADS`].
///
/// **A whitelist, so an unrecognised shape degrades to the bare verb** rather
/// than to a guess. `CREATE OR REPLACE VIEW`, `CREATE TABLE IF NOT EXISTS` and
/// MySQL's `CREATE ALGORITHM=… DEFINER=… VIEW` all differ in what sits between
/// the two words that matter, and a rule that skipped "modifier words" would be
/// a list of every modifier three engines have — wrong the first time a version
/// adds one, and wrong silently.
const OBJECT_KINDS: &[&str] = &[
    "TABLE",
    "VIEW",
    "INDEX",
    "TRIGGER",
    "FUNCTION",
    "PROCEDURE",
    "DATABASE",
    "SCHEMA",
    "SEQUENCE",
    "TYPE",
    "DOMAIN",
    "EVENT",
    "ROLE",
    "USER",
    "EXTENSION",
];

/// How far into a statement [`statement_kind`] looks for its object word. See
/// [`sql::leading_words`] for why eight is the floor.
const KIND_WORDS: usize = 12;

/// What this statement *does*, in the words the modal reads out: `INSERT`,
/// `CREATE TABLE`, `DROP VIEW`.
///
/// Deliberately coarse. It exists so a user can see what a file will do to their
/// database before running it, and at that moment the difference that matters is
/// `CREATE TABLE` against `DROP TABLE` — not which flavour of `CREATE TABLE`.
pub fn statement_kind(sql: &str, dialect: SqlDialect) -> Option<String> {
    let words = sql::leading_words(sql, KIND_WORDS, dialect);
    let head = words.first()?;
    if !QUALIFIED_HEADS.contains(&head.as_str()) {
        return Some(head.clone());
    }
    Some(
        words
            .iter()
            .skip(1)
            .find(|w| OBJECT_KINDS.contains(&w.as_str()))
            .map(|kind| format!("{head} {kind}"))
            .unwrap_or_else(|| head.clone()),
    )
}

/// Statements that destroy or delete data, for the plain-language warning the
/// app gives before a script runs.
///
/// **This line is the confirmation**, which is what sets its width. The modal
/// has no "are you sure" step: a script is chosen from a dialog and run from a
/// panel that first says what the file will do, and *this sentence* is the part
/// of that saying the irreversible part. So the net is drawn wide enough that a
/// file it stays silent about is genuinely one worth running.
///
/// `DELETE` is therefore counted, though it is not destructive in the
/// `DROP`/`TRUNCATE` sense and `dump.rs` never writes one. A hand-written
/// script does, and a warning that said nothing about four hundred `DELETE`s
/// would be worse than no warning: it would have been read, and found
/// reassuring.
///
/// It stays a *count*, not a verdict. Whether a `DELETE` is missing a `WHERE` is
/// `sql::first_unsafe`'s question, and answering it a second time here in
/// different words is exactly the drift that predicate exists to prevent.
fn is_destructive(kind: &str) -> bool {
    kind.starts_with("DROP") || kind == "TRUNCATE" || kind == "DELETE"
}

/// How much of a script the probe reads before answering.
///
/// The same bound, for the same reason, as [`crate::import::SAMPLE_MAX_BYTES`]:
/// the user asked to *look* at a file, and a look must not turn into reading two
/// gigabytes off a disk. Every count past it is reported as a floor
/// ([`Probe::more`]) rather than rounded up to a lie.
pub const PROBE_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// And a ceiling on statements, for the file that is eight megabytes of tiny
/// `INSERT`s: past a couple of thousand the histogram has long since stopped
/// changing shape, and the modal is showing a summary, not an inventory.
pub const PROBE_MAX_STATEMENTS: usize = 2_000;

/// What a bounded look at the start of a script found.
///
/// The counterpart to [`crate::import::Sample`] — what the second step of the
/// Import modal shows once a file is picked. A CSV's sample can show the opening
/// *rows*; a script's can only show what the opening statements *do*, which is
/// the thing worth knowing before running one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Probe {
    /// Statements found in the probed prefix.
    pub statements: usize,
    /// Kinds and their counts, commonest first, ties by name so the readout is
    /// stable between two probes of the same file.
    pub kinds: Vec<(String, usize)>,
    /// The probe stopped before the end of the file, so **every count here is a
    /// floor**. The modal says `400+`, not `400`.
    pub more: bool,
    /// The script opens its own transaction, so the runner must not wrap it in
    /// another — `dump.rs`'s *Replaying → One transaction* already put one in
    /// the file, and nesting is not what either engine does with a second
    /// `BEGIN`.
    pub own_transaction: bool,
    /// Statements that destroy something. Named in plain language before the
    /// run, the way generated DDL is.
    pub destructive: usize,
    /// Bytes the probe actually read.
    pub bytes_read: u64,
}

impl Probe {
    /// A count as the modal should print it — `400+` when the probe stopped
    /// early, because the real total is unknowable without reading the file.
    pub fn count_label(&self, n: usize) -> String {
        if self.more {
            format!("{n}+")
        } else {
            n.to_string()
        }
    }
}

/// Read the start of a script and report what it holds.
///
/// Bounded by [`PROBE_MAX_BYTES`] and [`PROBE_MAX_STATEMENTS`]; either bound sets
/// [`Probe::more`].
///
/// **A truncated read ends on a partial statement, and that statement is
/// dropped.** Half an `INSERT` classifies as an `INSERT` and would inflate the
/// count by one, but the reason to drop it is sharper than that: the probe stops
/// mid-file at an arbitrary byte, and [`Splitter::finish`] means "the file ended
/// here" — calling it on a prefix would assert something untrue about the file.
pub fn probe<R: std::io::Read>(r: R, dialect: SqlDialect) -> std::io::Result<Probe> {
    // One byte past the bound, purely as proof there is more: `read_to_end` of
    // exactly the bound cannot tell a file that fits from one that was cut.
    use std::io::Read as _;
    let mut buf = Vec::new();
    r.take(PROBE_MAX_BYTES + 1).read_to_end(&mut buf)?;
    let truncated = buf.len() as u64 > PROBE_MAX_BYTES;
    if truncated {
        buf.truncate(PROBE_MAX_BYTES as usize);
    }
    let bytes_read = buf.len() as u64;

    let mut splitter = Splitter::new(dialect);
    let mut stmts = splitter.push(&buf);
    if !truncated {
        stmts.extend(splitter.finish());
    }
    let capped = stmts.len() > PROBE_MAX_STATEMENTS;
    stmts.truncate(PROBE_MAX_STATEMENTS);

    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut own_transaction = false;
    let mut destructive = 0usize;
    for s in &stmts {
        let Some(kind) = statement_kind(&s.sql, dialect) else {
            continue;
        };
        // Both spellings our own dump writes — `START TRANSACTION` on MySQL,
        // `BEGIN` on PostgreSQL and SQLite (`dump::transaction_sql`).
        if kind == "BEGIN" || kind == "START" {
            own_transaction = true;
        }
        if is_destructive(&kind) {
            destructive += 1;
        }
        *counts.entry(kind).or_default() += 1;
    }
    let mut kinds: Vec<(String, usize)> = counts.into_iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    Ok(Probe {
        statements: stmts.len(),
        kinds,
        more: truncated || capped,
        own_transaction,
        destructive,
        bytes_read,
    })
}

/// How the reading half of a run ended — the file side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadEnd {
    /// The file was read to its end and every statement handed over.
    Eof,
    /// The user stopped it.
    Cancelled,
    /// The file could not be read: a disk error, a revoked permission, a
    /// vanished network mount.
    Failed(String),
    /// One statement grew past [`MAX_PENDING_BYTES`] — a file with no statement
    /// terminator in it at all, rather than a script with a long statement.
    NoTerminator,
    /// The executor stopped consuming, so reading stopped too. **Carries no
    /// cause on purpose**: the executor has the real one.
    Stopped,
}

/// How the executing half of a run ended — the server side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecEnd {
    /// Every statement the reader handed over was run.
    Done,
    /// The server refused one.
    Failed {
        message: String,
        sql: String,
        line: u64,
    },
    /// The connection could not be opened at all, so nothing ran.
    Connect(String),
    /// The executor saw the cancellation itself, mid-statement.
    Cancelled,
}

/// What to report about a finished run.
///
/// **Every arm carries `ran`**, including the failures. A script is not
/// transactional unless the file said so, so "it stopped" always leaves the
/// question "how much of it happened?" — and that number is the answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Done {
        ran: usize,
    },
    Cancelled {
        ran: usize,
    },
    Failed {
        message: String,
        ran: usize,
        /// The statement the server refused and the line it began on. `None`
        /// when the failure was not a statement's — a file that could not be
        /// read has none to name.
        at: Option<(String, u64)>,
    },
}

/// Which half's ending the user is told about.
///
/// **Cancel is the reader's to declare, and so is every failure the reader can
/// see.** That second clause is where this parts company with
/// [`crate::dump::dump_verdict`], and the two must not be made to match: there
/// the *database* reads and the *file* is written, so a full disk fails the
/// writer and the reader only ever learns "nobody is reading any more". Here the
/// halves are swapped. A file that stops being readable is invisible to the
/// executor — it sees a channel close, which is indistinguishable from a script
/// that simply ended — so a reader failure outranks even the server error it
/// caused. A truncated read hands over half a statement, the server answers
/// "syntax error near…", and reporting *that* would name the file's last line as
/// the problem when the disk is.
///
/// The remaining order follows the same rule the dump's does: whichever half
/// holds the true cause describes it.
pub fn run_outcome(read: ReadEnd, exec: ExecEnd, ran: usize) -> RunOutcome {
    let failed =
        |message: String, at: Option<(String, u64)>| RunOutcome::Failed { message, ran, at };
    match (read, exec) {
        // Either half may witness the stop; neither is a finished run.
        (ReadEnd::Cancelled, _) | (_, ExecEnd::Cancelled) => RunOutcome::Cancelled { ran },
        // The reader's own failures outrank the server error they cause — see
        // the doc above, and `a_read_failure_outranks_the_server_error_it_caused`.
        (ReadEnd::Failed(e), _) => failed(e, None),
        (ReadEnd::NoTerminator, _) => failed(
            "This file holds no statement terminator, so it is not a SQL script \
             Schemaic can run. Check that you picked the file you meant."
                .to_string(),
            None,
        ),
        (_, ExecEnd::Connect(e)) => failed(e, None),
        (_, ExecEnd::Failed { message, sql, line }) => failed(message, Some((sql, line))),
        (ReadEnd::Eof, ExecEnd::Done) => RunOutcome::Done { ran },
        // The reader stopped because the executor did, and the executor says it
        // finished: the statements it was given all ran. Reachable when the
        // driver stops feeding for its own reasons rather than on a failure.
        (ReadEnd::Stopped, ExecEnd::Done) => RunOutcome::Done { ran },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every construct that can hide a `;` from a naive split, plus a
    /// `DELIMITER` block in the shape our own dump writes.
    const SCRIPT: &str = "-- a comment holding a ; semicolon\n\
                          SELECT 'a;b' AS x;\n\
                          INSERT INTO t VALUES (1), (2);\n\
                          DELIMITER $$\n\
                          CREATE TRIGGER tr BEFORE INSERT ON t FOR EACH ROW\n\
                          BEGIN\n\
                            SET NEW.a = 1;\n\
                            SET NEW.b = 2;\n\
                          END$$\n\
                          DELIMITER ;\n\
                          UPDATE t SET a = 3 WHERE id = 1;\n";

    /// Read `sql` in `block`-byte reads, the way the driver will.
    fn run(sql: &str, dialect: SqlDialect, block: usize) -> Vec<Statement> {
        let mut sp = Splitter::new(dialect);
        let mut out = Vec::new();
        for piece in sql.as_bytes().chunks(block) {
            out.extend(sp.push(piece));
        }
        out.extend(sp.finish());
        out
    }

    /// **The composition test.** However the file is cut up, the statements must
    /// be the ones reading it whole gives — at every block size, so the boundary
    /// falls inside a comment, a string, a directive and a trigger body in turn.
    ///
    /// Compared against `executable_statements`, not `statement_ranges`: what a
    /// runner sends is the text with the client's delimiter off, and comparing
    /// against the ranges is what let `END$$` reach a server for one release.
    #[test]
    fn a_script_read_in_blocks_yields_what_reading_it_whole_does() {
        for dialect in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            let whole: Vec<String> = sql::executable_statements(SCRIPT, dialect);
            for block in 1..=SCRIPT.len() {
                let got: Vec<String> = run(SCRIPT, dialect, block)
                    .into_iter()
                    .map(|s| s.sql)
                    .collect();
                assert_eq!(got, whole, "{dialect:?} at a {block}-byte read");
            }
        }
    }

    /// `DELIMITER` is the client's word; MySQL answers a syntax error to it. A
    /// runner that handed it back would fail every dump carrying a routine.
    #[test]
    fn a_delimiter_directive_is_never_handed_back_as_a_statement() {
        for block in [1, 7, 64, SCRIPT.len()] {
            let stmts = run(SCRIPT, SqlDialect::MySql, block);
            assert!(
                !stmts
                    .iter()
                    .any(|s| s.sql.to_uppercase().starts_with("DELIMITER")),
                "at a {block}-byte read: {:?}",
                stmts.iter().map(|s| &s.sql).collect::<Vec<_>>()
            );
        }
    }

    /// The reason a statement carries a position at all: naming the one that
    /// failed, in a file too big to open in the editor. The line must be the
    /// statement's own, not the block's or the buffer's.
    #[test]
    fn a_statement_carries_the_line_and_offset_it_starts_on() {
        for block in 1..=SCRIPT.len() {
            let stmts = run(SCRIPT, SqlDialect::MySql, block);
            let update = stmts
                .iter()
                .find(|s| s.sql.starts_with("UPDATE"))
                .unwrap_or_else(|| panic!("no UPDATE at a {block}-byte read"));
            // Counted from the fixture itself rather than written down, so the
            // assertion survives an edit to the script above.
            let want_offset = SCRIPT.find("UPDATE").expect("the fixture has one");
            let want_line = SCRIPT[..want_offset].matches('\n').count() as u64 + 1;
            assert_eq!(update.line, want_line, "line at a {block}-byte read");
            assert_eq!(
                update.offset, want_offset as u64,
                "offset at a {block}-byte read"
            );
        }
    }

    /// The first statement is on line 1, not line 0 — the off-by-one every
    /// line-numbering scheme is entitled to.
    #[test]
    fn the_first_statement_is_on_line_one() {
        let stmts = run("SELECT 1;\nSELECT 2;\n", SqlDialect::MySql, 4);
        assert_eq!(stmts[0].line, 1);
        assert_eq!(stmts[1].line, 2);
        assert_eq!(stmts[0].offset, 0);
        assert_eq!(stmts[1].offset, 10);
    }

    /// A script's last statement need not carry a terminator, and a runner that
    /// dropped it would replay a dump one statement short — silently.
    #[test]
    fn finish_yields_a_last_statement_with_no_terminator() {
        let stmts = run("SELECT 1;\nSELECT 2", SqlDialect::MySql, 3);
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[1].sql, "SELECT 2");
    }

    /// A dump ends with a comment banner more often than not. It is not a
    /// statement, and sending it would be an "empty query" error at the very end
    /// of a successful replay.
    #[test]
    fn a_comment_only_tail_is_not_a_statement() {
        let stmts = run("SELECT 1;\n-- done\n", SqlDialect::MySql, 5);
        assert_eq!(stmts.len(), 1, "{stmts:?}");
    }

    /// The UTF-8 seam. A multi-byte character straddling a block boundary must
    /// come back whole — inside a string literal it is data on its way to the
    /// server, and two replacement characters there is a corrupted row.
    #[test]
    fn a_multi_byte_character_split_across_blocks_survives() {
        let sql = "INSERT INTO t VALUES ('räksmörgås');\nSELECT 'πλ';\n";
        // Every possible cut, including ones landing inside each of the
        // two-byte and four-byte sequences.
        for block in 1..=sql.len() {
            let stmts = run(sql, SqlDialect::MySql, block);
            assert_eq!(stmts.len(), 2, "at a {block}-byte read");
            assert!(
                stmts[0].sql.contains("räksmörgås"),
                "at a {block}-byte read: {}",
                stmts[0].sql
            );
            assert!(stmts[1].sql.contains("πλ"), "at a {block}-byte read");
        }
    }

    /// An emoji is four bytes and lands outside the BMP — the case a splitter
    /// that reasoned in `char`s rather than bytes gets wrong.
    #[test]
    fn a_four_byte_character_split_across_blocks_survives() {
        let sql = "SELECT '🎉' AS party;";
        for block in 1..=sql.len() {
            let stmts = run(sql, SqlDialect::MySql, block);
            assert_eq!(stmts.len(), 1, "at a {block}-byte read");
            assert_eq!(stmts[0].sql, sql, "at a {block}-byte read");
        }
    }

    /// A truly invalid byte costs one character, not the statement around it.
    #[test]
    fn an_invalid_byte_becomes_one_replacement_character() {
        let mut sp = Splitter::new(SqlDialect::MySql);
        let mut out = sp.push(b"SELECT '\xFF' AS x;");
        out.extend(sp.finish());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].sql, "SELECT '\u{FFFD}' AS x;");
    }

    /// A file that ends mid-character is truncated; the last statement is worth
    /// more than the last character.
    #[test]
    fn a_file_ending_mid_character_still_yields_its_last_statement() {
        let mut sp = Splitter::new(SqlDialect::MySql);
        // The first two bytes of a three-byte sequence, and then nothing.
        let mut out = sp.push("SELECT 'π".as_bytes());
        assert!(out.is_empty(), "nothing is complete yet");
        out.extend(sp.finish());
        assert_eq!(out.len(), 1);
        assert!(out[0].sql.starts_with("SELECT '"));
    }

    /// What the driver watches to tell "a long statement" from "a file with no
    /// statement terminator in it".
    #[test]
    fn pending_reports_what_is_still_held() {
        let mut sp = Splitter::new(SqlDialect::MySql);
        assert_eq!(sp.pending(), 0);
        assert!(sp.push(b"SELECT 1").is_empty());
        assert_eq!(sp.pending(), 8, "an unfinished statement is held");
        assert_eq!(sp.push(b";").len(), 1);
        assert_eq!(sp.pending(), 0, "and released once it is complete");
    }

    /// Progress is reported against the file's *bytes*, because its statement
    /// count is unknowable without reading it. `consumed` must therefore reach
    /// the file's length exactly — an accounting slip here is a progress bar
    /// that stops at 98%.
    #[test]
    fn consumed_reaches_the_length_of_the_file() {
        for block in 1..=SCRIPT.len() {
            let mut sp = Splitter::new(SqlDialect::MySql);
            for piece in SCRIPT.as_bytes().chunks(block) {
                sp.push(piece);
            }
            sp.finish();
            assert_eq!(sp.consumed(), SCRIPT.len() as u64, "at a {block}-byte read");
        }
    }

    /// **Progress is the file's bytes, not the decoded text's.** Lossy decoding
    /// is not length-preserving — one invalid byte becomes a three-byte
    /// `U+FFFD` — so counting the buffer made a mis-encoded dump report more
    /// progress than it had, and a label read "5.2 MB of 4.9 MB".
    ///
    /// `consumed_reaches_the_length_of_the_file` could not catch this: its
    /// fixture is ASCII, where the two counts are equal.
    #[test]
    fn consumed_counts_the_files_bytes_even_when_they_do_not_decode() {
        // Three bytes in, one of them invalid — six bytes of decoded text.
        let raw = b"\xFF';\xFF';";
        let mut sp = Splitter::new(SqlDialect::MySql);
        sp.push(raw);
        sp.finish();
        assert_eq!(
            sp.consumed(),
            raw.len() as u64,
            "progress ran past the end of the file"
        );
    }

    /// An empty file is not an error and is not a statement.
    #[test]
    fn an_empty_file_yields_nothing() {
        assert!(run("", SqlDialect::MySql, 1).is_empty());
        assert!(run("   \n\n  ", SqlDialect::MySql, 1).is_empty());
    }

    // ── the probe ────────────────────────────────────────────────────────────

    fn kind(sql: &str) -> Option<String> {
        statement_kind(sql, SqlDialect::MySql)
    }

    /// The verb alone for an unqualified head, verb *and* object for the three
    /// that need it.
    #[test]
    fn a_statement_is_classified_by_its_verb_and_object() {
        assert_eq!(kind("INSERT INTO t VALUES (1)"), Some("INSERT".into()));
        assert_eq!(kind("SELECT 1"), Some("SELECT".into()));
        assert_eq!(kind("CREATE TABLE t (a int)"), Some("CREATE TABLE".into()));
        assert_eq!(kind("DROP TABLE IF EXISTS t"), Some("DROP TABLE".into()));
        assert_eq!(
            kind("ALTER TABLE t ADD COLUMN b int"),
            Some("ALTER TABLE".into())
        );
        assert_eq!(
            kind("CREATE OR REPLACE VIEW v AS SELECT 1"),
            Some("CREATE VIEW".into())
        );
        assert_eq!(
            kind("CREATE UNIQUE INDEX i ON t (a)"),
            Some("CREATE INDEX".into())
        );
        assert_eq!(
            kind("CREATE TEMPORARY TABLE t (a int)"),
            Some("CREATE TABLE".into())
        );
    }

    /// **The case a "skip the modifier words" rule gets wrong**, and the reason
    /// the object list is a whitelist instead. `mysqldump` writes its views with
    /// this preamble, and the back-quoted account is skipped as an identifier,
    /// so `VIEW` is the eighth word — which is also what sets the lower bound on
    /// `sql::leading_words`' `n`.
    #[test]
    fn a_mysql_view_preamble_is_still_a_create_view() {
        let sql = "CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`localhost` \
                   SQL SECURITY DEFINER VIEW `v` AS SELECT 1";
        assert_eq!(kind(sql), Some("CREATE VIEW".into()));
    }

    /// A shape the whitelist doesn't know degrades to the bare verb rather than
    /// to a wrong guess.
    #[test]
    fn an_unrecognised_object_falls_back_to_the_verb() {
        assert_eq!(
            kind("CREATE TABLESPACE ts ADD DATAFILE 'x'"),
            Some("CREATE".into())
        );
    }

    /// A comment is not a statement and has no kind.
    #[test]
    fn a_comment_has_no_kind() {
        assert_eq!(kind("-- nothing here"), None);
        assert_eq!(kind(""), None);
    }

    /// A `;` inside a string must not be read as a word boundary that promotes
    /// some later word into the classification.
    #[test]
    fn a_string_body_is_not_read_for_the_kind() {
        assert_eq!(
            kind("INSERT INTO t VALUES ('DROP TABLE users')"),
            Some("INSERT".into())
        );
    }

    fn probed(sql: &str) -> Probe {
        probe(sql.as_bytes(), SqlDialect::MySql).expect("a slice never fails to read")
    }

    /// The histogram the modal reads out: commonest first, ties by name so two
    /// probes of one file print the same order.
    #[test]
    fn the_probe_counts_kinds_commonest_first() {
        let p = probed(
            "CREATE TABLE a (x int);\
             INSERT INTO a VALUES (1);\
             INSERT INTO a VALUES (2);\
             INSERT INTO a VALUES (3);\
             CREATE TABLE b (x int);",
        );
        assert_eq!(p.statements, 5);
        assert_eq!(
            p.kinds,
            vec![("INSERT".to_string(), 3), ("CREATE TABLE".to_string(), 2)]
        );
        assert!(!p.more, "the whole file was read");
        assert_eq!(p.destructive, 0);
    }

    /// What the app says out loud before running anything irreversible — and
    /// **this line is the confirmation**, so its net is deliberately wide.
    ///
    /// `DELETE` is counted. It is not "destructive" in the `DROP`/`TRUNCATE`
    /// sense and a dump never writes one, but a hand-written script very much
    /// does, and a sentence that stayed silent about four hundred `DELETE`s
    /// would be worse than no sentence at all — it would have been *read* and
    /// found reassuring.
    #[test]
    fn the_probe_counts_what_the_script_destroys() {
        let p = probed("DROP TABLE a; TRUNCATE TABLE b; DROP VIEW v; INSERT INTO c VALUES (1);");
        assert_eq!(p.destructive, 3, "{:?}", p.kinds);
    }

    /// The wide half of that net.
    #[test]
    fn a_delete_counts_as_something_the_script_destroys() {
        let p = probed("DELETE FROM a WHERE id = 1; DELETE FROM b; SELECT 1;");
        assert_eq!(p.destructive, 2, "{:?}", p.kinds);
    }

    /// And nothing else is swept in with it: a script of reads and inserts
    /// raises no warning, or the warning stops meaning anything.
    #[test]
    fn an_ordinary_load_destroys_nothing() {
        let p = probed(
            "CREATE TABLE a (x int); INSERT INTO a VALUES (1); \
             UPDATE a SET x = 2; SELECT * FROM a;",
        );
        assert_eq!(p.destructive, 0, "{:?}", p.kinds);
    }

    /// Both spellings our own dump writes — `dump::transaction_sql` emits
    /// `START TRANSACTION` on MySQL and `BEGIN` elsewhere. Missing either would
    /// have the runner wrap an already-wrapped file.
    #[test]
    fn the_probe_notices_a_script_that_opens_its_own_transaction() {
        assert!(probed("START TRANSACTION;\nINSERT INTO t VALUES (1);\nCOMMIT;").own_transaction);
        assert!(
            probe(
                b"BEGIN;\nINSERT INTO t VALUES (1);\nCOMMIT;".as_slice(),
                SqlDialect::Postgres
            )
            .expect("a slice reads")
            .own_transaction
        );
        assert!(!probed("INSERT INTO t VALUES (1);").own_transaction);
    }

    /// **The composition, not the strip.** A splitter round-trip over a BOM'd
    /// file would go green against the fix's description while the thing that
    /// mattered — the red confirmation the modal renders — stayed silent, so
    /// the assertion is made where the failure was: `probe`.
    ///
    /// Every Windows tool writes the BOM, so this is the first file a Windows
    /// user picks. Before the strip, `statement_kind` read `\u{feff}DROP`,
    /// `is_destructive` did not match it, and `<BOM>DROP DATABASE production;`
    /// probed to `destructive = 0`.
    #[test]
    fn a_byte_order_mark_does_not_hide_what_the_first_statement_destroys() {
        let p = probed("\u{feff}DROP DATABASE production;\nSELECT 1;");
        assert_eq!(p.destructive, 1, "{:?}", p.kinds);
        assert_eq!(
            p.kinds,
            vec![("DROP DATABASE".to_string(), 1), ("SELECT".to_string(), 1)]
        );
    }

    /// The other half the BOM broke: a file that wraps itself read as one that
    /// does not, so the runner would have added a transaction of its own and
    /// the panel would have told the user a Stop leaves work behind when it
    /// does not.
    #[test]
    fn a_byte_order_mark_does_not_hide_the_scripts_own_transaction() {
        assert!(
            probed("\u{feff}START TRANSACTION;\nINSERT INTO t VALUES (1);\nCOMMIT;")
                .own_transaction
        );
    }

    /// And the statement itself is clean, whatever read size the driver used —
    /// U+FEFF is three bytes, so a one-byte read splits it across three calls.
    #[test]
    fn a_byte_order_mark_never_reaches_the_server() {
        for block in 1..=8 {
            let got: Vec<String> = run("\u{feff}SELECT 1;\nSELECT 2;\n", SqlDialect::MySql, block)
                .into_iter()
                .map(|s| s.sql)
                .collect();
            assert_eq!(
                got,
                vec!["SELECT 1;", "SELECT 2;"],
                "at a {block}-byte read"
            );
        }
    }

    /// Only the first three bytes, and only at the start of the file: a U+FEFF
    /// inside a string literal is the user's data.
    #[test]
    fn a_byte_order_mark_inside_the_file_is_left_alone() {
        let got = run(
            "SELECT '\u{feff}' AS bom;\n",
            SqlDialect::MySql,
            SCRIPT.len(),
        );
        assert_eq!(got[0].sql, "SELECT '\u{feff}' AS bom;");
    }

    /// A SQLite trigger body opens with `BEGIN`, but it is *inside* a
    /// `CREATE TRIGGER` — one statement, whose kind is `CREATE TRIGGER`. Reading
    /// it as a transaction would tell the user a file wraps itself when it does
    /// not.
    #[test]
    fn a_trigger_body_is_not_a_transaction() {
        let p = probe(
            b"CREATE TRIGGER tr AFTER INSERT ON t BEGIN UPDATE t SET a = 1; END;".as_slice(),
            SqlDialect::Sqlite,
        )
        .expect("a slice reads");
        assert_eq!(p.statements, 1, "{:?}", p.kinds);
        assert_eq!(p.kinds, vec![("CREATE TRIGGER".to_string(), 1)]);
        assert!(!p.own_transaction);
    }

    /// Past the statement ceiling every count is a floor, and says so.
    #[test]
    fn a_long_script_reports_its_counts_as_floors() {
        let sql = "INSERT INTO t VALUES (1);\n".repeat(PROBE_MAX_STATEMENTS + 50);
        let p = probed(&sql);
        assert_eq!(p.statements, PROBE_MAX_STATEMENTS);
        assert!(p.more, "the probe stopped early and must admit it");
        assert_eq!(
            p.count_label(p.statements),
            format!("{PROBE_MAX_STATEMENTS}+")
        );
    }

    /// A file read to its end reports exact counts, not floors — the other half
    /// of the label, and the one a "+ everywhere" bug would hide.
    #[test]
    fn a_short_script_reports_exact_counts() {
        let p = probed("SELECT 1;");
        assert!(!p.more);
        assert_eq!(p.count_label(1), "1");
    }

    /// The byte ceiling is the other way the probe stops, and the trailing
    /// partial statement it stops on is dropped rather than counted: half an
    /// `INSERT` is not an `INSERT`, and `finish` would be asserting the file
    /// ended at a byte the probe merely stopped at.
    #[test]
    fn a_truncated_read_drops_the_statement_it_was_cut_in_half() {
        // Two complete statements, then a third the bound cuts in half.
        let mut sql = String::from("SELECT 1;\nSELECT 'x';\n");
        // Pad past the byte bound *inside* a final unterminated statement.
        sql.push_str("INSERT INTO t VALUES ('");
        sql.push_str(&"a".repeat(PROBE_MAX_BYTES as usize));
        let p = probed(&sql);
        assert!(p.more, "the byte bound was hit");
        assert_eq!(p.bytes_read, PROBE_MAX_BYTES);
        // The two complete statements, and not the unterminated third.
        assert_eq!(p.statements, 2, "{:?}", p.kinds);
        assert!(
            !p.kinds.iter().any(|(k, _)| k == "INSERT"),
            "the cut statement was counted: {:?}",
            p.kinds
        );
    }

    /// A `DELIMITER` directive is not a statement and must not appear in the
    /// histogram — it would read as a kind the server has never heard of.
    #[test]
    fn the_histogram_holds_no_delimiter_directive() {
        let p = probed(SCRIPT);
        assert!(
            !p.kinds.iter().any(|(k, _)| k.starts_with("DELIMITER")),
            "{:?}",
            p.kinds
        );
        assert!(
            p.kinds.iter().any(|(k, _)| k == "CREATE TRIGGER"),
            "{:?}",
            p.kinds
        );
    }

    /// An empty file probes cleanly rather than erroring — the user picked a
    /// file, and "it holds nothing" is an answer.
    #[test]
    fn an_empty_file_probes_to_nothing() {
        let p = probed("");
        assert_eq!(p, Probe::default());
    }

    // ── the ending split ─────────────────────────────────────────────────────

    fn refused() -> ExecEnd {
        ExecEnd::Failed {
            message: "syntax error at or near \"VALUE\"".into(),
            sql: "INSERT INTO t VALUE (1)".into(),
            line: 412,
        }
    }

    /// **The bug this function exists to prevent.** Cancelling closes the
    /// channel, which the executor cannot tell from a script that simply ended
    /// — so on its own it reports a clean finish over a half-applied file.
    #[test]
    fn a_cancelled_read_is_not_a_finished_run() {
        assert_eq!(
            run_outcome(ReadEnd::Cancelled, ExecEnd::Done, 4_000),
            RunOutcome::Cancelled { ran: 4_000 }
        );
    }

    /// Same shape, different cause: a file that stops being readable is equally
    /// invisible to the executor.
    #[test]
    fn a_file_that_could_not_be_read_is_not_a_finished_run() {
        match run_outcome(
            ReadEnd::Failed("The device is not ready.".into()),
            ExecEnd::Done,
            120,
        ) {
            RunOutcome::Failed { message, ran, at } => {
                assert!(message.contains("device is not ready"), "{message}");
                assert_eq!(ran, 120);
                assert_eq!(at, None, "a disk error names no statement");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    /// **Where this parts company with `dump_verdict`, and the reason the two
    /// must not be unified.** A truncated read hands over half a statement, the
    /// server answers "syntax error", and reporting the server would name the
    /// file's last line as the problem when the disk is.
    #[test]
    fn a_read_failure_outranks_the_server_error_it_caused() {
        match run_outcome(
            ReadEnd::Failed("The device is not ready.".into()),
            refused(),
            9,
        ) {
            RunOutcome::Failed { message, .. } => {
                assert!(message.contains("device is not ready"), "{message}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    /// A file with no terminator in it is a different sentence from a disk
    /// error, and the user needs to hear which it was — most likely they picked
    /// the wrong file.
    #[test]
    fn a_file_with_no_statement_terminator_says_so() {
        match run_outcome(ReadEnd::NoTerminator, ExecEnd::Done, 0) {
            RunOutcome::Failed { message, at, .. } => {
                assert!(
                    message.to_lowercase().contains("terminator")
                        || message.to_lowercase().contains(";"),
                    "the message should name the missing terminator: {message}"
                );
                assert_eq!(at, None);
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    /// The ordinary failure: the server refused a statement, the reader stopped
    /// because the executor did, and the report names the statement and its
    /// line — the whole reason a `Statement` carries one.
    #[test]
    fn a_refused_statement_is_reported_with_its_line() {
        match run_outcome(ReadEnd::Stopped, refused(), 411) {
            RunOutcome::Failed { message, ran, at } => {
                assert!(message.contains("syntax error"), "{message}");
                assert_eq!(ran, 411);
                assert_eq!(at, Some(("INSERT INTO t VALUE (1)".into(), 412)));
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    /// Nothing ran, so there is no statement to name.
    #[test]
    fn a_connection_failure_names_no_statement() {
        match run_outcome(ReadEnd::Stopped, ExecEnd::Connect("refused".into()), 0) {
            RunOutcome::Failed { at, ran, .. } => {
                assert_eq!(at, None);
                assert_eq!(ran, 0);
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    /// The only way to a clean finish is both halves ending cleanly.
    #[test]
    fn a_clean_run_is_the_only_way_to_done() {
        assert_eq!(
            run_outcome(ReadEnd::Eof, ExecEnd::Done, 7),
            RunOutcome::Done { ran: 7 }
        );
        for read in [
            ReadEnd::Cancelled,
            ReadEnd::Failed("x".into()),
            ReadEnd::NoTerminator,
        ] {
            assert!(
                !matches!(
                    run_outcome(read.clone(), ExecEnd::Done, 7),
                    RunOutcome::Done { .. }
                ),
                "{read:?} reported a finished run"
            );
        }
    }

    /// **Every arm carries `ran`.** A script is not transactional unless the
    /// file said so, so "it stopped" always leaves "how much of it happened?" —
    /// and an arm that dropped the count would drop the only answer.
    #[test]
    fn every_outcome_reports_how_many_statements_ran() {
        let reads = [
            ReadEnd::Eof,
            ReadEnd::Cancelled,
            ReadEnd::Failed("disk".into()),
            ReadEnd::NoTerminator,
            ReadEnd::Stopped,
        ];
        let execs = [
            ExecEnd::Done,
            ExecEnd::Cancelled,
            ExecEnd::Connect("refused".into()),
            refused(),
        ];
        for read in &reads {
            for exec in &execs {
                let ran = 1234;
                let got = match run_outcome(read.clone(), exec.clone(), ran) {
                    RunOutcome::Done { ran } => ran,
                    RunOutcome::Cancelled { ran } => ran,
                    RunOutcome::Failed { ran, .. } => ran,
                };
                assert_eq!(got, ran, "{read:?} + {exec:?} lost the count");
            }
        }
    }
}
