# `core/date.rs`

`date.rs` — civil dates, clock times, and the month grid a picker draws. `Date`/`Time` are
checked constructions (`0000-00-00` and a `TIME` column's `838:59:59` duration are both
rejected, which is how `celledit::fits` knows a calendar can't represent them); `Stamp` splits a
timestamp's text into the parts a picker edits and the parts it must **preserve byte for byte** —
the fractional seconds, the timezone offset, and whether the source separated date from time
with a space or a `T`. Picking a day out of `2024-01-15 10:30:00.123456+02` must not quietly
drop the rest, which is the same silent-rewrite class `jsontree` keeps a number's source text
for. **The tail is kept without being understood, but not without being checked**: `Stamp::parse`
validates it as an offset — `Z`, or a sign and one to three digit pairs — and answers `None`
otherwise, because everything downstream treats *any* tail as one (`has_offset` says so,
`with_offset` replaces it). A PostgreSQL BC timestamp (`0044-03-15 00:00:00 BC`) came through
with `tail = " BC"`, so `celledit::fits` said a calendar could show it, the picker opened on
March 44 **AD**, and *Now* wrote the era away by replacing ` BC` with `+02:00`. The date-only
spelling was already refused, since `take_time("BC")` fails; the asymmetry was that adding a time
let the same suffix through. The arithmetic is Howard Hinnant's `days_from_civil`/`civil_from_days` pair — the
workspace's only date maths, `format.rs`'s epoch formatter included — and `month_cells` is
**always 42 dates** (six weeks, Monday-first) so the panel doesn't change height as you page
through the year. `Date::today`, `Time::now` and `local_now` are the module's only readers of
the clock and, with `Time::at_local`, its only use of `chrono`: they read the **local** clock, because
`SystemTime` is UTC and a picker that highlights yesterday between local midnight and the offset
is wrong at exactly the hours somebody is most likely to be looking at it. **A timestamp field
goes through `local_now`**, which returns the day, the time of day and the zone's offset from
**one** reading of the clock: two reads are two instants and local midnight can fall between
them, which is how the calendar's *Now* — `Date::today` plus `Time::now` — wrote yesterday's
date beside today's time, a stamp a day in the past from the one control whose whole job is the
current instant. A caller that wants only the day still wants `Date::today`.
`Time::at_local(epoch_secs)` is the time of day a *given* instant shows on the local clock —
Compare results' *Ran 11:42:08* — so it reads the zone but never the clock, and answers `None`
for an instant past what chrono holds.
