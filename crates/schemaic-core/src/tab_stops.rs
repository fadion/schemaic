//! **A snippet's tab stops** — its `:name` parameters, walked with Tab once
//! the snippet is in the editor.
//!
//! No new syntax: a stop is a parameter `params::scan` finds in the body, so
//! the one template mechanism a snippet has (`snippet`'s module doc) gains a
//! way to be filled in from the keyboard rather than a second mechanism
//! beside it. A stop the user does not fill stays what it was, a `:name`
//! parameter the parameters bar asks for at run time.
//!
//! The editor asks this for what to select; it never decides. On insert,
//! [`TabStops::new`] takes the body and where it landed, and the first stop is
//! selected. Each edit is carried through [`TabStops::shift`] — the same
//! `text_ops::shift_range` the find bar's in-selection scope follows edits
//! with, so typing over a selected stop makes the stop *what was typed*. Tab
//! moves to the next stop and Shift+Tab to the previous; Tab past the last
//! puts the caret at the end of the snippet and ends the walk. A name used
//! twice is one stop, at its first use: both take the same value at run time,
//! so there is only one thing to fill in.

use crate::intel::SqlDialect;
use crate::text_ops::shift_range;

/// The stops of one inserted snippet, as byte ranges into the editor's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabStops {
    stops: Vec<(usize, usize)>,
    current: usize,
    /// Just past the snippet, where Tab past the last stop leaves the caret.
    end: usize,
}

/// Where a Tab or Shift+Tab sends the caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Select this range — the stop moved to.
    Select(usize, usize),
    /// Put the caret here and end the walk — Tab past the last stop.
    Finish(usize),
}

impl TabStops {
    /// The stops of `body`, inserted at byte `at` — `None` when it has no
    /// parameters, so nothing is walked and Tab indents as ever.
    pub fn new(body: &str, at: usize, dialect: SqlDialect) -> Option<TabStops> {
        let mut seen: Vec<String> = Vec::new();
        let mut stops = Vec::new();
        for p in crate::params::scan(body, dialect) {
            // Exactly, as `params::names` keys them: `:id` and `:ID` are two
            // parameters to the bar, so two stops here.
            if seen.contains(&p.name) {
                continue;
            }
            seen.push(p.name);
            stops.push((at + p.start, at + p.end));
        }
        (!stops.is_empty()).then_some(TabStops {
            stops,
            current: 0,
            end: at + body.len(),
        })
    }

    /// The stop to select now.
    pub fn current(&self) -> (usize, usize) {
        self.stops[self.current]
    }

    /// Carry every stop, and the snippet's end, through one edit:
    /// `start..end` of the old text replaced by `inserted` bytes.
    pub fn shift(&mut self, start: usize, end: usize, inserted: usize) {
        for s in &mut self.stops {
            *s = shift_range(*s, start, end, inserted);
        }
        self.end = shift_range((self.end, self.end), start, end, inserted).1;
    }

    /// Tab: the next stop, or past the last one the snippet's end.
    pub fn forward(&mut self) -> Step {
        if self.current + 1 < self.stops.len() {
            self.current += 1;
            let (a, b) = self.current();
            Step::Select(a, b)
        } else {
            Step::Finish(self.end)
        }
    }

    /// Shift+Tab: the previous stop, staying on the first.
    pub fn back(&mut self) -> Step {
        self.current = self.current.saturating_sub(1);
        let (a, b) = self.current();
        Step::Select(a, b)
    }

    /// Is the caret at `offset` still in the walk — inside or at either edge
    /// of a stop? Typing goes on inside one; the caret anywhere else (a click
    /// away, an arrow out) ends it.
    pub fn holds(&self, offset: usize) -> bool {
        self.stops.iter().any(|&(a, b)| a <= offset && offset <= b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG: SqlDialect = SqlDialect::Postgres;

    #[test]
    fn a_body_without_parameters_has_no_stops() {
        assert_eq!(TabStops::new("SELECT 1", 0, PG), None);
        // A parameter-shaped string or comment is not one.
        assert_eq!(TabStops::new("SELECT ':x' -- :y", 0, PG), None);
        // Nor is a cast.
        assert_eq!(TabStops::new("SELECT 1::int", 0, PG), None);
    }

    #[test]
    fn the_stops_are_the_parameters_where_the_body_landed() {
        let body = "SELECT * FROM t WHERE a = :a AND b = :b";
        let s = TabStops::new(body, 10, PG).unwrap();
        let a = body.find(":a").unwrap() + 10;
        assert_eq!(s.current(), (a, a + 2));
        assert!(s.holds(a) && s.holds(a + 2));
        assert!(!s.holds(10));
    }

    #[test]
    fn tab_walks_forward_and_past_the_last_finishes_at_the_snippets_end() {
        let body = "x = :a, y = :b";
        let mut s = TabStops::new(body, 0, PG).unwrap();
        let b = body.find(":b").unwrap();
        assert_eq!(s.forward(), Step::Select(b, b + 2));
        assert_eq!(s.forward(), Step::Finish(body.len()));
        assert_eq!(s.back(), Step::Select(4, 6));
        assert_eq!(s.back(), Step::Select(4, 6), "the first stays the first");
    }

    #[test]
    fn typing_over_a_stop_makes_the_stop_what_was_typed() {
        let body = "x = :a, y = :b";
        let mut s = TabStops::new(body, 0, PG).unwrap();
        // `:a` (4..6) replaced by `42`, then `0` appended inside it.
        s.shift(4, 6, 2);
        assert_eq!(s.current(), (4, 6));
        s.shift(6, 6, 1);
        assert_eq!(s.current(), (4, 7), "typed at the stop's end grows it");
        // The later stop and the end moved by the net change, +1.
        let b = body.find(":b").unwrap() + 1;
        assert_eq!(s.forward(), Step::Select(b, b + 2));
        assert_eq!(s.forward(), Step::Finish(body.len() + 1));
    }

    #[test]
    fn an_edit_before_the_snippet_moves_every_stop() {
        let mut s = TabStops::new(":a", 5, PG).unwrap();
        s.shift(0, 0, 3);
        assert_eq!(s.current(), (8, 10));
        assert_eq!(s.forward(), Step::Finish(10));
    }

    #[test]
    fn a_name_used_twice_is_one_stop() {
        let mut s = TabStops::new("a = :id OR b = :id OR c = :ID", 0, PG).unwrap();
        assert_eq!(s.current(), (4, 7));
        // `:ID` is another parameter to the bar, so its own stop.
        let upper = "a = :id OR b = :id OR c = ".len();
        assert_eq!(s.forward(), Step::Select(upper, upper + 3));
    }
}
