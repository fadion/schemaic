# `core/script.rs`

`core/script.rs` — reading a `.sql` script **back**, as a stream of statements. The counterpart
to `dump.rs`: that module decides what a replayable file holds and hands the driver a plan to write
it, this one takes such a file apart a block at a time, so a dump far larger than memory can be
replayed. Until it existed the round trip did not close — Schemaic wrote `.sql` files only
another tool could read, because the import path takes CSV, JSON and Excel and
`sqlfile::open_verdict`
refuses a `.sql` past 64 MB.
**What a statement is and what a server may be *sent* are two different strings**, and
conflating them shipped a bug: a range carries its terminator — right for the editor, which
selects and highlights with it — but when that terminator is the client's `DELIMITER` token it
must come off. `END$$` lexes as a single identifier in MySQL, so the compound body never closes;
every dump carrying a trigger or routine failed at its first one, half-loaded, on *both* this
runner and Run Everything (which had it all along). `sql::Bound` now records how many of a
boundary's bytes are the client's, `;` keeping `strip: 0` because every engine accepts a trailing
one, and `sql::executable_statements` / `executable_range` are the single answer to "what is
sent" that both paths ask. Confirmed against MariaDB: `CREATE TRIGGER … END$$` is a 1064,
`… END` creates the trigger.
`Splitter` is the whole of it: `push` takes the next block's **bytes** and returns the
`Statement`s it completed. **A UTF-16 BOM, either byte order, makes it a UTF-16 read**, as a
tab opening the same file reads it (`sqlfile::decode`): decided on the file's first two bytes,
held back until both have arrived (and decoded as themselves by `finish` for a one-byte file),
with an odd byte or a surrogate pair's first half cut by a block waiting in the carry as a cut
UTF-8 sequence does. Read as UTF-8 it was a NUL between every character, and the probe could not
see a `DROP` (`a_utf16_script_splits_as_its_utf8_twin`). `finish` yields what is left (a script's final statement need not
carry a terminator, and a runner that dropped it would replay a dump one statement short,
silently). `finish` rescans the tail **as the end of the input**
(`sql::statement_bounds_closing`) and may yield more than one: mid-file a `GO` line with no
newline may still become `GOTO`, and a `;` whose next word has not arrived cannot yet say
whether an `ELSE` follows, so the scan waits — and the tail it waited with was sent whole, `GO`
and all, when `finish` returned it as one statement
(`a_last_go_with_no_newline_is_never_sent`). The splitting itself is `sql::statement_bounds_open` — the one boundary lexer, made
resumable — so what this module owns is the *discipline* around it: **it only ever drains up to
a boundary the scan actually found.** That single rule is what makes a block boundary landing
inside a string, a comment, a dollar-quoted body or a `DELIMITER` directive a non-event: an
unterminated construct yields no boundary, so those bytes stay in the buffer and are re-scanned
from a real statement start when the rest of the file arrives.
**`push` takes bytes rather than `&str` deliberately.** A block boundary lands on a byte offset,
which on a UTF-8 file is often mid-character, and a caller left to solve that itself is one that
will eventually call `from_utf8_lossy` per block — turning a character straddling the boundary
into two replacement characters *inside a string literal*, silently, on its way to the server.
The split sequence is held here and completed by the next block; a genuinely invalid byte costs
one `U+FFFD`, which is `sqlfile::decode`'s answer to the same question.
Each `Statement` carries the `line` and `offset` it started at, because the failure this has to
report is "statement 30,000 of a file you cannot open in the editor". Newlines are counted once
walking forward with the bounds rather than re-counted per statement, which would be quadratic
in a block holding thousands of small `INSERT`s. `pending` is the driver's backpressure signal
and `MAX_PENDING_BYTES` its ceiling: a file that reaches it is not a script with a long
statement, it is a file with no terminator in it at all. **Every driver sizes its reads by
`next_read`**, which grows the read with what is pending: the buffer is re-scanned from its
start on every block, so a fixed block made a long statement quadratic — the restore's scan
was that inline spelling a second time (a fixed 1 MiB, 32 MB in 5.4 s and 260 MB in 317 s),
and now shares the one function with the script runner
(`the_read_grows_with_what_is_still_unfinished`, `the_switch_scan_reads_in_growing_blocks`).
**`Splitter::skipping_copy_data` is the splitter for a scan that reads SQL rather than runs
it**: after a statement `copies_from_stdin` answers for — PostgreSQL's `COPY … FROM stdin`, the
head of every plain `pg_dump` table — it drops each line up to one that is exactly `\.`, as
psql reads them, holding at most the line it is in. Lexed as SQL, a row's `O'Brien` opened a
string that hid every later `\connect`, and a numeric table's rows (no `;`) grew one
"statement" past the ceiling, so *Restore from file* refused a 260 MB dump psql restores
(`copy_data_hides_no_later_switch`, `a_long_copy_section_is_never_held`). The runner's
splitter is the plain one: what a run does with `COPY … FROM stdin` is the server's answer.
`probe` is the other half, and is `import::read_sample`'s counterpart: what the Import modal's
second step shows once a file is picked. A CSV's sample can show the opening *rows*; a script's
can only show what its opening statements **do**, which is the thing worth knowing before
running one — a kind histogram (`INSERT ×400`, `DROP TABLE ×12`), how many statements destroy or
delete data (`is_destructive`, and see the write-guard invariant for why that count *is* the
confirmation and why its net therefore includes `DELETE` — and whatever `sql::drop_reason`
answers, so a `CREATE OR REPLACE TABLE` and an `ALTER TABLE … TRUNCATE PARTITION` are counted
where their kind alone would not be; `the_probe_counts_what_destroys_rows_by_another_name`), and
whether the file opens its own
transaction (`dump.rs`'s *Replaying → One
transaction* put one there, and the runner must not wrap an already-wrapped file). **It counts
the statements a piece holds, not the piece** (`sql::tsql_statements`; any other dialect's piece
is one): on T-SQL a piece is a batch, and from a `DECLARE` the rest of it is one piece
(`BatchScope`), so a migration opening with a variable probed as one `DECLARE` that destroyed
nothing over a `DELETE`, a `DROP COLUMN` and two `DROP`s
(`a_t_sql_script_counts_what_it_destroys_after_a_declare`). The transaction walk still indexes
pieces, because the `ran` that `durability` is asked about counts the pieces sent. And a T-SQL
`BEGIN` opens a transaction only as `BEGIN TRAN[SACTION]` or `BEGIN DISTRIBUTED`
(`opens_transaction`): a bare one opens a block, and read as an opener a `TRY … CATCH` made the
panel promise the file "lands whole or not at all" and report a later failure as undoing rows
that were committed (`a_t_sql_block_is_not_the_file_opening_a_transaction`). Nor does MariaDB's
`BEGIN NOT ATOMIC … END`, a compound block whose statements each autocommit, on any dialect —
no transaction `BEGIN` is followed by `NOT ATOMIC`
(`a_compound_block_is_not_the_file_opening_a_transaction`). **The transaction covers the file
from its opener on, not from statement 1**: `INSERT …; BEGIN; …` failing inside it keeps the
first `INSERT`, which ran in autocommit, and `durability` said "nothing was applied" over it on
every engine. `Probe::writes_before_opening` records a data change before the opener —
`sql::contains_write` less a session-only `SET`/`USE`/`DECLARE`/`PRINT` (`changes_data`), so a
dump's `SET NAMES` preamble is still no write — and turns that answer into
`Durability::AppliedBeforeTransaction` (`a_write_before_the_files_transaction_is_still_applied`).
It is bounded
by `PROBE_MAX_BYTES` — the same 8 MB as `SAMPLE_MAX_BYTES`, for the same reason: the user asked
to *look* at a file — and by `PROBE_MAX_STATEMENTS`. Either bound sets `Probe::more`, and every
count is then reported through `count_label` as a floor (`400+`), never rounded up to a total
that would have cost a full read to know. **A truncated read drops the statement it was cut in
half**: half an `INSERT` classifies as an `INSERT`, and `Splitter::finish` means "the file ended
here", which is not true of a byte the probe merely stopped at.
`statement_kind` is the vocabulary — the verb, plus the object for `CREATE`/`DROP`/`ALTER`,
where `CREATE TABLE` and `DROP TABLE` are not the same news. The object list is a **whitelist**,
so an unfamiliar shape degrades to the bare verb rather than to a guess: the alternative rule,
"skip the modifier words", is a list of every modifier three engines have, wrong the first time
a version adds one and wrong silently. The case that settles it is MySQL's view preamble
(`CREATE ALGORITHM=… DEFINER=…@… SQL SECURITY DEFINER VIEW`), which puts `VIEW` eighth once the
back-quoted account is skipped as an identifier — and which is what sets the floor on
`sql::leading_words`' bound. That function is `leading_keyword`'s plural and is bounded on
purpose: the private `word_tokens` tokenises to the end, which on a dump's sixteen-megabyte
extended `INSERT` allocates a `String` per word to learn one.
`run_outcome(ReadEnd, ExecEnd, ran)` is the two halves' ending split, and it is `dump.rs`'s
`dump_verdict` **inverted rather than copied** — the one thing to know before touching it. In a
dump the *database* reads and the *file* is written, so a full disk fails the writer and the
reader only ever learns "nobody is reading any more"; the writer therefore describes it. In a
load the halves swap: a file that stops being readable is invisible to the executor, which sees
a channel close it cannot tell from a script that simply ended. So here a **reader** failure
outranks even the server error it caused — a truncated read hands over half a statement, the
server answers "syntax error near…", and reporting that would name the file's last line as the
problem when the disk is. Cancel is either half's to witness and neither's to call a finish.
**Every arm carries `ran`**: a script is not transactional unless the file said so, so "it
stopped" always leaves "how much of it happened?", and that count is the only answer — which is
why an exhaustive test asserts no arm drops it.
**The panel *is* the confirmation, so the file it described has to be the file that runs** —
which is what `FileStamp` and `probe_still_describes` hold together. The probe reads a bounded
prefix and keeps only counts; the run re-opens the path and sends whatever is there at that
moment, and nothing else on the path ever looks at the content, `sql::script_verdict` treating a
whole `.sql` file as a write *without reading it* by design. So between the two opens the bytes
were free to change under a consent that named the old ones — benignly, when the user edits the
script in their editor while the modal stands (it is not window-modal and nothing re-probes),
and not benignly when the file is replaced outright. The failure and cancel reports compound it:
they read the probe *after* the run, so they would say "opened no transaction, so every
statement that ran is still applied" about a file that did open one. `FileStamp::of` takes the
length and the modification time in milliseconds off the file the probe opened, the stamp rides
on `ScriptRequest` beside the `Probe`, and `app/script.rs`'s reader refuses — *"This file changed
on disk after it was read, so the summary above is not what would run. Choose it again."* —
rather than running a file the panel never described. This is `sqlfile::changed_on_disk`'s
question for the **other** `.sql` path; that one compares *text*, because a tab holds the text it
last saved, and nothing here holds the file's bytes at all, so the comparison is the stamp.
**A missing timestamp is not a mismatch**: some filesystems report none, refusing on that would
make the feature unusable there for no gain, and the length still answers the replacement case,
which is the one with teeth. Two files of the same length modified within the same millisecond
are indistinguishable to this — stated rather than papered over, the alternative being to hash a
file the panel deliberately does not read whole.
**The write guard for this path is `sql::script_verdict`**, not `run_verdict` — see the
invariant.
**`database_switches` reads a script for the lines that take it off its database**, for
*Restore from file*, and reads **all** of it where `probe` stops at a prefix: `mysqldump
--databases a b` writes `USE b` half-way down. It feeds a `Splitter::skipping_copy_data` a
block at a time (see `next_read` above), so a dump is never held whole, and answers a `DatabaseSwitch { database, how, line }` for each
client command at the start of a line outside any string or comment — psql's `\c`/`\connect`,
the `mysql` client's `\u` and `\r` (`switch_commands`) — and each statement headed `USE` (MySQL,
SQL Server), the `mysql` client's bare `connect`, `CREATE`/`DROP DATABASE`, or on MySQL
`CREATE`/`DROP SCHEMA`, its synonym. A psql target may be a connection string — `pg_dump -C`
writes `\connect -reuse-previous=on "dbname='shop'"` on PostgreSQL 12 and later — and only a lone
`dbname` in one is read; a host or anything else beside it is `database: None`
(`a_pg_dump_create_script_names_its_connect`). A statement outgrowing `MAX_PENDING_BYTES` is
`InvalidData`: a file with no terminator, which the client could not run as a script either.
`restore_refusal(file, target, switches)` is the decision: a switch naming another database, or
one it cannot read, refuses with a `RestoreRefusal` — the first `RestoreRefusal::SHOWN` (three)
of those switches as values and a count of the rest, which the app words, its `Display` being
the English saying nothing was run (`a_databases_dump_names_the_database_it_switches_to`,
`an_unreadable_connect_is_refused_rather_than_trusted`,
`a_refusal_names_three_switches_and_counts_the_rest`); a switch to the target itself is no
move, compared case-sensitively, case being part of a MySQL database's name on Linux
(`a_dump_of_one_database_or_of_the_target_itself_is_not_refused`). Why it exists is the
write-guard invariant's restore paragraph.
