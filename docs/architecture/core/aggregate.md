# `core/aggregate.rs`

`aggregate.rs` — what a multi-cell grid selection adds up to (`aggregate` → `Aggregates` +
`summary`). The arithmetic is **fixed-point, not `f64`**, and that is the whole reason the
module has substance: `Column::is_numeric` counts `DECIMAL`/`NUMERIC`, while `Value` leaves
those cells as `Str` precisely so the wire's digits are never rounded — and a money column is
exactly what anyone wants a `Sum` for. Summing through a float would reintroduce, in the one
number the user reads, the error the storage model exists to avoid (`45.599999999999994` under
a column of tidy prices). So values parse into `Fixed` (an `i128` of units at a scale), sum and
compare at the widest scale present, and format back keeping the column's own decimals; only
the average divides, and it carries extra places to say so. Overflow degrades to *no* aggregate
rather than wrapping — a silently wrong total is worse than an absent one. NULLs are counted
but excluded (the average divides by what it actually had), a numeric cell that doesn't parse
is present-but-skipped rather than treated as zero, and a non-numeric column gets no arithmetic
— counts, a distinct count and its length range only: there is nothing to sum in a name, and a
lexicographic min/max reads as a bug more often than it answers anything.
**Distinct, median and length** came later, and two of them break the fold's rule on purpose.
`distinct` counts different non-NULL values — a parsed number by its value (the trimmed
`Fixed`'s text, so `1.0` and `1.00` are one), anything else by its text. `median` is exact like
the rest: the middle value, or the mean of the two middles, which is at most a half-unit off
the shared scale, so an odd pair takes one more decimal place (units × 5 at scale + 1) rather
than rounding (`the_median_is_the_exact_middle`). `length` is the shortest and longest value of
a non-numeric column in `char`s, not bytes, and **the readout says `chars`**: that is a
`VARCHAR(n)`'s unit on MySQL and PostgreSQL, but SQL Server's `nvarchar(n)` counts UTF-16
units and a UTF-8 `varchar(n)` bytes, so a bare "Length" sized a column too small there
(`a_text_column_reports_its_length_range_in_characters`). **Neither distinct nor length is
given over bytes** — a binary cell is its `<n bytes>` size, so a thousand different
`BINARY(16)` ids read "1 distinct · Length 10"; the column is known by `Column::is_binary`, or
(SQLite, whose type names nothing) by a cell that is `is_binary_display`
(`a_binary_selection_has_no_distinct_count_or_length`). Distinct and median need every value kept, which is
exactly what the fold below refuses to do, so they stop at their own `KEPT_VALUE_BUDGET`
(10,000 *cells*, NULLs included — ten times under the scan's cap) and are then omitted while
Sum, Min and Max still cover the whole selection (`distinct_and_median_stop_at_their_own_budget`);
a truncated scan reports none of the three, and its row and NULL counts show **and copy** with
the `+` (`100000+`), since a bare number on the clipboard reads exact and is not
(`a_truncated_count_shows_and_copies_as_a_floor`). `summary` prints distinct and median only once
there are two non-NULL values — one value is its own of each, and `1 row · Sum 7` stays
unpadded (`a_single_value_is_not_padded_with_distinct_or_median`).
**There are five overflow sites and they do not degrade alike**, which is how three different
answers to one question shipped; each is tested apart. `Fixed::parse` answers
`Parsed::{Value, Overflow, NotANumber}` — three, not two, because a number too wide returning
the same `None` as `"n/a"` made `aggregate` *skip* it, and a `double` column holding `1e39` and
`2` then reported `Sum 2`: the one path in the app that printed a number simply untrue
(`Value::Float` stores `f64::to_string()`, which never uses exponent form, so forty digits
really arrive). `rescale` overflows on scale **spread** alone and is left as a degrade
deliberately — capping the working scale would make it the module's first rounding decision.
The running `checked_add` is the third. The fourth is the average, and it alone is an `Option`
on `NumericAggregates`: carried as a plain `Fixed` it propagated `?` and withheld an exact Sum,
Min and Max because the *mean* of them didn't fit — and it divides before it scales, so the
`DECIMAL(38,10)` case `Fixed`'s own headroom paragraph calls comfortable is representable.
The fifth is the median's, an `Option` beside it for the same reason: the two middles'
`checked_add`, or the odd pair's `checked_mul(5)`, answers `None` for the median alone and
leaves the rest standing (`an_unrepresentable_median_costs_only_the_median`, one case for each).
The grid enters through the *text*-taking form — `aggregate_counted`, of which
`aggregate_capped` and `aggregate_texts` are the `Option`-shaped wrappers — not `aggregate` over
stored cells: a staged (green) edit and a pending new row are both on screen and neither is in
the `ResultSet`, and a total under an edit it doesn't include is the same defect as a stale one.
**A cell is a `Counted`, not an `Option`, because there are three answers and an `Option` has
two**: `Value`, `Null`, and `Unset` — a pending row's unset cell or a staged `DEFAULT`, whose
value the server has not yet decided. An `Unset` is a row and never a null: it raises
`Aggregates::unset`, and the printed null count is `rows - non_null - unset`. Carried as an
`Option`, both were `None` and printed as nulls, while this document said they counted as
neither (`a_staged_default_is_counted_as_a_row_and_not_as_a_null`, in `edit.rs`).
`From<Option<S>>` maps `None` to `Null`, which is what the wrappers rely on. The fold keeps no `Vec` — it raises its own
accumulators when it meets a wider scale — because the vector was 32 bytes per selected cell,
allocated and freed on **every** recompute (6.4 MB for a Ctrl+A over 200k rows, once per
auto-repeat and once per cell crossed in a drag).
**And the scan is now bounded**: `aggregate_counted` is the real body (`aggregate_capped` maps
its cells through `Counted::from` into it), `aggregate_texts` calls `aggregate_capped` with
`usize::MAX`, and the grid passes `AGGREGATE_CELL_BUDGET` (100,000). The selection
aggregate is undebounced and was uncapped, unlike the find-count effect fifty lines below it in
the same function — 150 ms debounced *and* budgeted at two million cells "so a big grid doesn't
stutter typing" — though both are per-cell scans over the same result feeding a small readout
in the same panel. It cannot take that debounce, being driven by the selection rather than by
typing, and a two-cell selection has to answer at once; so the cap is the guard, and it is much
smaller than the find bar's precisely because this one runs inside the frame. A drag down a
column re-scans the range-so-far on every row it enters, so what matters is the cost of one
run, and this bounds it. Stopping produces `Aggregates::truncated`: the counts become lower
bounds printed with a `+`, the idiom the find bar's `find_more` already uses in the same panel,
and `numeric` goes to `None` — **a partial sum is not a sum**, which is the same call this
module already makes for overflow, arrived at by a different route. It reads exactly one cell
past the budget, which is how "there were more" is told from "that was all" without a second
pass.
