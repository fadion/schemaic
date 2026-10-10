//! Which band header is pinned to the top of a scrolled list, and how far the
//! next one has pushed it up.
//!
//! The history and snippet panels list their rows in bands, each opened by a
//! header that scrolls with the rows. A pinned copy of the header whose band is
//! under the top edge stays put while that band scrolls, and the next header,
//! arriving from below, pushes it out rather than sliding under it. The panels
//! measure where their headers lie; this decides what that means.

/// The header to pin, and its vertical offset from the top of the viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pinned {
    /// Index of the band whose header is pinned.
    pub index: usize,
    /// Zero while the band has the top to itself; negative, down to
    /// `-pinned_height`, while the next header pushes it out.
    pub offset: f64,
}

/// The pinned header for a list scrolled to `scroll_y`.
///
/// `headers` is each in-flow header's `(top, height)` in the list's own
/// coordinates, in list order; `pinned_height` is the pinned copy's height.
///
/// **`None` until the first header has scrolled past the top.** At rest the
/// in-flow header is already where the pinned copy would be, and drawing both
/// paints the same band twice. A band is pinned from the moment its header's
/// top has gone above the edge — strictly, so a header lying exactly at the top
/// is left to show itself.
pub fn pinned(headers: &[(f64, f64)], scroll_y: f64, pinned_height: f64) -> Option<Pinned> {
    let index = headers.iter().rposition(|&(top, _)| top < scroll_y)?;
    let offset = headers
        .get(index + 1)
        .map_or(0.0, |&(next_top, _)| next_top - scroll_y - pinned_height)
        .clamp(-pinned_height.max(0.0), 0.0);
    Some(Pinned { index, offset })
}

#[cfg(test)]
mod tests {
    use super::{Pinned, pinned};

    /// Three 30px headers opening bands of 100, 200 and 50px of rows.
    const BANDS: [(f64, f64); 3] = [(0.0, 30.0), (130.0, 30.0), (360.0, 30.0)];

    #[test]
    fn nothing_is_pinned_at_rest() {
        assert_eq!(pinned(&BANDS, 0.0, 30.0), None);
    }

    #[test]
    fn nothing_is_pinned_in_an_empty_list() {
        assert_eq!(pinned(&[], 120.0, 30.0), None);
    }

    #[test]
    fn nothing_is_pinned_above_a_first_header_that_does_not_start_the_list() {
        assert_eq!(pinned(&[(12.0, 30.0)], 12.0, 30.0), None);
    }

    #[test]
    fn the_band_under_the_top_edge_is_pinned_flush() {
        assert_eq!(
            pinned(&BANDS, 50.0, 30.0),
            Some(Pinned {
                index: 0,
                offset: 0.0
            })
        );
    }

    #[test]
    fn the_next_header_pushes_the_pinned_one_up_by_its_overlap() {
        // The second header's top is 10px below the edge, 20px inside the
        // pinned copy's 30.
        assert_eq!(
            pinned(&BANDS, 120.0, 30.0),
            Some(Pinned {
                index: 0,
                offset: -20.0
            })
        );
    }

    #[test]
    fn a_header_exactly_at_the_top_shows_itself_and_has_pushed_the_last_one_out() {
        assert_eq!(
            pinned(&BANDS, 130.0, 30.0),
            Some(Pinned {
                index: 0,
                offset: -30.0
            })
        );
    }

    #[test]
    fn once_past_the_top_the_next_band_takes_the_pin() {
        assert_eq!(
            pinned(&BANDS, 131.0, 30.0),
            Some(Pinned {
                index: 1,
                offset: 0.0
            })
        );
    }

    #[test]
    fn the_last_band_is_never_pushed() {
        assert_eq!(
            pinned(&BANDS, 1000.0, 30.0),
            Some(Pinned {
                index: 2,
                offset: 0.0
            })
        );
    }

    #[test]
    fn the_push_is_measured_against_the_pinned_copys_height_not_the_in_flow_one() {
        // A pinned copy a pixel taller than the in-flow headers (its own top
        // rule) is pushed a pixel sooner.
        assert_eq!(
            pinned(&BANDS, 110.0, 31.0),
            Some(Pinned {
                index: 0,
                offset: -11.0
            })
        );
    }

    #[test]
    fn an_unmeasured_height_pins_without_pushing() {
        // Before layout has reported a height, nothing can push yet — and a
        // negative clamp bound must not panic.
        assert_eq!(
            pinned(&BANDS, 50.0, 0.0),
            Some(Pinned {
                index: 0,
                offset: 0.0
            })
        );
    }
}
