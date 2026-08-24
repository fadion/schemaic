//! Civil dates, clock times, and the month grid a date picker draws — the pure
//! half of the grid's date/datetime cell editor.
//!
//! A date arrives from every engine as **text** ([`crate::model::Value::Str`]),
//! and it goes back the same way, so nothing here converts to or from a database
//! type. What it does is read that text into parts a calendar can point at, and
//! write the parts back **keeping everything the user did not touch**: the
//! fractional seconds, the timezone offset, and whether the source separated
//! date from time with a space or a `T`. Picking a day out of a
//! `2024-01-15 10:30:00.123456+02` must not quietly become
//! `2024-01-16 10:30:00` — that is the same class of silent rewrite
//! [`crate::jsontree`] keeps a number's source text for.
//!
//! **A date that does not parse is not a date.** MySQL's zero date
//! (`0000-00-00`) and a `TIME` column's duration (`838:59:59`) both fail here on
//! purpose: the caller ([`crate::celledit::fits`]) reads that as "this value does
//! not fit a calendar" and leaves the cell on its plain text editor, which is the
//! only honest thing to offer for a value no picker can represent.
//!
//! The arithmetic is Howard Hinnant's public-domain `days_from_civil` /
//! `civil_from_days` pair — proleptic Gregorian, valid for any year — and every
//! other question ([`Date::weekday`], [`Date::add_months`], [`month_cells`]) is
//! answered through it rather than by a second rule.

/// A calendar date, with no timezone and no time of day.
///
/// Not validated by construction — [`Date::new`] is the checked constructor, and
/// it is what every parse goes through, so a `Date` that reached a caller has a
/// real month and a day that exists in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    /// 1–12.
    pub month: u32,
    /// 1–[`days_in_month`].
    pub day: u32,
}

/// A time of day. Seconds only — no engine's grid value carries anything finer
/// than the fractional part [`Stamp`] keeps verbatim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Time {
    /// 0–23. A `TIME` column may hold `838:59:59`, which is a *duration* and is
    /// deliberately rejected — see the module doc.
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// A timestamp's text, split into the parts a picker edits and the parts it must
/// preserve byte for byte.
///
/// [`Stamp::render`] is [`Stamp::parse`]'s inverse for everything but the
/// spelling of the date and time fields themselves, which come back zero-padded
/// and canonical (`2024-1-5` → `2024-01-05`) — a normalisation every engine would
/// perform on the write anyway.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    date: Date,
    time: Option<Time>,
    /// The byte between date and time as the source wrote it: `' '` or `'T'`.
    sep: char,
    /// Fractional seconds **including** the leading dot (`".123456"`), or empty.
    frac: String,
    /// Everything after the seconds: a timezone offset (`"+02"`, `"Z"`), or empty.
    tail: String,
}

/// Is `year` a leap year in the proleptic Gregorian calendar?
pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// How many days month `month` (1–12) has in `year`. Zero for a month outside
/// 1–12, so a caller that failed to validate gets an empty range rather than a
/// plausible-looking 31.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Full month names, indexed 0–11 (January first).
pub const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The weekday column headings a month grid draws, **Monday first** — the ISO
/// week, which is what [`Date::weekday`] numbers and what [`month_cells`] lays
/// out. One rule for both, since a grid whose headings and cells disagree about
/// where the week starts is off by a column and looks right.
pub const WEEKDAY_INITIALS: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

/// The full month name for 1–12, or an empty string.
pub fn month_name(month: u32) -> &'static str {
    MONTH_NAMES
        .get(month.wrapping_sub(1) as usize)
        .copied()
        .unwrap_or("")
}

/// Days since 1970-01-01 for a civil date. Howard Hinnant's `days_from_civil`
/// (public domain), proleptic Gregorian, valid for any year.
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if month > 2 { month - 3 } else { month + 9 } as i64; // [0, 11]
    let doy = (153 * mp + 2) / 5 + day as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Convert a count of days since 1970-01-01 into `(year, month, day)`.
/// Howard Hinnant's `civil_from_days` (public domain), valid for any date.
///
/// The inverse of [`days_from_civil`]; the two are the only date arithmetic in
/// the workspace, and [`crate::format`]'s epoch formatter reads this one too.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

impl Date {
    /// The checked constructor: `None` unless the month is 1–12 and the day
    /// exists in it (so `2023-02-29` and MySQL's `0000-00-00` are both `None`).
    pub fn new(year: i32, month: u32, day: u32) -> Option<Date> {
        (day >= 1 && day <= days_in_month(year, month)).then_some(Date { year, month, day })
    }

    /// Days since 1970-01-01.
    pub fn to_days(self) -> i64 {
        days_from_civil(self.year as i64, self.month, self.day)
    }

    /// The date `days` days after 1970-01-01. Years outside `i32` saturate, which
    /// only a caller doing arithmetic on nonsense can reach.
    pub fn from_days(days: i64) -> Date {
        let (y, m, d) = civil_from_days(days);
        Date {
            year: y.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            month: m,
            day: d,
        }
    }

    /// Day of the week, **0 = Monday** … 6 = Sunday — the index
    /// [`WEEKDAY_INITIALS`] is written in.
    pub fn weekday(self) -> u32 {
        // 1970-01-01 was a Thursday, which is index 3 in a Monday-first week.
        (self.to_days() + 3).rem_euclid(7) as u32
    }

    /// This date `n` days later (`n` may be negative).
    pub fn add_days(self, n: i64) -> Date {
        Date::from_days(self.to_days() + n)
    }

    /// This date `n` months later (`n` may be negative), **clamping the day** to
    /// the target month's length: 2024-01-31 plus one month is 2024-02-29, not
    /// the 2024-03-02 that adding 31 days would give. That is what a calendar's
    /// next-month button means, and it is why this isn't [`Date::add_days`] with
    /// arithmetic in the caller.
    pub fn add_months(self, n: i32) -> Date {
        let total = self.year as i64 * 12 + (self.month as i64 - 1) + n as i64;
        let year = total.div_euclid(12).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let month = total.rem_euclid(12) as u32 + 1;
        Date {
            year,
            month,
            day: self.day.min(days_in_month(year, month)),
        }
    }

    /// `YYYY-MM-DD`. Years are padded to four digits (a year past 9999 simply
    /// prints wider, as every engine does).
    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Read a whole string as `YYYY-MM-DD` (1–6 digit year, 1–2 digit month and
    /// day accepted, surrounding whitespace ignored). `None` for anything with
    /// trailing text — [`Stamp::parse`] is the reader for a value that carries a
    /// time as well.
    pub fn parse(text: &str) -> Option<Date> {
        let (date, rest) = take_date(text.trim())?;
        rest.is_empty().then_some(date)
    }

    /// Today's date on the **local** clock.
    ///
    /// Local, not UTC, because this answers "which day is today" for a person
    /// looking at a calendar: from UTC+2, everything between midnight and 02:00
    /// is a different day in the two frames, and a picker that highlights
    /// yesterday is wrong at exactly the hours somebody is most likely to be
    /// looking. It is the one impure function in this module.
    pub fn today() -> Date {
        let now = chrono::Local::now();
        let (y, m, d) = chrono_ymd(&now);
        Date {
            year: y,
            month: m,
            day: d,
        }
    }
}

impl Time {
    /// The checked constructor: `None` outside 0–23 : 0–59 : 0–60 (a leap second
    /// is a real value a server can hand back).
    pub fn new(hour: u32, minute: u32, second: u32) -> Option<Time> {
        (hour <= 23 && minute <= 59 && second <= 60).then_some(Time {
            hour,
            minute,
            second,
        })
    }

    /// `HH:MM:SS`.
    pub fn hms(self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }

    /// Read a whole string as `HH:MM[:SS]`. `None` for a duration (`838:59:59`)
    /// or anything with trailing text.
    pub fn parse(text: &str) -> Option<Time> {
        let (time, rest) = take_time(text.trim())?;
        rest.is_empty().then_some(time)
    }

    /// The current time of day on the **local** clock — see [`Date::today`].
    pub fn now() -> Time {
        let now = chrono::Local::now();
        let (h, m, s) = chrono_hms(&now);
        Time {
            hour: h,
            minute: m,
            second: s,
        }
    }

    /// Midnight — what a date-only value means once a column wants a time too.
    pub const MIDNIGHT: Time = Time {
        hour: 0,
        minute: 0,
        second: 0,
    };
}

/// `(year, month, day)` off a chrono timestamp, through its `Datelike` trait so
/// nothing else in the workspace has to import it.
fn chrono_ymd(now: &chrono::DateTime<chrono::Local>) -> (i32, u32, u32) {
    use chrono::Datelike;
    (now.year(), now.month(), now.day())
}

/// `(hour, minute, second)` off a chrono timestamp — the [`chrono_ymd`] pair.
fn chrono_hms(now: &chrono::DateTime<chrono::Local>) -> (u32, u32, u32) {
    use chrono::Timelike;
    (now.hour(), now.minute(), now.second())
}

impl Stamp {
    /// Split a timestamp's text into date, time, and the parts to preserve.
    ///
    /// `None` unless the text *starts* with a real date, and — when it carries
    /// anything after that date — unless the next byte is a space or `T` followed
    /// by a real time. Everything past the seconds is kept as [`Stamp::tail`]
    /// without being understood, which is how a `timestamptz` offset survives an
    /// edit to the day.
    pub fn parse(text: &str) -> Option<Stamp> {
        let s = text.trim();
        let (date, rest) = take_date(s)?;
        if rest.is_empty() {
            return Some(Stamp {
                date,
                time: None,
                sep: ' ',
                frac: String::new(),
                tail: String::new(),
            });
        }
        let sep = rest.chars().next()?;
        if sep != ' ' && sep != 'T' && sep != 't' {
            return None;
        }
        let (time, rest) = take_time(&rest[sep.len_utf8()..])?;
        let (frac, rest) = take_frac(rest);
        Some(Stamp {
            date,
            time: Some(time),
            sep,
            frac: frac.to_string(),
            tail: rest.to_string(),
        })
    }

    /// A date-only stamp, for a value there was nothing to parse in.
    pub fn from_date(date: Date) -> Stamp {
        Stamp {
            date,
            time: None,
            sep: ' ',
            frac: String::new(),
            tail: String::new(),
        }
    }

    pub fn date(&self) -> Date {
        self.date
    }

    pub fn time(&self) -> Option<Time> {
        self.time
    }

    /// The same stamp on another day; the time, the fraction and the timezone
    /// tail are untouched.
    pub fn with_date(mut self, date: Date) -> Stamp {
        self.date = date;
        self
    }

    /// The same stamp at another time of day. A stamp that had no time gains one
    /// (space-separated, the spelling every engine accepts); the fraction is
    /// dropped only when it was never there to begin with.
    pub fn with_time(mut self, time: Time) -> Stamp {
        self.time = Some(time);
        self
    }

    /// Drop the time of day, leaving a bare `YYYY-MM-DD`. The fraction and the
    /// timezone tail go with it — they qualify a time that is no longer there.
    pub fn without_time(mut self) -> Stamp {
        self.time = None;
        self.frac.clear();
        self.tail.clear();
        self
    }

    /// The text to write back: the canonical date (and time) plus whatever the
    /// source carried after it.
    pub fn render(&self) -> String {
        let mut out = self.date.iso();
        if let Some(t) = self.time {
            out.push(self.sep);
            out.push_str(&t.hms());
            out.push_str(&self.frac);
        }
        out.push_str(&self.tail);
        out
    }
}

/// The six-week grid a month is drawn in: 42 dates starting at the Monday on or
/// before the 1st, running through whatever falls in the last row.
///
/// **Always 42**, never trimmed to the weeks the month occupies, so the panel is
/// the same height in February as in a 31-day month starting on a Sunday —
/// a picker that changes size as you page through the year is the thing this
/// costs one extra row to avoid. Dates outside the month are still real dates
/// (the caller dims them and may let them be picked); ask
/// `d.month != month` to tell them apart.
///
/// An out-of-range `month` yields an empty grid rather than a guess.
pub fn month_cells(year: i32, month: u32) -> Vec<Date> {
    let Some(first) = Date::new(year, month, 1) else {
        return Vec::new();
    };
    let start = first.to_days() - first.weekday() as i64;
    (0..42).map(|i| Date::from_days(start + i)).collect()
}

// ── Scanners ────────────────────────────────────────────────────────────────
//
// Each takes the rest of the string and returns what it read plus what is left,
// so `Stamp::parse` is a walk rather than a set of overlapping slices.

/// Leading ASCII digits, at most `max` of them: `(value, rest)`, or `None` when
/// there isn't at least one.
fn take_digits(s: &str, max: usize) -> Option<(u32, &str)> {
    let end = s.bytes().take(max).take_while(u8::is_ascii_digit).count();
    if end == 0 {
        return None;
    }
    Some((s[..end].parse().ok()?, &s[end..]))
}

/// A leading `YYYY-MM-DD`, validated through [`Date::new`].
fn take_date(s: &str) -> Option<(Date, &str)> {
    let (year, rest) = take_digits(s, 6)?;
    let rest = rest.strip_prefix('-')?;
    let (month, rest) = take_digits(rest, 2)?;
    let rest = rest.strip_prefix('-')?;
    let (day, rest) = take_digits(rest, 2)?;
    Some((Date::new(year as i32, month, day)?, rest))
}

/// A leading `HH:MM[:SS]`, validated through [`Time::new`].
fn take_time(s: &str) -> Option<(Time, &str)> {
    let (hour, rest) = take_digits(s, 2)?;
    let rest = rest.strip_prefix(':')?;
    let (minute, rest) = take_digits(rest, 2)?;
    let (second, rest) = match rest.strip_prefix(':') {
        Some(r) => take_digits(r, 2)?,
        None => (0, rest),
    };
    Some((Time::new(hour, minute, second)?, rest))
}

/// A leading `.` plus digits, returned **with** the dot (`(".123", rest)`), or
/// `("", s)` when there is none.
fn take_frac(s: &str) -> (&str, &str) {
    let Some(digits) = s.strip_prefix('.') else {
        return ("", s);
    };
    let n = digits.bytes().take_while(u8::is_ascii_digit).count();
    if n == 0 {
        return ("", s);
    }
    s.split_at(n + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::new(y, m, day).expect("valid date")
    }

    // ── Arithmetic ──────────────────────────────────────────────────────────

    #[test]
    fn leap_years_follow_the_gregorian_rule() {
        assert!(is_leap(2024));
        assert!(!is_leap(2023));
        assert!(!is_leap(1900), "a century is not a leap year");
        assert!(is_leap(2000), "unless it divides by 400");
    }

    #[test]
    fn february_grows_in_a_leap_year() {
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 4), 30);
    }

    #[test]
    fn a_month_outside_the_year_has_no_days() {
        assert_eq!(days_in_month(2024, 0), 0);
        assert_eq!(days_in_month(2024, 13), 0);
    }

    #[test]
    fn the_epoch_is_day_zero_and_a_thursday() {
        assert_eq!(d(1970, 1, 1).to_days(), 0);
        assert_eq!(d(1970, 1, 1).weekday(), 3);
    }

    /// The two halves of the civil arithmetic are each other's inverse, which is
    /// the property everything else here leans on.
    #[test]
    fn days_and_civil_dates_round_trip_over_four_centuries() {
        for days in (-100_000..100_000).step_by(7) {
            let (y, m, dd) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, dd), days, "day {days}");
        }
    }

    #[test]
    fn weekdays_advance_one_per_day_and_wrap_at_sunday() {
        // 2024-01-01 was a Monday.
        let mon = d(2024, 1, 1);
        assert_eq!(mon.weekday(), 0);
        assert_eq!(mon.add_days(1).weekday(), 1);
        assert_eq!(mon.add_days(6).weekday(), 6, "Sunday");
        assert_eq!(mon.add_days(7).weekday(), 0);
    }

    #[test]
    fn adding_days_crosses_a_month_and_a_year() {
        assert_eq!(d(2024, 1, 31).add_days(1), d(2024, 2, 1));
        assert_eq!(d(2024, 12, 31).add_days(1), d(2025, 1, 1));
        assert_eq!(d(2024, 1, 1).add_days(-1), d(2023, 12, 31));
    }

    #[test]
    fn adding_a_month_clamps_the_day_to_the_shorter_month() {
        assert_eq!(d(2024, 1, 31).add_months(1), d(2024, 2, 29));
        assert_eq!(d(2023, 1, 31).add_months(1), d(2023, 2, 28));
        assert_eq!(d(2024, 3, 31).add_months(-1), d(2024, 2, 29));
    }

    #[test]
    fn adding_months_crosses_the_year_in_both_directions() {
        assert_eq!(d(2024, 12, 15).add_months(1), d(2025, 1, 15));
        assert_eq!(d(2024, 1, 15).add_months(-1), d(2023, 12, 15));
        assert_eq!(d(2024, 6, 15).add_months(-18), d(2022, 12, 15));
    }

    // ── The month grid ──────────────────────────────────────────────────────

    #[test]
    fn a_month_grid_is_always_six_weeks() {
        for (y, m) in [(2024, 2), (2024, 9), (2023, 2), (2021, 8)] {
            assert_eq!(month_cells(y, m).len(), 42, "{y}-{m}");
        }
    }

    #[test]
    fn a_month_grid_starts_on_the_monday_on_or_before_the_first() {
        // 2024-09-01 was a Sunday, so the grid opens on Monday the 26th of August.
        let cells = month_cells(2024, 9);
        assert_eq!(cells[0], d(2024, 8, 26));
        assert_eq!(cells[6], d(2024, 9, 1));
        // Every row starts on a Monday.
        for row in 0..6 {
            assert_eq!(cells[row * 7].weekday(), 0);
        }
    }

    #[test]
    fn a_month_starting_on_a_monday_needs_no_leading_days() {
        // 2024-01-01 was a Monday.
        let cells = month_cells(2024, 1);
        assert_eq!(cells[0], d(2024, 1, 1));
        assert_eq!(cells[41], d(2024, 2, 11), "and the tail runs into February");
    }

    #[test]
    fn a_month_grid_holds_every_day_of_its_month_in_order() {
        let cells = month_cells(2024, 2);
        let own: Vec<u32> = cells
            .iter()
            .filter(|c| c.month == 2 && c.year == 2024)
            .map(|c| c.day)
            .collect();
        assert_eq!(own, (1..=29).collect::<Vec<_>>());
    }

    #[test]
    fn an_impossible_month_yields_no_grid() {
        assert!(month_cells(2024, 0).is_empty());
        assert!(month_cells(2024, 13).is_empty());
    }

    // ── Construction + parsing ──────────────────────────────────────────────

    #[test]
    fn a_day_that_does_not_exist_is_rejected() {
        assert!(Date::new(2023, 2, 29).is_none());
        assert!(Date::new(2024, 2, 29).is_some());
        assert!(Date::new(2024, 4, 31).is_none());
        assert!(Date::new(2024, 13, 1).is_none());
        assert!(Date::new(2024, 1, 0).is_none());
    }

    /// MySQL's zero date is the value a date picker most needs to refuse: there
    /// is no such day to point at, and normalising it to one would rewrite it.
    #[test]
    fn the_mysql_zero_date_does_not_parse() {
        assert!(Date::parse("0000-00-00").is_none());
        assert!(Stamp::parse("0000-00-00 00:00:00").is_none());
    }

    #[test]
    fn a_plain_iso_date_parses_and_prints_back() {
        assert_eq!(Date::parse("2024-01-15"), Some(d(2024, 1, 15)));
        assert_eq!(d(2024, 1, 15).iso(), "2024-01-15");
    }

    #[test]
    fn a_date_parse_accepts_unpadded_fields_and_prints_them_padded() {
        assert_eq!(Date::parse("2024-1-5"), Some(d(2024, 1, 5)));
        assert_eq!(Date::parse(" 2024-01-05 "), Some(d(2024, 1, 5)));
        assert_eq!(d(2024, 1, 5).iso(), "2024-01-05");
    }

    #[test]
    fn text_after_a_date_is_not_a_date() {
        assert!(Date::parse("2024-01-15 10:00:00").is_none());
        assert!(Date::parse("2024-01-15x").is_none());
        assert!(Date::parse("not a date").is_none());
        assert!(Date::parse("").is_none());
    }

    #[test]
    fn a_time_parses_with_and_without_seconds() {
        assert_eq!(Time::parse("10:30:15"), Time::new(10, 30, 15));
        assert_eq!(Time::parse("10:30"), Time::new(10, 30, 0));
        assert_eq!(Time::parse("00:00:00"), Time::new(0, 0, 0));
        assert_eq!(
            Time::new(23, 59, 60).map(Time::hms).as_deref(),
            Some("23:59:60")
        );
    }

    /// A MySQL `TIME` column is a *duration* — `838:59:59` and `-01:00:00` are
    /// both legal values and neither is a time of day.
    #[test]
    fn a_duration_is_not_a_time_of_day() {
        assert!(Time::parse("838:59:59").is_none());
        assert!(Time::parse("-01:00:00").is_none());
        assert!(Time::parse("24:00:00").is_none());
        assert!(Time::parse("10:60:00").is_none());
    }

    // ── Stamps: what a picked date must not disturb ─────────────────────────

    #[test]
    fn a_datetime_splits_into_date_and_time() {
        let s = Stamp::parse("2024-01-15 10:30:00").expect("parses");
        assert_eq!(s.date(), d(2024, 1, 15));
        assert_eq!(s.time(), Time::new(10, 30, 0));
        assert_eq!(s.render(), "2024-01-15 10:30:00");
    }

    #[test]
    fn a_date_only_stamp_has_no_time() {
        let s = Stamp::parse("2024-01-15").expect("parses");
        assert_eq!(s.time(), None);
        assert_eq!(s.render(), "2024-01-15");
    }

    /// The point of the type: everything the picker doesn't edit survives it.
    #[test]
    fn picking_a_day_keeps_the_fraction_the_offset_and_the_separator() {
        let s = Stamp::parse("2024-01-15T10:30:00.123456+02:00").expect("parses");
        assert_eq!(s.date(), d(2024, 1, 15));
        assert_eq!(s.time(), Time::new(10, 30, 0));
        assert_eq!(
            s.with_date(d(2024, 3, 1)).render(),
            "2024-03-01T10:30:00.123456+02:00"
        );
    }

    #[test]
    fn setting_the_time_keeps_the_date_and_the_offset() {
        let s = Stamp::parse("2024-01-15 10:30:00+02").expect("parses");
        assert_eq!(
            s.with_time(Time::new(23, 5, 9).unwrap()).render(),
            "2024-01-15 23:05:09+02"
        );
    }

    #[test]
    fn a_zulu_suffix_is_kept_verbatim() {
        let s = Stamp::parse("2024-01-15T10:30:00Z").expect("parses");
        assert_eq!(s.with_date(d(2024, 1, 16)).render(), "2024-01-16T10:30:00Z");
    }

    #[test]
    fn a_date_only_value_gains_a_space_separated_time() {
        let s = Stamp::parse("2024-01-15").expect("parses");
        assert_eq!(s.with_time(Time::MIDNIGHT).render(), "2024-01-15 00:00:00");
    }

    #[test]
    fn dropping_the_time_drops_what_qualified_it() {
        let s = Stamp::parse("2024-01-15 10:30:00.5+02").expect("parses");
        assert_eq!(s.without_time().render(), "2024-01-15");
    }

    #[test]
    fn a_stamp_re_renders_a_value_it_did_not_change() {
        for src in [
            "2024-01-15",
            "2024-01-15 10:30:00",
            "2024-01-15T10:30:00",
            "2024-01-15 10:30:00.123",
            "2024-01-15 10:30:00.123456+02:00",
            "2024-01-15T10:30:00Z",
            "2024-01-15 10:30",
        ] {
            let s = Stamp::parse(src).unwrap_or_else(|| panic!("{src} parses"));
            let back = s.render();
            // `10:30` gains its seconds — canonical, and what the engine stores.
            let expected = if src.ends_with("10:30") {
                "2024-01-15 10:30:00"
            } else {
                src
            };
            assert_eq!(back, expected, "{src}");
        }
    }

    #[test]
    fn a_value_that_is_not_a_timestamp_does_not_parse() {
        assert!(Stamp::parse("hello").is_none());
        assert!(Stamp::parse("").is_none());
        assert!(Stamp::parse("2024-01-15x10:30:00").is_none());
        assert!(Stamp::parse("2024-01-15 25:00:00").is_none());
        assert!(Stamp::parse("2024-01-15 BC").is_none());
    }

    #[test]
    fn a_bare_date_stamp_can_be_built_for_an_empty_cell() {
        assert_eq!(Stamp::from_date(d(2024, 6, 1)).render(), "2024-06-01");
    }

    // ── The clock ───────────────────────────────────────────────────────────

    /// Not "is it the right day" — nothing pure can check that — but that the
    /// local clock produces a date the rest of this module accepts.
    #[test]
    fn today_is_a_real_date() {
        let t = Date::today();
        assert_eq!(Date::new(t.year, t.month, t.day), Some(t));
        assert!(t.year >= 2024, "the clock is at least this build's era");
        let n = Time::now();
        assert_eq!(Time::new(n.hour, n.minute, n.second), Some(n));
    }
}
