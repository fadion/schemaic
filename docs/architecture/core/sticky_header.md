# `core/sticky_header.rs`

`sticky_header.rs` — which band header of a scrolled list is **pinned** to its top edge, and
how far the next one has pushed it: the pure half of the history and snippet panels' sticky
bands (the view half is `widgets::banded_list`). `pinned(headers, scroll_y, pinned_height)`
takes each in-flow header's measured `(top, height)` in list order and answers
`Option<Pinned { index, offset }>`. **It is `None` until the first header's top has gone
*strictly* above the edge**: at rest the in-flow header already stands where the copy would,
and drawing both paints the same band twice — so a header lying exactly at the top is left to
show itself. Past that the pinned band is the last one whose top is above the edge, and
`offset` is how far the next header has climbed into the copy (`next_top − scroll_y −
pinned_height`, clamped to `[−pinned_height, 0]`), which is what makes the arriving header push
the pinned one out rather than slide under it. The last band has no next header and is never
pushed. **The push is measured against the copy's height, not the in-flow header's**, because
the two differ: the copy wears `first`'s top rule and is a pixel taller than every header but
the first (`the_push_is_measured_against_the_pinned_copys_height_not_the_in_flow_one`). An
unmeasured height (`0`, before layout has reported one) pins without pushing, and the clamp's
lower bound is `max(0)`-guarded so a negative height cannot panic `f64::clamp`.
