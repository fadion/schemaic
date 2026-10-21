# `core/celledit.rs`

`celledit.rs` — **which editor a column's cells get**, and the value rules that editor
enforces. `editor_for_type` reads a *declared* type into a `CellEditor` (`Bool(BoolWire)`,
`Enum(members)`, `Set(members)`, `Date`, `DateTime(Zoned)`, else `Text`) and `editor_for_column` adds
the one thing the type text can't answer — a PostgreSQL enum type's members, which live in
`DbSchema::enums`. **Declared, never the wire type**: MySQL sends an `ENUM` as
`MYSQL_TYPE_STRING` and a `BOOLEAN` as `TINYINT`, so `Column::type_name` knows neither the
member list nor the `tinyint(1)` width, and a result whose schema hasn't loaded falls back to
the wire type (where the date family still resolves and the rest stay text). `fits` is the
other half and the safety one: a control may only be offered for a value it could itself have
produced, so a `tinyint(1)` holding `7` and a `DATE` holding `0000-00-00` keep the plain text
field — a toggle rendered over `7` writes `0` the moment it is touched. An **empty** value fits
everything (it means "nothing chosen yet"), which is what hands a NULL field its dropdown — and
that arm answers `true` *before* it looks at the editor, so it is **not** the `ENUM` protection
this entry used to claim. A MySQL `ENUM` holding the empty string a rejected insert wrote in
non-strict mode gets the dropdown, and the dropdown has no row that writes `''` back; the cell
is also indistinguishable from a NULL one, which `start_edit` seeds from the empty string too.
Telling the two apart means telling `fits` *which cell* it is looking at, which no caller can
say, so the exception stands and the cost is written down instead: a NULL cell and a fresh
pending row are the common cases, a literal `''` in an `ENUM` is what a misconfigured server
produced once. `BoolWire` is which
two literals a boolean writes, and it is **the engine's own spelling, not the readable one**
(`1`/`0` on MySQL and SQLite, `t`/`f` on PostgreSQL): every engine accepts several on the way
in, so the round trip decides — a toggle back to what the column already reads back is
recognised as a revert and un-stages, where `true` written over a `t` leaves a green cell whose
`UPDATE` writes a value already there. **The row already *held* is the one exception, and writes
the text the cell already has** rather than the wire spelling (`pick_options`), because the
revert property only holds where the engine's read spelling *is* its write spelling. SQLite has
neither a boolean type nor an opinion about one, so such a column may legally hold `'true'`: the
picker showed that row as held, and clicking it to confirm staged `1` — a different stored value
out of an action whose only visible meaning was "yes, that one". Writing back is
`set_date`/`set_now` (which keep a datetime's other parts — see `date.rs`) and
`toggle_set_member`, which re-emits a `SET` in **declaration order** because that is the order
MySQL stores and returns one in.
**An offset is a property of the destination, not text carried from the old value.**
`CellEditor::DateTime` carries `Zoned::{Offset, Naive}`, decided by the **type name alone**
(`zoned_for_type` takes no dialect): only PostgreSQL's `timestamptz` — and its verbose
spelling — resolves an offset, while a MySQL `DATETIME`, PostgreSQL's bare `timestamp` (which
parses one and *discards* it) and SQLite's text stamps have
nowhere to put one. `set_now` asks the destination, where the gate used to be "did the old text
carry a tail" — which reads `false` for exactly the column that most needs an offset, since a
MySQL `TIMESTAMP` is rendered in the session zone with no tail at all. So **Now** sent a client
wall clock the server read as its own: server at `+00:00`, client at `+02`, an instant stored two
hours in the future and rendered back in the session zone so the cell re-read as correct. It
drops the old value's fraction for the same class of reason — those were its microseconds, not
this instant's. **`set_date` asks the same destination, and it is the pair that made that
necessary.** It used to clear the offset unconditionally, on the premise that the tail was the
old value's — the server's rendering, best handed back bare so the session zone resolves it.
True of a value read from the server and false of one `set_now` wrote two clicks earlier, which
is the *client's* assertion about which instant "now" is: client at UTC+2, server at UTC, **Now**
put `09:00:00+02:00` in the field, picking another day turned it into a bare `09:00:00`, and the
commit stored an instant two hours from the one the field had been showing — rendered back in the
server's zone, so nothing on screen said it had moved. Every test asked one of the two functions
at a time and nothing composed them, which is what
`now_then_pick_a_day_keeps_the_instant_now_stated` is for. So a `Zoned::Offset` column is
re-stated with the client's offset and one that cannot resolve an offset still gets none. **The
cost is stated rather than hidden**: an offset qualifies a particular instant and the client's is
*today's*, so picking a July day from Berlin in January states `+01`, an hour from the July wall
clock the field shows. Recomputing it needs the zone's DST rules, which `core` has no business
carrying, and the string alone cannot tell a server-rendered offset from a client-asserted one —
so this is a choice between two wrong answers, and it takes the one that is wrong only across a
DST boundary over the one that was wrong whenever client and server sit in different zones.
**MySQL's `TIMESTAMP` is the one entry this deliberately gets
wrong**, and it is `Zoned::Naive` on purpose: the type *does* resolve an offset — stored in UTC,
rendered in the session zone — but there is no offset it can be sent that every server on this
dialect reads. `[+-]hh:mm` inside a datetime literal is MySQL 8.0.19+, and **MariaDB accepts no
spelling of it at all**: `+02:00`, `+02`, an ISO `T` separator and `Z` each answer
`ERROR 1292 (22007) Incorrect datetime value` on MariaDB 10.11.14, and `SqlDialect` is one
variant for both servers with no version in it. The two failures are not symmetric — withholding
the offset stores a wrong instant only where the server's session zone differs from the
client's, while sending one *fails the edit outright* on every MariaDB — which is the same call
`alter_clauses` makes in emitting `DROP CONSTRAINT` over MySQL 8's `DROP CHECK`. The residual
divergence is the acknowledged cost, and closing it is not this layer's to do: it needs the
server's own `@@session.time_zone`, read on connect and carried to the write path, so the wall
clock can be *converted* rather than annotated
(`a_mysql_timestamp_takes_no_offset_because_the_dialect_cannot_promise_one` pins the choice).
The `ENUM`/`SET` member list is parsed off the type text
through `sql::skip_noncode` — the one boundary lexer — and unescaped as the exact inverse of
`export::sql_literal`, since splitting on commas loses a member containing one.
