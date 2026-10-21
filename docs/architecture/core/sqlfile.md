# `core/sqlfile.rs`

`sqlfile.rs` — the pure half of opening and saving a tab's SQL as a `.sql` file on disk: what a
file's bytes become in the editor (`decode`), what the editor's text becomes on the way back
(`encode`), what the tab is called once it has a file (`tab_title` — the file name *with* its
extension, or `orders.sql` and `orders.txt` would give two tabs one title), and what the Save
dialog should suggest for a tab that hasn't got one (`suggested_name`), plus the
`SQL_EXT`/`SQL_EXTENSIONS` both dialogs filter on — the filter's name beside them is the
interface's word (`file-sql-filter`, through the app's `tr_static!`), not core's.
**Line endings are remembered, not normalised away.** The editor works in `\n` and a `.sql` file
checked into a repository on Windows very often does not, so a save that rewrote every line
ending would turn a one-line edit into a whole-file diff — the kind of change that survives
review only because nobody can read it. `decode` collapses `\r\n` and *records* that it did, the
flag rides on the tab and on its `persist::SavedTab`, and `encode` puts it back (guarding
against a `\r\n` pasted into the buffer doubling into `\r\r\n`). Mixed endings resolve to the
majority and a tie to CRLF, because normalising the odd stray LF is the smaller lie; a lone `\r`
is not a line ending at all and is left exactly as it is, since that is what a string literal
may legitimately hold. `decode` also strips a UTF-8 BOM — a Windows tool writes one and it
arrives as an invisible character in front of the first keyword — and `encode` puts that back
too, on the same argument: dropping it rewrites the file's first three bytes for a one-line
edit. **A UTF-16 BOM, in either byte order, makes it a UTF-16 read** (`SqlFormat::utf16`, a
`Utf16::{Le, Be}`), which `encode` writes back in kind, BOM first — so `encode` answers bytes,
not a `String`. Nothing but the BOM identifies UTF-16, and read as UTF-8 it came back as a NUL
between every character and flagged lossy, so the one way to keep such a file — what Notepad's
and SSMS's *Unicode* write — was never to save it. A trailing odd byte or a lone surrogate is
substituted and flagged as a bad UTF-8 byte is
(`a_utf16_file_is_read_as_text_and_written_back_in_kind`, `a_damaged_utf16_file_is_read_lossily`).
Those flags, and one more, ride on `SqlFormat`, which is what the tab and its `SavedTab` carry.
**A lossy read is not a licence to write.** `decode` reads bytes it cannot make sense of as
U+FFFD, because a mis-encoded byte should cost a replacement character rather than the whole
file — and that is a decision about *reading*. Writing the result back is a different act: it
replaces every unreadable byte in the file permanently, including in the thousands of lines
nobody touched, and a Latin-1 `mysqldump` is the ordinary shape of it. So `SqlFormat::lossy`
records that it happened (free — `from_utf8_lossy` already allocates only when it substituted),
persists across a relaunch, and the app confirms before such a save — titled *Replace unreadable
bytes?* rather than after UTF-8, since a damaged UTF-16 file is saved as UTF-16. The `.sql` file is the one
artefact in this application Schemaic cannot regenerate, which is also why the write is
`persist::write_file_atomic` (stage beside, rename over — no `.bak`, since it is the user's own
file, and the mode it had carried onto the new inode; see `persist.rs` for the four parts of
that which read as tidiable) and why the save re-reads the bytes immediately before the rename
and refuses when they are not what the tab read.
**The same question is asked without a save, too.** `disk_change(disk, now)` is what the app's
window-focus check asks of each file-bound tab: the file's decoded text, but only where it
differs from the tab's `disk_sql` *through* `changed_on_disk` — so a line-ending conversion
alone is no change here, as it is none to the save guard, and a change that guard would not
block is never offered back. It answers `None` with no `disk` (a tab restored with unsaved edits
holds none, so there is nothing to compare against) and for a file gone or unreadable, which a
reload could not load either (`a_file_changed_under_its_tab_is_offered_back`).
`report_is_current(bound_path, bound_disk, read_path, read_disk)` is whether that answer, landing
later, still describes the tab: the same path, compared exactly, and the same on-disk text.
**Size is asked before the bytes are.** `open_verdict` confirms past 8 MiB and refuses past 64
MB, and the reason is the editor rather than the read: `fs::read` and `decode` are cheap even at
256 MB, while the editor works on the document for as long as it is open. The confirm stood at
1 MB while `intel::diagnostics` ran over the whole document on the UI thread 120 ms after every
pause in typing (710 ms at 1 MB, 11.4 s at 16 MB, on an empty catalogue). It went to 4 MiB once
that pass had moved to a worker through `intel::DiagnosticsCache` and the per-keystroke walks
had become passes taken forward over the edit, and to 8 MiB once an edit made one copy of the
text instead of a dozen, the caches kept none (`text::EditLog`), and the parameter names were
read once typing settles (release, a realistic script, a 200-table catalogue): at 8 MiB the UI
thread's whole-document work is 6–8 ms a keystroke, under a frame (~3.5 ms at 4 MiB), the
worker's re-check ~230 ms after an edit (~2.4 s for the first pass), nothing frozen. At 16 MiB
that comes to a frame or more a keystroke and a ~5 s first pass — a slow editor, which is why
past 8 MiB it still asks. `OPEN_WARN_BYTES`' doc holds the figures; the 64 MB refusal's are
extrapolated from them.
The naming half is entirely about what a file system will refuse: `suggested_name` maps the
characters Windows forbids to `-`, trims the trailing dots and spaces it also refuses, appends
`.sql` only when it isn't already there (a tab opened from `orders.sql` must not suggest
`orders.sql.sql`), and falls back to `query.sql` when nothing alphanumeric survives — a title of
`///` scrubs to `---`, which is legal and useless. `ensure_extension` fills a *missing*
extension only: the native dialogs mostly append the filter's own but not on every platform,
while `schema.ddl` is the user saying what they want and quietly writing `schema.ddl.sql`
instead is worse than honouring it. Pure + unit-tested inline.
**Whether two paths are one file is asked here as well** (`same_file`, over `same_path` with
`PATHS_IGNORE_CASE`), because two tabs bound to one file is a lost edit: each keeps its own copy
of the bytes on disk, so saving the second discards the first — and the first goes on reporting
itself clean, since its own copy still matches what it wrote. The comparison that allowed it was
`Path`'s `==`, which is component-wise and case-**sensitive** on every platform, wrong in exactly
the direction Windows and macOS need. The fold is a `const` per platform rather than a probe (a
wrong answer merges two files the user meant to keep apart), and both readings are tested. It is
deliberately only the path-shaped half: the app canonicalises first — which settles case, 8.3
short names, junctions and a substituted drive when the file exists — and asks this afterwards,
for the path being saved to for the first time, which cannot be canonicalised at all. The Open
command searches **every** tab through it, not the active connection's: the strip being
per-connection is a fact about visibility and no answer to the lost edit, and a hit on another
connection goes through `switch_conn` so the strip shows the tab it activates.
**A tab's binding to a file is one value, not three fields.** `FileBinding { path, disk_sql,
format }` with `restored_binding(path, file_dirty, query, format)` and `FileBinding::none()`,
because both failures here are a line left out. The file's *text* is not persisted, so whether
a restored tab knows its on-disk copy has exactly one answer per saved tab: a tab saved clean
had its editor text equal to the file and that text *is* persisted, while one saved dirty leaves
it unknown — and getting the fourth combination wrong brings a dirty tab back looking clean, so
the modified marker is gone and Ctrl+S is a no-op over unsaved work. `none()` is the other side:
the blank slate left when a connection's last tab closes must shed the path (or the next Ctrl+S
overwrites that file with an empty document) *and* the format (or a fresh script is written with
a BOM and CRLF it never had). `app/main.rs` reads both at its two sites; the decisions
themselves are tested here.
