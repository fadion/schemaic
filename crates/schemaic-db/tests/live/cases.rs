//! What each server can hand back, and the text the grid must show for it.
//!
//! **This is the tier's reason for existing.** A value's journey from a column
//! to a cell is decided by the driver, the wire protocol and this crate's
//! decoding together, and no pure test can reach any of it: `core`'s tests can
//! only assert what [`schemaic_core::model::Value`] does with text it is
//! *given*. Everything that goes wrong in between — a `DECIMAL` rounded through
//! an `f64`, a date reformatted, a `NULL` arriving as an empty string, a JSON
//! document renormalised — is invisible until a real server sends a real value.
//!
//! Every case is asserted twice: the text the grid renders
//! (`every_type_renders_as_the_grid_shows_it`), and that the same text, written
//! back, is still the same value (`the_text_the_grid_shows_writes_back_unchanged`).
//! The second is what catches a rendering that *looks* right and is lossy — a
//! scale dropped, a fraction truncated — because such a value survives the first
//! and cannot survive the second.
//!
//! **A case with no `display` is not a case with no assertion.** Some renderings
//! follow the server's own configuration rather than this crate's decoding
//! (`timestamptz` follows `TimeZone`), and pinning one would encode the machine
//! that happened to run it. Those cases still assert the NULL row and the write
//! back, which is where the decoding actually lives.

/// One value, in one column type, on one server.
pub struct TypeCase {
    /// Names the case in a failure and the table it gets. Must be a plain
    /// lower-case identifier.
    pub name: &'static str,
    /// The column type, as this server spells it.
    pub sql_type: &'static str,
    /// The literal, spliced into the `INSERT` verbatim — so a case can use
    /// `X'DEADBEEF'` or `b'10101010'` where a quoted string will not do.
    pub literal: &'static str,
    /// The exact text the grid must render, or `None` where the rendering
    /// follows the server's configuration and pinning it would test the machine.
    pub display: Option<&'static str>,
    /// Can the rendered text be written back as a literal? False for a column
    /// whose cell is the `<n bytes>` placeholder — the grid holds those
    /// read-only for exactly this reason, so a write-back assertion would be
    /// testing something the app never does.
    pub writable: bool,
}

/// A case whose text both renders and writes back.
const fn case(
    name: &'static str,
    sql_type: &'static str,
    literal: &'static str,
    display: &'static str,
) -> TypeCase {
    TypeCase {
        name,
        sql_type,
        literal,
        display: Some(display),
        writable: true,
    }
}

/// A case whose cell is raw bytes: the grid shows a placeholder and refuses to
/// edit it, so only the rendering and the NULL row are asserted.
const fn binary(
    name: &'static str,
    sql_type: &'static str,
    literal: &'static str,
    display: &'static str,
) -> TypeCase {
    TypeCase {
        name,
        sql_type,
        literal,
        display: Some(display),
        writable: false,
    }
}

/// A case whose rendering follows the server's configuration; the write back is
/// still asserted, since that is the part this crate decides.
const fn configured(name: &'static str, sql_type: &'static str, literal: &'static str) -> TypeCase {
    TypeCase {
        name,
        sql_type,
        literal,
        display: None,
        writable: true,
    }
}

/// Types MySQL and MariaDB agree on. Where they do not, the case lives in that
/// server's own list below — the divergences are the reason both are legs.
pub static MYSQL_FAMILY: &[TypeCase] = &[
    case("tinyint_min", "TINYINT", "-128", "-128"),
    case(
        "int_unsigned_max",
        "INT UNSIGNED",
        "4294967295",
        "4294967295",
    ),
    // The two ends no `i64` and no `f64` can both hold. A `BIGINT UNSIGNED` at
    // its maximum is the value that proves the text protocol is being kept as
    // text rather than parsed and reprinted.
    case(
        "bigint_min",
        "BIGINT",
        "-9223372036854775808",
        "-9223372036854775808",
    ),
    case(
        "bigint_unsigned_max",
        "BIGINT UNSIGNED",
        "18446744073709551615",
        "18446744073709551615",
    ),
    case(
        "decimal_wide",
        "DECIMAL(30,10)",
        "'12345678901234567890.1234567890'",
        "12345678901234567890.1234567890",
    ),
    // The scale is part of the value: an `f64` round trip prints `1.5`, and a
    // money column that loses its trailing zeros looks like a rendering bug and
    // is a data one.
    case("decimal_scale", "DECIMAL(10,4)", "'1.5000'", "1.5000"),
    case("double_tenth", "DOUBLE", "0.1", "0.1"),
    case("float_tenth", "FLOAT", "0.1", "0.1"),
    case("date", "DATE", "'2026-09-01'", "2026-09-01"),
    case(
        "datetime_micros",
        "DATETIME(6)",
        "'2026-09-01 12:34:56.123456'",
        "2026-09-01 12:34:56.123456",
    ),
    // Stored as UTC and rendered in the session's zone — but every connection
    // here reaches the same server with the same default, so the text is stable.
    case(
        "timestamp",
        "TIMESTAMP",
        "'2026-09-01 12:34:56'",
        "2026-09-01 12:34:56",
    ),
    // TIME is signed and is not a clock reading; a decoder that treats it as one
    // loses the sign.
    case("time_negative", "TIME", "'-01:02:03'", "-01:02:03"),
    case("year", "YEAR", "2026", "2026"),
    // MySQL strips a CHAR's padding on the way out; PostgreSQL keeps it. Both
    // are pinned, on purpose: it is the kind of difference that turns into a
    // "trailing whitespace" bug report against the grid.
    case("char_padded", "CHAR(4)", "'ab'", "ab"),
    case("varchar_unicode", "VARCHAR(32)", "'héllo 🌍'", "héllo 🌍"),
    case("text_newline", "TEXT", "'a\\nb'", "a\nb"),
    case("text_empty", "VARCHAR(8)", "''", ""),
    case("enum", "ENUM('a','b')", "'b'", "b"),
    case("set", "SET('a','b')", "'a,b'", "a,b"),
    // **A bit-field is not a blob, though MySQL sends one as bytes.**
    // `model::bit_cell` turns those bytes into the number they are, because
    // calling the column binary showed `<1 bytes>` in the grid and withheld it
    // from CSV and JSON outright. The write-back half of this case is the part
    // no pure test can reach: `bit_value` was always right, and the bug was the
    // *variant* its answer was wrapped in — a `Value::Str("170")` renders
    // identically and exports as `'170'`, which MySQL stores as the raw bits of
    // those three bytes and refuses on a `BIT(8)` as "Data too long".
    case("bit", "BIT(8)", "b'10101010'", "170"),
    binary("varbinary", "VARBINARY(4)", "X'DEADBEEF'", "<4 bytes>"),
];

/// **MariaDB's `JSON` is an alias for `LONGTEXT`**, so the server stores the
/// source text and hands it back untouched — key order, spacing and all.
pub static MARIADB_ONLY: &[TypeCase] = &[case(
    "json",
    "JSON",
    "'{\"b\":1, \"a\":2}'",
    "{\"b\":1, \"a\":2}",
)];

/// **MySQL's `JSON` is a parsed type**, so what comes back is the server's
/// normalisation of the document — keys sorted, spacing regularised — and not
/// what was written. Pinned rather than worked around: it is the server's
/// answer, and a grid that showed the input would be the one lying.
pub static MYSQL_ONLY: &[TypeCase] = &[case(
    "json",
    "JSON",
    "'{\"b\":1, \"a\":2}'",
    "{\"a\": 2, \"b\": 1}",
)];

pub static POSTGRES: &[TypeCase] = &[
    case("smallint_min", "smallint", "-32768", "-32768"),
    case(
        "bigint_min",
        "bigint",
        "-9223372036854775808",
        "-9223372036854775808",
    ),
    case(
        "numeric_wide",
        "numeric(40,15)",
        "'123456789012345678901234.123456789012345'",
        "123456789012345678901234.123456789012345",
    ),
    case("numeric_scale", "numeric(10,4)", "'1.5000'", "1.5000"),
    // Not a number at all, and a decoder that routes NUMERIC through a float
    // either loses it or panics on it.
    case("numeric_nan", "numeric", "'NaN'", "NaN"),
    case("float8_tenth", "double precision", "0.1", "0.1"),
    case("float4_tenth", "real", "0.1", "0.1"),
    // The text protocol's spelling of a boolean, which is what the grid shows.
    case("boolean", "boolean", "true", "t"),
    case("date", "date", "'2026-09-01'", "2026-09-01"),
    case(
        "timestamp_micros",
        "timestamp",
        "'2026-09-01 12:34:56.123456'",
        "2026-09-01 12:34:56.123456",
    ),
    // Rendered in the session's `TimeZone`, which is the server's setting rather
    // than this crate's decision.
    configured("timestamptz", "timestamptz", "'2026-09-01 12:34:56+02'"),
    case("time", "time", "'12:34:56'", "12:34:56"),
    case("interval", "interval", "'1 day 2:03:04'", "1 day 02:03:04"),
    case(
        "uuid",
        "uuid",
        "'0b7e0f6a-4d9f-4f2e-9b1a-2c3d4e5f6a7b'",
        "0b7e0f6a-4d9f-4f2e-9b1a-2c3d4e5f6a7b",
    ),
    case("inet", "inet", "'192.168.0.1/24'", "192.168.0.1/24"),
    // PostgreSQL keeps a `char(n)`'s padding where MySQL strips it.
    case("char_padded", "char(4)", "'ab'", "ab  "),
    case("text_unicode", "text", "'héllo 🌍'", "héllo 🌍"),
    case("text_newline", "text", "E'a\\nb'", "a\nb"),
    case("text_empty", "text", "''", ""),
    // `json` keeps the source text and `jsonb` reparses it — the pair is the
    // reason the JSON fixtures for this project live on PostgreSQL.
    case("json", "json", "'{\"b\":1, \"a\":2}'", "{\"b\":1, \"a\":2}"),
    case(
        "jsonb",
        "jsonb",
        "'{\"b\":1, \"a\":2}'",
        "{\"a\": 2, \"b\": 1}",
    ),
    case("int_array", "int[]", "'{1,2,3}'", "{1,2,3}"),
    // The element containing the delimiter: an array rendering that is really a
    // join loses the difference between one element and two.
    case("text_array", "text[]", "'{\"a,b\",c}'", "{\"a,b\",c}"),
    // PostgreSQL's bit-field never arrives as bytes at all — its text protocol
    // sends the digits — so the pair of these with MySQL's `BIT(8)` above is the
    // whole point: one type name, two wire forms, and a grid that must not
    // report either as a byte count.
    case("bit", "bit(8)", "'10101010'", "10101010"),
    case("varbit", "bit varying(8)", "'1010'", "1010"),
    binary("bytea", "bytea", "'\\xdeadbeef'", "<4 bytes>"),
];
