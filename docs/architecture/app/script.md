# `app/script.rs`

`app/script.rs` — the I/O half of `core::script`, and `cli/dump.rs`'s mirror image: that module
reads a database and writes a file, this reads a file and writes a database. Two halves at once — a
**blocking reader** walks the file in reads sized by `script::next_read`, feeds `script::Splitter` and pushes
completed statements into a bounded channel, while `Db::run_script` pulls from the other end.
Each reports how it ended and `script::run_outcome` decides which ending the user hears, which is
**not** the same precedence `dump_verdict` uses (see `core::script`).
`tx` is moved into the reader and dropped when it returns, so *every* exit — end of file, cancel,
disk error — closes the channel and lets the executor finish; and `ReadEnd::Stopped` is returned
with no message on purpose when the send fails, because that means the executor went away and it
is the one holding the reason.
**The reader's first act is to check that this is still the file the probe described.** It is the
*second* open of the path: `ScriptProbeDoneFn` hands the probe back as `(Probe, FileStamp)`, the
stamp rides on `ScriptRequest` beside the counts the panel printed, and `probe_still_describes`
compares it against the file just opened. A mismatch is a `ReadEnd::Failed` telling the user to
choose the file again, not a run under a summary that describes different bytes — see
`core::script` for why those counts *are* the confirmation.
**The bounded channel is the progress design.** The reader cannot get more than `SCRIPT_QUEUE`
statements ahead of the server, so `Splitter::consumed` tracks what has actually been applied
closely enough to report from — which is why there is no progress channel out of the executor,
and why a 2 GB file read at disk speed cannot pile up ahead of a server applying it one statement
at a time. Progress is reported in **bytes**: a file's statement total cannot be known without
reading it, and its byte length is known at `open`.
