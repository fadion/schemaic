//! Per-column **display formatters** for the results grid — display-only
//! transforms that never touch the stored value (inline edit and copy/export
//! still operate on the raw value; only the rendered cell text changes).
//!
//! The headline transform is epoch-int → readable datetime (which DataGrip can't
//! do, since it formats by column *type* and an epoch is just a `BIGINT`); plus
//! file-size and boolean glyphs. A choice is a [`ColumnFormat`], persisted per
//! `(connection, database, table, column)` as a [`ColumnFormatRule`] in
//! `format.json`. Pure + tested; the grid applies [`apply`] in the cell's display
//! and the app owns the rule store.

use serde::{Deserialize, Serialize};

use crate::model::Value;

/// A display transform for one column's cell values.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnFormat {
    /// No transform — render the raw value (also used to clear a formatter).
    #[default]
    None,
    /// Unix epoch integer → `YYYY-MM-DD HH:MM:SS` (UTC). The unit (seconds /
    /// milliseconds / microseconds / nanoseconds) is auto-detected by magnitude.
    Timestamp,
    /// Thousands separators (`1234567` → `1,234,567`), preserving sign + decimals.
    Grouped,
    /// Byte count → human-readable size (`1.5 MB`).
    Bytes,
    /// Truthiness → `true` / `false` (0 / empty / `false` are false).
    Bool,
}

impl ColumnFormat {
    /// Menu label for this format.
    pub fn label(self) -> &'static str {
        match self {
            ColumnFormat::None => "None",
            ColumnFormat::Timestamp => "Timestamp",
            ColumnFormat::Grouped => "Number grouping",
            ColumnFormat::Bytes => "File size",
            ColumnFormat::Bool => "Boolean",
        }
    }

    /// The formats offered in the header menu, in order (includes `None` to clear).
    pub const MENU: [ColumnFormat; 5] = [
        ColumnFormat::None,
        ColumnFormat::Timestamp,
        ColumnFormat::Grouped,
        ColumnFormat::Bytes,
        ColumnFormat::Bool,
    ];
}

/// A persisted per-column formatter choice, keyed by the connection + the real
/// base table/column it applies to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnFormatRule {
    pub conn_id: u64,
    pub database: String,
    pub table: String,
    pub column: String,
    pub format: ColumnFormat,
}

/// The persisted formatter file (`format.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FormatsFile {
    #[serde(default)]
    pub rules: Vec<ColumnFormatRule>,
}

/// The explicitly-stored format for a column, or `None` if the user has never set
/// one (in which case the caller falls back to [`smart_default`]). An explicit
/// `Some(ColumnFormat::None)` means the user deliberately chose "raw", overriding
/// any smart default — distinct from "no rule".
pub fn lookup(
    rules: &[ColumnFormatRule],
    conn_id: u64,
    database: &str,
    table: &str,
    column: &str,
) -> Option<ColumnFormat> {
    rules
        .iter()
        .find(|r| {
            r.conn_id == conn_id && r.database == database && r.table == table && r.column == column
        })
        .map(|r| r.format)
}

/// Record a column's explicit format choice: drop any existing rule for that key,
/// then store the new one. The chosen format is stored **as-is including
/// [`ColumnFormat::None`]**, so an explicit "None" persists as an override of a
/// [`smart_default`] rather than reverting to it.
pub fn upsert(
    rules: &mut Vec<ColumnFormatRule>,
    conn_id: u64,
    database: &str,
    table: &str,
    column: &str,
    format: ColumnFormat,
) {
    rules.retain(|r| {
        !(r.conn_id == conn_id && r.database == database && r.table == table && r.column == column)
    });
    rules.push(ColumnFormatRule {
        conn_id,
        database: database.to_string(),
        table: table.to_string(),
        column: column.to_string(),
        format,
    });
}

/// Render a cell value under a format. NULLs always render as `NULL` (unformatted)
/// and any value that doesn't fit the format falls back to its raw display, so a
/// formatter can never hide or corrupt data.
pub fn apply(format: ColumnFormat, v: &Value) -> String {
    if v.is_null() {
        return v.display();
    }
    match format {
        ColumnFormat::None => v.display(),
        ColumnFormat::Timestamp => as_i64(v)
            .map(|n| fmt_epoch_secs(epoch_to_seconds(n)))
            .unwrap_or_else(|| v.display()),
        ColumnFormat::Grouped => group_number(&v.display()).unwrap_or_else(|| v.display()),
        ColumnFormat::Bytes => as_i64(v).map(human_bytes).unwrap_or_else(|| v.display()),
        ColumnFormat::Bool => bool_glyph(v),
    }
}

/// Add thousands separators to a numeric string's integer part, preserving an
/// optional leading sign and decimal fraction. Returns `None` for anything that
/// isn't a plain number (so it falls back to raw).
fn group_number(s: &str) -> Option<String> {
    let t = s.trim();
    let (sign, rest) = match t.strip_prefix('-') {
        Some(r) => ("-", r),
        None => ("", t),
    };
    let (int_part, frac) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if let Some(f) = frac
        && !f.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let n = int_part.len();
    let mut grouped = String::with_capacity(n + n / 3);
    for (i, b) in int_part.bytes().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(b as char);
    }
    Some(match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    })
}

/// A suggested default formatter for a column that has no explicit rule yet:
/// auto-apply [`ColumnFormat::Timestamp`] to an **integer** column whose name
/// looks like a unix-time field (`*_at` / `*_time` / `*_ts` / `epoch` /
/// `timestamp`). The integer-type gate keeps it off real `DATE`/`DATETIME`
/// columns (which aren't epochs). Anything else defaults to raw.
pub fn smart_default(column: &str, type_name: &str) -> ColumnFormat {
    let is_int = type_name.to_ascii_uppercase().contains("INT");
    let c = column.to_ascii_lowercase();
    let looks_time = c.ends_with("_at")
        || c.ends_with("_time")
        || c.ends_with("_ts")
        || c.contains("epoch")
        || c == "timestamp";
    if is_int && looks_time {
        ColumnFormat::Timestamp
    } else {
        ColumnFormat::None
    }
}

/// Normalize a unix-epoch integer to **seconds**, auto-detecting the unit by
/// magnitude: seconds (`< 1e12`), milliseconds (`< 1e15`), microseconds
/// (`< 1e18`), else nanoseconds. Seconds only reach 1e12 in the year ~33000, so
/// modern timestamps never collide across buckets.
fn epoch_to_seconds(n: i64) -> i64 {
    let mag = n.unsigned_abs();
    if mag >= 1_000_000_000_000_000_000 {
        n / 1_000_000_000 // nanoseconds
    } else if mag >= 1_000_000_000_000_000 {
        n / 1_000_000 // microseconds
    } else if mag >= 1_000_000_000_000 {
        n / 1_000 // milliseconds
    } else {
        n // seconds
    }
}

/// Best-effort integer view of a value (parses a numeric string; truncates floats).
fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::UInt(u) => i64::try_from(*u).ok(),
        Value::Float(f) => Some(*f as i64),
        Value::Str(s) => s.trim().parse::<i64>().ok(),
        Value::Null => None,
    }
}

/// `YYYY-MM-DD HH:MM:SS` (UTC) for a unix-epoch second count.
fn fmt_epoch_secs(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (h, m, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}")
}

/// Convert a count of days since 1970-01-01 into `(year, month, day)`.
/// Howard Hinnant's `civil_from_days` (public-domain), valid for any date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
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

/// Human-readable byte size (`1.5 MB`); negatives fall back to the plain number.
fn human_bytes(n: i64) -> String {
    if n < 0 {
        return n.to_string();
    }
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// `true` / `false` for a value's truthiness (0 / empty / `false` are false).
fn bool_glyph(v: &Value) -> String {
    let falsey = match v {
        Value::Null => return v.display(),
        Value::Int(0) | Value::UInt(0) => true,
        Value::Int(_) | Value::UInt(_) => false,
        Value::Float(f) => *f == 0.0,
        Value::Str(s) => matches!(
            s.trim(),
            "" | "0" | "false" | "FALSE" | "False" | "no" | "NO" | "No"
        ),
    };
    if falsey {
        "false".to_string()
    } else {
        "true".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_seconds_to_utc_datetime() {
        // Epoch 0 → the unix epoch itself.
        assert_eq!(
            apply(ColumnFormat::Timestamp, &Value::Int(0)),
            "1970-01-01 00:00:00"
        );
        // 2021-01-01 00:00:00 UTC = 1609459200.
        assert_eq!(
            apply(ColumnFormat::Timestamp, &Value::Int(1_609_459_200)),
            "2021-01-01 00:00:00"
        );
        // A numeric string parses the same way.
        assert_eq!(
            apply(ColumnFormat::Timestamp, &Value::Str("1609459200".into())),
            "2021-01-01 00:00:00"
        );
    }

    #[test]
    fn timestamp_auto_detects_unit_by_magnitude() {
        // The same instant expressed in s / ms / µs / ns all format identically.
        assert_eq!(
            apply(ColumnFormat::Timestamp, &Value::Int(1_609_459_200)),
            "2021-01-01 00:00:00"
        );
        assert_eq!(
            apply(ColumnFormat::Timestamp, &Value::Int(1_609_459_200_000)),
            "2021-01-01 00:00:00"
        );
        assert_eq!(
            apply(ColumnFormat::Timestamp, &Value::Int(1_609_459_200_000_000)),
            "2021-01-01 00:00:00"
        );
        assert_eq!(
            apply(
                ColumnFormat::Timestamp,
                &Value::Int(1_609_459_200_000_000_000)
            ),
            "2021-01-01 00:00:00"
        );
    }

    #[test]
    fn bytes_are_human_readable() {
        assert_eq!(apply(ColumnFormat::Bytes, &Value::Int(512)), "512 B");
        assert_eq!(apply(ColumnFormat::Bytes, &Value::Int(1536)), "1.5 KB");
        assert_eq!(
            apply(ColumnFormat::Bytes, &Value::Int(5 * 1024 * 1024)),
            "5.0 MB"
        );
    }

    #[test]
    fn bool_glyphs() {
        assert_eq!(apply(ColumnFormat::Bool, &Value::Int(0)), "false");
        assert_eq!(apply(ColumnFormat::Bool, &Value::Int(1)), "true");
        assert_eq!(
            apply(ColumnFormat::Bool, &Value::Str("false".into())),
            "false"
        );
        assert_eq!(apply(ColumnFormat::Bool, &Value::Str("yes".into())), "true");
    }

    #[test]
    fn null_and_unparseable_fall_back_to_raw() {
        assert_eq!(apply(ColumnFormat::Timestamp, &Value::Null), "NULL");
        // Non-numeric string can't be an epoch → raw display.
        assert_eq!(
            apply(ColumnFormat::Timestamp, &Value::Str("hello".into())),
            "hello"
        );
    }

    #[test]
    fn grouping_adds_thousands_separators() {
        assert_eq!(
            apply(ColumnFormat::Grouped, &Value::Int(1_234_567)),
            "1,234,567"
        );
        assert_eq!(apply(ColumnFormat::Grouped, &Value::Int(-1_000)), "-1,000");
        assert_eq!(apply(ColumnFormat::Grouped, &Value::Int(999)), "999");
        // Decimal fraction preserved, only the integer part grouped.
        assert_eq!(
            apply(ColumnFormat::Grouped, &Value::Str("1234567.89".into())),
            "1,234,567.89"
        );
        // Non-numeric → raw.
        assert_eq!(
            apply(ColumnFormat::Grouped, &Value::Str("abc".into())),
            "abc"
        );
    }

    #[test]
    fn smart_default_only_for_int_timestamp_names() {
        // Integer columns named like time fields → Timestamp.
        assert_eq!(
            smart_default("created_at", "BIGINT"),
            ColumnFormat::Timestamp
        );
        assert_eq!(
            smart_default("update_time", "INT UNSIGNED"),
            ColumnFormat::Timestamp
        );
        assert_eq!(smart_default("event_ts", "bigint"), ColumnFormat::Timestamp);
        // A real DATETIME column of the same name is NOT touched (not an epoch int).
        assert_eq!(smart_default("created_at", "DATETIME"), ColumnFormat::None);
        // Non-time integer column → raw.
        assert_eq!(smart_default("quantity", "INT"), ColumnFormat::None);
    }

    #[test]
    fn lookup_and_upsert_roundtrip() {
        let mut rules = Vec::new();
        // No rule yet → None (caller then applies a smart default).
        assert_eq!(lookup(&rules, 1, "db", "t", "created_at"), None);
        upsert(
            &mut rules,
            1,
            "db",
            "t",
            "created_at",
            ColumnFormat::Timestamp,
        );
        assert_eq!(
            lookup(&rules, 1, "db", "t", "created_at"),
            Some(ColumnFormat::Timestamp)
        );
        // Different connection / column is unaffected.
        assert_eq!(lookup(&rules, 2, "db", "t", "created_at"), None);
        assert_eq!(lookup(&rules, 1, "db", "t", "other"), None);
        // Re-setting replaces (no duplicate).
        upsert(&mut rules, 1, "db", "t", "created_at", ColumnFormat::Bytes);
        assert_eq!(rules.len(), 1);
        assert_eq!(
            lookup(&rules, 1, "db", "t", "created_at"),
            Some(ColumnFormat::Bytes)
        );
        // An explicit None is stored as an override (distinct from "no rule").
        upsert(&mut rules, 1, "db", "t", "created_at", ColumnFormat::None);
        assert_eq!(rules.len(), 1);
        assert_eq!(
            lookup(&rules, 1, "db", "t", "created_at"),
            Some(ColumnFormat::None)
        );
    }
}
