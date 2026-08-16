//! SQL intelligence: the structure-aware layer over a real per-dialect AST.
//!
//! This is the bounded module that turns *cursor + catalog* into structured
//! facts — scope resolution (which tables/aliases/CTEs are visible), completion
//! context, and catalog-aware diagnostics. It sits on two engines by design:
//!
//! - **`sqlparser`** (per-dialect AST) parses *complete, valid* statements and
//!   answers structural questions the token stream can't (this ident is a table
//!   ref in FROM vs a column in SELECT; these are the CTE names in scope).
//! - **The `skip_noncode` lexer** ([`crate::sql`]) stays the source of *byte
//!   positions* (squiggle placement, word boundaries) and the *mid-edit
//!   fallback*: sqlparser isn't error-tolerant, so while a statement is being
//!   typed (and doesn't parse) we degrade to the lexer heuristics rather than
//!   losing all structure.
//!
//! The live database remains the semantic authority — offline diagnostics here
//! cover the instant, catalog-only cases (unknown table/column); dialect-exact
//! validation via PREPARE/EXPLAIN is a later, additive tier.

use sqlparser::dialect::{Dialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};

use crate::sql::skip_noncode;

/// Which SQL dialect a connection speaks. MySQL, PostgreSQL and SQLite are wired;
/// the point of the seam is that adding an engine is a dialect swap, not a
/// rewrite (sqlparser already ships those dialects). The AST classification is
/// dialect-exact here, and so are the byte positions — `crate::sql`'s lexer reads
/// its boundary rules off the per-dialect table in that module rather than
/// comparing against one engine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SqlDialect {
    #[default]
    MySql,
    Postgres,
    Sqlite,
}

impl SqlDialect {
    /// The `sqlparser` dialect backing this connection kind.
    pub(crate) fn parser(self) -> Box<dyn Dialect> {
        match self {
            SqlDialect::MySql => Box::new(MySqlDialect {}),
            SqlDialect::Postgres => Box::new(PostgreSqlDialect {}),
            SqlDialect::Sqlite => Box::new(SQLiteDialect {}),
        }
    }

    /// How to name this engine when telling a language model what it is
    /// generating SQL for.
    ///
    /// Every AI surface used to hardcode "MySQL/MariaDB", so on a PostgreSQL
    /// connection the assistant was asked for the wrong engine's SQL — and it
    /// obliges, producing backticks and `LIMIT x, y` that the server rejects.
    pub fn engine_label(self) -> &'static str {
        match self {
            SqlDialect::MySql => "MySQL/MariaDB",
            SqlDialect::Postgres => "PostgreSQL",
            SqlDialect::Sqlite => "SQLite",
        }
    }

    /// Map a saved connection's `db_type` label to a dialect. Anything not
    /// recognizably Postgres or SQLite falls back to MySQL (the historical
    /// default), so old saved connections keep parsing as before.
    ///
    /// It **delegates** to [`crate::connection`]'s predicates rather than
    /// re-spelling the label match, which is what it used to do: the aliases were
    /// written out twice, in two modules, with no test comparing them, and a label
    /// the connection list accepted could have parsed here as a different engine
    /// entirely. Same rule as the one identifier quoter and the one boundary lexer.
    pub fn from_db_type(db_type: &str) -> SqlDialect {
        if crate::connection::is_postgres(db_type) {
            SqlDialect::Postgres
        } else if crate::connection::is_sqlite(db_type) {
            SqlDialect::Sqlite
        } else {
            SqlDialect::MySql
        }
    }
}

// ── Keyword data ─────────────────────────────────────────────────────────────
// The single home for the completion/analysis keyword sets (previously in the UI
// crate's `completion.rs`, which core can't depend on). `sql_highlight`'s coloring
// list is intentionally separate — it's a broader set that also tints data-type
// and DDL words, a different role from suggestion/analysis.

/// Common SQL keywords offered by autocomplete (identifiers come from the
/// introspected schema).
pub const SQL_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "LIKE",
    "BETWEEN",
    "AS",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "OUTER",
    "CROSS",
    "ON",
    "USING",
    "GROUP",
    "ORDER",
    "BY",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "DISTINCT",
    "UNION",
    "ALL",
    "EXISTS",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "ASC",
    "DESC",
    // COUNT/SUM/AVG/MIN/MAX intentionally live only in `FUNCTIONS` (they're
    // functions, not keywords) — deduped to remove the tier ambiguity.
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "TABLE",
    "VIEW",
    "INDEX",
    "ALTER",
    "DROP",
    "TRUNCATE",
    "PRIMARY",
    "KEY",
    "FOREIGN",
    "REFERENCES",
    "DEFAULT",
    "AUTO_INCREMENT",
    "UNIQUE",
];

/// A built-in SQL function: its name, a display signature (parameters), and a one-
/// line summary. Backs autocomplete (the signature is shown as the row detail) and
/// the "misspelled function" diagnostic (a near-miss of a real name isn't flagged).
#[derive(Clone, Copy)]
pub struct SqlFunction {
    /// Upper-case function name.
    pub name: &'static str,
    /// Display signature including the parameter list, e.g. `POWER(X, Y)`.
    pub signature: &'static str,
    /// Short description of what it returns.
    pub summary: &'static str,
}

const fn f(name: &'static str, signature: &'static str, summary: &'static str) -> SqlFunction {
    SqlFunction {
        name,
        signature,
        summary,
    }
}

/// The authoritative catalog of MySQL/MariaDB built-in functions offered by
/// autocomplete and trusted by the typo checker. Grouped by family; each carries its
/// parameter signature so the completion popup can show it. (Aggregate/window
/// functions are included — they complete in value position like any other call.)
pub const FUNCTIONS: &[SqlFunction] = &[
    // ── Aggregate ────────────────────────────────────────────────────────────
    f(
        "COUNT",
        "COUNT(expr)",
        "Count of non-NULL rows (COUNT(*) counts all)",
    ),
    f("SUM", "SUM(expr)", "Sum of values"),
    f("AVG", "AVG(expr)", "Average of values"),
    f("MIN", "MIN(expr)", "Minimum value"),
    f("MAX", "MAX(expr)", "Maximum value"),
    f(
        "GROUP_CONCAT",
        "GROUP_CONCAT(expr [ORDER BY ...] [SEPARATOR s])",
        "Concatenate group values",
    ),
    f("STD", "STD(expr)", "Population standard deviation"),
    f("STDDEV", "STDDEV(expr)", "Population standard deviation"),
    f(
        "STDDEV_POP",
        "STDDEV_POP(expr)",
        "Population standard deviation",
    ),
    f(
        "STDDEV_SAMP",
        "STDDEV_SAMP(expr)",
        "Sample standard deviation",
    ),
    f("VARIANCE", "VARIANCE(expr)", "Population variance"),
    f("VAR_POP", "VAR_POP(expr)", "Population variance"),
    f("VAR_SAMP", "VAR_SAMP(expr)", "Sample variance"),
    f("BIT_AND", "BIT_AND(expr)", "Bitwise AND over the group"),
    f("BIT_OR", "BIT_OR(expr)", "Bitwise OR over the group"),
    f("BIT_XOR", "BIT_XOR(expr)", "Bitwise XOR over the group"),
    // ── Window ───────────────────────────────────────────────────────────────
    f(
        "ROW_NUMBER",
        "ROW_NUMBER() OVER (...)",
        "Sequential row number within a partition",
    ),
    f("RANK", "RANK() OVER (...)", "Rank with gaps for ties"),
    f(
        "DENSE_RANK",
        "DENSE_RANK() OVER (...)",
        "Rank without gaps for ties",
    ),
    f(
        "PERCENT_RANK",
        "PERCENT_RANK() OVER (...)",
        "Relative rank in [0,1]",
    ),
    f(
        "CUME_DIST",
        "CUME_DIST() OVER (...)",
        "Cumulative distribution",
    ),
    f("NTILE", "NTILE(n) OVER (...)", "Bucket number of n buckets"),
    f(
        "LAG",
        "LAG(expr [, offset [, default]]) OVER (...)",
        "Value from a preceding row",
    ),
    f(
        "LEAD",
        "LEAD(expr [, offset [, default]]) OVER (...)",
        "Value from a following row",
    ),
    f(
        "FIRST_VALUE",
        "FIRST_VALUE(expr) OVER (...)",
        "First value in the window frame",
    ),
    f(
        "LAST_VALUE",
        "LAST_VALUE(expr) OVER (...)",
        "Last value in the window frame",
    ),
    f(
        "NTH_VALUE",
        "NTH_VALUE(expr, n) OVER (...)",
        "Nth value in the window frame",
    ),
    // ── String ───────────────────────────────────────────────────────────────
    f(
        "ASCII",
        "ASCII(str)",
        "Numeric code of the leftmost character",
    ),
    f("BIN", "BIN(n)", "Binary string representation of n"),
    f("BIT_LENGTH", "BIT_LENGTH(str)", "Length of str in bits"),
    f(
        "CHAR",
        "CHAR(n, ...)",
        "Characters for the given code points",
    ),
    f(
        "CHAR_LENGTH",
        "CHAR_LENGTH(str)",
        "Length of str in characters",
    ),
    f(
        "CHARACTER_LENGTH",
        "CHARACTER_LENGTH(str)",
        "Length of str in characters",
    ),
    f("CONCAT", "CONCAT(str, ...)", "Concatenate strings"),
    f(
        "CONCAT_WS",
        "CONCAT_WS(sep, str, ...)",
        "Concatenate with a separator",
    ),
    f("ELT", "ELT(n, str1, str2, ...)", "The nth string argument"),
    f(
        "EXPORT_SET",
        "EXPORT_SET(bits, on, off [, sep [, n]])",
        "String of on/off per bit",
    ),
    f(
        "FIELD",
        "FIELD(str, s1, s2, ...)",
        "Index of str in the argument list",
    ),
    f(
        "FIND_IN_SET",
        "FIND_IN_SET(str, strlist)",
        "Index of str in a comma list",
    ),
    f(
        "FORMAT",
        "FORMAT(x, d)",
        "Format number with d decimals and grouping",
    ),
    f("FROM_BASE64", "FROM_BASE64(str)", "Decode a base-64 string"),
    f("HEX", "HEX(n_or_str)", "Hexadecimal representation"),
    f(
        "INSERT",
        "INSERT(str, pos, len, newstr)",
        "Replace a substring by position",
    ),
    f(
        "INSTR",
        "INSTR(str, substr)",
        "Position of the first occurrence of substr",
    ),
    f("LCASE", "LCASE(str)", "Lower-case (alias of LOWER)"),
    f("LEFT", "LEFT(str, len)", "Leftmost len characters"),
    f("LENGTH", "LENGTH(str)", "Length of str in bytes"),
    f(
        "LOCATE",
        "LOCATE(substr, str [, pos])",
        "Position of substr, optionally from pos",
    ),
    f("LOWER", "LOWER(str)", "Lower-case the string"),
    f(
        "LPAD",
        "LPAD(str, len, pad)",
        "Left-pad str to len with pad",
    ),
    f("LTRIM", "LTRIM(str)", "Trim leading spaces"),
    f(
        "MAKE_SET",
        "MAKE_SET(bits, str, ...)",
        "Set of strings selected by bits",
    ),
    f(
        "MID",
        "MID(str, pos, len)",
        "Substring (alias of SUBSTRING)",
    ),
    f("OCT", "OCT(n)", "Octal string representation of n"),
    f(
        "OCTET_LENGTH",
        "OCTET_LENGTH(str)",
        "Length of str in bytes",
    ),
    f(
        "ORD",
        "ORD(str)",
        "Code of the leftmost (multi-byte) character",
    ),
    f(
        "POSITION",
        "POSITION(substr IN str)",
        "Position of substr in str",
    ),
    f("QUOTE", "QUOTE(str)", "Escape and quote str for SQL"),
    f(
        "REGEXP_INSTR",
        "REGEXP_INSTR(str, pattern [, pos [, occ [, ret [, mt]]]])",
        "Position of a regex match",
    ),
    f(
        "REGEXP_LIKE",
        "REGEXP_LIKE(str, pattern [, match_type])",
        "Whether str matches the pattern",
    ),
    f(
        "REGEXP_REPLACE",
        "REGEXP_REPLACE(str, pattern, replace [, pos [, occ [, mt]]])",
        "Replace regex matches",
    ),
    f(
        "REGEXP_SUBSTR",
        "REGEXP_SUBSTR(str, pattern [, pos [, occ [, mt]]])",
        "The matching substring",
    ),
    f("REPEAT", "REPEAT(str, count)", "Repeat str count times"),
    f(
        "REPLACE",
        "REPLACE(str, from_str, to_str)",
        "Replace all occurrences",
    ),
    f("REVERSE", "REVERSE(str)", "Reverse the characters"),
    f("RIGHT", "RIGHT(str, len)", "Rightmost len characters"),
    f(
        "RPAD",
        "RPAD(str, len, pad)",
        "Right-pad str to len with pad",
    ),
    f("RTRIM", "RTRIM(str)", "Trim trailing spaces"),
    f("SOUNDEX", "SOUNDEX(str)", "Soundex phonetic key"),
    f("SPACE", "SPACE(n)", "A string of n spaces"),
    f(
        "STRCMP",
        "STRCMP(str1, str2)",
        "Compare two strings (-1/0/1)",
    ),
    f(
        "SUBSTR",
        "SUBSTR(str, pos [, len])",
        "Substring (alias of SUBSTRING)",
    ),
    f(
        "SUBSTRING",
        "SUBSTRING(str, pos [, len])",
        "Substring from pos",
    ),
    f(
        "SUBSTRING_INDEX",
        "SUBSTRING_INDEX(str, delim, count)",
        "Substring before the count-th delimiter",
    ),
    f("TO_BASE64", "TO_BASE64(str)", "Base-64 encode"),
    f(
        "TRIM",
        "TRIM([{BOTH|LEADING|TRAILING} [rem] FROM] str)",
        "Trim characters from a string",
    ),
    f("UCASE", "UCASE(str)", "Upper-case (alias of UPPER)"),
    f("UNHEX", "UNHEX(str)", "Bytes for a hexadecimal string"),
    f("UPPER", "UPPER(str)", "Upper-case the string"),
    f(
        "WEIGHT_STRING",
        "WEIGHT_STRING(str)",
        "Collation weight string",
    ),
    // ── Numeric ──────────────────────────────────────────────────────────────
    f("ABS", "ABS(x)", "Absolute value"),
    f("ACOS", "ACOS(x)", "Arc cosine"),
    f("ASIN", "ASIN(x)", "Arc sine"),
    f("ATAN", "ATAN(x)", "Arc tangent"),
    f("ATAN2", "ATAN2(y, x)", "Arc tangent of y/x"),
    f("CEIL", "CEIL(x)", "Smallest integer >= x"),
    f("CEILING", "CEILING(x)", "Smallest integer >= x"),
    f(
        "CONV",
        "CONV(n, from_base, to_base)",
        "Convert a number between bases",
    ),
    f("COS", "COS(x)", "Cosine"),
    f("COT", "COT(x)", "Cotangent"),
    f("CRC32", "CRC32(str)", "Cyclic redundancy check value"),
    f("DEGREES", "DEGREES(x)", "Radians to degrees"),
    f("EXP", "EXP(x)", "e raised to the power x"),
    f("FLOOR", "FLOOR(x)", "Largest integer <= x"),
    f("GREATEST", "GREATEST(x, ...)", "Largest argument"),
    f("LEAST", "LEAST(x, ...)", "Smallest argument"),
    f("LN", "LN(x)", "Natural logarithm"),
    f(
        "LOG",
        "LOG([base,] x)",
        "Logarithm (natural, or to the given base)",
    ),
    f("LOG10", "LOG10(x)", "Base-10 logarithm"),
    f("LOG2", "LOG2(x)", "Base-2 logarithm"),
    f("MOD", "MOD(n, m)", "Remainder of n / m"),
    f("PI", "PI()", "The value of pi"),
    f("POW", "POW(x, y)", "x raised to the power y"),
    f("POWER", "POWER(x, y)", "x raised to the power y"),
    f("RADIANS", "RADIANS(x)", "Degrees to radians"),
    f("RAND", "RAND([seed])", "Random float in [0,1)"),
    f("ROUND", "ROUND(x [, d])", "Round to d decimals"),
    f("SIGN", "SIGN(x)", "Sign of x (-1/0/1)"),
    f("SIN", "SIN(x)", "Sine"),
    f("SQRT", "SQRT(x)", "Square root"),
    f("TAN", "TAN(x)", "Tangent"),
    f(
        "TRUNCATE",
        "TRUNCATE(x, d)",
        "Truncate to d decimals (no rounding)",
    ),
    // ── Date & time ──────────────────────────────────────────────────────────
    f(
        "ADDDATE",
        "ADDDATE(date, INTERVAL expr unit)",
        "Add an interval to a date",
    ),
    f(
        "ADDTIME",
        "ADDTIME(expr1, expr2)",
        "Add two time/datetime values",
    ),
    f(
        "CONVERT_TZ",
        "CONVERT_TZ(dt, from_tz, to_tz)",
        "Convert a datetime between time zones",
    ),
    f("CURDATE", "CURDATE()", "Current date"),
    f("CURRENT_DATE", "CURRENT_DATE()", "Current date"),
    f("CURRENT_TIME", "CURRENT_TIME()", "Current time"),
    f(
        "CURRENT_TIMESTAMP",
        "CURRENT_TIMESTAMP()",
        "Current date and time",
    ),
    f("CURTIME", "CURTIME()", "Current time"),
    f("DATE", "DATE(expr)", "Date part of a datetime"),
    f(
        "DATE_ADD",
        "DATE_ADD(date, INTERVAL expr unit)",
        "Add an interval to a date",
    ),
    f(
        "DATE_FORMAT",
        "DATE_FORMAT(date, format)",
        "Format a date by a pattern",
    ),
    f(
        "DATE_SUB",
        "DATE_SUB(date, INTERVAL expr unit)",
        "Subtract an interval from a date",
    ),
    f(
        "DATEDIFF",
        "DATEDIFF(expr1, expr2)",
        "Days between two dates",
    ),
    f("DAY", "DAY(date)", "Day of the month"),
    f("DAYNAME", "DAYNAME(date)", "Weekday name"),
    f("DAYOFMONTH", "DAYOFMONTH(date)", "Day of the month (0-31)"),
    f("DAYOFWEEK", "DAYOFWEEK(date)", "Weekday index (1=Sunday)"),
    f("DAYOFYEAR", "DAYOFYEAR(date)", "Day of the year (1-366)"),
    f(
        "EXTRACT",
        "EXTRACT(unit FROM date)",
        "Extract a part of a date",
    ),
    f("FROM_DAYS", "FROM_DAYS(n)", "Date from a day number"),
    f(
        "FROM_UNIXTIME",
        "FROM_UNIXTIME(ts [, format])",
        "Datetime from a Unix timestamp",
    ),
    f(
        "GET_FORMAT",
        "GET_FORMAT({DATE|TIME|DATETIME}, 'format')",
        "A standard format string",
    ),
    f("HOUR", "HOUR(time)", "Hour of a time"),
    f("LAST_DAY", "LAST_DAY(date)", "Last day of the month"),
    f("LOCALTIME", "LOCALTIME()", "Current date and time"),
    f(
        "LOCALTIMESTAMP",
        "LOCALTIMESTAMP()",
        "Current date and time",
    ),
    f(
        "MAKEDATE",
        "MAKEDATE(year, dayofyear)",
        "Date from year and day-of-year",
    ),
    f(
        "MAKETIME",
        "MAKETIME(hour, minute, second)",
        "Time from components",
    ),
    f("MICROSECOND", "MICROSECOND(expr)", "Microseconds of a time"),
    f("MINUTE", "MINUTE(time)", "Minute of a time"),
    f("MONTH", "MONTH(date)", "Month number (1-12)"),
    f("MONTHNAME", "MONTHNAME(date)", "Month name"),
    f("NOW", "NOW()", "Current date and time"),
    f(
        "PERIOD_ADD",
        "PERIOD_ADD(period, n)",
        "Add n months to a YYYYMM period",
    ),
    f(
        "PERIOD_DIFF",
        "PERIOD_DIFF(p1, p2)",
        "Months between two YYYYMM periods",
    ),
    f("QUARTER", "QUARTER(date)", "Quarter of the year (1-4)"),
    f(
        "SEC_TO_TIME",
        "SEC_TO_TIME(seconds)",
        "Time from a number of seconds",
    ),
    f("SECOND", "SECOND(time)", "Second of a time"),
    f(
        "STR_TO_DATE",
        "STR_TO_DATE(str, format)",
        "Parse a string into a date",
    ),
    f(
        "SUBDATE",
        "SUBDATE(date, INTERVAL expr unit)",
        "Subtract an interval from a date",
    ),
    f(
        "SUBTIME",
        "SUBTIME(expr1, expr2)",
        "Subtract two time/datetime values",
    ),
    f(
        "SYSDATE",
        "SYSDATE()",
        "Time at which the function executes",
    ),
    f("TIME", "TIME(expr)", "Time part of a datetime"),
    f(
        "TIME_FORMAT",
        "TIME_FORMAT(time, format)",
        "Format a time by a pattern",
    ),
    f("TIME_TO_SEC", "TIME_TO_SEC(time)", "Seconds since midnight"),
    f(
        "TIMEDIFF",
        "TIMEDIFF(expr1, expr2)",
        "Difference between two times",
    ),
    f(
        "TIMESTAMP",
        "TIMESTAMP(expr [, time])",
        "Datetime, optionally adding a time",
    ),
    f(
        "TIMESTAMPADD",
        "TIMESTAMPADD(unit, interval, datetime)",
        "Add an interval to a datetime",
    ),
    f(
        "TIMESTAMPDIFF",
        "TIMESTAMPDIFF(unit, dt1, dt2)",
        "Difference between datetimes in unit",
    ),
    f("TO_DAYS", "TO_DAYS(date)", "Day number of a date"),
    f("TO_SECONDS", "TO_SECONDS(expr)", "Seconds since year 0"),
    f(
        "UNIX_TIMESTAMP",
        "UNIX_TIMESTAMP([date])",
        "Unix timestamp (now or of date)",
    ),
    f("UTC_DATE", "UTC_DATE()", "Current UTC date"),
    f("UTC_TIME", "UTC_TIME()", "Current UTC time"),
    f(
        "UTC_TIMESTAMP",
        "UTC_TIMESTAMP()",
        "Current UTC date and time",
    ),
    f("WEEK", "WEEK(date [, mode])", "Week number of the year"),
    f("WEEKDAY", "WEEKDAY(date)", "Weekday index (0=Monday)"),
    f("WEEKOFYEAR", "WEEKOFYEAR(date)", "Calendar week (1-53)"),
    f("YEAR", "YEAR(date)", "Year of a date"),
    f("YEARWEEK", "YEARWEEK(date [, mode])", "Year and week"),
    // ── Flow control & comparison ─────────────────────────────────────────────
    f(
        "COALESCE",
        "COALESCE(value, ...)",
        "First non-NULL argument",
    ),
    f("IF", "IF(cond, if_true, if_false)", "Conditional value"),
    f(
        "IFNULL",
        "IFNULL(expr1, expr2)",
        "expr1, or expr2 when expr1 is NULL",
    ),
    f("ISNULL", "ISNULL(expr)", "1 if expr is NULL, else 0"),
    f(
        "NULLIF",
        "NULLIF(expr1, expr2)",
        "NULL if the two are equal, else expr1",
    ),
    f(
        "INTERVAL",
        "INTERVAL(n, n1, n2, ...)",
        "Index of the last value <= n",
    ),
    // ── Cast ─────────────────────────────────────────────────────────────────
    f(
        "CAST",
        "CAST(expr AS type)",
        "Convert a value to another type",
    ),
    f(
        "CONVERT",
        "CONVERT(expr, type)",
        "Convert a value (or USING charset)",
    ),
    // ── JSON ─────────────────────────────────────────────────────────────────
    f("JSON_ARRAY", "JSON_ARRAY(val, ...)", "Build a JSON array"),
    f(
        "JSON_ARRAYAGG",
        "JSON_ARRAYAGG(expr)",
        "Aggregate values into a JSON array",
    ),
    f(
        "JSON_ARRAY_APPEND",
        "JSON_ARRAY_APPEND(json, path, val, ...)",
        "Append to arrays at paths",
    ),
    f(
        "JSON_ARRAY_INSERT",
        "JSON_ARRAY_INSERT(json, path, val, ...)",
        "Insert into arrays at paths",
    ),
    f(
        "JSON_CONTAINS",
        "JSON_CONTAINS(target, candidate [, path])",
        "Whether target contains candidate",
    ),
    f(
        "JSON_CONTAINS_PATH",
        "JSON_CONTAINS_PATH(json, one_or_all, path, ...)",
        "Whether any/all paths exist",
    ),
    f(
        "JSON_DEPTH",
        "JSON_DEPTH(json)",
        "Maximum depth of a JSON document",
    ),
    f(
        "JSON_EXTRACT",
        "JSON_EXTRACT(json, path, ...)",
        "Value(s) at the given path(s)",
    ),
    f(
        "JSON_INSERT",
        "JSON_INSERT(json, path, val, ...)",
        "Insert values without overwriting",
    ),
    f(
        "JSON_KEYS",
        "JSON_KEYS(json [, path])",
        "Keys of a JSON object",
    ),
    f(
        "JSON_LENGTH",
        "JSON_LENGTH(json [, path])",
        "Number of elements",
    ),
    f(
        "JSON_MERGE_PATCH",
        "JSON_MERGE_PATCH(json, ...)",
        "RFC 7386 merge of documents",
    ),
    f(
        "JSON_MERGE_PRESERVE",
        "JSON_MERGE_PRESERVE(json, ...)",
        "Merge, preserving duplicate keys",
    ),
    f(
        "JSON_OBJECT",
        "JSON_OBJECT(key, val, ...)",
        "Build a JSON object",
    ),
    f(
        "JSON_OBJECTAGG",
        "JSON_OBJECTAGG(key, value)",
        "Aggregate pairs into a JSON object",
    ),
    f(
        "JSON_OVERLAPS",
        "JSON_OVERLAPS(json1, json2)",
        "Whether two documents share elements",
    ),
    f(
        "JSON_PRETTY",
        "JSON_PRETTY(json)",
        "Pretty-print a JSON document",
    ),
    f(
        "JSON_QUOTE",
        "JSON_QUOTE(string)",
        "Quote a string as a JSON value",
    ),
    f(
        "JSON_REMOVE",
        "JSON_REMOVE(json, path, ...)",
        "Remove elements at paths",
    ),
    f(
        "JSON_REPLACE",
        "JSON_REPLACE(json, path, val, ...)",
        "Replace existing values",
    ),
    f(
        "JSON_SEARCH",
        "JSON_SEARCH(json, one_or_all, search [, esc [, path]])",
        "Path(s) to a matching string",
    ),
    f(
        "JSON_SET",
        "JSON_SET(json, path, val, ...)",
        "Insert or update values",
    ),
    f("JSON_TYPE", "JSON_TYPE(json)", "Type of a JSON value"),
    f(
        "JSON_UNQUOTE",
        "JSON_UNQUOTE(json)",
        "Unquote a JSON string value",
    ),
    f(
        "JSON_VALID",
        "JSON_VALID(val)",
        "Whether a value is valid JSON",
    ),
    f(
        "JSON_STORAGE_SIZE",
        "JSON_STORAGE_SIZE(json)",
        "Bytes used to store the document",
    ),
    // ── Information ───────────────────────────────────────────────────────────
    f(
        "BENCHMARK",
        "BENCHMARK(count, expr)",
        "Evaluate expr count times (timing)",
    ),
    f("CHARSET", "CHARSET(str)", "Character set of a string"),
    f(
        "COERCIBILITY",
        "COERCIBILITY(str)",
        "Collation coercibility",
    ),
    f("COLLATION", "COLLATION(str)", "Collation of a string"),
    f("CONNECTION_ID", "CONNECTION_ID()", "The connection's id"),
    f("CURRENT_ROLE", "CURRENT_ROLE()", "The active roles"),
    f(
        "CURRENT_USER",
        "CURRENT_USER()",
        "Authenticated user for the session",
    ),
    f("DATABASE", "DATABASE()", "The current database name"),
    f(
        "FOUND_ROWS",
        "FOUND_ROWS()",
        "Rows the last statement would have returned",
    ),
    f(
        "LAST_INSERT_ID",
        "LAST_INSERT_ID([expr])",
        "Last AUTO_INCREMENT value",
    ),
    f(
        "ROW_COUNT",
        "ROW_COUNT()",
        "Rows affected by the last statement",
    ),
    f("SCHEMA", "SCHEMA()", "The current database name"),
    f("SESSION_USER", "SESSION_USER()", "The session user"),
    f("SYSTEM_USER", "SYSTEM_USER()", "The session user"),
    f("USER", "USER()", "The connected user and host"),
    f("VERSION", "VERSION()", "The server version string"),
    // ── Encryption, encoding & misc ───────────────────────────────────────────
    f(
        "AES_DECRYPT",
        "AES_DECRYPT(crypt, key [, iv])",
        "AES-decrypt a value",
    ),
    f(
        "AES_ENCRYPT",
        "AES_ENCRYPT(str, key [, iv])",
        "AES-encrypt a value",
    ),
    f("COMPRESS", "COMPRESS(str)", "Compress a string"),
    f(
        "UNCOMPRESS",
        "UNCOMPRESS(str)",
        "Uncompress a compressed string",
    ),
    f(
        "UNCOMPRESSED_LENGTH",
        "UNCOMPRESSED_LENGTH(str)",
        "Original length of compressed data",
    ),
    f("MD5", "MD5(str)", "MD5 128-bit checksum (hex)"),
    f("SHA1", "SHA1(str)", "SHA-1 160-bit checksum (hex)"),
    f("SHA", "SHA(str)", "SHA-1 checksum (alias of SHA1)"),
    f(
        "SHA2",
        "SHA2(str, hash_length)",
        "SHA-2 checksum (224/256/384/512)",
    ),
    f("RANDOM_BYTES", "RANDOM_BYTES(len)", "len random bytes"),
    f("UUID", "UUID()", "A version-1 UUID string"),
    f("UUID_SHORT", "UUID_SHORT()", "A 64-bit unique integer"),
    f(
        "UUID_TO_BIN",
        "UUID_TO_BIN(uuid [, swap_flag])",
        "Binary form of a UUID",
    ),
    f(
        "BIN_TO_UUID",
        "BIN_TO_UUID(bin [, swap_flag])",
        "UUID string from binary",
    ),
    f(
        "IS_UUID",
        "IS_UUID(str)",
        "Whether a string is a valid UUID",
    ),
    f("INET_ATON", "INET_ATON(ip)", "Integer for an IPv4 address"),
    f("INET_NTOA", "INET_NTOA(n)", "IPv4 address from an integer"),
    f(
        "INET6_ATON",
        "INET6_ATON(ip)",
        "Binary for an IPv4/IPv6 address",
    ),
    f("INET6_NTOA", "INET6_NTOA(bin)", "IP address from binary"),
    f("IS_IPV4", "IS_IPV4(ip)", "Whether a string is IPv4"),
    f("IS_IPV6", "IS_IPV6(ip)", "Whether a string is IPv6"),
    f(
        "IS_IPV4_COMPAT",
        "IS_IPV4_COMPAT(ip)",
        "Whether an IPv6 is IPv4-compatible",
    ),
    f(
        "IS_IPV4_MAPPED",
        "IS_IPV4_MAPPED(ip)",
        "Whether an IPv6 is IPv4-mapped",
    ),
    f("SLEEP", "SLEEP(seconds)", "Pause for the given seconds"),
    f(
        "GET_LOCK",
        "GET_LOCK(name, timeout)",
        "Acquire a named advisory lock",
    ),
    f(
        "RELEASE_LOCK",
        "RELEASE_LOCK(name)",
        "Release a named advisory lock",
    ),
    f(
        "IS_FREE_LOCK",
        "IS_FREE_LOCK(name)",
        "Whether a named lock is free",
    ),
    f(
        "IS_USED_LOCK",
        "IS_USED_LOCK(name)",
        "Connection id holding a named lock",
    ),
    f(
        "NAME_CONST",
        "NAME_CONST(name, value)",
        "A column with the given name and value",
    ),
    f("BIT_COUNT", "BIT_COUNT(n)", "Number of set bits"),
];

/// The function names (upper-case), for the typo checker and keyword-set membership.
pub fn function_names() -> impl Iterator<Item = &'static str> {
    FUNCTIONS.iter().map(|f| f.name)
}

/// Keywords that begin a statement (offered at statement start).
pub const STMT_KEYWORDS: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "TRUNCATE", "WITH", "SHOW",
    "EXPLAIN", "DESCRIBE", "USE", "REPLACE", "CALL",
];

/// Clause keywords that determine the completion context (see [`clause_context`]).
const CLAUSE_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "JOIN", "ON", "AND", "OR", "HAVING", "SET", "USING", "BY", "GROUP",
    "ORDER", "DISTINCT", "INTO", "UPDATE", "TABLE", "TRUNCATE", "DESCRIBE", "VALUES", "LIMIT",
    "OFFSET", "WHEN", "THEN", "ELSE",
];

/// Case-insensitive membership in the SQL keyword set (used to reject a keyword
/// as an implicit table alias).
pub fn is_sql_keyword(word: &str) -> bool {
    let up = word.to_ascii_uppercase();
    SQL_KEYWORDS.iter().any(|k| *k == up)
}

// ── Byte-position lexer (word/punctuation scan over `skip_noncode`) ───────────
// This is the *positional* engine: it agrees with the statement splitter / WHERE
// guard on string/comment/backtick boundaries and yields absolute byte offsets,
// which the AST (whose spans are still maturing) can't reliably provide.

pub(crate) use crate::sql::{is_word_byte, is_word_start};

/// Lightweight SQL token used by the context analysis (words + the punctuation
/// that matters for it). Strings and comments are skipped by the tokenizer.
#[derive(Clone)]
enum TkKind {
    Word(String),
    Dot,
    Comma,
    LParen,
    RParen,
}

/// A token plus its absolute byte offset in `sql`.
struct Token {
    at: usize,
    /// One past the token's last byte **in the source**, quotes included.
    ///
    /// Carried rather than recomputed as `at + text.len()`, which is wrong for
    /// exactly the tokens that need it most: a quoted identifier's `at` is the
    /// opening quote while its payload is the *inner* text, so the arithmetic
    /// underlined `"NoSuchTb` — starting on the quote and stopping two bytes
    /// short — and a doubled quote inside the name widened the error further.
    /// The tokenizer is the only place that knows what it consumed.
    end: usize,
    kind: TkKind,
    /// The word came from a quoted identifier (`` `select` ``). Quoting is what
    /// makes a reserved word legal as a name, so the alias checks must not flag
    /// it — and the scope resolver must still see the name.
    quoted: bool,
}

/// The parts of an AST object name as their **unquoted** identifier text.
///
/// `ObjectNamePart`'s `Display` re-adds the quoting, so `` `MyTable` `` /
/// `"MyTable"` would come back quote-wrapped and never match the catalog (which
/// is keyed on bare names) — the alias path beside every call site already reads
/// `Ident::value` for exactly this reason. A non-identifier part (a
/// dialect-specific function name) has no bare form, so it falls back to
/// `Display`.
fn object_name_parts(name: &sqlparser::ast::ObjectName) -> Vec<String> {
    name.0
        .iter()
        .map(|p| {
            p.as_ident()
                .map(|i| i.value.clone())
                .unwrap_or_else(|| p.to_string())
        })
        .collect()
}

/// If `open` starts a quoted *identifier* in `dialect`, the byte that closes it
/// and whether doubling that byte escapes it. Everything else `skip_noncode`
/// consumes at that position is a string or comment, i.e. genuinely not code.
///
/// It answers per-byte rather than returning "the" quote character because
/// **SQLite has three** — `"x"` (standard), `` `x` `` (MySQL compatibility) and
/// `[x]` (SQL-Server compatibility) — and the third doesn't even close with the
/// byte it opened with. A single-byte answer silently picked one of the three and
/// tokenized names written the other two ways as opaque non-code, which is a
/// completion popup that goes blank on a name the user quoted.
fn ident_quote(dialect: SqlDialect, open: u8) -> Option<(u8, bool)> {
    match (dialect, open) {
        (SqlDialect::MySql, b'`') => Some((b'`', true)),
        (SqlDialect::Postgres, b'"') => Some((b'"', true)),
        (SqlDialect::Sqlite, b'"') => Some((b'"', true)),
        (SqlDialect::Sqlite, b'`') => Some((b'`', true)),
        // No escape exists inside `[…]`, so nothing is doubled.
        (SqlDialect::Sqlite, b'[') => Some((b']', false)),
        _ => None,
    }
}

/// Tokenize `sql[lo..hi]` into words + `. , ( )`, skipping string literals and
/// comments via the shared [`skip_noncode`] primitive.
///
/// A **quoted identifier becomes a `Word`** carrying its unquoted text, rather
/// than being skipped like a string: `` `shop`.`customers` `` (MySQL) and
/// `"sales"."orders"` (PostgreSQL) have to resolve to the same scope as the bare
/// names. Schemaic generates the quoted form itself (see
/// [`crate::filter::table_query`], which quotes a mixed-case name on PG), and this
/// is the *fallback* path — the one that runs the moment a statement stops
/// parsing, i.e. exactly while the user is typing a `WHERE`. Skipping the name
/// there cost the tab its column completion.
///
/// It is **dialect-aware**: which byte quotes an identifier differs (`"` is a
/// *string* in MySQL but an identifier in PG), and so do comment and string
/// boundaries (`#` is a comment in MySQL, an operator in PG; `$tag$…$tag$` is a
/// PG string whose contents must not be read as code). `skip_noncode` decides
/// every boundary, so there's no second scanner here — only the content of a
/// quoted-identifier span is lifted out.
fn tokenize_range(sql: &str, lo: usize, hi: usize, dialect: SqlDialect) -> Vec<Token> {
    let b = sql.as_bytes();
    let mut out = Vec::new();
    let mut i = lo;
    let push = |out: &mut Vec<Token>, at: usize, end: usize, kind: TkKind| {
        out.push(Token {
            at,
            end,
            kind,
            quoted: false,
        })
    };
    while i < hi {
        // Quoted identifier → a word. Must be tried before `skip_noncode`, which
        // would otherwise consume it as an opaque non-code run.
        if let Some((close, doubles)) = ident_quote(dialect, b[i])
            && let Some(j) = skip_noncode(b, i, dialect)
        {
            let end = j.min(hi);
            // `j` points past the closing quote; an unterminated quote runs to the
            // end of the range, in which case there's no closer to trim.
            let closed = j <= hi && b.get(j - 1) == Some(&close) && j - 1 > i;
            let inner = sql
                .get(i + 1..if closed { j - 1 } else { end })
                .unwrap_or("");
            if !inner.is_empty() {
                // A doubled quote inside the name is one literal quote char —
                // where the dialect has such an escape at all (`[…]` has none).
                let text = if doubles {
                    let doubled = [close, close];
                    let doubled = std::str::from_utf8(&doubled).unwrap_or("");
                    let single = [close];
                    let single = std::str::from_utf8(&single).unwrap_or("");
                    inner.replace(doubled, single)
                } else {
                    inner.to_string()
                };
                out.push(Token {
                    at: i,
                    // Past the closing quote — the span the user sees.
                    end,
                    kind: TkKind::Word(text),
                    quoted: true,
                });
            }
            i = end;
            continue;
        }
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j.min(hi);
            continue;
        }
        let c = b[i];
        if is_word_start(c) {
            let s = i;
            let mut j = i + 1;
            while j < hi && is_word_byte(b[j]) {
                j += 1;
            }
            push(&mut out, s, j, TkKind::Word(sql[s..j].to_string()));
            i = j;
            continue;
        }
        match c {
            b'.' => push(&mut out, i, i + 1, TkKind::Dot),
            b',' => push(&mut out, i, i + 1, TkKind::Comma),
            b'(' => push(&mut out, i, i + 1, TkKind::LParen),
            b')' => push(&mut out, i, i + 1, TkKind::RParen),
            _ => {}
        }
        i += 1;
    }
    out
}

/// The set of paren-scope ids open at `caret` (each `(` is numbered by encounter
/// order; `0` is the top level and always included).
fn caret_scope_chain(toks: &[Token], caret: usize) -> std::collections::HashSet<usize> {
    let mut next_id = 1usize;
    let mut open: Vec<usize> = Vec::new();
    for t in toks {
        if t.at >= caret {
            break;
        }
        match t.kind {
            TkKind::LParen => {
                open.push(next_id);
                next_id += 1;
            }
            TkKind::RParen => {
                open.pop();
            }
            _ => {}
        }
    }
    let mut chain: std::collections::HashSet<usize> = open.into_iter().collect();
    chain.insert(0);
    chain
}

// ── Scope + context ──────────────────────────────────────────────────────────

/// A table reference visible in a statement's scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRef {
    pub name: String,
    pub alias: Option<String>,
    /// Qualifying database, if written `db.table`.
    pub db: Option<String>,
}

/// The tables and CTE names in scope at a caret position.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub tables: Vec<TableRef>,
    pub ctes: Vec<String>,
}

/// What kind of token is expected at the caret, deciding which suggestions to
/// rank first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClauseCtx {
    /// Start of a statement → statement keywords.
    Start,
    /// After SELECT / WHERE / ON / SET / … → columns, functions, keywords.
    Column,
    /// After FROM / JOIN / UPDATE / INTO → tables, databases.
    Table,
    /// Right after `qualifier.` → that table's columns (or that db's tables).
    Qualified(String),
    /// Anything else → the full mixed list (keywords + tables + columns).
    Other,
}

/// Byte offset where the identifier ending at `offset` begins.
pub fn word_start(text: &str, offset: usize) -> usize {
    let bytes = text.as_bytes();
    let mut start = offset.min(text.len());
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    start
}

/// Classify the caret context from the statement `sql[lo..hi]`. `word_lo` is the
/// byte offset where the caret's current word begins. Lexer-based, so it stays
/// correct mid-edit (when the statement doesn't parse).
pub fn clause_context(sql: &str, lo: usize, word_lo: usize, dialect: SqlDialect) -> ClauseCtx {
    let toks = tokenize_range(sql, lo, word_lo, dialect);
    // Qualified reference: the char just before the word is a `.`. Read the
    // qualifier off the **tokenizer**, not the raw bytes — stepping back over
    // word bytes stops at the closing quote of `"Orders"` or `` `orders` ``, so
    // the qualifier came out empty and the branch fell through to the generic
    // list. Schemaic writes those quoted forms itself (a mixed-case PostgreSQL
    // name, a reserved-word MySQL one), so this is the shape you get by opening
    // a table from the tree. Going through the lexer also gets the qualifier of
    // a dotted pair (`"sales"."Orders".`) right for free.
    if word_lo > lo
        && sql.as_bytes()[word_lo - 1] == b'.'
        && let [.., q, d] = toks.as_slice()
        && matches!(d.kind, TkKind::Dot)
        && d.at == word_lo - 1
        && let TkKind::Word(name) = &q.kind
        && !name.is_empty()
    {
        return ClauseCtx::Qualified(name.clone());
    }
    // The last clause keyword strictly before the caret's word decides the rest.
    let mut last_kw: Option<String> = None;
    for t in &toks {
        if let TkKind::Word(w) = &t.kind {
            let up = w.to_ascii_uppercase();
            if CLAUSE_KEYWORDS.contains(&up.as_str()) {
                last_kw = Some(up);
            }
        }
    }
    match last_kw.as_deref() {
        None => {
            if toks.iter().any(|t| matches!(t.kind, TkKind::Word(_))) {
                ClauseCtx::Other
            } else {
                ClauseCtx::Start
            }
        }
        Some("FROM" | "JOIN" | "INTO" | "UPDATE" | "TABLE" | "TRUNCATE" | "DESCRIBE") => {
            ClauseCtx::Table
        }
        Some(
            "SELECT" | "WHERE" | "ON" | "AND" | "OR" | "HAVING" | "SET" | "USING" | "BY" | "GROUP"
            | "ORDER" | "DISTINCT" | "WHEN" | "THEN" | "ELSE",
        ) => ClauseCtx::Column,
        _ => ClauseCtx::Other,
    }
}

// ── Expected-token continuation model ────────────────────────────────────────
// Beyond "which clause are we in" ([`clause_context`], which schema *candidates* to
// rank), the completion popup needs "which *keyword* comes next" from SQL's fixed
// clause grammar — so `WHERE` outranks the generic keyword bag after a complete
// table ref, `FROM` is #1 after the projection, and multi-word phrases (`GROUP BY`,
// `LEFT JOIN`, `IS NOT NULL`) are single suggestions. SQL's clause order is small +
// stable, so a hand table (grown case-by-case, each with a test) beats a generic
// parser here and stays dialect-perfect.

/// The leading statement kind, which decides the clause grammar to follow.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StmtKind {
    Select,
    Insert,
    Update,
    Delete,
    Other,
}

/// The clause the caret currently sits in, tracked by [`scan_clauses`]. Multi-word
/// clauses have both an incomplete arm (`Group` = `GROUP` typed, `BY` expected) and
/// a complete one (`GroupBy`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Clause {
    None,
    Select,
    From,
    JoinMod, // a bare `LEFT`/`INNER`/… — `JOIN` still expected
    Join,
    On,
    Using,
    Where,
    Group,
    GroupBy,
    Having,
    Order,
    OrderBy,
    Limit,
    Offset,
    Update,
    Set,
    Insert,
    Into,
    Values,
}

/// Ranked keyword/phrase continuations the SQL grammar expects at the caret, plus
/// whether the completion popup should auto-open on an empty prefix here. Feeds the
/// **top tier** of the completion popup so a legal next clause keyword outranks the
/// generic keyword bag (and, after a complete table ref, the schema table names).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Continuation {
    /// Expected next keyword/phrase suggestions, best first.
    pub keywords: Vec<String>,
    /// True when the caret sits right after a clause keyword (or comma) that takes
    /// an operand — `WHERE`/`ON`/`SET`/`ORDER BY`/`FROM`/… — so the popup opens
    /// without a typed prefix (DataGrip-style: columns after `WHERE`, tables after
    /// `FROM`).
    pub auto_show: bool,
}

/// The byte offset to start scanning the caret's local clause sequence from: just
/// inside the innermost unclosed `(` before the caret (so a subquery's clauses
/// don't bleed in from the outer query), else `lo`.
fn local_scope_start(sql: &str, lo: usize, caret: usize, dialect: SqlDialect) -> usize {
    let toks = tokenize_range(sql, lo, caret, dialect);
    let mut stack: Vec<usize> = Vec::new();
    for t in &toks {
        match t.kind {
            TkKind::LParen => stack.push(t.at),
            TkKind::RParen => {
                stack.pop();
            }
            _ => {}
        }
    }
    stack.last().map(|&at| at + 1).unwrap_or(lo)
}

/// Reduce the top-level token sequence before the caret into the current clause,
/// how much of its operand slot is filled, and which clauses have already appeared.
/// Returns `(kind, current_clause, operand_count, seen, select_has_content,
/// select_has_star, distinct_seen)`. `operand_count` resets at each clause keyword
/// *and* comma, so it answers "is the current slot non-empty" (a trailing comma →
/// empty again); `select_has_content` persists across commas (any projection item
/// ever seen), so DISTINCT is only offered before the very first one.
fn scan_clauses(
    sql: &str,
    toks: &[Token],
    scan_end: usize,
) -> (StmtKind, Clause, usize, Vec<Clause>, bool, bool, bool) {
    let word_up = |t: &Token| -> Option<String> {
        if let TkKind::Word(w) = &t.kind {
            Some(w.to_ascii_uppercase())
        } else {
            None
        }
    };
    let mut kind = StmtKind::Other;
    let mut cur = Clause::None;
    let mut operand = 0usize;
    let mut seen: Vec<Clause> = Vec::new();
    let mut select_has_content = false;
    let mut distinct_seen = false;
    let mut select_kw_end = 0usize;
    let mut first = true;
    let push_seen = |seen: &mut Vec<Clause>, c: Clause| {
        if !seen.contains(&c) {
            seen.push(c);
        }
    };
    let mut i = 0;
    while i < toks.len() {
        if let Some(up) = word_up(&toks[i]) {
            if first {
                kind = match up.as_str() {
                    "SELECT" | "WITH" => StmtKind::Select,
                    "INSERT" | "REPLACE" => StmtKind::Insert,
                    "UPDATE" => StmtKind::Update,
                    "DELETE" => StmtKind::Delete,
                    _ => StmtKind::Other,
                };
                first = false;
            }
            match up.as_str() {
                "SELECT" => {
                    cur = Clause::Select;
                    operand = 0;
                    select_kw_end = toks[i].at + 6;
                    push_seen(&mut seen, Clause::Select);
                }
                "FROM" => {
                    cur = Clause::From;
                    operand = 0;
                    push_seen(&mut seen, Clause::From);
                }
                "WHERE" => {
                    cur = Clause::Where;
                    operand = 0;
                    push_seen(&mut seen, Clause::Where);
                }
                "HAVING" => {
                    cur = Clause::Having;
                    operand = 0;
                    push_seen(&mut seen, Clause::Having);
                }
                "LIMIT" => {
                    cur = Clause::Limit;
                    operand = 0;
                    push_seen(&mut seen, Clause::Limit);
                }
                "OFFSET" => {
                    cur = Clause::Offset;
                    operand = 0;
                    push_seen(&mut seen, Clause::Offset);
                }
                "SET" => {
                    cur = Clause::Set;
                    operand = 0;
                    push_seen(&mut seen, Clause::Set);
                }
                "VALUES" => {
                    cur = Clause::Values;
                    operand = 0;
                }
                "ON" => {
                    cur = Clause::On;
                    operand = 0;
                }
                "USING" => {
                    cur = Clause::Using;
                    operand = 0;
                }
                "JOIN" | "STRAIGHT_JOIN" => {
                    cur = Clause::Join;
                    operand = 0;
                    push_seen(&mut seen, Clause::Join);
                }
                "LEFT" | "RIGHT" | "INNER" | "OUTER" | "CROSS" | "FULL" => {
                    cur = Clause::JoinMod;
                    operand = 0;
                }
                "UPDATE" => {
                    cur = Clause::Update;
                    operand = 0;
                }
                "INSERT" | "REPLACE" => {
                    cur = Clause::Insert;
                    operand = 0;
                }
                "INTO" => {
                    cur = Clause::Into;
                    operand = 0;
                }
                "GROUP" => {
                    if toks
                        .get(i + 1)
                        .and_then(&word_up)
                        .as_deref()
                        .is_some_and(|w| w == "BY")
                    {
                        cur = Clause::GroupBy;
                        push_seen(&mut seen, Clause::GroupBy);
                        i += 1;
                    } else {
                        cur = Clause::Group;
                    }
                    operand = 0;
                }
                "ORDER" => {
                    if toks
                        .get(i + 1)
                        .and_then(&word_up)
                        .as_deref()
                        .is_some_and(|w| w == "BY")
                    {
                        cur = Clause::OrderBy;
                        push_seen(&mut seen, Clause::OrderBy);
                        i += 1;
                    } else {
                        cur = Clause::Order;
                    }
                    operand = 0;
                }
                "BY" => {
                    // A `BY` reached on its own (e.g. `GROUP  BY` with extra space):
                    // complete whichever partial clause preceded it.
                    cur = match cur {
                        Clause::Group => {
                            push_seen(&mut seen, Clause::GroupBy);
                            Clause::GroupBy
                        }
                        Clause::Order => {
                            push_seen(&mut seen, Clause::OrderBy);
                            Clause::OrderBy
                        }
                        other => other,
                    };
                    operand = 0;
                }
                // Boolean connectives keep us in the current clause but reopen the
                // operand slot (a new column/value is expected after them).
                "AND" | "OR" => operand = 0,
                // Modifiers that don't fill the operand slot on their own.
                "AS" | "NOT" | "ALL" | "ASC" | "DESC" => {}
                "DISTINCT" => distinct_seen = true,
                _ => {
                    operand += 1;
                    if cur == Clause::Select {
                        select_has_content = true;
                    }
                }
            }
        } else if matches!(toks[i].kind, TkKind::Comma) {
            // A new list item (projection / FROM list / ORDER BY / VALUES) reopens
            // the operand slot.
            operand = 0;
        }
        i += 1;
    }
    // `*` (and `count(*)`) is a projection but the tokenizer drops it, so scan the
    // raw SELECT-clause span (up to the caret's word) for one to know the projection
    // is non-empty.
    let select_has_star = matches!(cur, Clause::Select)
        && sql
            .get(select_kw_end..scan_end)
            .is_some_and(|s| s.contains('*'));
    (
        kind,
        cur,
        operand,
        seen,
        select_has_content,
        select_has_star,
        distinct_seen,
    )
}

/// The keyword/phrase continuations SQL grammar expects at the caret, and whether
/// the popup should auto-open on an empty prefix. `word_lo` is the byte offset where
/// the caret's current (possibly empty) word begins; only tokens strictly before it
/// are considered, so a phrase like `GROUP BY` is offered while `GROUP` is still
/// being typed. Lexer-based (correct mid-edit) and microsecond-cheap — it runs every
/// keystroke.
pub fn clause_continuation(
    sql: &str,
    lo: usize,
    word_lo: usize,
    dialect: SqlDialect,
) -> Continuation {
    let start = local_scope_start(sql, lo, word_lo, dialect);
    let toks = tokenize_range(sql, start, word_lo, dialect);
    let (kind, cur, operand, seen, select_has_content, select_has_star, distinct_seen) =
        scan_clauses(sql, &toks, word_lo);
    let filled = operand >= 1;
    let has = |c: Clause| seen.contains(&c);
    let mut kws: Vec<&str> = Vec::new();

    // The downstream single-statement clauses, in canonical order, each offered only
    // if not already present — shared by the several "after a complete operand"
    // positions (post table ref, post-condition, …).
    let downstream = |kws: &mut Vec<&str>, from_where: bool| {
        if from_where && !has(Clause::Where) {
            kws.push("WHERE");
        }
        if !has(Clause::GroupBy) {
            kws.push("GROUP BY");
        }
        if !has(Clause::Having) {
            kws.push("HAVING");
        }
        if !has(Clause::OrderBy) {
            kws.push("ORDER BY");
        }
        if !has(Clause::Limit) {
            kws.push("LIMIT");
        }
    };

    match (kind, cur) {
        (StmtKind::Delete, Clause::None) => kws.push("FROM"),
        (_, Clause::Select) => {
            // FROM once the *current* projection item is complete (a column typed,
            // or `*`); a trailing comma reopens the slot (`filled` false → no FROM).
            if filled || select_has_star {
                kws.push("FROM");
            } else if !select_has_content && !distinct_seen {
                // Only right after SELECT with nothing projected yet.
                kws.push("DISTINCT");
            }
        }
        (_, Clause::From) if filled => {
            downstream(&mut kws, true);
            kws.extend(["JOIN", "LEFT JOIN", "RIGHT JOIN", "INNER JOIN", "AS"]);
        }
        (_, Clause::JoinMod) => kws.extend(["JOIN", "OUTER JOIN"]),
        (_, Clause::Join) if filled => {
            kws.extend(["ON", "USING", "AS"]);
            downstream(&mut kws, true);
        }
        (_, Clause::On | Clause::Using) if filled => {
            kws.extend(["AND", "OR"]);
            kws.extend(["JOIN", "LEFT JOIN", "RIGHT JOIN", "INNER JOIN"]);
            downstream(&mut kws, true);
        }
        (_, Clause::Where) if filled => {
            kws.extend([
                "AND",
                "OR",
                "IS NULL",
                "IS NOT NULL",
                "LIKE",
                "IN",
                "BETWEEN",
            ]);
            downstream(&mut kws, false);
        }
        (_, Clause::Group) | (_, Clause::Order) => kws.push("BY"),
        (_, Clause::GroupBy) if filled => downstream(&mut kws, false),
        (_, Clause::Having) if filled => {
            kws.extend(["AND", "OR"]);
            downstream(&mut kws, false);
        }
        (_, Clause::OrderBy) if filled => kws.extend(["ASC", "DESC", "LIMIT"]),
        (_, Clause::Limit) if filled => kws.push("OFFSET"),
        (StmtKind::Update, Clause::Update) if filled => kws.push("SET"),
        (StmtKind::Update, Clause::Set) if filled => kws.push("WHERE"),
        (StmtKind::Insert, Clause::Insert) if !filled => kws.push("INTO"),
        (StmtKind::Insert, Clause::Into) if filled => kws.push("VALUES"),
        _ => {}
    }

    // Auto-open on an empty prefix right after a clause keyword (or comma) that
    // takes an operand — columns after WHERE/ON/BY/SET/HAVING/SELECT, tables after
    // FROM/JOIN/INTO. Not after LIMIT/OFFSET (a bare number) or a filled slot.
    let auto_show = !filled
        && matches!(
            cur,
            Clause::Select
                | Clause::From
                | Clause::Join
                | Clause::On
                | Clause::Using
                | Clause::Where
                | Clause::GroupBy
                | Clause::Having
                | Clause::OrderBy
                | Clause::Set
                | Clause::Into
        );

    Continuation {
        keywords: kws.into_iter().map(|s| s.to_string()).collect(),
        auto_show,
    }
}

/// Parse the tables (and aliases) visible at `caret` using the byte-position
/// lexer + paren-scope chain — the fallback used when the statement doesn't parse
/// as a complete AST. Handles `db.table`, `AS alias`, implicit `table alias`, and
/// comma FROM-lists; scopes to the caret's query or an enclosing one.
fn lexer_scope(
    sql: &str,
    lo: usize,
    hi: usize,
    caret: usize,
    dialect: SqlDialect,
) -> Vec<TableRef> {
    let toks = tokenize_range(sql, lo, hi, dialect);
    let chain = caret_scope_chain(&toks, caret);
    let word = |k: &TkKind| -> Option<String> {
        if let TkKind::Word(w) = k {
            Some(w.clone())
        } else {
            None
        }
    };
    let mut out = Vec::new();
    let mut next_id = 1usize;
    let mut open: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        match &toks[i].kind {
            TkKind::LParen => {
                open.push(next_id);
                next_id += 1;
                i += 1;
                continue;
            }
            TkKind::RParen => {
                open.pop();
                i += 1;
                continue;
            }
            TkKind::Word(w) => {
                let up = w.to_ascii_uppercase();
                let is_from = up == "FROM";
                if !matches!(up.as_str(), "FROM" | "JOIN" | "INTO" | "UPDATE") {
                    i += 1;
                    continue;
                }
                let scope = *open.last().unwrap_or(&0);
                i += 1;
                while let Some(mut name) = toks.get(i).and_then(|t| word(&t.kind)) {
                    if is_reserved_word(&name, dialect) {
                        break;
                    }
                    let mut db = None;
                    i += 1;
                    if matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Dot)) {
                        match toks.get(i + 1).and_then(|t| word(&t.kind)) {
                            Some(second) => {
                                db = Some(name);
                                name = second;
                                i += 2;
                            }
                            // `name.` with no table after the dot — a `db.` qualifier
                            // still being typed. Don't register `name` as a table (that
                            // spurious entry shadowed database-qualified completion);
                            // consume the dot and stop this FROM-list.
                            None => {
                                i += 1;
                                break;
                            }
                        }
                    }
                    let mut alias = None;
                    match toks.get(i).map(|t| &t.kind) {
                        Some(TkKind::Word(a)) if a.eq_ignore_ascii_case("AS") => {
                            if let Some(al) = toks.get(i + 1).and_then(|t| word(&t.kind)) {
                                // A reserved keyword after AS isn't a valid alias
                                // (needs backticks) — don't register it, matching the
                                // implicit-alias arm below. Still consume both tokens.
                                if !is_reserved_word(&al, dialect) {
                                    alias = Some(al);
                                }
                                i += 2;
                            }
                        }
                        Some(TkKind::Word(a)) if !is_reserved_word(a, dialect) => {
                            alias = Some(a.clone());
                            i += 1;
                        }
                        _ => {}
                    }
                    if chain.contains(&scope) {
                        out.push(TableRef { name, alias, db });
                    }
                    if is_from && matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Comma)) {
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    out
}

/// The tables + CTE names visible at `caret` within the statement `sql[lo..hi]`.
///
/// Uses the real AST when the statement parses (robust alias/CTE/`db.table`/
/// derived-table resolution the paren hack got wrong); falls back to the
/// byte-position lexer scope when it doesn't (mid-edit). AST resolution returns
/// the union of table refs across the statement — a superset is the safe
/// direction for both completion (extra candidates rank low) and diagnostics
/// (never a false "unknown column").
pub fn statement_scope(
    sql: &str,
    lo: usize,
    hi: usize,
    caret: usize,
    dialect: SqlDialect,
) -> Scope {
    let stmt = sql.get(lo..hi).unwrap_or("");
    if let Ok(mut asts) = sqlparser::parser::Parser::parse_sql(&*dialect.parser(), stmt)
        && asts.len() == 1
    {
        let mut scope = Scope::default();
        ast_scope::collect_statement(&asts.pop().unwrap(), &mut scope);
        if !scope.tables.is_empty() || !scope.ctes.is_empty() {
            return scope;
        }
    }
    Scope {
        tables: lexer_scope(sql, lo, hi, caret, dialect),
        ctes: Vec::new(),
    }
}

/// AST-walk helpers: collect the table refs + CTE names from a parsed statement.
/// Recurses into subqueries/CTE bodies (union of all refs).
mod ast_scope {
    use super::{Scope, TableRef};
    use sqlparser::ast::{
        Cte, FromTable, Query, Select, SetExpr, Statement, TableFactor, TableObject, TableWithJoins,
    };

    pub(super) fn collect_statement(stmt: &Statement, out: &mut Scope) {
        match stmt {
            Statement::Query(q) => collect_query(q, out),
            Statement::Insert(insert) => {
                if let TableObject::TableName(name) = &insert.table
                    && let Some(r) = table_ref_of(name, None)
                {
                    push_ref(r, out);
                }
                if let Some(src) = &insert.source {
                    collect_query(src, out);
                }
            }
            Statement::Update(u) => {
                // The target table (and any `UPDATE t JOIN …`) — the FROM-source
                // variants are dialect-specific and not needed for scope here.
                collect_twj(&u.table, out);
            }
            Statement::Delete(del) => {
                let tables = match &del.from {
                    FromTable::WithFromKeyword(v) | FromTable::WithoutKeyword(v) => v,
                };
                for twj in tables {
                    collect_twj(twj, out);
                }
            }
            _ => {}
        }
    }

    fn collect_query(q: &Query, out: &mut Scope) {
        if let Some(with) = &q.with {
            for cte in &with.cte_tables {
                collect_cte(cte, out);
            }
        }
        collect_setexpr(&q.body, out);
    }

    fn collect_cte(cte: &Cte, out: &mut Scope) {
        out.ctes.push(cte.alias.name.value.clone());
        collect_query(&cte.query, out);
    }

    fn collect_setexpr(body: &SetExpr, out: &mut Scope) {
        match body {
            SetExpr::Select(sel) => collect_select(sel, out),
            SetExpr::Query(q) => collect_query(q, out),
            SetExpr::SetOperation { left, right, .. } => {
                collect_setexpr(left, out);
                collect_setexpr(right, out);
            }
            _ => {}
        }
    }

    fn collect_select(sel: &Select, out: &mut Scope) {
        for twj in &sel.from {
            collect_twj(twj, out);
        }
    }

    fn collect_twj(twj: &TableWithJoins, out: &mut Scope) {
        collect_factor(&twj.relation, out);
        for join in &twj.joins {
            collect_factor(&join.relation, out);
        }
    }

    fn collect_factor(factor: &TableFactor, out: &mut Scope) {
        match factor {
            TableFactor::Table { name, alias, .. } => {
                let alias = alias.as_ref().map(|a| a.name.value.clone());
                if let Some(r) = table_ref_of(name, alias) {
                    push_ref(r, out);
                }
            }
            TableFactor::Derived {
                subquery, alias, ..
            } => {
                // A derived table's columns aren't in the catalog; keep recursing
                // so the inner real tables still resolve. The alias itself has no
                // catalog columns, so we don't push it as a TableRef.
                let _ = alias;
                collect_query(subquery, out);
            }
            TableFactor::NestedJoin {
                table_with_joins, ..
            } => collect_twj(table_with_joins, out),
            _ => {}
        }
    }

    /// One `ObjectName` → one [`TableRef`], the **only** place that decides how a
    /// parsed name splits into database and table.
    ///
    /// Through [`super::object_name_parts`], per the invariant: `ObjectNamePart`'s
    /// `Display` re-adds the quoting, so a `` `t` ``/`"t"` name comes back
    /// quote-wrapped and never matches the catalog, which is keyed on bare names.
    /// The `INSERT` arm used to do this itself off `Display` plus a
    /// `trim_matches('`')` that knew MySQL's quote character and no other — so a
    /// PostgreSQL target kept its quotes — and split on a raw `.`, which cuts
    /// `"my.table"` in the wrong place. Both are gone by having one implementation.
    fn table_ref_of(name: &sqlparser::ast::ObjectName, alias: Option<String>) -> Option<TableRef> {
        let parts: Vec<String> = super::object_name_parts(name);
        let (db, table) = match parts.as_slice() {
            [t] => (None, t.clone()),
            [d, t] => (Some(d.clone()), t.clone()),
            // db.schema.table etc. → last is the table, prior is its db.
            [.., d, t] => (Some(d.clone()), t.clone()),
            [] => return None,
        };
        Some(TableRef {
            name: table,
            alias,
            db,
        })
    }

    fn push_ref(r: TableRef, out: &mut Scope) {
        if !out
            .tables
            .iter()
            .any(|e| e.name.eq_ignore_ascii_case(&r.name) && e.alias == r.alias)
        {
            out.tables.push(r);
        }
    }
}

// ── Catalog ──────────────────────────────────────────────────────────────────

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::schema::DbSchema;

/// Whether a referenced table can be judged against the catalog.
enum TableStatus {
    /// The table exists in the (loaded) catalog.
    Found,
    /// The relevant database is loaded and does not contain the table.
    NotFound,
    /// The relevant database's schema isn't loaded — we can't judge (no squiggle).
    Unknown,
}

/// A case-folded view over the introspected schema, answering the existence /
/// column questions offline diagnostics need. Built from the *loaded* database
/// schemas plus the tab's active database (which scopes unqualified references).
pub struct Catalog {
    /// (db_lower, table_lower) → column names (original case).
    qualified: HashMap<(String, String), Vec<String>>,
    /// (schema_lower, table_lower) → column names, for a PostgreSQL two-part
    /// `schema.table` reference. Separate from `qualified` because there the
    /// first part is a *database*: the two namespaces are unrelated, and a name
    /// that resolves in neither must stay "can't judge" rather than an error.
    schema_qualified: HashMap<(String, String), Vec<String>>,
    /// Every introspected PostgreSQL namespace (lower). A two-part reference
    /// whose qualifier is in here can be judged; one whose isn't isn't.
    known_schemas: HashSet<String>,
    /// table_lower → column names, for unqualified refs (active-db scoped).
    unqualified: HashMap<String, Vec<String>>,
    /// table_lower → its foreign-key edges (active-db scoped), for the FK-aware
    /// `JOIN … ON` completion.
    fks: HashMap<String, Vec<FkEdge>>,
    /// table_lower → its primary-key column names (lower, active-db scoped), for
    /// the `only_full_group_by` functional-dependency exemption.
    pks: HashMap<String, Vec<String>>,
    /// Loaded database names (lower).
    loaded_dbs: HashSet<String>,
    /// The active database (lower), if any.
    active_db: Option<String>,
    /// All known identifier names (lower) — dbs + tables + columns — used to keep
    /// the keyword-typo check from flagging real schema names.
    known_idents: HashSet<String>,
}

/// A memo over [`Catalog::build`], keyed on the **identity** of the schemas the
/// catalog was built from.
///
/// A keystroke in the editor can ask for the catalog up to four times — empty-prefix
/// column completion, JOIN targets, signature help and diagnostics — and each build
/// is O(tables × columns): 2.2 ms at 500 tables × 20 columns, 8.9 ms at 1500 × 25.
/// Sharing one build across those calls is the whole point of this type.
///
/// **Staleness is unrepresentable here, not merely avoided.** The alternative — a
/// generation counter bumped wherever a schema is written — is a discipline that a
/// new write site can silently break, and a stale catalog is invisible in tests and
/// wrong in the editor. Instead the cache keeps the very `Arc<DbSchema>`s it built
/// from, so:
///
/// * re-introspecting a database replaces that `Arc`, which changes its pointer and
///   misses — there is no site to remember to bump; and
/// * holding a strong reference means no address it compares against can have been
///   freed and reused for a different schema, so pointer equality really does mean
///   "the same schema".
///
/// The cost is that the last-used schemas stay alive until the next call replaces
/// them. Comparison is exact (not case-folded) on the database names and the active
/// database, so a case-only change misses and rebuilds — conservative in the safe
/// direction.
#[derive(Default)]
pub struct CatalogCache {
    /// `None` until the first build.
    entry: Option<CachedCatalog>,
}

/// One built catalog and the inputs it was built from.
struct CachedCatalog {
    /// The `(database, schema)` list, held by `Arc` so its pointers stay meaningful.
    loaded: Vec<(String, Arc<DbSchema>)>,
    active_db: Option<String>,
    catalog: Arc<Catalog>,
}

impl CatalogCache {
    /// The catalog for these loaded schemas, rebuilding only if the schema set,
    /// their order, or the active database differs from the cached one.
    pub fn get(
        &mut self,
        loaded: &[(String, Arc<DbSchema>)],
        active_db: Option<&str>,
    ) -> Arc<Catalog> {
        if let Some(hit) = &self.entry
            && hit.active_db.as_deref() == active_db
            && hit.loaded.len() == loaded.len()
            && hit
                .loaded
                .iter()
                .zip(loaded)
                .all(|((kd, ks), (ld, ls))| kd == ld && Arc::ptr_eq(ks, ls))
        {
            return Arc::clone(&hit.catalog);
        }
        let refs: Vec<(&str, &DbSchema)> = loaded.iter().map(|(d, s)| (d.as_str(), &**s)).collect();
        let catalog = Arc::new(Catalog::build(&refs, active_db));
        self.entry = Some(CachedCatalog {
            loaded: loaded.to_vec(),
            active_db: active_db.map(str::to_string),
            catalog: Arc::clone(&catalog),
        });
        catalog
    }
}

/// A foreign-key edge (this table's `columns` reference `ref_table`'s
/// `ref_columns`, aligned by position).
#[derive(Clone)]
struct FkEdge {
    columns: Vec<String>,
    ref_table: String,
    ref_columns: Vec<String>,
}

impl Catalog {
    /// Build from the loaded `(database, schema)` pairs and the active database.
    /// Only databases whose introspection has completed should be passed — an
    /// absent database is treated as "can't judge" rather than "not found".
    pub fn build(loaded: &[(&str, &DbSchema)], active_db: Option<&str>) -> Catalog {
        let mut qualified = HashMap::new();
        let mut schema_qualified: HashMap<(String, String), Vec<String>> = HashMap::new();
        let mut known_schemas = HashSet::new();
        let mut unqualified: HashMap<String, Vec<String>> = HashMap::new();
        let mut fks: HashMap<String, Vec<FkEdge>> = HashMap::new();
        let mut pks: HashMap<String, Vec<String>> = HashMap::new();
        let mut loaded_dbs = HashSet::new();
        let mut known_idents = HashSet::new();
        let active_lower = active_db.map(|d| d.to_ascii_lowercase());
        for (db, schema) in loaded {
            let db_lower = db.to_ascii_lowercase();
            loaded_dbs.insert(db_lower.clone());
            known_idents.insert(db_lower.clone());
            let in_scope = active_lower
                .as_deref()
                .is_none_or(|a| a == db_lower.as_str());
            for t in &schema.tables {
                let cols: Vec<String> = t.columns.iter().map(|c| c.name.clone()).collect();
                known_idents.insert(t.name.to_ascii_lowercase());
                for c in &cols {
                    known_idents.insert(c.to_ascii_lowercase());
                }
                qualified.insert(
                    (db_lower.clone(), t.name.to_ascii_lowercase()),
                    cols.clone(),
                );
                // PostgreSQL namespace, when the table carries one. Indexed only
                // for the in-scope database: `schema.table` never crosses a
                // database in PG, so an out-of-scope db's namespaces would just
                // be wrong answers.
                if in_scope && let Some(ns) = &t.schema {
                    let ns_lower = ns.to_ascii_lowercase();
                    known_schemas.insert(ns_lower.clone());
                    known_idents.insert(ns_lower.clone());
                    schema_qualified.insert((ns_lower, t.name.to_ascii_lowercase()), cols.clone());
                }
                if in_scope {
                    let entry = unqualified.entry(t.name.to_ascii_lowercase()).or_default();
                    for c in cols {
                        if !entry.iter().any(|e| e.eq_ignore_ascii_case(&c)) {
                            entry.push(c);
                        }
                    }
                    let pk: Vec<String> = t
                        .columns
                        .iter()
                        .filter(|c| c.primary_key)
                        .map(|c| c.name.to_ascii_lowercase())
                        .collect();
                    if !pk.is_empty() {
                        pks.insert(t.name.to_ascii_lowercase(), pk);
                    }
                    if !t.foreign_keys.is_empty() {
                        fks.entry(t.name.to_ascii_lowercase()).or_default().extend(
                            t.foreign_keys.iter().map(|fk| FkEdge {
                                columns: fk.columns.clone(),
                                ref_table: fk.ref_table.clone(),
                                ref_columns: fk.ref_columns.clone(),
                            }),
                        );
                    }
                }
            }
        }
        Catalog {
            qualified,
            schema_qualified,
            known_schemas,
            unqualified,
            fks,
            pks,
            loaded_dbs,
            active_db: active_lower,
            known_idents,
        }
    }

    /// The primary-key columns (lower) of an active-db table, if known and non-empty.
    fn pk_of(&self, table_lower: &str) -> Option<&Vec<String>> {
        self.pks.get(table_lower)
    }

    /// The database whose schema decides an unqualified reference: the active db,
    /// or (no active db) the sole loaded db if there's exactly one.
    fn unqualified_db_loaded(&self) -> bool {
        match &self.active_db {
            Some(a) => self.loaded_dbs.contains(a),
            None => !self.loaded_dbs.is_empty(),
        }
    }

    fn table_status(&self, r: &TableRef) -> TableStatus {
        match &r.db {
            Some(db) => {
                let db_lower = db.to_ascii_lowercase();
                let table_lower = r.name.to_ascii_lowercase();
                // A two-part name is `db.table` on MySQL and `schema.table` on
                // PostgreSQL. Try both: whichever namespace the qualifier names
                // decides, and a hit in either is a hit.
                let key = (db_lower.clone(), table_lower);
                if self.qualified.contains_key(&key) || self.schema_qualified.contains_key(&key) {
                    return TableStatus::Found;
                }
                // Only a qualifier we've actually introspected can be judged
                // absent; anything else is "can't judge".
                if self.loaded_dbs.contains(&db_lower) || self.known_schemas.contains(&db_lower) {
                    TableStatus::NotFound
                } else {
                    TableStatus::Unknown
                }
            }
            None => {
                if !self.unqualified_db_loaded() {
                    return TableStatus::Unknown;
                }
                if self.unqualified.contains_key(&r.name.to_ascii_lowercase()) {
                    TableStatus::Found
                } else {
                    TableStatus::NotFound
                }
            }
        }
    }

    /// The columns of a resolved table reference, if known.
    fn columns_of(&self, r: &TableRef) -> Option<&Vec<String>> {
        match &r.db {
            Some(db) => {
                let key = (db.to_ascii_lowercase(), r.name.to_ascii_lowercase());
                // Same two-part ambiguity as `table_status`: database on MySQL,
                // namespace on PostgreSQL.
                self.qualified
                    .get(&key)
                    .or_else(|| self.schema_qualified.get(&key))
            }
            None => self.unqualified.get(&r.name.to_ascii_lowercase()),
        }
    }
}

// ── Diagnostics ──────────────────────────────────────────────────────────────

/// A diagnostic's severity — drives the squiggle colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// A definite problem (unknown table/column, a syntax error).
    Error,
    /// A heuristic hint (a probable keyword typo).
    Warning,
}

/// One editor diagnostic: a byte range to underline, its severity, and the hover
/// message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: (usize, usize),
    pub severity: Severity,
    pub message: String,
}

/// Convert a 1-based (line, column) — as reported by a `sqlparser` / DB error
/// against `text` — into a byte offset. Column counts characters (SQL is ASCII in
/// practice; multi-byte is handled by char stepping). Clamps to `text.len()`.
pub fn offset_of_line_col(text: &str, line: u64, col: u64) -> usize {
    if line == 0 {
        return 0;
    }
    let mut off = 0usize;
    let mut cur_line = 1u64;
    let bytes = text.as_bytes();
    // Advance to the start of `line`.
    while cur_line < line && off < bytes.len() {
        if bytes[off] == b'\n' {
            cur_line += 1;
        }
        off += 1;
    }
    // Advance `col-1` characters into the line.
    let mut remaining = col.saturating_sub(1);
    while remaining > 0 && off < text.len() {
        let ch = text[off..].chars().next().unwrap();
        if ch == '\n' {
            break;
        }
        off += ch.len_utf8();
        remaining -= 1;
    }
    off.min(text.len())
}

/// Pull the ` at Line: L, Column: C` suffix `sqlparser` appends to its error
/// messages, returning `(line, col)` and the message with the suffix stripped.
fn split_error_location(msg: &str) -> (Option<(u64, u64)>, String) {
    if let Some(idx) = msg.find(" at Line: ") {
        let (head, tail) = msg.split_at(idx);
        // tail = " at Line: L, Column: C"
        let nums: Vec<u64> = tail
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        if let [line, col] = nums.as_slice() {
            let clean = head.trim().trim_end_matches(':').to_string();
            return (Some((*line, *col)), clean);
        }
    }
    (None, msg.to_string())
}

/// The byte range of the word at `off` in `sql` (single-char if not on a word).
fn word_range_at(sql: &str, off: usize) -> (usize, usize) {
    let b = sql.as_bytes();
    let n = sql.len();
    let off = off.min(n);
    if off < n && is_word_byte(b[off]) {
        let mut j = off + 1;
        while j < n && is_word_byte(b[j]) {
            j += 1;
        }
        (off, j)
    } else {
        (off, (off + 1).min(n))
    }
}

/// All FROM/JOIN/UPDATE/INTO table references in `sql[lo..hi]`, each with the byte
/// range of its *table-name* token (positions the AST can't reliably give). Unlike
/// [`lexer_scope`] this ignores paren scoping — for a parsed statement we want
/// every table reference in it.
fn table_refs_with_pos(
    sql: &str,
    lo: usize,
    hi: usize,
    dialect: SqlDialect,
) -> Vec<(TableRef, (usize, usize))> {
    let toks = tokenize_range(sql, lo, hi, dialect);
    let word = |k: &TkKind| -> Option<String> {
        if let TkKind::Word(w) = k {
            Some(w.clone())
        } else {
            None
        }
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let TkKind::Word(w) = &toks[i].kind else {
            i += 1;
            continue;
        };
        let up = w.to_ascii_uppercase();
        let is_from = up == "FROM";
        if !matches!(up.as_str(), "FROM" | "JOIN" | "INTO" | "UPDATE") {
            i += 1;
            continue;
        }
        i += 1;
        while let Some(mut name) = toks.get(i).and_then(|t| word(&t.kind)) {
            if is_reserved_word(&name, dialect) {
                break;
            }
            // The token's own span, not `at + name.len()`: `name` is the
            // *unquoted* text, so recomputing underlines `"NoSuchTb` for a
            // quoted identifier.
            let mut pos = (toks[i].at, toks[i].end);
            let mut db = None;
            i += 1;
            if matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Dot))
                && let Some(second) = toks.get(i + 1).and_then(|t| word(&t.kind))
            {
                db = Some(name);
                name = second;
                pos = (toks[i + 1].at, toks[i + 1].end);
                i += 2;
            }
            let mut alias = None;
            match toks.get(i).map(|t| &t.kind) {
                Some(TkKind::Word(a)) if a.eq_ignore_ascii_case("AS") => {
                    if let Some(al) = toks.get(i + 1).and_then(|t| word(&t.kind)) {
                        // A reserved keyword after AS isn't a valid alias — don't
                        // register it (matches the implicit arm + `lexer_scope`).
                        if !is_reserved_word(&al, dialect) {
                            alias = Some(al);
                        }
                        i += 2;
                    }
                }
                Some(TkKind::Word(a)) if !is_reserved_word(a, dialect) => {
                    alias = Some(a.clone());
                    i += 1;
                }
                _ => {}
            }
            out.push((TableRef { name, alias, db }, pos));
            if is_from && matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Comma)) {
                i += 1;
                continue;
            }
            break;
        }
    }
    out
}

/// Is `word` a likely misspelled SQL keyword? Not a known word (keyword/function/
/// schema ident) but a near-miss of a keyword. Conservative (short words + distant
/// matches ignored) to avoid flagging legitimate identifiers.
fn is_probable_typo(word: &str, known: &HashSet<String>) -> bool {
    if word.len() < 4 {
        return false;
    }
    let lw = word.to_ascii_lowercase();
    if known.contains(&lw) {
        return false;
    }
    let up = word.to_ascii_uppercase();
    let thresh = if word.len() >= 7 { 2 } else { 1 };
    SQL_KEYWORDS.iter().chain(STMT_KEYWORDS).any(|kw| {
        (kw.len() as isize - up.len() as isize).unsigned_abs() <= thresh
            && crate::sql::edit_distance(&up, kw) <= thresh
    })
}

/// Catalog-aware offline diagnostics: instant, no DB round-trip. For each
/// statement in `sql`:
/// - parses it with `dialect`; a parse failure on a *completed* statement (one
///   that isn't the still-being-typed final fragment) yields one syntax error;
/// - on a clean parse, resolves table references (unknown table) and qualified
///   `alias.col` / `table.col` references (unknown column) against `catalog`;
/// - flags probable keyword typos (a heuristic warning), excluding real schema
///   names.
///
/// Byte ranges come from the [`skip_noncode`] lexer, not AST spans (which are
/// still maturing upstream), so squiggle placement is exact.
pub fn diagnostics(sql: &str, catalog: &Catalog, dialect: SqlDialect) -> Vec<Diagnostic> {
    let ranges = crate::sql::statement_ranges(sql, dialect);
    let last = ranges.len().saturating_sub(1);
    let mut out: Vec<Diagnostic> = Vec::new();
    for (idx, &(lo, hi)) in ranges.iter().enumerate() {
        let stmt = &sql[lo..hi];
        let terminated = sql.as_bytes().get(hi - 1) == Some(&b';');
        let is_typing_tail = idx == last && !terminated;
        match sqlparser::parser::Parser::parse_sql(&*dialect.parser(), stmt) {
            Ok(asts) => {
                table_existence_checks(sql, lo, hi, catalog, dialect, &mut out);
                match asts.as_slice() {
                    // A single SELECT/query → per-scope column resolution (aware of
                    // subqueries / derived tables / CTEs; qualified + unqualified).
                    [ast @ sqlparser::ast::Statement::Query(_)] => {
                        colres::check(sql, lo, hi, catalog, ast, &mut out)
                    }
                    // Other statements (UPDATE/DELETE/…) → the flat qualified scan.
                    _ => qualified_column_checks(sql, lo, hi, catalog, dialect, &mut out),
                }
            }
            Err(e) => {
                // Don't nag about the fragment the user is still typing.
                if !is_typing_tail {
                    let (loc, msg) = split_error_location(&e.to_string());
                    let range = match loc {
                        Some((line, col)) => {
                            let at = lo + offset_of_line_col(stmt, line, col);
                            word_range_at(sql, at)
                        }
                        None => (lo, hi),
                    };
                    out.push(Diagnostic {
                        range,
                        severity: Severity::Error,
                        message: friendly_syntax_message(&msg),
                    });
                }
            }
        }
        typo_checks(sql, lo, hi, catalog, dialect, &mut out);
        function_typo_checks(sql, lo, hi, catalog, dialect, &mut out);
        // Reserved-keyword aliases (`orders AS or`, `orders or`) run unconditionally:
        // sqlparser is laxer than MySQL here (it *accepts* `AS or`), so gating on a
        // parse failure would miss the very case we want to flag.
        alias_checks(sql, lo, hi, dialect, &mut out);
    }
    dedup_diagnostics(out)
}

/// Keywords that legitimately follow `AS` without being an alias: a query/CTAS body
/// (`CREATE TABLE t AS SELECT …`, `CREATE VIEW v AS SELECT …`, `cte AS (SELECT …)`).
fn is_query_body_keyword(word: &str) -> bool {
    let up = word.to_ascii_uppercase();
    matches!(up.as_str(), "SELECT" | "WITH" | "VALUES")
}

/// MySQL/MariaDB **reserved** words — those that can't be used as a bare (unquoted)
/// identifier (table/column/alias) without backticks. This is the authoritative list
/// for [`is_reserved_word`] (the alias check + scope's alias resolution); it's a
/// superset of the small [`SQL_KEYWORDS`] completion set and deliberately excludes
/// non-reserved keywords (`OFFSET`, `VIEW`, `TRUNCATE`, …), which *are* legal aliases.
/// Sourced from MySQL 8.0's "Keywords and Reserved Words" (the `(R)` entries), plus
/// `INTERSECT`. The Postgres counterpart is [`PG_RESERVED`]; [`is_reserved_word`]
/// selects between them by [`SqlDialect`].
const MYSQL_RESERVED: &[&str] = &[
    "ACCESSIBLE",
    "ADD",
    "ALL",
    "ALTER",
    "ANALYZE",
    "AND",
    "AS",
    "ASC",
    "ASENSITIVE",
    "BEFORE",
    "BETWEEN",
    "BIGINT",
    "BINARY",
    "BLOB",
    "BOTH",
    "BY",
    "CALL",
    "CASCADE",
    "CASE",
    "CHANGE",
    "CHAR",
    "CHARACTER",
    "CHECK",
    "COLLATE",
    "COLUMN",
    "CONDITION",
    "CONSTRAINT",
    "CONTINUE",
    "CONVERT",
    "CREATE",
    "CROSS",
    "CUBE",
    "CUME_DIST",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "CURRENT_USER",
    "CURSOR",
    "DATABASE",
    "DATABASES",
    "DAY_HOUR",
    "DAY_MICROSECOND",
    "DAY_MINUTE",
    "DAY_SECOND",
    "DEC",
    "DECIMAL",
    "DECLARE",
    "DEFAULT",
    "DELAYED",
    "DELETE",
    "DENSE_RANK",
    "DESC",
    "DESCRIBE",
    "DETERMINISTIC",
    "DISTINCT",
    "DISTINCTROW",
    "DIV",
    "DOUBLE",
    "DROP",
    "DUAL",
    "EACH",
    "ELSE",
    "ELSEIF",
    "EMPTY",
    "ENCLOSED",
    "ESCAPED",
    "EXCEPT",
    "EXISTS",
    "EXIT",
    "EXPLAIN",
    "FALSE",
    "FETCH",
    "FIRST_VALUE",
    "FLOAT",
    "FLOAT4",
    "FLOAT8",
    "FOR",
    "FORCE",
    "FOREIGN",
    "FROM",
    "FULLTEXT",
    "FUNCTION",
    "GENERATED",
    "GET",
    "GRANT",
    "GROUP",
    "GROUPING",
    "GROUPS",
    "HAVING",
    "HIGH_PRIORITY",
    "HOUR_MICROSECOND",
    "HOUR_MINUTE",
    "HOUR_SECOND",
    "IF",
    "IGNORE",
    "IN",
    "INDEX",
    "INFILE",
    "INNER",
    "INOUT",
    "INSENSITIVE",
    "INSERT",
    "INT",
    "INT1",
    "INT2",
    "INT3",
    "INT4",
    "INT8",
    "INTEGER",
    "INTERSECT",
    "INTERVAL",
    "INTO",
    "IO_AFTER_GTIDS",
    "IO_BEFORE_GTIDS",
    "IS",
    "ITERATE",
    "JOIN",
    "JSON_TABLE",
    "KEY",
    "KEYS",
    "KILL",
    "LAG",
    "LAST_VALUE",
    "LATERAL",
    "LEAD",
    "LEADING",
    "LEAVE",
    "LEFT",
    "LIKE",
    "LIMIT",
    "LINEAR",
    "LINES",
    "LOAD",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "LOCK",
    "LONG",
    "LONGBLOB",
    "LONGTEXT",
    "LOOP",
    "LOW_PRIORITY",
    "MASTER_BIND",
    "MASTER_SSL_VERIFY_SERVER_CERT",
    "MATCH",
    "MAXVALUE",
    "MEDIUMBLOB",
    "MEDIUMINT",
    "MEDIUMTEXT",
    "MIDDLEINT",
    "MINUTE_MICROSECOND",
    "MINUTE_SECOND",
    "MOD",
    "MODIFIES",
    "NATURAL",
    "NOT",
    "NO_WRITE_TO_BINLOG",
    "NTH_VALUE",
    "NTILE",
    "NULL",
    "NUMERIC",
    "OF",
    "ON",
    "OPTIMIZE",
    "OPTIMIZER_COSTS",
    "OPTION",
    "OPTIONALLY",
    "OR",
    "ORDER",
    "OUT",
    "OUTER",
    "OUTFILE",
    "OVER",
    "PARTITION",
    "PERCENT_RANK",
    "PRECISION",
    "PRIMARY",
    "PROCEDURE",
    "PURGE",
    "RANGE",
    "RANK",
    "READ",
    "READS",
    "READ_WRITE",
    "REAL",
    "RECURSIVE",
    "REFERENCES",
    "REGEXP",
    "RELEASE",
    "RENAME",
    "REPEAT",
    "REPLACE",
    "REQUIRE",
    "RESIGNAL",
    "RESTRICT",
    "RETURN",
    "REVOKE",
    "RIGHT",
    "RLIKE",
    "ROW",
    "ROWS",
    "ROW_NUMBER",
    "SCHEMA",
    "SCHEMAS",
    "SECOND_MICROSECOND",
    "SELECT",
    "SENSITIVE",
    "SEPARATOR",
    "SET",
    "SHOW",
    "SIGNAL",
    "SMALLINT",
    "SPATIAL",
    "SPECIFIC",
    "SQL",
    "SQLEXCEPTION",
    "SQLSTATE",
    "SQLWARNING",
    "SQL_BIG_RESULT",
    "SQL_CALC_FOUND_ROWS",
    "SQL_SMALL_RESULT",
    "SSL",
    "STARTING",
    "STORED",
    "STRAIGHT_JOIN",
    "SYSTEM",
    "TABLE",
    "TERMINATED",
    "THEN",
    "TINYBLOB",
    "TINYINT",
    "TINYTEXT",
    "TO",
    "TRAILING",
    "TRIGGER",
    "TRUE",
    "UNDO",
    "UNION",
    "UNIQUE",
    "UNLOCK",
    "UNSIGNED",
    "UPDATE",
    "USAGE",
    "USE",
    "USING",
    "UTC_DATE",
    "UTC_TIME",
    "UTC_TIMESTAMP",
    "VALUES",
    "VARBINARY",
    "VARCHAR",
    "VARCHARACTER",
    "VARYING",
    "VIRTUAL",
    "WHEN",
    "WHERE",
    "WHILE",
    "WINDOW",
    "WITH",
    "WRITE",
    "XOR",
    "YEAR_MONTH",
    "ZEROFILL",
];

/// PostgreSQL **reserved** words — those that can't be a bare (unquoted)
/// identifier/alias without double-quoting. The union of Postgres 16's "reserved"
/// and "reserved (can be function or type name)" categories (Appendix C): both are
/// unusable as a plain table/column *alias* (`FROM t inner`, `SELECT 1 AS user` are
/// errors), which is what this list gates. Differs from [`MYSQL_RESERVED`] — e.g.
/// PG reserves `USER`/`OFFSET`/`LATERAL` where MySQL doesn't, and MySQL reserves
/// `UNSIGNED`/`RLIKE`/`DIV` where PG doesn't.
const PG_RESERVED: &[&str] = &[
    "ALL",
    "ANALYSE",
    "ANALYZE",
    "AND",
    "ANY",
    "ARRAY",
    "AS",
    "ASC",
    "ASYMMETRIC",
    "AUTHORIZATION",
    "BINARY",
    "BOTH",
    "CASE",
    "CAST",
    "CHECK",
    "COLLATE",
    "COLLATION",
    "COLUMN",
    "CONCURRENTLY",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "CURRENT_CATALOG",
    "CURRENT_DATE",
    "CURRENT_ROLE",
    "CURRENT_SCHEMA",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "CURRENT_USER",
    "DEFAULT",
    "DEFERRABLE",
    "DESC",
    "DISTINCT",
    "DO",
    "ELSE",
    "END",
    "EXCEPT",
    "FALSE",
    "FETCH",
    "FOR",
    "FOREIGN",
    "FREEZE",
    "FROM",
    "FULL",
    "GRANT",
    "GROUP",
    "HAVING",
    "ILIKE",
    "IN",
    "INITIALLY",
    "INNER",
    "INTERSECT",
    "INTO",
    "IS",
    "ISNULL",
    "JOIN",
    "LATERAL",
    "LEADING",
    "LEFT",
    "LIKE",
    "LIMIT",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "NATURAL",
    "NOT",
    "NOTNULL",
    "NULL",
    "OFFSET",
    "ON",
    "ONLY",
    "OR",
    "ORDER",
    "OUTER",
    "OVERLAPS",
    "PLACING",
    "PRIMARY",
    "REFERENCES",
    "RETURNING",
    "RIGHT",
    "SELECT",
    "SESSION_USER",
    "SIMILAR",
    "SOME",
    "SYMMETRIC",
    "SYSTEM_USER",
    "TABLE",
    "TABLESAMPLE",
    "THEN",
    "TO",
    "TRAILING",
    "TRUE",
    "UNION",
    "UNIQUE",
    "USER",
    "USING",
    "VARIADIC",
    "VERBOSE",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
];

/// SQLite **reserved** words — those it will not accept as a bare **alias**.
///
/// Deliberately much shorter than the other two, and that is not an omission.
/// SQLite's parser *falls back* to treating most of its ~147 keywords as
/// identifiers wherever the grammar allows one (`SELECT key FROM t` is fine), so
/// the set that genuinely can't be an alias is small. This list backs a
/// **warning**, and the module's rule is that uncertainty never yields a false
/// positive — a word wrongly listed here squiggles working SQL, which is worse
/// than missing one the server will reject with a clearer message than ours.
///
/// **It is the alias set, not the identifier set**, and on SQLite those differ:
/// a quoter must ask [`must_quote_ident`], which adds the three words that are
/// refused as a bare table or column name yet accepted as an alias. Both sets are
/// checked against the engine's own keyword table by `db::sqlite`'s
/// `the_reserved_lists_match_what_sqlite_itself_refuses`, so neither is a
/// transcription anybody has to trust.
const SQLITE_RESERVED: &[&str] = &[
    "ADD",
    "ALL",
    "ALTER",
    "AND",
    "AS",
    "AUTOINCREMENT",
    "BETWEEN",
    "CASE",
    "CHECK",
    "COLLATE",
    "COMMIT",
    "CONSTRAINT",
    "CREATE",
    "DEFAULT",
    "DEFERRABLE",
    "DELETE",
    "DISTINCT",
    "DROP",
    "ELSE",
    "ESCAPE",
    "EXCEPT",
    "EXISTS",
    "FOREIGN",
    "FROM",
    "GROUP",
    "HAVING",
    "IN",
    "INDEX",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "ISNULL",
    "JOIN",
    "LIMIT",
    "NOT",
    "NOTHING",
    "NOTNULL",
    "NULL",
    "ON",
    "OR",
    "ORDER",
    "PRIMARY",
    "REFERENCES",
    "RETURNING",
    "SELECT",
    "SET",
    "TABLE",
    "THEN",
    "TO",
    "TRANSACTION",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USING",
    "VALUES",
    "WHEN",
    "WHERE",
];

/// A word that can't be a bare (unquoted) identifier/alias in `dialect` — reserved.
/// Backs the alias diagnostic and the scope's alias resolution so they agree on what
/// counts as a valid alias. See [`MYSQL_RESERVED`] / [`PG_RESERVED`] /
/// [`SQLITE_RESERVED`].
pub fn is_reserved_word(word: &str, dialect: SqlDialect) -> bool {
    let up = word.to_ascii_uppercase();
    let list = match dialect {
        SqlDialect::MySql => MYSQL_RESERVED,
        SqlDialect::Postgres => PG_RESERVED,
        SqlDialect::Sqlite => SQLITE_RESERVED,
    };
    list.contains(&up.as_str())
}

/// Words that can't be a bare **identifier** (a table or column name) but *can*
/// be an alias, so [`is_reserved_word`] must not list them.
///
/// Empty on MySQL and PostgreSQL: there, a reserved word is reserved everywhere,
/// and one list answers both questions. SQLite is the engine where the two
/// questions come apart — see [`must_quote_ident`].
fn alias_ok_but_unquotable(dialect: SqlDialect) -> &'static [&'static str] {
    match dialect {
        SqlDialect::MySql | SqlDialect::Postgres => &[],
        // `CAST(x AS t)`, `IF NOT EXISTS`, `RAISE(ABORT, …)` — each is a bare
        // keyword the parser commits to on sight in a name position, and each is
        // still accepted as an alias, where `AS` has already told it what follows.
        SqlDialect::Sqlite => &["CAST", "IF", "RAISE"],
    }
}

/// Must `word` be quoted to be used as an identifier — a table or column name?
///
/// **This, not [`is_reserved_word`], is the question a quoter asks**, and the two
/// have opposite costs, which is why they are separate. Missing a word here emits
/// SQL that does not parse; listing one wrongly only adds quotes nobody needed.
/// `is_reserved_word` backs a *warning* about an alias, where the trade runs the
/// other way — a word wrongly listed there squiggles working SQL.
///
/// On SQLite the sets genuinely differ. Its parser falls back to treating most of
/// its ~147 keywords as identifiers wherever the grammar allows one, so both sets
/// are small, but `CAST`, `IF` and `RAISE` sit in the gap: refused as a bare
/// column or table name, accepted as an `AS` alias. Both lists are checked
/// against SQLite itself by `db::sqlite`'s
/// `the_reserved_lists_match_what_sqlite_itself_refuses`, which walks the engine's
/// own keyword table rather than a transcription of its documentation.
pub fn must_quote_ident(word: &str, dialect: SqlDialect) -> bool {
    if is_reserved_word(word, dialect) {
        return true;
    }
    let up = word.to_ascii_uppercase();
    alias_ok_but_unquotable(dialect).contains(&up.as_str())
}

/// Keywords that legitimately follow a *table reference* (so a reserved keyword here
/// is a clause/join continuation, not a botched implicit alias): the join family,
/// the clause boundaries, set operations, MySQL locking/index-hint words, `WINDOW`,
/// and `AS` itself. Anything else reserved in that slot (`OR`, `AND`, `IN`, …) was
/// meant as an alias.
fn is_table_ref_continuation(word: &str) -> bool {
    matches!(
        word.to_ascii_uppercase().as_str(),
        "JOIN"
            | "INNER"
            | "LEFT"
            | "RIGHT"
            | "OUTER"
            | "CROSS"
            | "FULL"
            | "NATURAL"
            | "STRAIGHT_JOIN"
            | "ON"
            | "USING"
            | "WHERE"
            | "GROUP"
            | "ORDER"
            | "HAVING"
            | "LIMIT"
            | "OFFSET"
            | "UNION"
            | "EXCEPT"
            | "INTERSECT"
            | "FOR"
            | "LOCK"
            | "USE"
            | "FORCE"
            | "IGNORE"
            | "PARTITION"
            | "WINDOW"
            | "AS"
            // Ends the table reference of a data-modifying statement —
            // `DELETE FROM zap RETURNING *`. Without it the alias check read
            // `RETURNING` as an alias for `zap` and, since it is reserved on
            // PostgreSQL, squiggled the standard archive idiom as broken.
            // MariaDB supports `RETURNING` too, so this is not dialect-gated.
            | "RETURNING"
    )
}

/// Flag a reserved keyword used as an alias — explicit (`orders AS or`, `id AS key`)
/// or implicit (`orders or`) — a syntax error unless backtick-quoted. Runs
/// unconditionally: sqlparser is laxer than MySQL here (it *accepts* `AS or`), so
/// gating on a parse failure would miss it. Only genuinely-reserved words are flagged
/// ([`is_reserved_word`]) and only where an alias is actually expected, so well-formed
/// SQL isn't squiggled.
fn alias_checks(sql: &str, lo: usize, hi: usize, dialect: SqlDialect, out: &mut Vec<Diagnostic>) {
    let toks = tokenize_range(sql, lo, hi, dialect);
    let flag = |out: &mut Vec<Diagnostic>, at: usize, kw: &str| {
        out.push(Diagnostic {
            range: (at, at + kw.len()),
            severity: Severity::Error,
            message: format!(
                "`{kw}` is a reserved keyword and can't be used as an alias (quote it with backticks)"
            ),
        });
    };
    // Only a CTAS / view (`CREATE … AS SELECT`) legitimately puts a query *body*
    // after `AS`; in a plain SELECT, `col AS select` is a reserved-word alias mistake.
    let is_create = toks
        .iter()
        .find_map(|t| match &t.kind {
            TkKind::Word(w) => Some(w.to_ascii_uppercase()),
            _ => None,
        })
        .is_some_and(|w| w == "CREATE");
    // Explicit `AS <reserved>` — table OR column alias, anywhere.
    for w in toks.windows(2) {
        let (TkKind::Word(a), TkKind::Word(b)) = (&w[0].kind, &w[1].kind) else {
            continue;
        };
        if !a.eq_ignore_ascii_case("AS") || !is_reserved_word(b, dialect) {
            continue;
        }
        // A CTAS/view body isn't an alias (`CREATE TABLE t AS SELECT …`).
        if is_create && is_query_body_keyword(b) {
            continue;
        }
        // `AS `select`` is legal — quoting is the whole remedy this diagnostic
        // recommends, so a quoted alias is never the mistake.
        if w[1].quoted {
            continue;
        }
        // The alias must sit immediately after AS (whitespace only between). A
        // skipped quote/comment in the gap means `b` is the *following* token
        // (e.g. FROM) rather than the alias — never flag that.
        let as_end = w[0].end;
        if sql
            .get(as_end..w[1].at)
            .is_some_and(|g| g.trim().is_empty())
        {
            flag(out, w[1].at, b);
        }
    }
    // Implicit `<table> <reserved>` in a FROM / JOIN ref position. Restricted to
    // FROM/JOIN (where implicit aliases are idiomatic — not INSERT INTO / UPDATE,
    // whose VALUES/SET/SELECT would false-trigger) and to keywords that aren't a
    // legitimate table-ref continuation, so `WHERE`/`ORDER`/`ON`/`JOIN`/… are safe.
    let word = |k: &TkKind| -> Option<String> {
        if let TkKind::Word(w) = k {
            Some(w.clone())
        } else {
            None
        }
    };
    let mut i = 0;
    while i < toks.len() {
        let Some(kw) = word(&toks[i].kind) else {
            i += 1;
            continue;
        };
        if !matches!(kw.to_ascii_uppercase().as_str(), "FROM" | "JOIN") {
            i += 1;
            continue;
        }
        let is_from = kw.eq_ignore_ascii_case("FROM");
        i += 1;
        while let Some(name) = toks.get(i).and_then(|t| word(&t.kind)) {
            if is_reserved_word(&name, dialect) {
                break; // a clause keyword, not a table name (`FROM WHERE …` etc.)
            }
            i += 1;
            // Optional `db.table`.
            if matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Dot))
                && toks.get(i + 1).and_then(|t| word(&t.kind)).is_some()
            {
                i += 2;
            }
            // The alias slot right after the table name.
            match toks.get(i).map(|t| &t.kind) {
                Some(TkKind::Word(a)) if a.eq_ignore_ascii_case("AS") => {
                    // Handled by the `AS` scan above; consume `AS` + the next token.
                    i += toks.get(i + 1).map_or(1, |_| 2);
                }
                // A quoted alias is always legal, reserved word or not — and a
                // quoted name is never a clause keyword ending the ref.
                Some(TkKind::Word(_)) if toks[i].quoted => i += 1,
                // A clause/join keyword ends this ref — not an alias (check before
                // `is_reserved_word`, since these are reserved too).
                Some(TkKind::Word(a)) if is_table_ref_continuation(a) => break,
                Some(TkKind::Word(a)) if is_reserved_word(a, dialect) => {
                    flag(out, toks[i].at, a);
                    break;
                }
                Some(TkKind::Word(_)) => i += 1, // a valid alias (identifier or non-reserved word)
                _ => {}
            }
            // A comma continues the FROM list with another table reference.
            if is_from && matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Comma)) {
                i += 1;
                continue;
            }
            break;
        }
    }
}

/// Unknown-table checks: flag a FROM/JOIN/UPDATE/INTO table reference the catalog
/// definitively doesn't contain (only when the relevant database is loaded).
fn table_existence_checks(
    sql: &str,
    lo: usize,
    hi: usize,
    catalog: &Catalog,
    dialect: SqlDialect,
    out: &mut Vec<Diagnostic>,
) {
    // A CTE name is a source this statement declares, so it is never a missing
    // *table* however its body is written. `statement_scope` already collects
    // them unconditionally — including a `DELETE … RETURNING` body, which the
    // column resolver's own collector skips, and that disagreement is what
    // flagged `gone` in the standard archive idiom
    // `WITH gone AS (DELETE FROM t RETURNING *) SELECT count(*) FROM gone`.
    let ctes: HashSet<String> = statement_scope(sql, lo, hi, hi, dialect)
        .ctes
        .into_iter()
        .map(|c| c.to_ascii_lowercase())
        .collect();
    for (r, pos) in table_refs_with_pos(sql, lo, hi, dialect) {
        if r.db.is_none() && ctes.contains(&r.name.to_ascii_lowercase()) {
            continue;
        }
        if let TableStatus::NotFound = catalog.table_status(&r) {
            let where_db =
                r.db.as_deref()
                    .map(|d| format!(" in `{d}`"))
                    .unwrap_or_default();
            out.push(Diagnostic {
                range: pos,
                severity: Severity::Error,
                message: format!("Table `{}` not found{where_db}", r.name),
            });
        }
    }
}

/// Flat qualified-column check (`alias.col` / `table.col`): flag a column that
/// definitively isn't in the resolved table. Used for non-SELECT statements
/// (UPDATE/DELETE/…); a SELECT goes through the per-scope resolver instead.
fn qualified_column_checks(
    sql: &str,
    lo: usize,
    hi: usize,
    catalog: &Catalog,
    dialect: SqlDialect,
    out: &mut Vec<Diagnostic>,
) {
    let refs = table_refs_with_pos(sql, lo, hi, dialect);
    let toks = tokenize_range(sql, lo, hi, dialect);
    for w in toks.windows(3) {
        let (TkKind::Word(q), TkKind::Dot, TkKind::Word(col)) =
            (&w[0].kind, &w[1].kind, &w[2].kind)
        else {
            continue;
        };
        let Some(table) = resolve_qualifier(q, &refs) else {
            continue;
        };
        if let Some(cols) = catalog.columns_of(&table)
            && !cols.iter().any(|c| c.eq_ignore_ascii_case(col))
        {
            out.push(Diagnostic {
                range: (w[2].at, w[2].end),
                severity: Severity::Error,
                message: format!("Column `{col}` not found in `{}`", table.name),
            });
        }
    }
}

/// Per-scope unknown-column resolver for a `SELECT`. Unlike the flat
/// [`qualified_column_checks`], this walks the query's scope tree (subqueries,
/// derived tables, CTEs each get their own scope) and resolves every column
/// reference — qualified *and* unqualified — against the sources visible at that
/// point, honouring correlation (an inner scope can see outer columns).
///
/// Design (kept conservative — never squiggle valid SQL):
/// - **AST for classification, spans for position.** sqlparser 0.62 emits accurate
///   per-identifier spans; each column ref is positioned by its own span (so the same
///   name in an inner vs outer scope is placed independently) — a step past the older
///   "lexer for positions" note now that spans are reliable.
/// - Each `Query` node becomes a scope keyed by its byte range; a column ref is
///   resolved against the innermost containing scope and its ancestors.
/// - A source whose columns can't be fully enumerated (unloaded/unknown base table,
///   a derived table / CTE projecting `*` or an unnamed expression) is **open** — any
///   column against it is allowed, so uncertainty never yields a false positive.
mod colres {
    use std::collections::{HashMap, HashSet};
    use std::ops::ControlFlow;

    use sqlparser::ast::{
        Cte, Expr, GroupByExpr, JoinConstraint, JoinOperator, Query, Select, SelectItem, SetExpr,
        Spanned, Statement, TableAlias, TableFactor, TableWithJoins, Visit, Visitor,
    };
    use sqlparser::tokenizer::Span;

    use super::{Catalog, Diagnostic, Severity, TableRef, TableStatus, offset_of_line_col};

    /// The columns a FROM source exposes, or `Open` when they can't be enumerated.
    #[derive(Clone)]
    enum Cols {
        Known(HashSet<String>),
        Open,
    }

    /// One FROM source: the names it can be qualified by (alias / table name, lower),
    /// its columns, and the base-table name for the diagnostic message (if any).
    struct Src {
        quals: Vec<String>,
        cols: Cols,
        table: Option<String>,
    }

    /// A resolution scope: the byte range it spans, its FROM sources, and the output
    /// aliases of its projection (referenceable unqualified in ORDER BY / HAVING).
    struct Scope {
        range: (usize, usize),
        sources: Vec<Src>,
        proj_aliases: HashSet<String>,
        /// Byte range of this scope's `WHERE` clause, if any. A projection alias may
        /// be referenced in GROUP BY / HAVING / ORDER BY but **not** in WHERE (MySQL /
        /// Postgres both reject it), so an alias-only match inside this range is an
        /// error, not a resolution.
        where_range: Option<(usize, usize)>,
        /// Columns coalesced by a `USING(...)` join — an unqualified reference to
        /// one is unambiguous even when several sources expose it.
        coalesced: HashSet<String>,
        /// A `NATURAL` join is present → its coalesced (common) columns can't be
        /// enumerated cheaply, so ambiguity checks are suppressed for this scope.
        natural: bool,
    }

    /// A column reference to resolve, positioned by its identifier span.
    struct Ref {
        qualifier: Option<String>,
        col: String,
        range: (usize, usize),
    }

    /// Entry point: resolve every column ref in the parsed `SELECT` statement.
    pub(super) fn check(
        sql: &str,
        lo: usize,
        hi: usize,
        catalog: &Catalog,
        ast: &Statement,
        out: &mut Vec<Diagnostic>,
    ) {
        let stmt = &sql[lo..hi];
        let mut c = Collector {
            stmt,
            lo,
            catalog,
            ctes: HashMap::new(),
            scopes: Vec::new(),
            refs: Vec::new(),
            gb: Vec::new(),
        };
        let _ = ast.visit(&mut c);
        for r in &c.refs {
            if let Some(d) = resolve(r, &c.scopes) {
                out.push(d);
            }
        }
        out.append(&mut c.gb);
    }

    struct Collector<'a> {
        stmt: &'a str,
        lo: usize,
        catalog: &'a Catalog,
        /// CTE name (lower) → its output columns. Populated as queries are visited.
        ctes: HashMap<String, Cols>,
        scopes: Vec<Scope>,
        refs: Vec<Ref>,
        /// `only_full_group_by` warnings collected per SELECT scope as it's pushed.
        gb: Vec<Diagnostic>,
    }

    impl Visitor for Collector<'_> {
        type Break = ();

        fn pre_visit_query(&mut self, q: &Query) -> ControlFlow<()> {
            // Register this query's CTEs first (visible to its own FROM + siblings).
            if let Some(with) = &q.with {
                for cte in &with.cte_tables {
                    let cols = cte_cols(cte);
                    self.ctes
                        .insert(cte.alias.name.value.to_ascii_lowercase(), cols);
                }
            }
            match q.body.as_ref() {
                // A single SELECT body: the scope spans the whole *query*, so its
                // ORDER BY / LIMIT (which hang off the Query, not the Select) are
                // covered, with the SELECT's sources.
                SetExpr::Select(sel) => {
                    let range = to_range(self.stmt, self.lo, q.span());
                    self.push_scope(range, sel);
                }
                // A set operation (UNION/EXCEPT/INTERSECT): each branch SELECT is its
                // own scope, keyed by its own (disjoint) span, so a column resolves
                // against the branch it sits in.
                body @ SetExpr::SetOperation { .. } => {
                    let mut selects = Vec::new();
                    collect_selects(body, &mut selects);
                    let mut last_end = self.lo;
                    for sel in &selects {
                        let range = to_range(self.stmt, self.lo, sel.span());
                        last_end = last_end.max(range.1);
                        self.push_scope(range, sel);
                    }
                    // The union's own ORDER BY references the *output* columns (the
                    // first branch's projection names/aliases) and is positioned past
                    // every branch. Cover just that trailing region with a scope whose
                    // single source exposes the output columns, so an unknown one is
                    // flagged — while branch refs (which sit earlier, before `last_end`)
                    // never fall through to it. `SELECT *` outputs are `Open` → unchecked.
                    if q.order_by.is_some()
                        && let Some(first) = selects.first()
                    {
                        let qrange = to_range(self.stmt, self.lo, q.span());
                        self.scopes.push(Scope {
                            range: (last_end, qrange.1),
                            sources: vec![Src {
                                quals: Vec::new(),
                                cols: select_output_cols(first),
                                table: None,
                            }],
                            proj_aliases: HashSet::new(),
                            where_range: None,
                            coalesced: HashSet::new(),
                            natural: false,
                        });
                    }
                }
                // A parenthesized inner query self-handles via its own
                // pre_visit_query; VALUES/… have no columns to resolve.
                _ => {}
            }
            ControlFlow::Continue(())
        }

        fn pre_visit_expr(&mut self, e: &Expr) -> ControlFlow<()> {
            match e {
                Expr::Identifier(id) => self.refs.push(Ref {
                    qualifier: None,
                    col: id.value.to_ascii_lowercase(),
                    range: to_range(self.stmt, self.lo, id.span),
                }),
                Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
                    let col = &parts[parts.len() - 1];
                    let qual = &parts[parts.len() - 2];
                    self.refs.push(Ref {
                        qualifier: Some(qual.value.to_ascii_lowercase()),
                        col: col.value.to_ascii_lowercase(),
                        range: to_range(self.stmt, self.lo, col.span),
                    });
                }
                _ => {}
            }
            ControlFlow::Continue(())
        }
    }

    impl Collector<'_> {
        /// Record a scope for `sel` covering byte `range`, resolving its FROM sources
        /// against the catalog + the CTEs registered so far.
        fn push_scope(&mut self, range: (usize, usize), sel: &Select) {
            let (sources, proj_aliases) = build_sources(sel, self.catalog, &self.ctes);
            let (coalesced, natural) = coalesced_cols(sel);
            group_by_check(
                sel,
                &sources,
                self.catalog,
                self.stmt,
                self.lo,
                &mut self.gb,
            );
            cartesian_check(sel, self.stmt, self.lo, &mut self.gb);
            let where_range = sel
                .selection
                .as_ref()
                .map(|e| to_range(self.stmt, self.lo, e.span()));
            self.scopes.push(Scope {
                range,
                sources,
                proj_aliases,
                where_range,
                coalesced,
                natural,
            });
        }
    }

    /// Gather the leaf SELECTs of a set-expression tree (UNION/EXCEPT/INTERSECT). A
    /// parenthesized branch (`SetExpr::Query`) is left for its own `pre_visit_query`.
    fn collect_selects<'a>(body: &'a SetExpr, out: &mut Vec<&'a Select>) {
        match body {
            SetExpr::Select(s) => out.push(s),
            SetExpr::SetOperation { left, right, .. } => {
                collect_selects(left, out);
                collect_selects(right, out);
            }
            _ => {}
        }
    }

    /// Resolve one column ref against the scopes containing its position (innermost
    /// first). Returns a diagnostic only when the column definitively doesn't exist.
    fn resolve(r: &Ref, scopes: &[Scope]) -> Option<Diagnostic> {
        // Scopes whose range contains this ref, smallest (innermost) first.
        let mut chain: Vec<&Scope> = scopes
            .iter()
            .filter(|s| s.range.0 <= r.range.0 && r.range.1 <= s.range.1)
            .collect();
        chain.sort_by_key(|s| s.range.1 - s.range.0);
        if chain.is_empty() {
            return None;
        }

        match &r.qualifier {
            Some(q) => {
                // Find the source this qualifier names, in this or an enclosing scope.
                for s in &chain {
                    for src in &s.sources {
                        if src.quals.iter().any(|x| x == q) {
                            return match &src.cols {
                                Cols::Open => None,
                                Cols::Known(cols) if cols.contains(&r.col) => None,
                                Cols::Known(_) => Some(err(
                                    r,
                                    &format!(
                                        "Column `{}` not found in `{}`",
                                        r.col,
                                        src.table.clone().unwrap_or_else(|| q.clone())
                                    ),
                                )),
                            };
                        }
                    }
                }
                // Unknown qualifier (db-qualified, or an outer name we didn't model) —
                // don't flag; the table-existence check covers a bad table name.
                None
            }
            None => {
                // Resolve innermost-first (MySQL name resolution): the first scope
                // that supplies the column wins. Within that scope, two *concrete*
                // sources exposing it is an ambiguity error; an unenumerable (`Open`)
                // source means we can't judge, so we stay silent.
                for s in &chain {
                    let mut known_matches = 0usize;
                    let mut has_open = false;
                    for src in &s.sources {
                        match &src.cols {
                            Cols::Open => has_open = true,
                            Cols::Known(cols) if cols.contains(&r.col) => known_matches += 1,
                            Cols::Known(_) => {}
                        }
                    }
                    // Two known sources with the column → ambiguous, unless a
                    // `USING`/`NATURAL` join coalesced it into a single output column.
                    if known_matches >= 2 && !s.natural && !s.coalesced.contains(&r.col) {
                        return Some(err(r, &format!("Column `{}` is ambiguous", r.col)));
                    }
                    if known_matches >= 1 {
                        return None; // resolved unambiguously in this scope
                    }
                    if has_open {
                        return None; // an unenumerable source might provide it
                    }
                    // A projection alias resolves an unqualified ref — except in the
                    // WHERE clause, where an alias isn't in scope yet (only GROUP BY /
                    // HAVING / ORDER BY may use it). An alias-only match inside WHERE
                    // falls through to the not-found flag, matching MySQL / Postgres.
                    if s.proj_aliases.contains(&r.col)
                        && !s
                            .where_range
                            .is_some_and(|w| w.0 <= r.range.0 && r.range.1 <= w.1)
                    {
                        return None;
                    }
                    // Not in this scope → try the enclosing (correlated) scope.
                }
                // Not found and no open source anywhere in the chain → flag. Name the
                // table only when the innermost scope has a single base-table source.
                let where_tbl = match chain[0].sources.as_slice() {
                    [Src { table: Some(t), .. }] => format!(" in `{t}`"),
                    _ => String::new(),
                };
                Some(err(r, &format!("Column `{}` not found{where_tbl}", r.col)))
            }
        }
    }

    fn err(r: &Ref, message: &str) -> Diagnostic {
        Diagnostic {
            range: r.range,
            severity: Severity::Error,
            message: message.to_string(),
        }
    }

    /// A SELECT's FROM sources + projection output aliases.
    fn build_sources(
        sel: &Select,
        catalog: &Catalog,
        ctes: &HashMap<String, Cols>,
    ) -> (Vec<Src>, HashSet<String>) {
        let mut sources = Vec::new();
        for twj in &sel.from {
            add_source(&twj.relation, &mut sources, catalog, ctes);
            for join in &twj.joins {
                add_source(&join.relation, &mut sources, catalog, ctes);
            }
        }
        (sources, proj_aliases(sel))
    }

    /// Warn on an inner `JOIN`/`INNER JOIN`/`STRAIGHT_JOIN` with no `ON`/`USING`
    /// condition — MySQL treats it as a cross join (every row combined), almost
    /// always an accident. An explicit `CROSS JOIN` (a distinct operator) and a
    /// comma-join (a separate `TableWithJoins`, no join node) are intentional and
    /// left alone. Squiggles the joined relation.
    fn cartesian_check(sel: &Select, stmt: &str, lo: usize, out: &mut Vec<Diagnostic>) {
        fn walk(twj: &TableWithJoins, stmt: &str, lo: usize, out: &mut Vec<Diagnostic>) {
            walk_factor(&twj.relation, stmt, lo, out);
            for join in &twj.joins {
                walk_factor(&join.relation, stmt, lo, out);
                if is_unconstrained_inner_join(&join.join_operator) {
                    out.push(Diagnostic {
                        range: to_range(stmt, lo, join.relation.span()),
                        severity: Severity::Warning,
                        message: "JOIN has no ON/USING condition — this is a cross join; \
                                  use CROSS JOIN if that's intended"
                            .to_string(),
                    });
                }
            }
        }
        fn walk_factor(f: &TableFactor, stmt: &str, lo: usize, out: &mut Vec<Diagnostic>) {
            if let TableFactor::NestedJoin {
                table_with_joins, ..
            } = f
            {
                walk(table_with_joins, stmt, lo, out);
            }
        }
        for twj in &sel.from {
            walk(twj, stmt, lo, out);
        }
    }

    /// A `JOIN`/`INNER JOIN`/`STRAIGHT_JOIN` carrying no constraint (the valid-but-
    /// cartesian forms). `CROSS JOIN` is a separate operator; outer joins without a
    /// constraint don't parse in MySQL, so they never reach here.
    fn is_unconstrained_inner_join(op: &JoinOperator) -> bool {
        use JoinOperator::*;
        matches!(
            op,
            Join(JoinConstraint::None)
                | Inner(JoinConstraint::None)
                | StraightJoin(JoinConstraint::None)
        )
    }

    /// `only_full_group_by`: with a `GROUP BY`, every projected column must be a
    /// grouping column or live inside an aggregate. Deliberately conservative — it
    /// fires only for a *single, fully-known base table* (multi-source / derived /
    /// unloaded FROMs are skipped), skips non-column grouping expressions and
    /// wildcards, and exempts the whole query when the grouping set contains the
    /// table's full primary key (functional dependency, MySQL's own exemption).
    fn group_by_check(
        sel: &Select,
        sources: &[Src],
        catalog: &Catalog,
        stmt: &str,
        lo: usize,
        out: &mut Vec<Diagnostic>,
    ) {
        let GroupByExpr::Expressions(exprs, _) = &sel.group_by else {
            return;
        };
        if exprs.is_empty() {
            return;
        }
        // The grouping set, as simple column names. A non-column grouping expression
        // (e.g. `GROUP BY YEAR(hired)`) is too complex to reason about → bail.
        let mut grouped: HashSet<String> = HashSet::new();
        for e in exprs {
            match e {
                Expr::Identifier(id) => {
                    grouped.insert(id.value.to_ascii_lowercase());
                }
                Expr::CompoundIdentifier(parts) if !parts.is_empty() => {
                    grouped.insert(parts[parts.len() - 1].value.to_ascii_lowercase());
                }
                _ => return,
            }
        }
        // Need exactly one base table with a known column set to judge safely.
        let [src] = sources else { return };
        let Cols::Known(known) = &src.cols else {
            return;
        };
        let Some(table) = &src.table else { return };
        // Grouping by the full primary key functionally determines every column.
        if let Some(pk) = catalog.pk_of(&table.to_ascii_lowercase())
            && pk.iter().all(|k| grouped.contains(k))
        {
            return;
        }
        for item in &sel.projection {
            let expr = match item {
                SelectItem::UnnamedExpr(e) => e,
                SelectItem::ExprWithAlias { expr, .. } => expr,
                _ => continue, // `*` / `t.*` → can't enumerate, skip
            };
            let (name, span) = match expr {
                Expr::Identifier(id) => (id.value.to_ascii_lowercase(), id.span),
                Expr::CompoundIdentifier(parts) if !parts.is_empty() => {
                    let last = &parts[parts.len() - 1];
                    (last.value.to_ascii_lowercase(), last.span)
                }
                _ => continue, // aggregate / expression / literal → not a bare column
            };
            if grouped.contains(&name) || !known.contains(&name) {
                // Grouped (fine) or not a real column of this table (an
                // unknown-column error already covers it — don't double-flag).
                continue;
            }
            out.push(Diagnostic {
                range: to_range(stmt, lo, span),
                severity: Severity::Warning,
                message: format!(
                    "Column `{name}` must appear in GROUP BY or be used in an aggregate"
                ),
            });
        }
    }

    /// Columns coalesced by a `USING(...)` join in this SELECT's FROM (lowercased),
    /// plus whether a `NATURAL` join is present. A reference to a coalesced column is
    /// unambiguous even when several sources expose it; NATURAL suppresses the check
    /// entirely (its common-column set isn't enumerated here).
    fn coalesced_cols(sel: &Select) -> (HashSet<String>, bool) {
        let mut cols = HashSet::new();
        let mut natural = false;
        fn walk(twj: &TableWithJoins, cols: &mut HashSet<String>, natural: &mut bool) {
            walk_factor(&twj.relation, cols, natural);
            for join in &twj.joins {
                walk_factor(&join.relation, cols, natural);
                constraint(&join.join_operator, cols, natural);
            }
        }
        fn walk_factor(f: &TableFactor, cols: &mut HashSet<String>, natural: &mut bool) {
            if let TableFactor::NestedJoin {
                table_with_joins, ..
            } = f
            {
                walk(table_with_joins, cols, natural);
            }
        }
        fn constraint(op: &JoinOperator, cols: &mut HashSet<String>, natural: &mut bool) {
            match join_constraint(op) {
                Some(JoinConstraint::Using(names)) => {
                    for n in names {
                        if let Some(id) = n.0.last().and_then(|p| p.as_ident()) {
                            cols.insert(id.value.to_ascii_lowercase());
                        }
                    }
                }
                Some(JoinConstraint::Natural) => *natural = true,
                _ => {}
            }
        }
        for twj in &sel.from {
            walk(twj, &mut cols, &mut natural);
        }
        (cols, natural)
    }

    /// The `JoinConstraint` carried by a join operator, if any (the `APPLY`/`ARRAY
    /// JOIN` forms carry none).
    fn join_constraint(op: &JoinOperator) -> Option<&JoinConstraint> {
        use JoinOperator::*;
        match op {
            Join(c) | Inner(c) | Left(c) | LeftOuter(c) | Right(c) | RightOuter(c)
            | FullOuter(c) | CrossJoin(c) | Semi(c) | LeftSemi(c) | RightSemi(c) | Anti(c)
            | LeftAnti(c) | RightAnti(c) | StraightJoin(c) => Some(c),
            AsOf { constraint, .. } => Some(constraint),
            CrossApply | OuterApply | ArrayJoin | LeftArrayJoin | InnerArrayJoin => None,
        }
    }

    fn add_source(
        factor: &TableFactor,
        sources: &mut Vec<Src>,
        catalog: &Catalog,
        ctes: &HashMap<String, Cols>,
    ) {
        match factor {
            TableFactor::Table {
                name, alias, args, ..
            } => {
                // A table-valued function's columns are unknowable → open.
                if args.is_some() {
                    sources.push(open_src());
                    return;
                }
                let parts: Vec<String> = super::object_name_parts(name);
                let (db, tname) = match parts.as_slice() {
                    [t] => (None, t.clone()),
                    [.., d, t] => (Some(d.clone()), t.clone()),
                    [] => return,
                };
                let alias_name = alias.as_ref().map(|a| a.name.value.clone());
                let quals: Vec<String> = std::iter::once(&tname)
                    .chain(alias_name.as_ref())
                    .map(|s| s.to_ascii_lowercase())
                    .collect();
                // A bare name matching a CTE resolves to the CTE's columns.
                if db.is_none()
                    && let Some(cols) = ctes.get(&tname.to_ascii_lowercase())
                {
                    sources.push(Src {
                        quals,
                        cols: cols.clone(),
                        table: None,
                    });
                    return;
                }
                let tref = TableRef {
                    name: tname.clone(),
                    alias: alias_name,
                    db,
                };
                let cols = match catalog.table_status(&tref) {
                    TableStatus::Found => catalog
                        .columns_of(&tref)
                        .map(|c| Cols::Known(c.iter().map(|s| s.to_ascii_lowercase()).collect()))
                        .unwrap_or(Cols::Open),
                    _ => Cols::Open, // not loaded / not found → can't judge its columns
                };
                sources.push(Src {
                    quals,
                    cols,
                    table: Some(tname),
                });
            }
            TableFactor::Derived {
                subquery, alias, ..
            } => {
                let quals = alias
                    .as_ref()
                    .map(|a| vec![a.name.value.to_ascii_lowercase()])
                    .unwrap_or_default();
                sources.push(Src {
                    quals,
                    cols: output_cols(subquery),
                    table: None,
                });
            }
            TableFactor::NestedJoin {
                table_with_joins, ..
            } => {
                add_source(&table_with_joins.relation, sources, catalog, ctes);
                for join in &table_with_joins.joins {
                    add_source(&join.relation, sources, catalog, ctes);
                }
            }
            _ => sources.push(open_src()),
        }
    }

    /// The output column names of a subquery/CTE body, or `Open` when they can't be
    /// cleanly enumerated (`SELECT *`, a set-op, or an unnamed non-column expression).
    fn output_cols(q: &Query) -> Cols {
        match q.body.as_ref() {
            SetExpr::Select(sel) => select_output_cols(sel),
            _ => Cols::Open,
        }
    }

    /// The output column names of a single `SELECT`'s projection — each item's alias,
    /// or the bare column name for an unaliased identifier. `Open` when any item can't
    /// be named (`*`, `t.*`, multi-alias, or an unnamed expression like `a + b`).
    fn select_output_cols(sel: &Select) -> Cols {
        let mut names = HashSet::new();
        for item in &sel.projection {
            match item {
                SelectItem::UnnamedExpr(Expr::Identifier(id)) => {
                    names.insert(id.value.to_ascii_lowercase());
                }
                SelectItem::UnnamedExpr(Expr::CompoundIdentifier(parts)) if !parts.is_empty() => {
                    names.insert(parts[parts.len() - 1].value.to_ascii_lowercase());
                }
                SelectItem::ExprWithAlias { alias, .. } => {
                    names.insert(alias.value.to_ascii_lowercase());
                }
                // `*`, `t.*`, multi-alias, or an unnamed expression → can't enumerate.
                _ => return Cols::Open,
            }
        }
        Cols::Known(names)
    }

    /// A CTE's output columns: its explicit column list if given, else its body's.
    fn cte_cols(cte: &Cte) -> Cols {
        let TableAlias { columns, .. } = &cte.alias;
        if !columns.is_empty() {
            return Cols::Known(
                columns
                    .iter()
                    .map(|c| c.name.value.to_ascii_lowercase())
                    .collect(),
            );
        }
        output_cols(&cte.query)
    }

    fn proj_aliases(sel: &Select) -> HashSet<String> {
        let mut out = HashSet::new();
        for item in &sel.projection {
            match item {
                SelectItem::ExprWithAlias { alias, .. } => {
                    out.insert(alias.value.to_ascii_lowercase());
                }
                SelectItem::ExprWithAliases { aliases, .. } => {
                    out.extend(aliases.iter().map(|a| a.value.to_ascii_lowercase()));
                }
                _ => {}
            }
        }
        out
    }

    fn open_src() -> Src {
        Src {
            quals: Vec::new(),
            cols: Cols::Open,
            table: None,
        }
    }

    /// A sqlparser span (line/col, relative to `stmt`) → absolute byte range in the
    /// buffer. Spans are 1-based, end-exclusive; `offset_of_line_col` clamps.
    fn to_range(stmt: &str, lo: usize, s: Span) -> (usize, usize) {
        (
            lo + offset_of_line_col(stmt, s.start.line, s.start.column),
            lo + offset_of_line_col(stmt, s.end.line, s.end.column),
        )
    }
}

/// Resolve a `qualifier` (before a `.`) to a table reference: an in-scope alias
/// first, else a table referenced by that bare name.
fn resolve_qualifier(q: &str, refs: &[(TableRef, (usize, usize))]) -> Option<TableRef> {
    for (r, _) in refs {
        if r.alias
            .as_deref()
            .is_some_and(|a| a.eq_ignore_ascii_case(q))
        {
            return Some(r.clone());
        }
    }
    for (r, _) in refs {
        if r.alias.is_none() && r.name.eq_ignore_ascii_case(q) {
            return Some(r.clone());
        }
    }
    None
}

/// Probable keyword-typo warnings across `sql[lo..hi]` (skips the identifier after
/// a `.` and real schema names).
fn typo_checks(
    sql: &str,
    lo: usize,
    hi: usize,
    catalog: &Catalog,
    dialect: SqlDialect,
    out: &mut Vec<Diagnostic>,
) {
    let mut known: HashSet<String> = SQL_KEYWORDS
        .iter()
        .chain(STMT_KEYWORDS.iter())
        .map(|k| k.to_ascii_lowercase())
        .chain(function_names().map(|f| f.to_ascii_lowercase()))
        .collect();
    known.extend(catalog.known_idents.iter().cloned());

    let b = sql.as_bytes();
    let mut i = lo;
    while i < hi {
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j.min(hi);
            continue;
        }
        let c = b[i];
        if is_word_start(c) {
            let s = i;
            let mut j = i + 1;
            while j < hi && is_word_byte(b[j]) {
                j += 1;
            }
            let qualified = s > 0 && b[s - 1] == b'.';
            if !qualified && is_probable_typo(&sql[s..j], &known) {
                out.push(Diagnostic {
                    range: (s, j),
                    severity: Severity::Warning,
                    message: format!("`{}` looks like a misspelled keyword", &sql[s..j]),
                });
            }
            i = j;
            continue;
        }
        i += 1;
    }
}

/// Flag a word in **function-call position** (`word(`) that is a near-miss of a
/// known builtin but isn't itself one — a probable typo like `COUTN(...)` for
/// `COUNT(...)`. Conservative: the name must be within edit distance of an entry in
/// the [`FUNCTIONS`] catalog, so user-defined functions and unlisted builtins pass
/// through untouched; qualified calls (`pkg.func(`) and real schema identifiers are
/// skipped too.
fn function_typo_checks(
    sql: &str,
    lo: usize,
    hi: usize,
    catalog: &Catalog,
    dialect: SqlDialect,
    out: &mut Vec<Diagnostic>,
) {
    let b = sql.as_bytes();
    let mut i = lo;
    while i < hi {
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j.min(hi);
            continue;
        }
        let c = b[i];
        if is_word_start(c) {
            let s = i;
            let mut j = i + 1;
            while j < hi && is_word_byte(b[j]) {
                j += 1;
            }
            // A call only if the next non-blank byte is `(`.
            let mut k = j;
            while k < hi && (b[k] == b' ' || b[k] == b'\t') {
                k += 1;
            }
            let is_call = k < hi && b[k] == b'(';
            let word = &sql[s..j];
            let lw = word.to_ascii_lowercase();
            let qualified = s > lo && b[s - 1] == b'.';
            if is_call
                && !qualified
                && !is_known_function(&lw)
                && !is_sql_keyword(word)
                && !STMT_KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(word))
                && !catalog.known_idents.contains(&lw)
                && is_probable_function_typo(word)
            {
                out.push(Diagnostic {
                    range: (s, j),
                    severity: Severity::Warning,
                    message: format!("`{word}` looks like a misspelled function"),
                });
            }
            i = j;
            continue;
        }
        i += 1;
    }
}

/// Case-insensitive membership in the builtin function catalog.
fn is_known_function(word_lower: &str) -> bool {
    function_names().any(|f| f.eq_ignore_ascii_case(word_lower))
}

/// Is `word` a near-miss of a known builtin function name? A near-miss is a small
/// Levenshtein distance (1, or 2 for longer names) *or* a single adjacent
/// transposition (`COUTN`↔`COUNT`) — the latter is distance 2 under plain
/// Levenshtein but by far the most common typo, so it's matched explicitly rather
/// than by loosening the distance threshold (which would flag names like
/// `format_x` as a typo of `FORMAT`).
fn is_probable_function_typo(word: &str) -> bool {
    if word.len() < 4 {
        return false;
    }
    let up = word.to_ascii_uppercase();
    let thresh = if word.len() >= 7 { 2 } else { 1 };
    function_names().any(|f| {
        let close = (f.len() as isize - up.len() as isize).unsigned_abs() <= thresh
            && crate::sql::edit_distance(&up, f) <= thresh;
        close || is_adjacent_transposition(up.as_bytes(), f.as_bytes())
    })
}

/// True when `a` becomes `b` by swapping exactly one adjacent pair of characters.
fn is_adjacent_transposition(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let diffs: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
    diffs.len() == 2
        && diffs[1] == diffs[0] + 1
        && a[diffs[0]] == b[diffs[1]]
        && a[diffs[1]] == b[diffs[0]]
}

/// Tidy a raw `sqlparser` error into a short editor message.
fn friendly_syntax_message(msg: &str) -> String {
    let m = msg
        .trim()
        .strip_prefix("sql parser error:")
        .unwrap_or(msg)
        .trim();
    if m.is_empty() {
        "Syntax error".to_string()
    } else {
        format!("Syntax error: {m}")
    }
}

/// Drop diagnostics whose range is fully covered by an earlier, higher-or-equal
/// severity one (e.g. a keyword typo under a syntax squiggle), so a token isn't
/// double-underlined.
fn dedup_diagnostics(mut v: Vec<Diagnostic>) -> Vec<Diagnostic> {
    // Errors first so a Warning on the same span is the one dropped.
    v.sort_by(|a, b| {
        a.range
            .0
            .cmp(&b.range.0)
            .then((a.severity == Severity::Warning).cmp(&(b.severity == Severity::Warning)))
    });
    let mut out: Vec<Diagnostic> = Vec::new();
    for d in v {
        let covered = out
            .iter()
            .any(|e| e.range.0 <= d.range.0 && d.range.1 <= e.range.1);
        if !covered {
            out.push(d);
        }
    }
    out
}

// ── FK-aware JOIN … ON completion ────────────────────────────────────────────

impl Catalog {
    /// The foreign-key edges declared on `table` (case-insensitive), if any.
    fn fks_of(&self, table: &str) -> Option<&Vec<FkEdge>> {
        self.fks.get(&table.to_ascii_lowercase())
    }

    /// All FK entries (`table_lower` → its edges) — for enumerating *reverse* FK
    /// relationships (a table that references one already in scope).
    fn all_fks(&self) -> impl Iterator<Item = (&String, &Vec<FkEdge>)> {
        self.fks.iter()
    }
}

/// The qualifier to write for a table reference in a predicate: its alias if it
/// has one, else its (bare) name.
fn ref_qualifier(r: &TableRef) -> &str {
    r.alias.as_deref().unwrap_or(&r.name)
}

/// Build the equality predicate joining `left`'s `lcols` to `right`'s `rcols`
/// (aligned by position), e.g. `o.customer_id = c.id` (composite → `AND`-joined).
fn build_predicate(
    left: &TableRef,
    lcols: &[String],
    right: &TableRef,
    rcols: &[String],
    dialect: SqlDialect,
) -> String {
    let q = |s: &str| crate::export::ident_if_needed(s, dialect);
    lcols
        .iter()
        .zip(rcols)
        .map(|(lc, rc)| {
            format!(
                "{}.{} = {}.{}",
                q(ref_qualifier(left)),
                q(lc),
                q(ref_qualifier(right)),
                q(rc)
            )
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// The FK predicate linking two table references, in either direction (`a`→`b` or
/// `b`→`a`), or `None` if no foreign key connects them.
fn fk_predicate(
    catalog: &Catalog,
    a: &TableRef,
    b: &TableRef,
    dialect: SqlDialect,
) -> Option<String> {
    if let Some(edges) = catalog.fks_of(&a.name) {
        for e in edges {
            if e.ref_table.eq_ignore_ascii_case(&b.name) {
                return Some(build_predicate(a, &e.columns, b, &e.ref_columns, dialect));
            }
        }
    }
    if let Some(edges) = catalog.fks_of(&b.name) {
        for e in edges {
            if e.ref_table.eq_ignore_ascii_case(&a.name) {
                return Some(build_predicate(b, &e.columns, a, &e.ref_columns, dialect));
            }
        }
    }
    None
}

/// If the caret sits in a *fresh* `JOIN … ON` (right after `ON`, nothing typed
/// yet), and a foreign key connects the just-joined table to another table in
/// scope, return the ready-to-insert join predicate (e.g. `o.customer_id = c.id`).
/// This is the DataGrip-style auto-join: the completion layer offers it as the top
/// suggestion. `None` when the caret isn't in an empty ON, or no FK links the
/// tables (the user then completes the condition by hand).
pub fn join_condition(
    sql: &str,
    lo: usize,
    hi: usize,
    caret: usize,
    catalog: &Catalog,
    dialect: SqlDialect,
) -> Option<String> {
    let toks = tokenize_range(sql, lo, hi, dialect);
    // The last clause keyword strictly before the caret must be `ON`.
    let mut last_kw_idx = None;
    for (i, t) in toks.iter().enumerate() {
        if t.at >= caret {
            break;
        }
        if let TkKind::Word(w) = &t.kind
            && CLAUSE_KEYWORDS.contains(&w.to_ascii_uppercase().as_str())
        {
            last_kw_idx = Some(i);
        }
    }
    let on_idx = last_kw_idx?;
    let TkKind::Word(kw) = &toks[on_idx].kind else {
        return None;
    };
    if !kw.eq_ignore_ascii_case("ON") {
        return None;
    }
    // The ON expression must be empty (no word typed between `ON` and the caret) —
    // we only auto-fill a blank condition, never overwrite a hand-typed one.
    for t in &toks[on_idx + 1..] {
        if t.at >= caret {
            break;
        }
        if matches!(t.kind, TkKind::Word(_)) {
            return None;
        }
    }
    // The JOIN this ON belongs to, and the table it introduced.
    let join_idx = toks[..on_idx]
        .iter()
        .rposition(|t| matches!(&t.kind, TkKind::Word(w) if w.eq_ignore_ascii_case("JOIN")))?;
    let join_at = toks[join_idx].at;
    let refs = table_refs_with_pos(sql, lo, hi, dialect);
    let joined = refs
        .iter()
        .filter(|(_, p)| p.0 > join_at)
        .min_by_key(|(_, p)| p.0)
        .map(|(r, _)| r.clone())?;
    // Pair it with each other in-scope table; first FK match wins.
    for (other, _) in &refs {
        if other.name.eq_ignore_ascii_case(&joined.name) && other.alias == joined.alias {
            continue;
        }
        if let Some(pred) = fk_predicate(catalog, &joined, other, dialect) {
            return Some(pred);
        }
    }
    None
}

/// A foreign-key-connected table to offer at a `JOIN` slot: the table to insert and
/// the ready-to-write `ON` predicate linking it to a table already in scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinTarget {
    /// Candidate table name to join, bare — what the popup shows and matches the
    /// typed prefix against. Quoting it here would put a leading `"` in front of
    /// every mixed-case PostgreSQL name the user is trying to type.
    pub table: String,
    /// The same name quoted for the dialect where that matters — what gets
    /// **inserted**, ahead of the predicate.
    pub table_sql: String,
    /// The `ON` predicate (without the `ON` keyword), e.g. `orders.customer_id = c.id`.
    pub predicate: String,
}

/// FK-aware JOIN targets for completion. When the caret sits at a `JOIN` table slot
/// (the last clause keyword before the partial name is `JOIN`, nothing else typed),
/// returns every table connected by a foreign key — in either direction — to a table
/// already in scope, each with a ready-to-insert `ON` predicate. The completion layer
/// offers these at the top of the table list, inserting `table ON <predicate>`.
/// Empty when the caret isn't at a JOIN slot or nothing is FK-connected.
pub fn join_targets(
    sql: &str,
    lo: usize,
    hi: usize,
    caret: usize,
    catalog: &Catalog,
    dialect: SqlDialect,
) -> Vec<JoinTarget> {
    let toks = tokenize_range(sql, lo, hi, dialect);
    let word_lo = word_start(sql, caret);
    // The last clause keyword strictly before the partial table name must be `JOIN`.
    let mut join_idx = None;
    for (i, t) in toks.iter().enumerate() {
        if t.at >= word_lo {
            break;
        }
        if let TkKind::Word(w) = &t.kind
            && CLAUSE_KEYWORDS.contains(&w.to_ascii_uppercase().as_str())
        {
            join_idx = w.eq_ignore_ascii_case("JOIN").then_some(i);
        }
    }
    let Some(join_idx) = join_idx else {
        return Vec::new();
    };
    // No complete table already typed between `JOIN` and the partial prefix.
    for t in &toks[join_idx + 1..] {
        if t.at >= word_lo {
            break;
        }
        if matches!(t.kind, TkKind::Word(_)) {
            return Vec::new();
        }
    }
    let scope = statement_scope(sql, lo, hi, caret, dialect).tables;
    join_targets_for(&scope, catalog, dialect)
}

/// The FK-connected join candidates for a set of in-scope tables (both edge
/// directions), deduped by candidate table.
fn join_targets_for(scope: &[TableRef], catalog: &Catalog, dialect: SqlDialect) -> Vec<JoinTarget> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let in_scope = |name: &str| scope.iter().any(|s| s.name.eq_ignore_ascii_case(name));
    // Forward: an in-scope table's FK references candidate `T`.
    for s in scope {
        if let Some(edges) = catalog.fks_of(&s.name) {
            for e in edges {
                if in_scope(&e.ref_table) || !seen.insert(e.ref_table.to_ascii_lowercase()) {
                    continue;
                }
                let cand = TableRef {
                    name: e.ref_table.clone(),
                    alias: None,
                    db: None,
                };
                out.push(JoinTarget {
                    table: e.ref_table.clone(),
                    table_sql: crate::export::ident_if_needed(&e.ref_table, dialect),
                    predicate: build_predicate(s, &e.columns, &cand, &e.ref_columns, dialect),
                });
            }
        }
    }
    // Reverse: a candidate table `T` has an FK referencing an in-scope table.
    for (t, edges) in catalog.all_fks() {
        if in_scope(t) || seen.contains(t) {
            continue;
        }
        for e in edges {
            if let Some(s) = scope
                .iter()
                .find(|s| e.ref_table.eq_ignore_ascii_case(&s.name))
            {
                let cand = TableRef {
                    name: t.clone(),
                    alias: None,
                    db: None,
                };
                if seen.insert(t.clone()) {
                    out.push(JoinTarget {
                        table: t.clone(),
                        table_sql: crate::export::ident_if_needed(t, dialect),
                        predicate: build_predicate(&cand, &e.columns, s, &e.ref_columns, dialect),
                    });
                }
                break;
            }
        }
    }
    out
}

/// A `SELECT *` / `t.*` expansion: the byte range of the star (or `qualifier.*`) and
/// the explicit column list to replace it with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StarExpansion {
    /// Byte range to replace (the `*`, or `t.*` for a qualified star).
    pub range: (usize, usize),
    /// The comma-separated column list, e.g. `id, name` or `e.id, e.name, d.id`.
    pub replacement: String,
}

/// If the caret sits right after a projection `*` (or `t.*`), return the explicit
/// column list to expand it to. Columns are qualified by alias/table name when more
/// than one table is in scope (or the star is qualified), unqualified for a single
/// table. Conservative: only a genuine projection star expands — a `COUNT(*)`
/// argument, a multiplication (`a * b`), or a star outside the SELECT list is left
/// alone; an in-scope table with unknown (unloaded) columns aborts the whole
/// expansion rather than emit a partial list.
pub fn expand_star(
    sql: &str,
    lo: usize,
    hi: usize,
    caret: usize,
    catalog: &Catalog,
    dialect: SqlDialect,
) -> Option<StarExpansion> {
    let b = sql.as_bytes();
    // The caret must sit right after a `*` (trailing whitespace allowed).
    let mut p = caret.min(hi);
    while p > lo && matches!(b[p - 1], b' ' | b'\t' | b'\n' | b'\r') {
        p -= 1;
    }
    if p == lo || b[p - 1] != b'*' {
        return None;
    }
    let star_end = p;
    let star = p - 1;
    // A `qualifier.` immediately before the star → a qualified `t.*`.
    let (range_start, qualifier) = if star > lo && b[star - 1] == b'.' {
        let dot = star - 1;
        let mut s = dot;
        while s > lo && is_word_byte(b[s - 1]) {
            s -= 1;
        }
        if s < dot {
            (s, Some(sql[s..dot].to_string()))
        } else {
            (star, None)
        }
    } else {
        (star, None)
    };
    let toks = tokenize_range(sql, lo, hi, dialect);
    // Must be in the SELECT projection clause.
    let mut last_kw = None;
    for t in &toks {
        if t.at >= star {
            break;
        }
        if let TkKind::Word(w) = &t.kind
            && CLAUSE_KEYWORDS.contains(&w.to_ascii_uppercase().as_str())
        {
            last_kw = Some(w.to_ascii_uppercase());
        }
    }
    if last_kw.as_deref() != Some("SELECT") {
        return None;
    }
    // For a plain star, the immediately-preceding token must be `SELECT`/`DISTINCT`
    // or a comma — otherwise it's an operator (`a * b`) or a function arg (`COUNT(*)`).
    if qualifier.is_none() {
        let prev = toks.iter().rfind(|t| t.at < star);
        let standalone = match prev.map(|t| &t.kind) {
            Some(TkKind::Word(w)) => {
                w.eq_ignore_ascii_case("SELECT") || w.eq_ignore_ascii_case("DISTINCT")
            }
            Some(TkKind::Comma) => true,
            _ => false,
        };
        if !standalone {
            return None;
        }
    }
    let scope = statement_scope(sql, lo, hi, caret, dialect).tables;
    if scope.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    // Quoted per dialect: the expansion is spliced into the editor and run, and
    // PostgreSQL folds a bare `ArtistId` to `artistid`, which resolves to nothing.
    let q = |s: &str| crate::export::ident_if_needed(s, dialect);
    match &qualifier {
        Some(qual) => {
            let tref = scope.iter().find(|r| {
                r.alias
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(qual))
                    || (r.alias.is_none() && r.name.eq_ignore_ascii_case(qual))
            })?;
            for c in catalog.columns_of(tref)? {
                parts.push(format!("{}.{}", q(qual), q(c)));
            }
        }
        None => {
            let multi = scope.len() > 1;
            for r in &scope {
                let cols = catalog.columns_of(r)?; // any unknown → bail (conservative)
                let qual = r.alias.clone().unwrap_or_else(|| r.name.clone());
                for c in cols {
                    if multi {
                        parts.push(format!("{}.{}", q(&qual), q(c)));
                    } else {
                        parts.push(q(c));
                    }
                }
            }
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(StarExpansion {
        range: (range_start, star_end),
        replacement: parts.join(", "),
    })
}

// ── Signature help ───────────────────────────────────────────────────────────

/// Signature help for the function call enclosing the caret: its name, parameter
/// signature, summary, the zero-based index of the argument being typed, and the
/// byte range of that parameter within `signature` (for emphasis).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureHelp {
    pub name: &'static str,
    pub signature: &'static str,
    pub summary: &'static str,
    pub active_arg: usize,
    pub active_range: Option<(usize, usize)>,
}

/// If the caret sits inside a built-in function call's parentheses, return its
/// signature help. Resolves the *innermost* enclosing call (so `POWER(a, FLOOR(b|`
/// describes `FLOOR`), counts top-level commas to find the active argument, and
/// ignores grouping parens / non-function calls (`(a+b)`, `IN (…)`, subqueries).
pub fn signature_help(
    sql: &str,
    lo: usize,
    hi: usize,
    caret: usize,
    dialect: SqlDialect,
) -> Option<SignatureHelp> {
    let toks = tokenize_range(sql, lo, hi, dialect);
    // Stack of open parens up to the caret, each with its top-level comma count.
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for (idx, t) in toks.iter().enumerate() {
        if t.at >= caret {
            break;
        }
        match t.kind {
            TkKind::LParen => stack.push((idx, 0)),
            TkKind::RParen => {
                stack.pop();
            }
            TkKind::Comma => {
                if let Some(top) = stack.last_mut() {
                    top.1 += 1;
                }
            }
            _ => {}
        }
    }
    let &(paren_idx, comma_count) = stack.last()?;
    if paren_idx == 0 {
        return None;
    }
    // The token immediately before the `(` must be a known function name.
    let TkKind::Word(name) = &toks[paren_idx - 1].kind else {
        return None;
    };
    let func = FUNCTIONS
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case(name))?;
    Some(SignatureHelp {
        name: func.name,
        signature: func.signature,
        summary: func.summary,
        active_arg: comma_count,
        active_range: active_param_range(func.signature, comma_count),
    })
}

/// The byte range within `signature` of the `active_arg`-th parameter (top-level,
/// comma-separated, respecting nested `()`/`[]`), for emphasis. Clamps to the last
/// parameter (varargs), and returns `None` for a parameterless signature.
fn active_param_range(signature: &str, active_arg: usize) -> Option<(usize, usize)> {
    let b = signature.as_bytes();
    let open = signature.find('(')?;
    // Only parentheses nest; `[optional]` brackets are transparent so a comma inside
    // them still separates arguments (matching how the args are actually counted).
    let mut depth = 0i32;
    let mut params: Vec<(usize, usize)> = Vec::new();
    let mut start = open + 1;
    for (i, &c) in b.iter().enumerate().skip(open) {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    params.push((start, i));
                    break;
                }
            }
            b',' if depth == 1 => {
                params.push((start, i));
                start = i + 1;
            }
            _ => {}
        }
    }
    let (s, e) = *params.get(active_arg.min(params.len().checked_sub(1)?))?;
    // Trim whitespace and the optional-argument brackets around the parameter.
    let trim = |c: char| c.is_whitespace() || c == '[' || c == ']';
    let sub = &signature[s..e];
    let ts = s + (sub.len() - sub.trim_start_matches(trim).len());
    let te = e - (sub.len() - sub.trim_end_matches(trim).len());
    (ts < te).then_some((ts, te))
}

// ── DB-validated diagnostics (Tier 2) ────────────────────────────────────────

/// Does `stmt` parse cleanly as SQL in `dialect`? The live DB validation gates on
/// this so it only round-trips syntactically-complete statements — a half-typed
/// fragment (which the server would reject) never triggers a spurious error.
pub fn parses(stmt: &str, dialect: SqlDialect) -> bool {
    sqlparser::parser::Parser::parse_sql(&*dialect.parser(), stmt).is_ok()
}

/// A table a statement reads from, as written: the bare name plus whatever
/// qualified it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceTable {
    /// The qualifier before the name — a database on MySQL, a namespace on
    /// PostgreSQL, an attached database on SQLite. Unquoted; `None` for a bare
    /// name.
    pub qualifier: Option<String>,
    /// The table's own name, **unquoted** — the form the catalogue is keyed on.
    pub name: String,
}

/// Is `query` the structurally simple single-table `SELECT` that both the header
/// filter's rewrite and SQLite's write-back provenance require, and if so, which
/// table does it read?
///
/// **One definition of "simple enough", deliberately.** Two callers ask this
/// question for different reasons — [`crate::filter::build_query`] needs to know
/// it may splice a `WHERE` into the statement that produced a result, and the
/// SQLite backend needs to know which base table a grid row belongs to before it
/// will let anything write to it — but a wrong answer costs the same thing in
/// both: SQL aimed at rows the user didn't mean. Two predicates that agreed on
/// the day they were written is precisely the arrangement `ident_sql` exists to
/// rule out.
///
/// Simple means: one statement, a `SELECT` body (not a set operation), no CTE,
/// exactly one `FROM` entry, no joins, and a plain named table rather than a
/// derived subquery or a table function.
pub(crate) fn simple_select_source(
    query: &sqlparser::ast::Query,
) -> Option<&sqlparser::ast::ObjectName> {
    use sqlparser::ast::{SetExpr, TableFactor};
    if query.with.is_some() {
        return None;
    }
    let SetExpr::Select(select) = query.body.as_ref() else {
        return None;
    };
    if select.from.len() != 1 || !select.from[0].joins.is_empty() {
        return None;
    }
    match &select.from[0].relation {
        TableFactor::Table { name, .. } => Some(name),
        _ => None,
    }
}

/// The single base table `sql` selects from, or `None` when the statement isn't
/// simple enough to say — see [`simple_select_source`] for what that means.
///
/// Identifiers come back **unquoted**: `ObjectNamePart`'s `Display` re-adds the
/// quoting, so a `"t"` name would never match a catalogue keyed on bare names.
pub fn single_source_table(sql: &str, dialect: SqlDialect) -> Option<SourceTable> {
    let stmts = sqlparser::parser::Parser::parse_sql(&*dialect.parser(), sql).ok()?;
    if stmts.len() != 1 {
        return None;
    }
    let sqlparser::ast::Statement::Query(query) = &stmts[0] else {
        return None;
    };
    let parts = object_name_parts(simple_select_source(query)?);
    match parts.len() {
        1 => Some(SourceTable {
            qualifier: None,
            name: parts[0].clone(),
        }),
        2 => Some(SourceTable {
            qualifier: Some(parts[0].clone()),
            name: parts[1].clone(),
        }),
        // Three parts is `db.schema.table`, which no engine here produces from
        // its own generated SQL; reading only the last two would silently drop a
        // qualifier that changes which table this is.
        _ => None,
    }
}

/// What a simple single-table `SELECT` projects, in result-column order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Projection {
    /// `SELECT *` / `SELECT t.*` alone — every column of the source table, in
    /// table order. The caller expands it, since only the catalogue knows them.
    Wildcard,
    /// One entry per result column: the **base column** it is a reference to, or
    /// `None` where it is computed and belongs to no column.
    Items(Vec<Option<String>>),
    /// The items projected *ahead* of a lone trailing `*`, in order — one entry
    /// each, read exactly as [`Projection::Items`] reads them. The wildcard then
    /// expands into every result column after them, so the caller appends the
    /// source table's own columns the way it does for [`Projection::Wildcard`].
    ///
    /// This is the shape `SELECT rowid, * FROM t` has, and the reason it is
    /// answered rather than refused: a leading item sits at a position the
    /// wildcard's unknown width cannot move. Only a *trailing* wildcard qualifies
    /// — see [`projection_of`].
    LeadingThenWildcard(Vec<Option<String>>),
}

/// Which base column each result column of a simple single-table `SELECT` comes
/// from — the statement-derived stand-in for the per-column provenance MySQL and
/// PostgreSQL get from their wire protocols.
///
/// **It is positional, not name-matched, and that is the whole point.** Matching
/// a result column's *name* against the table's columns looks equivalent and
/// isn't: `SELECT a AS b, b FROM t` produces a first column named `b` that holds
/// `a`, and a name match would map it to column `b` — so an edit to it would
/// `UPDATE` the wrong column, silently, with the grid showing the change as
/// though it had worked. Reading the projection positionally gets both columns
/// right, and gets an alias right for the same reason MySQL's `org_name` does: an
/// alias renames the output, not the column behind it.
///
/// Everything that isn't a bare column reference — an expression, a literal, a
/// function call — is `None` and therefore not editable, which is exactly what
/// the other two engines report for the same statement.
///
/// A `*` mixed with anything else is resolved only as far as it safely can be.
/// The wildcard's width isn't knowable here, so no position *after* one can be
/// either, and a provenance list off by one column is the worst possible answer
/// — a `*` with anything behind it, or a second `*`, still returns `None`
/// overall. Positions *ahead* of a lone trailing wildcard are a different case:
/// nothing the wildcard expands to can shift them, so they are placed and
/// returned as [`Projection::LeadingThenWildcard`]. That is what makes
/// `SELECT rowid, * FROM t` — a keyless SQLite table's editable form — analysable
/// without the caller having to spell out every column.
pub fn projection_of(sql: &str, dialect: SqlDialect) -> Option<(SourceTable, Projection)> {
    use sqlparser::ast::{Expr, SelectItem, SetExpr, Statement};

    let source = single_source_table(sql, dialect)?;
    let stmts = sqlparser::parser::Parser::parse_sql(&*dialect.parser(), sql).ok()?;
    let Statement::Query(query) = &stmts[0] else {
        return None;
    };
    let SetExpr::Select(select) = query.body.as_ref() else {
        return None;
    };

    let is_wildcard = |p: &SelectItem| {
        matches!(
            p,
            SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(..)
        )
    };
    // Where the wildcards are. More than one, or one that isn't last, leaves
    // every position after it at an unknowable offset — refuse the statement.
    let wildcards: Vec<usize> = select
        .projection
        .iter()
        .enumerate()
        .filter(|(_, p)| is_wildcard(p))
        .map(|(i, _)| i)
        .collect();
    let trailing_wildcard = match wildcards.as_slice() {
        [] => None,
        [i] if *i == select.projection.len() - 1 => Some(*i),
        _ => return None,
    };
    if trailing_wildcard == Some(0) {
        return Some((source, Projection::Wildcard));
    }

    let base_of = |e: &Expr| -> Option<String> {
        match e {
            Expr::Identifier(id) => Some(id.value.clone()),
            // `t.a` — the qualifier is the source table or its alias either way,
            // since a single-table SELECT has nothing else it could name.
            Expr::CompoundIdentifier(parts) if parts.len() == 2 => Some(parts[1].value.clone()),
            _ => None,
        }
    };
    // The wildcard itself, when there is one, is the last item and is dropped —
    // the caller expands it into the columns after these.
    let placed = trailing_wildcard.unwrap_or(select.projection.len());
    let items: Vec<Option<String>> = select.projection[..placed]
        .iter()
        .map(|p| match p {
            SelectItem::UnnamedExpr(e) => base_of(e),
            SelectItem::ExprWithAlias { expr, .. } => base_of(expr),
            _ => None,
        })
        .collect();
    Some((
        source,
        match trailing_wildcard {
            Some(_) => Projection::LeadingThenWildcard(items),
            None => Projection::Items(items),
        },
    ))
}

/// The column names a `SELECT` produces, **in order**, when they can be read off
/// the statement alone.
///
/// `None` means "can't tell from here", and callers must treat that as unknown
/// rather than as an empty list — a `*`, a set operation, or an unnamed
/// expression all land there, and each of them names its columns from the
/// catalogue or from the server's own rules.
///
/// Ordered, unlike the set [`colres`](self) builds for name resolution, because
/// the caller that needs this — PostgreSQL's rule that
/// `CREATE OR REPLACE VIEW` may only *append* columns — is a rule about
/// position.
pub fn select_output_names(sql: &str, dialect: SqlDialect) -> Option<Vec<String>> {
    use sqlparser::ast::{Expr, SelectItem, SetExpr, Statement};
    let mut asts = sqlparser::parser::Parser::parse_sql(&*dialect.parser(), sql).ok()?;
    if asts.len() != 1 {
        return None;
    }
    let query = match asts.pop()? {
        Statement::Query(q) => q,
        _ => return None,
    };
    let select = match *query.body {
        SetExpr::Select(s) => s,
        // A set operation's names come from its left arm, but its *types* come
        // from both; a union is exactly where an appended column is least safe
        // to assume. Left as unknown.
        _ => return None,
    };
    let mut out = Vec::with_capacity(select.projection.len());
    for item in &select.projection {
        match item {
            SelectItem::UnnamedExpr(Expr::Identifier(id)) => out.push(id.value.clone()),
            SelectItem::UnnamedExpr(Expr::CompoundIdentifier(parts)) if !parts.is_empty() => {
                out.push(parts[parts.len() - 1].value.clone())
            }
            SelectItem::ExprWithAlias { alias, .. } => out.push(alias.value.clone()),
            // `*`, `t.*`, or an unnamed expression the server names by its own
            // rules (`?column?`, the expression text) — unknowable here.
            _ => return None,
        }
    }
    Some(out)
}

/// Case-insensitive search for `needle` in `hay`, returning its byte range.
fn find_ci(hay: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    (0..=h.len().saturating_sub(n.len())).find_map(|i| {
        if h[i..i + n.len()]
            .iter()
            .zip(n)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            Some((i, i + n.len()))
        } else {
            None
        }
    })
}

/// Generic phrases MySQL/MariaDB puts in quotes that aren't object names.
fn is_error_phrase(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "field list"
            | "where clause"
            | "on clause"
            | "order clause"
            | "having clause"
            | "group statement"
    )
}

/// Locate the offending token of a DB error `message` within the statement text
/// `stmt`: the `near '<tok>'` clause of a syntax error, or the first quoted object
/// name of a name error (`Unknown column 'x'`, `Table 'db.t' doesn't exist`).
fn locate_db_error(stmt: &str, message: &str) -> Option<(usize, usize)> {
    // Syntax error: MySQL's `… near 'FRM employees' at line 1`, PostgreSQL's
    // `syntax error at or near "FRM"` → first word of the quoted run.
    const NEAR: &str = "near ";
    for q in ['\'', '"'] {
        if let Some(idx) = message.find(&format!("{NEAR}{q}")) {
            let rest = &message[idx + NEAR.len() + q.len_utf8()..];
            if let Some(end) = rest.find(q) {
                let tok = rest[..end].split_whitespace().next().unwrap_or("");
                let tok = tok.trim_matches('`');
                if !tok.is_empty()
                    && let Some(r) = find_ci(stmt, tok)
                {
                    return r.into();
                }
            }
        }
    }
    // Name error: the first quoted object name, last `.`-segment (skipping
    // generic phrases like 'field list').
    //
    // **Both quote styles**, because the engines differ and this used to know
    // only MySQL's: PostgreSQL writes `column "nosuchcol" does not exist` and
    // `relation "Orders" does not exist`, so every PG squiggle fell through to
    // the caller's fallback and landed on the statement's first word — under
    // `SELECT`, for a feature whose whole job is putting the error where the
    // mistake is.
    let mut i = 0;
    while let Some(open) = message[i..].find(['\'', '"']) {
        let quote = message[i + open..].chars().next().unwrap_or('\'');
        let start = i + open + quote.len_utf8();
        if let Some(close_rel) = message[start..].find(quote) {
            let content = &message[start..start + close_rel];
            i = start + close_rel + quote.len_utf8();
            if is_error_phrase(content) {
                continue;
            }
            let seg = content
                .rsplit('.')
                .next()
                .unwrap_or(content)
                .trim_matches('`');
            if let Some(r) = find_ci(stmt, seg) {
                return Some(r);
            }
        } else {
            break;
        }
    }
    None
}

/// Strip the boilerplate MySQL/MariaDB pads its syntax errors with, and cut to the
/// first line, so the hover message stays short.
fn clean_db_message(message: &str) -> String {
    let first_line = message.lines().next().unwrap_or(message);
    let cleaned = first_line
        .replace(
            "check the manual that corresponds to your MariaDB server version for the right syntax to use ",
            "",
        )
        .replace(
            "check the manual that corresponds to your MySQL server version for the right syntax to use ",
            "",
        );
    cleaned.trim().to_string()
}

/// Turn a database validation error (`message`, from a failed PREPARE/EXPLAIN) into
/// a [`Diagnostic`] positioned within the statement `sql[lo..hi]`. The server names
/// the offending token — a `near '<tok>'` clause or a quoted object name — which we
/// locate to place the squiggle; failing that, the statement's leading token is
/// underlined (never the whole statement, which reads as noise).
pub fn db_error_diagnostic(sql: &str, lo: usize, hi: usize, message: &str) -> Diagnostic {
    let stmt = sql.get(lo..hi).unwrap_or("");
    let range = match locate_db_error(stmt, message) {
        Some((s, e)) => (lo + s, lo + e),
        // Fall back to the statement's first token, not the whole statement.
        None => {
            let (_, first_end) = word_range_at(sql, lo);
            (lo, first_end.max(lo + 1).min(hi))
        }
    };
    Diagnostic {
        range,
        severity: Severity::Error,
        message: clean_db_message(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::parser::Parser;

    #[test]
    fn sql_dialect_from_db_type_maps_engines() {
        assert_eq!(SqlDialect::from_db_type("PostgreSQL"), SqlDialect::Postgres);
        assert_eq!(SqlDialect::from_db_type("postgres"), SqlDialect::Postgres);
        assert_eq!(SqlDialect::from_db_type("pg"), SqlDialect::Postgres);
        // Anything else — including MySQL/MariaDB and unknown/empty — is MySQL.
        assert_eq!(SqlDialect::from_db_type("MySQL"), SqlDialect::MySql);
        assert_eq!(SqlDialect::from_db_type("MariaDB"), SqlDialect::MySql);
        assert_eq!(SqlDialect::from_db_type(""), SqlDialect::MySql);
    }

    #[test]
    fn sqlparser_parses_a_postgres_statement() {
        // The Postgres arm parses a `::` cast + double-quoted identifier — syntax
        // the MySQL dialect wouldn't accept the same way.
        let sql = r#"SELECT "id"::text FROM "users" WHERE created_at > now()"#;
        let ok = Parser::parse_sql(&*SqlDialect::Postgres.parser(), sql).is_ok();
        assert!(ok, "Postgres dialect should parse a ::cast + quoted idents");
    }

    fn names(scope: &Scope) -> Vec<String> {
        let mut v: Vec<String> = scope.tables.iter().map(|t| t.name.clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn single_source_table_reads_a_plain_select_and_refuses_everything_else() {
        let t = |s: &str| single_source_table(s, SqlDialect::Sqlite);
        assert_eq!(
            t("SELECT * FROM artist"),
            Some(SourceTable {
                qualifier: None,
                name: "artist".into()
            })
        );
        // A quoted name comes back **unquoted** — `Display` would re-add the
        // quotes and never match a catalogue keyed on bare names.
        assert_eq!(t(r#"SELECT * FROM "my tbl""#).unwrap().name, "my tbl");
        assert_eq!(t("SELECT * FROM `my tbl`").unwrap().name, "my tbl");
        assert_eq!(t("SELECT * FROM [my tbl]").unwrap().name, "my tbl");
        // A qualifier is kept, since it is part of which table this is.
        assert_eq!(
            t("SELECT * FROM main.artist"),
            Some(SourceTable {
                qualifier: Some("main".into()),
                name: "artist".into()
            })
        );
        // Everything a write must not be aimed at from one statement.
        for sql in [
            "SELECT * FROM a JOIN b ON a.id = b.a_id",
            "SELECT * FROM a, b",
            "WITH c AS (SELECT 1) SELECT * FROM c",
            "SELECT * FROM (SELECT * FROM a) x",
            "SELECT a FROM t UNION SELECT b FROM u",
            "SELECT 1",
            "UPDATE t SET a = 1",
            "SELECT * FROM a; SELECT * FROM b",
            "not sql at all",
        ] {
            assert_eq!(t(sql), None, "{sql}");
        }
    }

    /// The case a name match gets wrong, and the reason the derivation is
    /// positional: the first result column is *named* `b` but *is* `a`, so a
    /// name-matched provenance would aim an edit to it at column `b`.
    #[test]
    fn projection_is_positional_so_an_alias_cannot_shadow_another_column() {
        let (src, proj) = projection_of("SELECT a AS b, b FROM t", SqlDialect::Sqlite).unwrap();
        assert_eq!(src.name, "t");
        assert_eq!(
            proj,
            Projection::Items(vec![Some("a".into()), Some("b".into())])
        );
    }

    #[test]
    fn projection_reads_bare_references_and_refuses_computed_ones() {
        let p = |s: &str| projection_of(s, SqlDialect::Sqlite).map(|(_, p)| p);
        assert_eq!(p("SELECT * FROM t"), Some(Projection::Wildcard));
        assert_eq!(p("SELECT t.* FROM t"), Some(Projection::Wildcard));
        // A qualified reference is still the column behind it.
        assert_eq!(
            p("SELECT t.a, a FROM t"),
            Some(Projection::Items(vec![Some("a".into()), Some("a".into())]))
        );
        // Anything computed belongs to no column — not editable, exactly as the
        // other two engines report for the same statement.
        assert_eq!(
            p("SELECT a, a * 2, count(*), 'x' FROM t"),
            Some(Projection::Items(vec![Some("a".into()), None, None, None]))
        );
        // A `*` beside anything else can't be placed positionally, so the whole
        // answer is withheld rather than being off by however wide it is.
        assert_eq!(p("SELECT *, 1 FROM t"), None);
        assert_eq!(p("SELECT a FROM x JOIN y ON x.id = y.id"), None);
    }

    /// Columns *ahead* of a trailing `*` sit at positions the wildcard's unknown
    /// width can't move, so they are placed — which is what makes
    /// `SELECT rowid, * FROM t` an editable statement.
    #[test]
    fn projection_places_the_columns_ahead_of_a_trailing_wildcard() {
        let p = |s: &str| projection_of(s, SqlDialect::Sqlite).map(|(_, p)| p);
        assert_eq!(
            p("SELECT rowid, * FROM t"),
            Some(Projection::LeadingThenWildcard(vec![Some("rowid".into())]))
        );
        assert_eq!(
            p("SELECT a, b, * FROM t"),
            Some(Projection::LeadingThenWildcard(vec![
                Some("a".into()),
                Some("b".into())
            ]))
        );
        // Qualified on both sides is the same statement.
        assert_eq!(
            p("SELECT t.rowid, t.* FROM t"),
            Some(Projection::LeadingThenWildcard(vec![Some("rowid".into())]))
        );
        // A computed leading item is still a placed position — just not one that
        // belongs to a column.
        assert_eq!(
            p("SELECT a * 2, * FROM t"),
            Some(Projection::LeadingThenWildcard(vec![None]))
        );
    }

    /// The relaxation above is only sound while the wildcard is *last* and alone.
    /// Anything after one, or a second one, puts every following position at an
    /// unknowable offset again.
    #[test]
    fn projection_still_refuses_a_wildcard_that_is_not_the_last_item() {
        let p = |s: &str| projection_of(s, SqlDialect::Sqlite).map(|(_, p)| p);
        assert_eq!(p("SELECT *, 1 FROM t"), None);
        assert_eq!(p("SELECT a, *, b FROM t"), None);
        assert_eq!(p("SELECT *, * FROM t"), None);
        assert_eq!(p("SELECT rowid, *, rowid FROM t"), None);
    }

    /// The filter rewrite and the write-back provenance ask the same question, so
    /// they must answer it identically — a statement one calls simple and the
    /// other doesn't is a statement where a `WHERE` gets spliced into rows nobody
    /// meant to touch, or an `UPDATE` gets aimed at the wrong table.
    #[test]
    fn the_filter_rewrite_and_single_source_table_agree_on_eligibility() {
        for sql in [
            "SELECT * FROM artist",
            "SELECT id, name FROM artist ORDER BY id",
            "SELECT * FROM a JOIN b ON a.id = b.a_id",
            "WITH c AS (SELECT 1) SELECT * FROM c",
            "SELECT a FROM t UNION SELECT b FROM u",
            "SELECT * FROM (SELECT 1) x",
        ] {
            let filterable = crate::filter::build_query(sql, "1=1", &[], SqlDialect::Sqlite)
                .expect("a valid filter never errors here")
                .is_some();
            let has_source = single_source_table(sql, SqlDialect::Sqlite).is_some();
            assert_eq!(filterable, has_source, "{sql}");
        }
    }

    #[test]
    fn select_output_names_reads_a_projection_in_order() {
        let out = |s: &str| select_output_names(s, SqlDialect::Postgres);
        assert_eq!(
            out("SELECT id, t.name, price * 2 AS gross FROM t"),
            Some(vec!["id".into(), "name".into(), "gross".into()])
        );
        // Case is preserved — the caller decides how to fold it.
        assert_eq!(out("SELECT Id FROM t"), Some(vec!["Id".into()]));
        // Everything the statement alone can't name.
        assert_eq!(out("SELECT * FROM t"), None);
        assert_eq!(out("SELECT t.* FROM t"), None);
        assert_eq!(out("SELECT count(*) FROM t"), None);
        assert_eq!(out("SELECT a FROM t UNION SELECT b FROM u"), None);
        assert_eq!(out("VALUES (1)"), None);
        assert_eq!(out("UPDATE t SET a = 1"), None);
        assert_eq!(out("SELECT a FROM t; SELECT b FROM u"), None);
        assert_eq!(out("not sql"), None);
        // A CTE's body is what the view produces.
        assert_eq!(
            out("WITH x AS (SELECT 1 AS n) SELECT x.n FROM x"),
            Some(vec!["n".into()])
        );
    }

    #[test]
    fn sqlparser_parses_a_mysql_statement() {
        // Smoke test: the dependency + dialect seam parse a backtick-quoted,
        // `LIMIT offset, count` MySQL statement (a construct the hand lexer can't
        // structure). Proves the AST engine is wired before we build on it.
        let d = SqlDialect::MySql.parser();
        let ast = Parser::parse_sql(&*d, "SELECT `id` FROM `users` u LIMIT 10, 5").unwrap();
        assert_eq!(ast.len(), 1);
    }

    // ── statement_scope: AST path ─────────────────────────────────────────────

    fn scope(sql: &str) -> Scope {
        statement_scope(sql, 0, sql.len(), sql.len(), SqlDialect::MySql)
    }

    #[test]
    fn scope_resolves_simple_from_alias() {
        let s = scope("SELECT * FROM employees e");
        assert_eq!(names(&s), vec!["employees"]);
        assert_eq!(s.tables[0].alias.as_deref(), Some("e"));
    }

    #[test]
    fn scope_resolves_db_qualified_table() {
        let s = scope("SELECT * FROM sakila.actor a");
        assert_eq!(s.tables[0].name, "actor");
        assert_eq!(s.tables[0].db.as_deref(), Some("sakila"));
        assert_eq!(s.tables[0].alias.as_deref(), Some("a"));
    }

    #[test]
    fn scope_resolves_backticked_names_when_parsing_fails() {
        // The generated "open this table" statement is backtick-quoted, and the
        // moment the user starts a WHERE the statement no longer parses — so the
        // mid-edit lexer fallback has to see through the quoting, or the tab
        // loses column completion the instant you type `WHERE `.
        let s = scope("SELECT * FROM `shop`.`customers` WHERE ");
        assert_eq!(names(&s), vec!["customers"]);
        assert_eq!(s.tables[0].db.as_deref(), Some("shop"));
    }

    /// A mid-edit scope resolved with Postgres rules.
    fn pg_scope(sql: &str) -> Scope {
        statement_scope(sql, 0, sql.len(), sql.len(), SqlDialect::Postgres)
    }

    #[test]
    fn scope_resolves_pg_double_quoted_names_mid_edit() {
        // The Postgres analog of the backtick case above. `"…"` is a *string* in
        // MySQL but an *identifier* in Postgres, so tokenizing PG with MySQL rules
        // swallowed the name whole and the tab lost column completion the instant
        // you typed `WHERE`.
        assert_eq!(
            names(&pg_scope("SELECT * FROM \"customers\" WHERE ")),
            vec!["customers"]
        );
        // Qualified, and the namespace comes through as the qualifier.
        let s = pg_scope("SELECT * FROM \"sales\".\"orders\" WHERE ");
        assert_eq!(names(&s), vec!["orders"]);
        assert_eq!(s.tables[0].db.as_deref(), Some("sales"));
        // With an alias, like the backtick test.
        let s = pg_scope("SELECT * FROM \"orders\" \"o\" WHERE ");
        assert_eq!(names(&s), vec!["orders"]);
        assert_eq!(s.tables[0].alias.as_deref(), Some("o"));
        // A doubled quote inside the name is one literal quote.
        assert_eq!(
            names(&pg_scope("SELECT * FROM \"we\"\"ird\" WHERE ")),
            vec!["we\"ird"]
        );
    }

    #[test]
    fn scope_strips_quotes_from_a_parsed_table_name() {
        // The AST path, not the lexer fallback: these statements parse, and
        // sqlparser's `Display` re-adds the quoting. Keeping it would have made
        // the name unmatchable against the catalog (which is keyed on bare
        // names) — the alias beside it was already unquoted, so the two disagreed.
        let s = scope("SELECT * FROM `shop`.`MyTable` ORDER BY id");
        assert_eq!(names(&s), vec!["MyTable"]);
        assert_eq!(s.tables[0].db.as_deref(), Some("shop"));

        let sql = "SELECT * FROM \"sales\".\"MyTable\" ORDER BY id";
        let s = statement_scope(sql, 0, sql.len(), sql.len(), SqlDialect::Postgres);
        assert_eq!(names(&s), vec!["MyTable"]);
        assert_eq!(s.tables[0].db.as_deref(), Some("sales"));
    }

    #[test]
    fn scope_resolves_the_generated_mixed_case_pg_statement() {
        // Not hypothetical: `filter::table_query` quotes a mixed-case name on
        // Postgres (it folds to lower case unquoted), so opening `MyTable` and
        // typing `WHERE` hit exactly the gap above.
        let sql = "SELECT * FROM \"MyTable\" ORDER BY \"Id\" ASC LIMIT 100";
        let s = statement_scope(sql, 0, sql.len(), sql.len(), SqlDialect::Postgres);
        assert_eq!(names(&s), vec!["MyTable"]);
    }

    #[test]
    fn scope_ignores_tables_named_inside_a_dollar_quoted_string() {
        // `$$ … $$` is a Postgres string. Tokenized MySQL-style its contents were
        // ordinary words, so a `FROM` inside one invented a table that isn't in
        // scope — a false positive, not just a miss.
        let s = pg_scope("SELECT * FROM orders WHERE note = $$ FROM ghosts $$ AND ");
        assert_eq!(names(&s), vec!["orders"]);
        // Tagged form too.
        let s = pg_scope("SELECT * FROM orders WHERE b = $tag$ JOIN ghosts $tag$ AND ");
        assert_eq!(names(&s), vec!["orders"]);
    }

    #[test]
    fn scope_treats_pg_hash_as_an_operator_not_a_comment() {
        // `#` starts a comment in MySQL but is an operator in Postgres, so a name
        // after one *on the same line* is real code there and dead text here. The
        // discriminating table is the one written after the `#`.
        let sql = "SELECT * FROM orders JOIN items ON a = b # 1 JOIN more ON c = d WHERE ";
        assert_eq!(names(&pg_scope(sql)), vec!["items", "more", "orders"]);
        assert_eq!(names(&scope(sql)), vec!["items", "orders"]);
    }

    #[test]
    fn scope_still_ignores_pg_string_literals() {
        // The dialect switch must not turn a single-quoted string into code.
        let s = pg_scope("SELECT * FROM orders WHERE note = 'FROM ghosts' AND ");
        assert_eq!(names(&s), vec!["orders"]);
        // Backticks aren't identifier quotes in Postgres — they must not resolve
        // a name there the way they do on MySQL.
        let s = pg_scope("SELECT * FROM \"orders\" WHERE ");
        assert_eq!(names(&s), vec!["orders"]);
    }

    #[test]
    fn scope_resolves_a_backticked_alias_and_escaped_quotes() {
        let s = scope("SELECT * FROM `orders` `o` WHERE ");
        assert_eq!(names(&s), vec!["orders"]);
        assert_eq!(s.tables[0].alias.as_deref(), Some("o"));
        // A doubled backtick inside a quoted name is one literal backtick.
        let s = scope("SELECT * FROM `we``ird` WHERE ");
        assert_eq!(names(&s), vec!["we`ird"]);
    }

    #[test]
    fn scope_collects_joins() {
        let s = scope(
            "SELECT * FROM orders o JOIN customers c ON o.cust_id = c.id LEFT JOIN items i ON i.order_id = o.id",
        );
        assert_eq!(names(&s), vec!["customers", "items", "orders"]);
    }

    #[test]
    fn scope_collects_comma_join_list() {
        let s = scope("SELECT * FROM a, b, c");
        assert_eq!(names(&s), vec!["a", "b", "c"]);
    }

    #[test]
    fn scope_resolves_cte_names_and_inner_tables() {
        // The CTE name is recorded; the real table inside its body resolves too.
        let s = scope("WITH recent AS (SELECT * FROM orders) SELECT * FROM recent");
        assert!(s.ctes.iter().any(|c| c == "recent"));
        assert!(s.tables.iter().any(|t| t.name == "orders"));
    }

    #[test]
    fn scope_recurses_into_derived_table() {
        // The paren-hack over-scoped derived tables; the AST resolves the inner
        // real table (`events`) while not inventing a catalog table for `d`.
        let s = scope("SELECT * FROM (SELECT id FROM events) d");
        assert_eq!(names(&s), vec!["events"]);
    }

    #[test]
    fn scope_handles_update_and_delete() {
        assert!(
            scope("UPDATE employees SET salary = 1")
                .tables
                .iter()
                .any(|t| t.name == "employees")
        );
        assert!(
            scope("DELETE FROM logs WHERE id = 1")
                .tables
                .iter()
                .any(|t| t.name == "logs")
        );
    }

    #[test]
    fn scope_falls_back_to_lexer_when_unparseable() {
        // Mid-edit, incomplete SQL doesn't parse — the lexer scope still resolves
        // the FROM table + alias so completion keeps working.
        let sql = "SELECT e. FROM employees e WHERE ";
        let s = statement_scope(sql, 0, sql.len(), 9, SqlDialect::MySql);
        assert!(
            s.tables
                .iter()
                .any(|t| t.name == "employees" && t.alias.as_deref() == Some("e"))
        );
    }

    #[test]
    fn scope_ignores_dangling_db_qualifier() {
        // While typing `db.` the trailing qualifier must NOT register as a table:
        // a spurious entry there used to shadow database-qualified completion.
        let s = scope("SELECT * FROM orders o JOIN sakila.");
        let n = names(&s);
        assert!(n.contains(&"orders".to_string()), "{n:?}");
        assert!(!n.iter().any(|t| t == "sakila"), "{n:?}");
        // A dangling qualifier as the only source → no tables at all.
        assert!(scope("SELECT * FROM sakila.").tables.is_empty());
        // A *complete* `db.table` still resolves (with its db).
        let c = scope("SELECT * FROM sakila.actor a");
        assert!(
            c.tables
                .iter()
                .any(|t| t.name == "actor" && t.db.as_deref() == Some("sakila"))
        );
    }

    // ── clause_context ────────────────────────────────────────────────────────

    fn ctx_at(sql: &str, caret: usize) -> ClauseCtx {
        let word_lo = word_start(sql, caret);
        clause_context(sql, 0, word_lo, SqlDialect::MySql)
    }

    #[test]
    fn context_start_then_table_then_column() {
        assert_eq!(ctx_at("SEL", 3), ClauseCtx::Start);
        // Caret after the trailing space (offset 14) → in FROM's target position.
        assert_eq!(ctx_at("SELECT * FROM ", 14), ClauseCtx::Table);
        // Caret in the projection (between SELECT and FROM) → column context.
        assert_eq!(ctx_at("SELECT  FROM t", 7), ClauseCtx::Column);
    }

    #[test]
    fn context_qualified_after_dot() {
        // `e.` → qualified on `e`.
        assert_eq!(
            ctx_at("SELECT e. FROM employees e", 9),
            ClauseCtx::Qualified("e".to_string())
        );
    }

    // ── clause_continuation (expected-token model) ────────────────────────────

    fn cont_at(sql: &str, caret: usize) -> Continuation {
        let word_lo = word_start(sql, caret);
        clause_continuation(sql, 0, word_lo, SqlDialect::MySql)
    }
    fn cont(sql: &str) -> Continuation {
        cont_at(sql, sql.len())
    }
    fn kws(sql: &str) -> Vec<String> {
        cont(sql).keywords
    }

    #[test]
    fn continuation_from_ranks_after_projection() {
        // `select * f` → FROM is the expected continuation (the projection is
        // complete via `*`), so the ranker can lift it to #1.
        assert_eq!(kws("SELECT * f"), vec!["FROM"]);
        // A named projection column, mid-typing the next keyword.
        assert_eq!(kws("SELECT id f"), vec!["FROM"]);
    }

    #[test]
    fn continuation_distinct_right_after_select() {
        // Right after SELECT (nothing projected yet) → DISTINCT, not FROM.
        assert_eq!(kws("SELECT d"), vec!["DISTINCT"]);
        // Already `DISTINCT` typed → don't re-offer it (and no complete projection
        // yet, so no FROM either).
        assert!(!kws("SELECT DISTINCT ").contains(&"DISTINCT".to_string()));
        // Once a projection column is complete, FROM follows (even with DISTINCT).
        assert_eq!(kws("SELECT DISTINCT id f"), vec!["FROM"]);
    }

    #[test]
    fn continuation_no_from_mid_projection_list() {
        // A trailing comma reopens the projection slot → don't offer FROM (the user
        // is adding another column); auto-show columns instead.
        let c = cont("SELECT id, ");
        assert!(
            !c.keywords.contains(&"FROM".to_string()),
            "got {:?}",
            c.keywords
        );
        assert!(c.auto_show);
    }

    #[test]
    fn continuation_where_after_complete_table_ref() {
        // `select * from table w` → WHERE must be an expected continuation (today's
        // known miss: context stayed in "table" mode).
        let k = kws("SELECT * FROM users w");
        assert!(k.contains(&"WHERE".to_string()), "got {k:?}");
        assert!(k.contains(&"GROUP BY".to_string()));
        assert!(k.contains(&"ORDER BY".to_string()));
        assert!(k.contains(&"LIMIT".to_string()));
        // An alias'd ref is just as "complete".
        assert!(kws("SELECT * FROM users u w").contains(&"WHERE".to_string()));
    }

    #[test]
    fn continuation_no_keywords_immediately_after_from() {
        // Right after FROM (no table yet) → no keyword continuations (tables come
        // from the schema), but auto-show so the table list opens.
        let c = cont("SELECT * FROM ");
        assert!(c.keywords.is_empty(), "got {:?}", c.keywords);
        assert!(c.auto_show);
    }

    #[test]
    fn continuation_group_by_is_one_phrase() {
        // Typing `GROUP` after a complete FROM → offer `GROUP BY` as one item.
        assert!(kws("SELECT * FROM t GROUP").contains(&"GROUP BY".to_string()));
        // After `GROUP ` (space) the partial clause expects `BY`.
        assert_eq!(kws("SELECT * FROM t GROUP "), vec!["BY"]);
    }

    #[test]
    fn continuation_order_by_offers_columns_then_direction() {
        // `order by ` (trailing space) → auto-show columns (no keyword clutter).
        let c = cont("SELECT * FROM t ORDER BY ");
        assert!(c.auto_show);
        assert!(c.keywords.is_empty(), "got {:?}", c.keywords);
        // After a sort column → ASC/DESC/LIMIT.
        let k = kws("SELECT * FROM t ORDER BY name ");
        assert!(k.contains(&"ASC".to_string()) && k.contains(&"DESC".to_string()));
        assert!(k.contains(&"LIMIT".to_string()));
    }

    #[test]
    fn continuation_where_condition_offers_connectives() {
        let k = kws("SELECT * FROM t WHERE id = 1 ");
        assert!(k.contains(&"AND".to_string()) && k.contains(&"OR".to_string()));
        assert!(k.contains(&"ORDER BY".to_string()));
        // A completed WHERE isn't re-offered downstream.
        assert!(!k.contains(&"WHERE".to_string()));
    }

    #[test]
    fn continuation_join_then_on() {
        // After the joined table name → ON/USING.
        let k = kws("SELECT * FROM a JOIN b ");
        assert!(k.contains(&"ON".to_string()) && k.contains(&"USING".to_string()));
        // A bare join modifier still expects JOIN.
        assert_eq!(kws("SELECT * FROM a LEFT "), vec!["JOIN", "OUTER JOIN"]);
    }

    #[test]
    fn continuation_auto_show_after_operand_keywords() {
        // Columns auto-show after WHERE/SET/ON and after a comma in a list.
        assert!(cont("SELECT * FROM t WHERE ").auto_show);
        assert!(cont("SELECT * FROM t ORDER BY a, ").auto_show);
        assert!(cont("UPDATE t SET ").auto_show);
        // But not after a filled slot (a plain trailing space post-table-ref).
        assert!(!cont("SELECT * FROM t ").auto_show);
        // And not after LIMIT (a bare number is expected, not a suggestion).
        assert!(!cont("SELECT * FROM t LIMIT ").auto_show);
    }

    #[test]
    fn continuation_update_and_insert_and_delete() {
        assert!(kws("UPDATE employees ").contains(&"SET".to_string()));
        assert!(kws("UPDATE employees SET salary = 1 ").contains(&"WHERE".to_string()));
        assert!(kws("INSERT ").contains(&"INTO".to_string()));
        assert!(kws("INSERT INTO t ").contains(&"VALUES".to_string()));
        assert!(kws("DELETE ").contains(&"FROM".to_string()));
    }

    #[test]
    fn continuation_scopes_to_subquery() {
        // The caret inside a subquery shouldn't inherit the outer query's FROM as a
        // completed slot — here the inner `SELECT * ` still expects FROM.
        let sql = "SELECT * FROM (SELECT * f";
        assert_eq!(kws(sql), vec!["FROM"]);
    }

    #[test]
    fn continuation_empty_for_non_dml() {
        // A statement start / DDL we don't model → no continuations, no auto-show.
        assert_eq!(cont("CREATE TABLE t "), Continuation::default());
    }

    // ── diagnostics ───────────────────────────────────────────────────────────

    use crate::schema::{ColumnInfo, DbSchema, TableInfo};

    fn tbl(name: &str, cols: &[&str]) -> TableInfo {
        TableInfo {
            schema: None,
            name: name.to_string(),
            columns: cols
                .iter()
                .map(|c| ColumnInfo {
                    name: c.to_string(),
                    type_name: "int".to_string(),
                    nullable: true,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    fn sample_catalog() -> (DbSchema, &'static str) {
        (
            DbSchema {
                tables: vec![
                    tbl("employees", &["id", "name", "salary", "dept_id"]),
                    tbl("departments", &["id", "name"]),
                ],
                ..Default::default()
            },
            "company",
        )
    }

    fn diag(sql: &str) -> Vec<Diagnostic> {
        let (schema, db) = sample_catalog();
        let cat = Catalog::build(&[(db, &schema)], Some(db));
        diagnostics(sql, &cat, SqlDialect::MySql)
    }

    fn diag_d(sql: &str, dialect: SqlDialect) -> Vec<Diagnostic> {
        let (schema, db) = sample_catalog();
        let cat = Catalog::build(&[(db, &schema)], Some(db));
        diagnostics(sql, &cat, dialect)
    }

    /// The standard PostgreSQL archive idiom. It is valid, it parses, it runs —
    /// and the editor drew error squiggles under both `RETURNING` and `gone`,
    /// telling the user a correct query was broken. Found when the user opened
    /// the exact statement [A3-L5-01] is about.
    ///
    /// Two independent causes: `alias_checks` read `RETURNING` as an alias for
    /// table `zap`, and `colres` didn't register a CTE whose body is a
    /// `DELETE … RETURNING` as an in-scope source.
    #[test]
    fn a_data_modifying_cte_raises_no_diagnostics() {
        let sql = "WITH gone AS (DELETE FROM employees RETURNING *) SELECT count(*) FROM gone";
        for dialect in [SqlDialect::Postgres, SqlDialect::MySql] {
            let d = diag_d(sql, dialect);
            assert!(
                d.is_empty(),
                "valid data-modifying CTE squiggled on {dialect:?}: {:?}",
                d.iter().map(|x| &x.message).collect::<Vec<_>>()
            );
        }
    }

    /// The clause keyword must not be read as an alias — in any of the shapes a
    /// `RETURNING` can follow a table reference.
    #[test]
    fn returning_is_not_flagged_as_an_alias() {
        for sql in [
            "WITH x AS (DELETE FROM employees RETURNING *) SELECT * FROM x",
            "WITH x AS (UPDATE employees SET name='a' RETURNING id) SELECT * FROM x",
            "WITH x AS (INSERT INTO employees VALUES (1) RETURNING id) SELECT * FROM x",
        ] {
            let d = diag_d(sql, SqlDialect::Postgres);
            assert!(
                !d.iter().any(|x| x.message.contains("RETURNING")),
                "RETURNING read as an alias in {sql:?}: {:?}",
                d.iter().map(|x| &x.message).collect::<Vec<_>>()
            );
        }
    }

    /// The conservatism rule still has to bite the other way: a genuine typo in
    /// a table name must still be reported, or widening CTE acceptance would
    /// have bought the false-positive fix with a false negative.
    #[test]
    fn a_cte_fix_does_not_silence_a_real_unknown_table() {
        let d = diag_d(
            "WITH gone AS (DELETE FROM employees RETURNING *) SELECT * FROM gnoe",
            SqlDialect::Postgres,
        );
        assert!(
            d.iter().any(|x| x.message.contains("gnoe")),
            "a misspelled table must still be flagged: {:?}",
            d.iter().map(|x| &x.message).collect::<Vec<_>>()
        );
    }

    // ── multi-schema (PostgreSQL namespaces) ──────────────────────────────────

    /// A table in an explicit PostgreSQL namespace.
    fn tbl_in(schema: &str, name: &str, cols: &[&str]) -> TableInfo {
        TableInfo {
            schema: Some(schema.to_string()),
            ..tbl(name, cols)
        }
    }

    /// A single-database catalog whose tables span `public` and `sales`.
    fn pg_catalog() -> Catalog {
        let schema = DbSchema {
            tables: vec![
                tbl_in("public", "customers", &["id", "name"]),
                tbl_in("sales", "orders", &["id", "total"]),
            ],
            ..Default::default()
        };
        // Build over an owned schema; the catalog copies what it needs.
        Catalog::build(&[("warehouse", &schema)], Some("warehouse"))
    }

    fn pg_diag(sql: &str) -> Vec<Diagnostic> {
        diagnostics(sql, &pg_catalog(), SqlDialect::Postgres)
    }

    #[test]
    fn diag_schema_qualified_table_resolves() {
        // `sales.orders` is a namespace-qualified reference, not `db.table` —
        // it must resolve, and its columns must resolve through it too.
        assert!(
            pg_diag("SELECT id, total FROM sales.orders").is_empty(),
            "{:?}",
            pg_diag("SELECT id, total FROM sales.orders")
        );
        // Through an alias as well.
        assert!(pg_diag("SELECT o.total FROM sales.orders o").is_empty());
    }

    #[test]
    fn diag_unknown_column_in_a_qualified_schema_is_flagged() {
        // The namespace is known, so the catalog can actually judge its columns.
        let d = pg_diag("SELECT nope FROM sales.orders");
        assert!(
            d.iter().any(|d| d.message.contains("nope")),
            "expected an unknown-column diagnostic, got {d:?}"
        );
    }

    #[test]
    fn diag_missing_table_in_a_known_schema_is_flagged() {
        let d = pg_diag("SELECT * FROM sales.ghosts");
        assert!(
            d.iter().any(|d| d.message.contains("ghosts")),
            "expected an unknown-table diagnostic, got {d:?}"
        );
    }

    #[test]
    fn diag_unknown_qualifier_stays_unjudged() {
        // Neither a loaded database nor an introspected namespace → we can't tell
        // whether it exists, so we must not invent an error.
        assert!(
            pg_diag("SELECT * FROM elsewhere.orders").is_empty(),
            "{:?}",
            pg_diag("SELECT * FROM elsewhere.orders")
        );
    }

    #[test]
    fn diag_unqualified_name_unions_across_schemas() {
        // Without a qualifier we don't know the server's real `search_path`, so
        // an unqualified name resolves against every schema's columns. Permissive
        // on purpose: a false "unknown column" is worse than a missed one.
        assert!(pg_diag("SELECT total FROM orders").is_empty());
        assert!(pg_diag("SELECT name FROM customers").is_empty());
    }

    #[test]
    fn diag_mysql_two_part_name_still_means_database() {
        // The namespace index must not change what `db.table` means on MySQL.
        assert!(diag("SELECT id FROM company.employees").is_empty());
        let d = diag("SELECT id FROM company.ghosts");
        assert!(
            d.iter().any(|d| d.message.contains("ghosts")),
            "expected an unknown-table diagnostic, got {d:?}"
        );
    }

    #[test]
    fn diag_clean_query_has_none() {
        assert!(diag("SELECT id, name FROM employees WHERE salary > 100").is_empty());
        assert!(
            diag("SELECT e.name FROM employees e JOIN departments d ON e.dept_id = d.id")
                .is_empty()
        );
    }

    #[test]
    fn diag_unknown_table() {
        let d = diag("SELECT * FROM employes");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].severity, Severity::Error);
        assert!(d[0].message.contains("Table `employes` not found"));
        // The squiggle covers the table name, not the whole statement.
        assert_eq!(
            &"SELECT * FROM employes"[d[0].range.0..d[0].range.1],
            "employes"
        );
    }

    #[test]
    fn diag_on_a_quoted_table_underlines_the_whole_quoted_name() {
        // The span used to be the *unquoted* length measured from the opening
        // quote, so it started on the quote and stopped two bytes short:
        // `"NoSuchTb`. The last character and the closing quote had no squiggle
        // and no hover tooltip. PostgreSQL feels this most — every mixed-case
        // name must be quoted, and Schemaic writes the quoted form itself.
        for (sql, dialect) in [
            (r#"SELECT 1 FROM "NoSuchTbl""#, SqlDialect::Postgres),
            ("SELECT 1 FROM `NoSuchTbl`", SqlDialect::MySql),
        ] {
            let d = diag_d(sql, dialect);
            let found = d
                .iter()
                .find(|x| x.message.contains("not found"))
                .unwrap_or_else(|| panic!("no unknown-table diagnostic for {sql}"));
            let quote = if dialect == SqlDialect::Postgres {
                '"'
            } else {
                '`'
            };
            assert_eq!(
                &sql[found.range.0..found.range.1],
                format!("{quote}NoSuchTbl{quote}"),
                "{sql}"
            );
        }
    }

    #[test]
    fn diag_on_a_quoted_name_holding_a_doubled_quote_still_spans_it() {
        // A doubled quote is one character of the name but two bytes of source,
        // so the old arithmetic drifted further with every one of them.
        let sql = r#"SELECT 1 FROM "we""ird""#;
        let d = diag_d(sql, SqlDialect::Postgres);
        let found = d
            .iter()
            .find(|x| x.message.contains("not found"))
            .expect("unknown table");
        assert_eq!(&sql[found.range.0..found.range.1], r#""we""ird""#);
    }

    #[test]
    fn diag_unknown_column_qualified() {
        let d = diag("SELECT e.salery FROM employees e");
        assert_eq!(d.len(), 1);
        assert!(
            d[0].message
                .contains("Column `salery` not found in `employees`")
        );
        assert_eq!(
            &"SELECT e.salery FROM employees e"[d[0].range.0..d[0].range.1],
            "salery"
        );
    }

    #[test]
    fn diag_known_column_qualified_is_clean() {
        assert!(diag("SELECT e.salary FROM employees e").is_empty());
        // Table-qualified (no alias) also resolves.
        assert!(diag("SELECT employees.name FROM employees").is_empty());
    }

    // ── unqualified unknown columns ────────────────────────────────────────────

    fn col_errors(sql: &str) -> Vec<String> {
        diag(sql)
            .into_iter()
            .filter(|d| d.message.starts_with("Column "))
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn diag_unqualified_unknown_column_flagged() {
        let d = diag("SELECT salery FROM employees");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].severity, Severity::Error);
        assert!(
            d[0].message
                .contains("Column `salery` not found in `employees`")
        );
        assert_eq!(
            &"SELECT salery FROM employees"[d[0].range.0..d[0].range.1],
            "salery"
        );
    }

    #[test]
    fn diag_unqualified_unknown_in_where_and_order() {
        // Multiple positions, WHERE + ORDER BY.
        assert!(col_errors("SELECT id FROM employees WHERE nope = 1").len() == 1);
        assert!(col_errors("SELECT id FROM employees ORDER BY nope").len() == 1);
        // Every occurrence squiggles.
        assert_eq!(col_errors("SELECT nope, nope FROM employees").len(), 2);
    }

    #[test]
    fn diag_alias_in_where_flagged() {
        // MySQL forbids referencing a SELECT alias in WHERE (only GROUP BY / HAVING /
        // ORDER BY may). The alias resolves everywhere else, so this was a false neg.
        let e = col_errors("SELECT salary AS s FROM employees WHERE s > 100");
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("`s`"));
        // The same alias is legal in HAVING / ORDER BY, and a real column in WHERE is
        // unaffected. (GROUP-BY/HAVING cases are chosen so they don't independently
        // trip the `only_full_group_by` check.)
        for sql in [
            "SELECT salary AS s FROM employees ORDER BY s",
            "SELECT dept_id AS d, COUNT(*) AS c FROM employees GROUP BY dept_id HAVING c > 1",
            "SELECT salary AS s FROM employees WHERE salary > 100",
        ] {
            assert!(col_errors(sql).is_empty(), "false positive: {sql}");
        }
    }

    #[test]
    fn diag_union_order_by_checks_output_columns() {
        // A union's own ORDER BY references the OUTPUT columns (positioned past every
        // branch); an unknown one is flagged.
        assert_eq!(
            col_errors("SELECT id FROM employees UNION SELECT id FROM departments ORDER BY nope")
                .len(),
            1
        );
        // Output column name that isn't among the union outputs → flagged (MySQL/PG
        // both forbid ordering a union by a non-output column).
        assert_eq!(
            col_errors(
                "SELECT salary FROM employees UNION SELECT id FROM departments ORDER BY name"
            )
            .len(),
            1
        );
        // Output column name, first-branch alias, and positional refs are all clean.
        for sql in [
            "SELECT id FROM employees UNION SELECT id FROM departments ORDER BY id",
            "SELECT id AS x FROM employees UNION SELECT id FROM departments ORDER BY x",
            "SELECT id FROM employees UNION SELECT id FROM departments ORDER BY 1",
            // A `SELECT *` branch → outputs can't be enumerated → conservatively unchecked.
            "SELECT * FROM employees UNION SELECT * FROM departments ORDER BY whatever",
            // Branch-internal refs still resolve against their own branch, not the union.
            "SELECT salary FROM employees WHERE name = 'x' UNION SELECT id, name FROM departments",
        ] {
            assert!(col_errors(sql).is_empty(), "false positive: {sql}");
        }
    }

    #[test]
    fn diag_unqualified_known_columns_clean() {
        for sql in [
            "SELECT id, name, salary, dept_id FROM employees",
            "SELECT name FROM employees WHERE salary > 100 ORDER BY name",
            "SELECT dept_id FROM employees GROUP BY dept_id HAVING COUNT(*) > 1",
            // A column unique to one joined table resolves (and isn't ambiguous).
            "SELECT salary FROM employees e JOIN departments d ON e.dept_id = d.id",
            "SELECT salary FROM employees, departments",
            // Projection alias referenced downstream is valid.
            "SELECT salary AS s FROM employees ORDER BY s",
            // Functions / interval units are not columns (must not false-positive).
            "SELECT COUNT(*), MAX(salary) FROM employees",
            "SELECT DATE_ADD(id, INTERVAL 1 DAY) FROM employees",
            // Columns nested in CASE / functions / arithmetic / operators.
            "SELECT CASE WHEN salary > 100 THEN name ELSE dept_id END FROM employees",
            "SELECT UPPER(TRIM(name)) FROM employees",
            "SELECT id + salary AS total FROM employees ORDER BY total",
            "SELECT id FROM employees WHERE name LIKE '%a%' AND salary BETWEEN 1 AND 2",
            "SELECT id FROM employees WHERE name IS NOT NULL",
            // Mixed qualified + unqualified reference to the same column.
            "SELECT id FROM employees e WHERE e.salary > salary",
        ] {
            assert!(
                col_errors(sql).is_empty(),
                "false positive: {sql} -> {:?}",
                col_errors(sql)
            );
        }
    }

    #[test]
    fn diag_ambiguous_unqualified_column() {
        // `name` exists in BOTH joined tables → ambiguous (MySQL errors here).
        let sql = "SELECT name FROM employees e JOIN departments d ON e.dept_id = d.id";
        let d = diag(sql);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].severity, Severity::Error);
        assert!(d[0].message.contains("ambiguous"), "{}", d[0].message);
        // The squiggle covers the offending column.
        assert_eq!(&sql[d[0].range.0..d[0].range.1], "name");
        // Comma-join is the same shape.
        assert!(
            col_errors("SELECT id FROM employees, departments")
                .iter()
                .any(|m| m.contains("ambiguous"))
        );
        // A column unique to one side resolves cleanly.
        assert!(
            col_errors("SELECT salary FROM employees e JOIN departments d ON e.dept_id = d.id")
                .is_empty()
        );
        // A qualified reference is never ambiguous.
        assert!(
            col_errors("SELECT e.name FROM employees e JOIN departments d ON e.dept_id = d.id")
                .is_empty()
        );
    }

    #[test]
    fn diag_ambiguous_suppressed_when_coalesced_or_uncertain() {
        // `USING(id)` coalesces `id` into one output column → unambiguous.
        assert!(col_errors("SELECT id FROM employees e JOIN departments d USING (id)").is_empty());
        // Both sources unenumerable (`SELECT *`) → can't prove ambiguity → silent.
        assert!(
            col_errors(
                "SELECT id FROM (SELECT * FROM employees) a JOIN (SELECT * FROM departments) b ON a.id = b.id"
            )
            .is_empty()
        );
        // A single table is never ambiguous.
        assert!(col_errors("SELECT id FROM employees").is_empty());
    }

    // ── only_full_group_by ─────────────────────────────────────────────────────

    fn gb_warnings(sql: &str) -> Vec<String> {
        diag(sql)
            .into_iter()
            .filter(|d| d.severity == Severity::Warning && d.message.contains("GROUP BY"))
            .map(|d| d.message)
            .collect()
    }

    /// Like `diag`, but with `employees.id` marked as the primary key.
    fn diag_pk(sql: &str) -> Vec<Diagnostic> {
        let mut emp = tbl("employees", &["id", "name", "salary", "dept_id"]);
        emp.columns[0].primary_key = true;
        let schema = DbSchema {
            tables: vec![emp, tbl("departments", &["id", "name"])],
            ..Default::default()
        };
        let cat = Catalog::build(&[("company", &schema)], Some("company"));
        diagnostics(sql, &cat, SqlDialect::MySql)
    }

    #[test]
    fn diag_only_full_group_by_flags_ungrouped_column() {
        // `name` is neither grouped nor aggregated → warning on the column itself.
        let sql = "SELECT dept_id, name FROM employees GROUP BY dept_id";
        let w: Vec<_> = diag(sql)
            .into_iter()
            .filter(|x| x.severity == Severity::Warning && x.message.contains("GROUP BY"))
            .collect();
        assert_eq!(w.len(), 1);
        assert_eq!(&sql[w[0].range.0..w[0].range.1], "name");
    }

    #[test]
    fn diag_only_full_group_by_clean_cases() {
        for sql in [
            "SELECT dept_id, COUNT(*) FROM employees GROUP BY dept_id",
            "SELECT dept_id, SUM(salary) FROM employees GROUP BY dept_id",
            "SELECT dept_id FROM employees GROUP BY dept_id HAVING COUNT(*) > 1",
            "SELECT COUNT(*) FROM employees", // no GROUP BY at all
            "SELECT name, salary FROM employees GROUP BY name, salary", // all grouped
            "SELECT * FROM employees GROUP BY dept_id", // wildcard → skip
        ] {
            assert!(gb_warnings(sql).is_empty(), "false positive: {sql}");
        }
    }

    #[test]
    fn diag_only_full_group_by_pk_functional_dependency() {
        // Grouping by the primary key determines every column → no violation.
        assert!(
            diag_pk("SELECT id, name, salary FROM employees GROUP BY id")
                .iter()
                .all(|d| !d.message.contains("GROUP BY"))
        );
        // Grouping by a non-PK column still flags the ungrouped projection.
        assert!(
            diag_pk("SELECT id, name FROM employees GROUP BY dept_id")
                .iter()
                .any(|d| d.message.contains("GROUP BY"))
        );
    }

    #[test]
    fn diag_only_full_group_by_conservative_on_multi_source() {
        // Multiple sources → skipped (avoid false positives across functional deps).
        assert!(
            gb_warnings(
                "SELECT e.name, d.name FROM employees e JOIN departments d ON e.dept_id = d.id GROUP BY e.dept_id"
            )
            .is_empty()
        );
        // A non-column grouping expression → skipped.
        assert!(gb_warnings("SELECT name FROM employees GROUP BY UPPER(name)").is_empty());
    }

    #[test]
    fn diag_only_full_group_by_inside_subquery() {
        // The rule applies to a derived table's own SELECT too.
        let sql = "SELECT * FROM (SELECT dept_id, name FROM employees GROUP BY dept_id) d";
        assert!(gb_warnings(sql).iter().any(|m| m.contains("`name`")));
    }

    // ── cartesian joins ────────────────────────────────────────────────────────

    fn cross_warnings(sql: &str) -> Vec<Diagnostic> {
        diag(sql)
            .into_iter()
            .filter(|d| d.message.contains("cross join"))
            .collect()
    }

    #[test]
    fn diag_unconstrained_join_flagged() {
        let sql = "SELECT * FROM employees e JOIN departments d";
        let w = cross_warnings(sql);
        assert_eq!(w.len(), 1, "{:?}", diag(sql));
        assert_eq!(w[0].severity, Severity::Warning);
        assert_eq!(&sql[w[0].range.0..w[0].range.1], "departments d");
        // `INNER JOIN` with no ON is the same.
        assert_eq!(
            cross_warnings("SELECT * FROM employees INNER JOIN departments").len(),
            1
        );
    }

    #[test]
    fn diag_cross_join_and_conditioned_joins_clean() {
        for sql in [
            "SELECT * FROM employees e CROSS JOIN departments d", // explicit → intended
            "SELECT * FROM employees e JOIN departments d ON e.dept_id = d.id",
            "SELECT * FROM employees e JOIN departments d USING (id)",
            "SELECT * FROM employees, departments", // comma-join → not flagged
            "SELECT * FROM employees",
        ] {
            assert!(cross_warnings(sql).is_empty(), "false positive: {sql}");
        }
    }

    #[test]
    fn diag_column_per_scope_subquery() {
        // Unknown in the OUTER scope is flagged; the inner subquery resolves against
        // its own table.
        assert_eq!(
            col_errors("SELECT nope FROM employees WHERE id IN (SELECT id FROM departments)"),
            vec!["Column `nope` not found in `employees`"]
        );
        // Unknown INSIDE the subquery is flagged (positioned within it).
        let sql = "SELECT id FROM employees WHERE id IN (SELECT bogus FROM departments)";
        let d = diag(sql);
        assert!(d.iter().any(|x| x.message.contains("`bogus`")));
        assert_eq!(&sql[d[0].range.0..d[0].range.1], "bogus");
        // A clean nested query has no errors.
        assert!(
            col_errors("SELECT id FROM employees WHERE id IN (SELECT id FROM departments)")
                .is_empty()
        );
    }

    #[test]
    fn diag_column_correlated_subquery_sees_outer() {
        // The inner subquery may reference an outer table's column (correlation).
        assert!(
            col_errors(
                "SELECT id FROM employees e WHERE EXISTS \
                 (SELECT 1 FROM departments d WHERE d.id = e.dept_id)"
            )
            .is_empty()
        );
        // …but a genuinely unknown correlated column is still flagged.
        assert!(
            col_errors(
                "SELECT id FROM employees e WHERE EXISTS \
                 (SELECT 1 FROM departments d WHERE d.id = e.nope)"
            )
            .iter()
            .any(|m| m.contains("`nope`"))
        );
    }

    #[test]
    fn diag_column_derived_table() {
        // A derived table's columns are its projection's outputs.
        assert!(col_errors("SELECT x FROM (SELECT id AS x FROM employees) sub").is_empty());
        assert!(col_errors("SELECT sub.x FROM (SELECT id AS x FROM employees) sub").is_empty());
        assert!(
            col_errors("SELECT nope FROM (SELECT id AS x FROM employees) sub")
                .iter()
                .any(|m| m.contains("`nope`"))
        );
        assert!(
            col_errors("SELECT sub.nope FROM (SELECT id AS x FROM employees) sub")
                .iter()
                .any(|m| m.contains("`nope`"))
        );
        // A `*` projection makes the derived table open → no false positives.
        assert!(col_errors("SELECT anything FROM (SELECT * FROM employees) sub").is_empty());
    }

    #[test]
    fn diag_column_cte() {
        assert!(
            col_errors("WITH c AS (SELECT id, name FROM employees) SELECT id, name FROM c")
                .is_empty()
        );
        assert!(
            col_errors("WITH c AS (SELECT id FROM employees) SELECT nope FROM c")
                .iter()
                .any(|m| m.contains("`nope`"))
        );
        // Explicit CTE column list defines the output names.
        assert!(
            col_errors("WITH c (a, b) AS (SELECT id, name FROM employees) SELECT a, b FROM c")
                .is_empty()
        );
    }

    #[test]
    fn diag_column_nested_valid_stays_clean() {
        // False-positive sweep over valid nested queries.
        for sql in [
            "SELECT id, (SELECT COUNT(*) FROM departments) AS cnt FROM employees",
            "SELECT e.name FROM employees e WHERE e.dept_id IN (SELECT id FROM departments)",
            "SELECT name FROM employees WHERE salary > (SELECT AVG(salary) FROM employees)",
            "SELECT sub.total FROM (SELECT dept_id, SUM(salary) AS total FROM employees GROUP BY dept_id) sub",
            "WITH d AS (SELECT id FROM departments) SELECT id FROM employees WHERE dept_id IN (SELECT id FROM d)",
            "WITH a AS (SELECT id FROM employees), b AS (SELECT id FROM departments) SELECT a.id FROM a JOIN b ON a.id = b.id",
            "SELECT e.name, d.name FROM employees e JOIN departments d ON e.dept_id = d.id WHERE e.salary > 100",
        ] {
            assert!(
                col_errors(sql).is_empty(),
                "false positive: {sql} -> {:?}",
                col_errors(sql)
            );
        }
    }

    #[test]
    fn diag_column_union_branches() {
        // Each branch resolves against its own FROM — a clean union is clean.
        assert!(col_errors("SELECT id FROM employees UNION SELECT id FROM departments").is_empty());
        assert!(
            col_errors("SELECT name FROM employees UNION ALL SELECT name FROM departments")
                .is_empty()
        );
        // An unknown column in the FIRST branch is flagged there.
        let a = "SELECT nope FROM employees UNION SELECT id FROM departments";
        let da = diag(a);
        assert!(da.iter().any(|x| x.message.contains("`nope`")));
        assert_eq!(&a[da[0].range.0..da[0].range.1], "nope");
        // …and in the SECOND branch, positioned within it.
        let b = "SELECT id FROM employees UNION SELECT bogus FROM departments";
        let db = diag(b);
        assert!(db.iter().any(|x| x.message.contains("`bogus`")));
        assert_eq!(&b[db[0].range.0..db[0].range.1], "bogus");
        // A column valid in one branch's table but not the other is only flagged in
        // the branch where it's actually unknown (`salary` ∈ employees, ∉ departments).
        let c = "SELECT salary FROM employees UNION SELECT salary FROM departments";
        let dc = diag(c);
        assert_eq!(
            dc.iter().filter(|x| x.message.contains("`salary`")).count(),
            1
        );
        assert_eq!(&c[dc[0].range.0..dc[0].range.1], "salary");
        assert!(dc[0].range.0 > c.find("UNION").unwrap()); // the second occurrence
    }

    #[test]
    fn diag_column_open_on_unknown_source() {
        // Unknown table → open → columns unjudgeable (only the table error surfaces).
        let d = diag("SELECT nope FROM nonexistent");
        assert!(
            d.iter()
                .any(|x| x.message.contains("Table `nonexistent` not found"))
        );
        assert!(!d.iter().any(|x| x.message.starts_with("Column ")));
        // A derived table over an unknown table is likewise open.
        assert!(col_errors("SELECT whatever FROM (SELECT * FROM mystery) sub").is_empty());
    }

    #[test]
    fn diag_unqualified_column_needs_loaded_schema() {
        // No loaded schema → can't judge columns → no false positives.
        let cat = Catalog::build(&[], Some("company"));
        let d = diagnostics("SELECT whatever FROM anything", &cat, SqlDialect::MySql);
        assert!(!d.iter().any(|x| x.message.starts_with("Column ")));
    }

    #[test]
    fn diag_syntax_error_on_terminated_statement() {
        // A completed (`;`-terminated) broken statement → a syntax error.
        let d = diag("SELECT FROM WHERE;");
        assert!(
            d.iter()
                .any(|x| x.severity == Severity::Error && x.message.starts_with("Syntax error"))
        );
    }

    #[test]
    fn diag_suppresses_syntax_on_typing_tail() {
        // The final, unterminated fragment being typed must not throw a red error.
        let d = diag("SELECT id FROM employees WHERE ");
        assert!(!d.iter().any(|x| x.message.starts_with("Syntax error")));
    }

    #[test]
    fn diag_multi_statement_isolates() {
        // First statement valid, second references an unknown table.
        let d = diag("SELECT * FROM employees; SELECT * FROM nope;");
        assert_eq!(
            d.iter().filter(|x| x.message.contains("not found")).count(),
            1
        );
    }

    #[test]
    fn diag_no_false_positive_when_schema_unloaded() {
        // No loaded schema → we can't judge existence, so no unknown-table noise.
        let cat = Catalog::build(&[], Some("company"));
        assert!(diagnostics("SELECT * FROM anything", &cat, SqlDialect::MySql).is_empty());
    }

    #[test]
    fn diag_keyword_typo_is_a_warning() {
        let sql = "SELCT * FROM employees";
        let d = diag(sql);
        // `SELCT` → the statement doesn't parse; a typo warning still pinpoints it.
        assert!(
            d.iter().any(|x| {
                x.severity == Severity::Warning && &sql[x.range.0..x.range.1] == "SELCT"
            })
        );
    }

    #[test]
    fn diag_function_typo_flagged() {
        // Transposition (`COUTN` ↔ `COUNT`) and insertion (`LENGHT`... transposition of
        // `LENGTH`) are both caught, and pinpoint the call name.
        let sql = "SELECT COUTN(*) FROM employees";
        let w: Vec<_> = diag(sql)
            .into_iter()
            .filter(|x| x.message.contains("misspelled function"))
            .collect();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].severity, Severity::Warning);
        assert_eq!(&sql[w[0].range.0..w[0].range.1], "COUTN");
        assert!(
            diag("SELECT LENGHT(name) FROM employees")
                .iter()
                .any(|x| x.message.contains("misspelled function"))
        );
        // A dropped letter (`SUBSTRIN`) too.
        assert!(
            diag("SELECT SUBSTRIN(name, 1) FROM employees")
                .iter()
                .any(|x| x.message.contains("misspelled function"))
        );
    }

    #[test]
    fn diag_function_typo_no_false_positives() {
        for sql in [
            "SELECT COUNT(*) FROM employees",         // correct
            "SELECT POWER(salary, 2) FROM employees", // real builtin
            "SELECT JSON_EXTRACT(name, '$.a') FROM employees",
            "SELECT COALESCE(name, 'x') FROM employees",
            "SELECT my_custom_func(id) FROM employees", // UDF, not near a builtin
            "SELECT id FROM employees WHERE id IN (1, 2)", // `IN (` is a keyword
            "SELECT * FROM employees e JOIN departments d ON e.dept_id = d.id",
            "SELECT db.helper(id) FROM employees", // qualified call → skip
        ] {
            let msgs: Vec<String> = diag(sql)
                .into_iter()
                .filter(|x| x.message.contains("misspelled function"))
                .map(|x| x.message)
                .collect();
            assert!(msgs.is_empty(), "false positive: {sql} -> {msgs:?}");
        }
    }

    #[test]
    fn function_catalog_is_sane() {
        use std::collections::HashSet;
        // A builtin that used to be missing from the suggestion set is now present
        // and trusted by the typo checker.
        assert!(function_names().any(|f| f == "POWER"));
        assert!(is_known_function("power"));
        // A comprehensive catalog.
        assert!(FUNCTIONS.len() > 150, "only {} functions", FUNCTIONS.len());
        // Each entry is well-formed: unique upper-case name, signature leads with the
        // name, non-empty summary.
        let mut seen = HashSet::new();
        for func in FUNCTIONS {
            assert!(seen.insert(func.name), "duplicate function: {}", func.name);
            assert!(
                func.name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
                "non-canonical name: {}",
                func.name
            );
            assert!(
                func.signature.starts_with(func.name),
                "signature {} doesn't lead with {}",
                func.signature,
                func.name
            );
            assert!(!func.summary.is_empty(), "empty summary for {}", func.name);
        }
    }

    #[test]
    fn diag_qualified_unloaded_db_not_flagged() {
        // `otherdb.t` — that db isn't loaded, so no unknown-table false positive.
        assert!(diag("SELECT * FROM otherdb.things t").is_empty());
    }

    // ── reserved-keyword aliases ───────────────────────────────────────────────

    fn has_reserved_alias(d: &[Diagnostic], sql: &str, tok: &str) -> bool {
        d.iter().any(|x| {
            x.severity == Severity::Error
                && &sql[x.range.0..x.range.1] == tok
                && x.message.contains("reserved keyword")
        })
    }

    #[test]
    fn reserved_alias_is_dialect_specific() {
        // `UNSIGNED` is reserved in MySQL but a legal identifier in Postgres.
        let s1 = "SELECT id AS unsigned FROM employees";
        assert!(has_reserved_alias(
            &diag_d(s1, SqlDialect::MySql),
            s1,
            "unsigned"
        ));
        assert!(!has_reserved_alias(
            &diag_d(s1, SqlDialect::Postgres),
            s1,
            "unsigned"
        ));
        // `USER` is reserved in Postgres but non-reserved (legal) in MySQL.
        let s2 = "SELECT id AS user FROM employees";
        assert!(has_reserved_alias(
            &diag_d(s2, SqlDialect::Postgres),
            s2,
            "user"
        ));
        assert!(!has_reserved_alias(
            &diag_d(s2, SqlDialect::MySql),
            s2,
            "user"
        ));
    }

    #[test]
    fn reserved_word_lookup_per_dialect() {
        assert!(is_reserved_word("unsigned", SqlDialect::MySql));
        assert!(!is_reserved_word("unsigned", SqlDialect::Postgres));
        assert!(is_reserved_word("user", SqlDialect::Postgres));
        assert!(!is_reserved_word("user", SqlDialect::MySql));
        // Words reserved in both dialects stay reserved either way.
        for d in [SqlDialect::MySql, SqlDialect::Postgres] {
            assert!(is_reserved_word("select", d));
            assert!(is_reserved_word("where", d));
            assert!(is_reserved_word("or", d));
        }
    }

    #[test]
    fn diag_reserved_keyword_table_alias_flagged() {
        // `AS or` — OR is reserved, so it can't be a bare alias → error on `or`.
        let sql = "SELECT * FROM employees AS or";
        assert!(has_reserved_alias(&diag(sql), sql, "or"));
    }

    #[test]
    fn diag_reserved_keyword_column_alias_flagged() {
        // Column aliases can't be reserved words either.
        let sql = "SELECT id AS or FROM employees";
        assert!(has_reserved_alias(&diag(sql), sql, "or"));
    }

    #[test]
    fn diag_implicit_reserved_keyword_alias_flagged() {
        // The shorthand form `orders or` must squiggle `or` too — parity with the
        // explicit `AS or` (this was the reported gap).
        let sql = "SELECT * FROM employees or";
        assert!(has_reserved_alias(&diag(sql), sql, "or"));
    }

    #[test]
    fn diag_implicit_alias_parity_with_as_form() {
        for sql in [
            "SELECT * FROM employees or",
            "SELECT * FROM employees AS or",
        ] {
            assert!(
                diag(sql).iter().any(|x| {
                    &sql[x.range.0..x.range.1] == "or" && x.message.contains("reserved keyword")
                }),
                "missed: {sql}"
            );
        }
    }

    #[test]
    fn diag_clause_keywords_after_table_not_flagged() {
        // Legitimate continuations after a table ref aren't alias mistakes.
        for sql in [
            "SELECT * FROM employees WHERE id = 1",
            "SELECT * FROM employees GROUP BY id",
            "SELECT * FROM employees ORDER BY id",
            "SELECT * FROM employees LIMIT 5",
            "SELECT * FROM employees e JOIN departments d ON e.dept_id = d.id",
            "SELECT * FROM employees, departments",
        ] {
            assert!(
                !diag(sql)
                    .iter()
                    .any(|x| x.message.contains("reserved keyword")),
                "false positive: {sql}"
            );
        }
    }

    #[test]
    fn diag_reserved_word_outside_small_keyword_set_flagged() {
        // Words reserved in MySQL but NOT in the small `SQL_KEYWORDS` completion set
        // (`RANK`, `OVER`, `SYSTEM`) — the full reserved list now catches them.
        for sql in [
            "SELECT id AS rank FROM employees",
            "SELECT * FROM employees AS system",
            "SELECT * FROM employees over",
        ] {
            assert!(
                diag(sql)
                    .iter()
                    .any(|x| x.message.contains("reserved keyword")),
                "missed reserved word: {sql}"
            );
        }
    }

    // ── Broad corpus: false-positive / false-negative sweep ────────────────────
    // A wide net of queries to surface anything the diagnostics get wrong. Valid SQL
    // must stay clean; known-bad SQL must be flagged. Uses an *empty* catalog so the
    // unknown-table/column checks are inert — this isolates syntax / alias / typo
    // reporting (the catalog checks have their own focused tests above).

    fn diag_bare(sql: &str) -> Vec<Diagnostic> {
        let cat = Catalog::build(&[], None);
        diagnostics(sql, &cat, SqlDialect::MySql)
    }

    #[test]
    fn corpus_valid_queries_produce_no_diagnostics() {
        let valid = [
            // Basics.
            "SELECT 1;",
            "SELECT * FROM employees;",
            "SELECT id, name FROM employees;",
            "SELECT id, name FROM employees WHERE salary > 100;",
            "SELECT * FROM employees WHERE name IS NULL;",
            "SELECT * FROM employees WHERE name IS NOT NULL;",
            "SELECT * FROM employees WHERE name LIKE '%a%';",
            "SELECT * FROM employees WHERE id IN (1, 2, 3);",
            "SELECT * FROM employees WHERE salary BETWEEN 100 AND 200;",
            "SELECT * FROM employees WHERE dept_id = 1 AND salary > 100 OR name = 'x';",
            "SELECT DISTINCT dept_id FROM employees;",
            // Aliases (explicit, implicit, non-reserved keyword, backtick-quoted).
            "SELECT e.id FROM employees e;",
            "SELECT e.id FROM employees AS e;",
            "SELECT id AS offset FROM employees;",
            // NOTE: `SELECT * FROM employees view` (VIEW as an implicit alias) is valid
            // MySQL but sqlparser rejects it, so our syntax diagnostic false-positives
            // on it — a known upstream-parser gap, not our bug. Omitted here.
            "SELECT id AS `select` FROM employees;",
            "SELECT `order`, `group` FROM employees;",
            // Ordering / grouping / limits.
            "SELECT * FROM employees ORDER BY name;",
            "SELECT * FROM employees ORDER BY name DESC, id ASC;",
            "SELECT dept_id, COUNT(*) FROM employees GROUP BY dept_id;",
            "SELECT dept_id, COUNT(*) c FROM employees GROUP BY dept_id HAVING c > 5;",
            "SELECT * FROM employees LIMIT 10;",
            "SELECT * FROM employees LIMIT 10 OFFSET 5;",
            "SELECT * FROM employees LIMIT 5, 10;",
            // Joins.
            "SELECT e.name, d.name FROM employees e JOIN departments d ON e.dept_id = d.id;",
            "SELECT * FROM employees e LEFT JOIN departments d ON e.dept_id = d.id;",
            "SELECT * FROM employees e INNER JOIN departments d ON e.dept_id = d.id;",
            "SELECT * FROM employees, departments;",
            "SELECT * FROM employees e, departments d WHERE e.dept_id = d.id;",
            // Subqueries / derived / CTE / set ops.
            "SELECT * FROM (SELECT id FROM employees) AS sub;",
            "SELECT * FROM (SELECT id FROM employees) sub;",
            "WITH recent AS (SELECT * FROM employees) SELECT * FROM recent;",
            "SELECT id FROM employees UNION SELECT id FROM departments;",
            "SELECT id FROM employees UNION ALL SELECT id FROM departments;",
            "SELECT id FROM employees e WHERE EXISTS (SELECT 1 FROM departments d WHERE d.id = e.dept_id);",
            "SELECT * FROM employees WHERE salary > (SELECT AVG(salary) FROM employees);",
            // Expressions / functions / CASE.
            "SELECT CASE WHEN salary > 100 THEN 'high' ELSE 'low' END FROM employees;",
            "SELECT COALESCE(name, 'n/a') FROM employees;",
            "SELECT COUNT(*), MAX(salary), MIN(salary), AVG(salary) FROM employees;",
            "SELECT NOW();",
            "SELECT 'it''s a test' FROM employees;",
            // DML / DDL.
            "INSERT INTO employees (id, name) VALUES (1, 'a');",
            "INSERT INTO employees VALUES (1, 'a', 100, 2);",
            "INSERT INTO employees SELECT * FROM employees;",
            "UPDATE employees SET salary = 200 WHERE id = 1;",
            "DELETE FROM employees WHERE id = 1;",
            "CREATE VIEW v AS SELECT id FROM employees;",
            "CREATE TABLE t2 AS SELECT id FROM employees;",
            // Comments, strings containing keywords, multi-statement.
            "SELECT /* block */ id FROM employees;",
            "SELECT id FROM employees WHERE id = 1; SELECT 2;",
            "SELECT 'FROM WHERE OR AS' AS lit FROM employees;",
            // Reserved words, correctly backtick-quoted as identifiers.
            "SELECT * FROM `order`;",
            "SELECT t.`order` FROM employees t;",
            "SELECT `select` FROM employees;",
            // More joins / CTEs / set positions.
            "SELECT * FROM employees e CROSS JOIN departments d;",
            "SELECT * FROM employees e RIGHT JOIN departments d ON e.dept_id = d.id;",
            "WITH a AS (SELECT 1 AS x), b AS (SELECT 2 AS y) SELECT * FROM a JOIN b ON a.x = b.y;",
            "SELECT * FROM employees e JOIN departments d ON e.dept_id = d.id WHERE d.id > 1;",
        ];
        let failures: Vec<String> = valid
            .iter()
            .filter_map(|q| {
                let d = diag_bare(q);
                (!d.is_empty()).then(|| {
                    let msgs: Vec<&str> = d.iter().map(|x| x.message.as_str()).collect();
                    format!("  {q}\n      -> {msgs:?}")
                })
            })
            .collect();
        assert!(
            failures.is_empty(),
            "false positives on valid SQL ({} of {}):\n{}",
            failures.len(),
            valid.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn corpus_invalid_queries_are_flagged() {
        // Each must produce at least one Error diagnostic.
        let invalid = [
            "SELECT * FROM employees AS or;",
            "SELECT * FROM employees or;",
            "SELECT id AS select FROM employees;",
            "SELECT id AS rank FROM employees;",
            "SELECT * FROM employees AS and;",
            "SELECT * FROM employees e JOIN departments or ON 1 = 1;",
            "SELECT FROM WHERE;",
            "SELECT * FROM;",
            "SELECT * FROM employees WHERE;",
            "SELECT * FROM employees GROUP BY;",
            "SELECT a, FROM employees;",
        ];
        let misses: Vec<&str> = invalid
            .iter()
            .copied()
            .filter(|q| !diag_bare(q).iter().any(|x| x.severity == Severity::Error))
            .collect();
        assert!(
            misses.is_empty(),
            "missed errors on invalid SQL:\n{misses:#?}"
        );
    }

    #[test]
    fn diag_nonreserved_keyword_alias_not_flagged() {
        // `OFFSET`/`VIEW` are non-reserved in MySQL — legal as bare aliases, so no
        // squiggle (explicit or implicit).
        for sql in [
            "SELECT id AS offset FROM employees",
            "SELECT * FROM employees view",
        ] {
            assert!(
                !diag(sql)
                    .iter()
                    .any(|x| x.message.contains("reserved keyword")),
                "false positive: {sql}"
            );
        }
    }

    #[test]
    fn diag_insert_and_update_bodies_not_flagged() {
        // INSERT/UPDATE aren't checked for implicit aliases → VALUES/SET are safe.
        for sql in [
            "INSERT INTO employees VALUES (1)",
            "UPDATE employees SET id = 1",
        ] {
            assert!(
                !diag(sql)
                    .iter()
                    .any(|x| x.message.contains("reserved keyword")),
                "false positive: {sql}"
            );
        }
    }

    #[test]
    fn diag_valid_alias_not_flagged() {
        // A non-keyword alias is fine (explicit and implicit).
        assert!(
            !diag("SELECT id AS ord FROM employees")
                .iter()
                .any(|x| x.message.contains("reserved keyword"))
        );
        assert!(
            !diag("SELECT * FROM employees emp")
                .iter()
                .any(|x| x.message.contains("reserved keyword"))
        );
    }

    #[test]
    fn diag_ctas_and_view_body_not_flagged() {
        // `AS SELECT` / `AS (SELECT …)` bodies aren't aliases.
        for sql in [
            "CREATE TABLE t AS SELECT id FROM employees",
            "CREATE VIEW v AS SELECT id FROM employees",
        ] {
            assert!(
                !diag(sql)
                    .iter()
                    .any(|x| x.message.contains("reserved keyword")),
                "false positive on: {sql}"
            );
        }
    }

    #[test]
    fn scope_rejects_reserved_keyword_alias_after_as() {
        // `orders AS or` must NOT resolve `or` as an alias (parity with the implicit
        // form `orders or`) — so completion won't offer `or.`'s columns for it.
        let sql = "SELECT or. FROM employees AS or WHERE ";
        let s = statement_scope(sql, 0, sql.len(), 10, SqlDialect::MySql);
        let emp = s.tables.iter().find(|t| t.name == "employees").unwrap();
        assert_eq!(emp.alias, None);
    }

    #[test]
    fn offset_of_line_col_maps_positions() {
        let sql = "SELECT 1\nFROM t\nWHERE x";
        assert_eq!(offset_of_line_col(sql, 1, 1), 0);
        assert_eq!(offset_of_line_col(sql, 2, 1), 9); // start of "FROM"
        assert_eq!(&sql[offset_of_line_col(sql, 2, 1)..][..4], "FROM");
        assert_eq!(offset_of_line_col(sql, 3, 7), sql.len() - 1); // the `x`
        assert_eq!(offset_of_line_col(sql, 3, 99), sql.len()); // past EOL clamps
    }

    // ── FK-aware JOIN … ON ────────────────────────────────────────────────────

    fn fk_catalog() -> (DbSchema, &'static str) {
        use crate::schema::ForeignKeyInfo;
        let mut orders = tbl("orders", &["id", "customer_id", "total"]);
        orders.foreign_keys = vec![ForeignKeyInfo {
            columns: vec!["customer_id".to_string()],
            ref_schema: None,
            ref_table: "customers".to_string(),
            ref_columns: vec!["id".to_string()],
            ..Default::default()
        }];
        let customers = tbl("customers", &["id", "name"]);
        // Composite FK: order_items(order_id, item_id) → … just test single here,
        // plus a composite pair below.
        let mut line_items = tbl("line_items", &["order_id", "sku", "qty"]);
        line_items.foreign_keys = vec![ForeignKeyInfo {
            columns: vec!["order_id".to_string()],
            ref_schema: None,
            ref_table: "orders".to_string(),
            ref_columns: vec!["id".to_string()],
            ..Default::default()
        }];
        (
            DbSchema {
                tables: vec![orders, customers, line_items],
                ..Default::default()
            },
            "shop",
        )
    }

    fn join_at(sql: &str, caret: usize) -> Option<String> {
        let (schema, db) = fk_catalog();
        let cat = Catalog::build(&[(db, &schema)], Some(db));
        join_condition(sql, 0, sql.len(), caret, &cat, SqlDialect::MySql)
    }

    #[test]
    fn join_suggests_fk_predicate_with_aliases() {
        // orders.customer_id → customers.id; caret right after `ON `.
        let sql = "SELECT * FROM orders o JOIN customers c ON ";
        assert_eq!(
            join_at(sql, sql.len()),
            Some("o.customer_id = c.id".to_string())
        );
    }

    #[test]
    fn join_predicate_uses_table_names_without_aliases() {
        let sql = "SELECT * FROM orders JOIN customers ON ";
        assert_eq!(
            join_at(sql, sql.len()),
            Some("orders.customer_id = customers.id".to_string())
        );
    }

    #[test]
    fn join_works_when_fk_points_the_other_way() {
        // The just-joined table (`orders`) is referenced BY `line_items` — the
        // predicate still resolves (direction-agnostic).
        let sql = "SELECT * FROM line_items l JOIN orders o ON ";
        assert_eq!(
            join_at(sql, sql.len()),
            Some("l.order_id = o.id".to_string())
        );
    }

    #[test]
    fn join_none_when_condition_already_typed() {
        // Something already typed after ON → don't overwrite it.
        let sql = "SELECT * FROM orders o JOIN customers c ON o.";
        assert_eq!(join_at(sql, sql.len()), None);
    }

    #[test]
    fn join_none_without_a_foreign_key() {
        // customers ⟷ line_items have no FK between them.
        let sql = "SELECT * FROM customers c JOIN line_items l ON ";
        assert_eq!(join_at(sql, sql.len()), None);
    }

    #[test]
    fn join_none_outside_on_context() {
        // Caret in the projection, not an ON clause.
        let sql = "SELECT  FROM orders o JOIN customers c ON x";
        assert_eq!(join_at(sql, 7), None);
    }

    // ── FK-aware JOIN completion targets ──────────────────────────────────────

    fn jt(sql: &str) -> Vec<JoinTarget> {
        let (schema, db) = fk_catalog();
        let cat = Catalog::build(&[(db, &schema)], Some(db));
        join_targets(sql, 0, sql.len(), sql.len(), &cat, SqlDialect::MySql)
    }

    #[test]
    fn join_targets_offers_fk_table_and_predicate() {
        // After `FROM orders o JOIN `, `customers` is FK-connected (forward) and
        // `line_items` is FK-connected (reverse).
        let ts = jt("SELECT * FROM orders o JOIN ");
        let names: Vec<&str> = ts.iter().map(|t| t.table.as_str()).collect();
        assert!(names.contains(&"customers"), "{names:?}");
        assert!(names.contains(&"line_items"), "{names:?}");
        let cust = ts.iter().find(|t| t.table == "customers").unwrap();
        assert_eq!(cust.predicate, "o.customer_id = customers.id");
    }

    #[test]
    fn join_targets_reverse_edge_predicate() {
        // `line_items.order_id → orders.id`; joining onto in-scope `orders o`.
        let ts = jt("SELECT * FROM orders o JOIN ");
        let li = ts.iter().find(|t| t.table == "line_items").unwrap();
        assert_eq!(li.predicate, "line_items.order_id = o.id");
    }

    #[test]
    fn join_targets_partial_prefix_still_offered() {
        // A partial table name being typed doesn't disable the suggestions.
        let ts = jt("SELECT * FROM orders o JOIN cust");
        assert!(ts.iter().any(|t| t.table == "customers"));
    }

    #[test]
    fn join_targets_empty_outside_join_slot() {
        // A FROM slot (not JOIN) → no ON-predicate suggestions.
        assert!(jt("SELECT * FROM ").is_empty());
        // After the table is fully typed and an ON is expected, not a table slot.
        let (schema, db) = fk_catalog();
        let cat = Catalog::build(&[(db, &schema)], Some(db));
        let sql = "SELECT * FROM orders o JOIN customers c ";
        assert!(join_targets(sql, 0, sql.len(), sql.len(), &cat, SqlDialect::MySql).is_empty());
    }

    #[test]
    fn join_targets_excludes_already_in_scope() {
        // `customers` already joined → don't re-suggest it.
        let ts = jt("SELECT * FROM orders o JOIN customers c JOIN ");
        assert!(!ts.iter().any(|t| t.table == "customers"));
    }

    // ── SELECT * expansion ────────────────────────────────────────────────────

    fn expand(sql: &str, caret: usize) -> Option<StarExpansion> {
        let (schema, db) = sample_catalog();
        let cat = Catalog::build(&[(db, &schema)], Some(db));
        expand_star(sql, 0, sql.len(), caret, &cat, SqlDialect::MySql)
    }

    #[test]
    fn expand_star_single_table_unqualified() {
        let sql = "SELECT * FROM employees";
        let e = expand(sql, 8).unwrap(); // caret right after `*`
        assert_eq!(&sql[e.range.0..e.range.1], "*");
        assert_eq!(e.replacement, "id, name, salary, dept_id");
    }

    #[test]
    fn expand_star_multi_table_qualifies_by_alias() {
        let sql = "SELECT * FROM employees e JOIN departments d ON e.dept_id = d.id";
        let e = expand(sql, 8).unwrap();
        assert_eq!(
            e.replacement,
            "e.id, e.name, e.salary, e.dept_id, d.id, d.name"
        );
    }

    #[test]
    fn expand_qualified_star() {
        let sql = "SELECT e.* FROM employees e";
        let e = expand(sql, 10).unwrap(); // right after the `*` in `e.*`
        assert_eq!(&sql[e.range.0..e.range.1], "e.*");
        assert_eq!(e.replacement, "e.id, e.name, e.salary, e.dept_id");
    }

    #[test]
    fn expand_star_ignores_non_projection_stars() {
        // COUNT(*) argument.
        assert!(expand("SELECT COUNT(*) FROM employees", 14).is_none());
        // Multiplication in an expression, not a projection star.
        assert!(expand("SELECT salary * dept_id FROM employees", 15).is_none());
        // No star at the caret.
        assert!(expand("SELECT id FROM employees", 9).is_none());
        // No FROM/scope → nothing to expand.
        assert!(expand("SELECT * ", 8).is_none());
    }

    // ── signature help ────────────────────────────────────────────────────────

    fn sig(sql: &str) -> Option<SignatureHelp> {
        signature_help(sql, 0, sql.len(), sql.len(), SqlDialect::MySql)
    }

    #[test]
    fn signature_help_tracks_active_argument() {
        let h = sig("SELECT POWER(salary, ").unwrap();
        assert_eq!(h.name, "POWER");
        assert_eq!(h.active_arg, 1); // past the first comma → second parameter
        let (s, e) = h.active_range.unwrap();
        assert_eq!(&h.signature[s..e], "y");
        // First argument, no comma yet.
        let h0 = sig("SELECT ROUND(x").unwrap();
        assert_eq!(h0.name, "ROUND");
        assert_eq!(h0.active_arg, 0);
        assert_eq!(
            &h0.signature[h0.active_range.unwrap().0..h0.active_range.unwrap().1],
            "x"
        );
    }

    #[test]
    fn signature_help_resolves_innermost_call() {
        let h = sig("SELECT POWER(a, FLOOR(b").unwrap();
        assert_eq!(h.name, "FLOOR");
        assert_eq!(h.active_arg, 0);
    }

    #[test]
    fn signature_help_none_outside_a_function_call() {
        assert!(sig("SELECT id FROM t WHERE ").is_none());
        assert!(sig("SELECT (a + b").is_none()); // grouping paren
        assert!(sig("SELECT id FROM t WHERE id IN (1, 2").is_none()); // IN isn't a function
        assert!(sig("SELECT POWER(a, b) FROM t").is_none()); // call already closed
    }

    #[test]
    fn active_param_range_varargs_and_noargs() {
        // Varargs clamp to the last shown parameter.
        let r = active_param_range("CONCAT(str, ...)", 5).unwrap();
        assert_eq!(&"CONCAT(str, ...)"[r.0..r.1], "...");
        // A parameterless signature has no active range.
        assert!(active_param_range("NOW()", 0).is_none());
    }

    // ── DB-validated diagnostics ──────────────────────────────────────────────

    #[test]
    fn db_error_locates_syntax_near_token() {
        let sql = "SELECT * FRM employees";
        let msg = "You have an error in your SQL syntax; check the manual that corresponds to your MariaDB server version for the right syntax to use near 'FRM employees' at line 1";
        let d = db_error_diagnostic(sql, 0, sql.len(), msg);
        assert_eq!(&sql[d.range.0..d.range.1], "FRM");
        assert!(!d.message.contains("check the manual")); // boilerplate stripped
    }

    #[test]
    fn db_error_locates_unknown_column() {
        let sql = "SELECT salery FROM employees";
        let d = db_error_diagnostic(sql, 0, sql.len(), "Unknown column 'salery' in 'field list'");
        assert_eq!(&sql[d.range.0..d.range.1], "salery");
    }

    #[test]
    fn db_error_locates_qualified_table_last_segment() {
        let sql = "SELECT * FROM employes";
        let d = db_error_diagnostic(sql, 0, sql.len(), "Table 'company.employes' doesn't exist");
        assert_eq!(&sql[d.range.0..d.range.1], "employes");
    }

    #[test]
    fn db_error_locates_postgres_messages_too() {
        // PostgreSQL double-quotes the object it is complaining about, and this
        // knew only MySQL's single quotes — so *every* PG squiggle fell through
        // to the leading-token fallback and landed on `SELECT`, in the feature
        // whose whole job is putting the error where the mistake is. The three
        // message shapes below are the ones the server really emits.
        let sql = r#"SELECT id, nosuchcol FROM "Orders""#;
        let d = db_error_diagnostic(sql, 0, sql.len(), r#"column "nosuchcol" does not exist"#);
        assert_eq!(&sql[d.range.0..d.range.1], "nosuchcol");

        let sql = r#"SELECT 1 FROM "Ordrs""#;
        let d = db_error_diagnostic(sql, 0, sql.len(), r#"relation "Ordrs" does not exist"#);
        assert_eq!(&sql[d.range.0..d.range.1], "Ordrs");

        let sql = "SELECT 1 FRM t";
        let d = db_error_diagnostic(sql, 0, sql.len(), r#"syntax error at or near "FRM""#);
        assert_eq!(&sql[d.range.0..d.range.1], "FRM");
    }

    #[test]
    fn db_error_still_prefers_a_findable_name_over_a_quoted_phrase() {
        // Reading both quote styles must not make a generic phrase win. MySQL's
        // `in 'field list'` is skipped as before, and a double-quoted phrase
        // that isn't in the statement simply doesn't match.
        let sql = "SELECT salery FROM employees";
        let d = db_error_diagnostic(sql, 0, sql.len(), "Unknown column 'salery' in 'field list'");
        assert_eq!(&sql[d.range.0..d.range.1], "salery");
    }

    #[test]
    fn db_error_falls_back_to_leading_token() {
        // A message naming nothing findable → underline the first token, not all.
        let sql = "SELECT 1";
        let d = db_error_diagnostic(sql, 0, sql.len(), "Some opaque server error");
        assert_eq!(&sql[d.range.0..d.range.1], "SELECT");
    }

    #[test]
    fn db_error_positions_within_a_later_statement() {
        // Statement offset by `lo` — the range is absolute in the full buffer.
        let sql = "SELECT 1;\nSELECT salery FROM employees";
        let (lo, hi) = (10, sql.len());
        let d = db_error_diagnostic(sql, lo, hi, "Unknown column 'salery' in 'field list'");
        assert_eq!(&sql[d.range.0..d.range.1], "salery");
    }

    // ── generated SQL is dialect-correct ─────────────────────────────────────

    use crate::schema::ForeignKeyInfo;

    /// A chinook-shaped catalog: mixed-case names throughout, an FK from
    /// `Album.ArtistId` to `Artist.ArtistId`. This is the shape of the project's
    /// own PostgreSQL fixture, where every generated identifier has to be quoted.
    fn chinook(dialect: SqlDialect) -> Catalog {
        let mut album = tbl("Album", &["AlbumId", "Title", "ArtistId"]);
        album.foreign_keys = vec![ForeignKeyInfo {
            name: "album_artist_fk".into(),
            columns: vec!["ArtistId".into()],
            ref_table: "Artist".into(),
            ref_columns: vec!["ArtistId".into()],
            ..Default::default()
        }];
        let schema = DbSchema {
            tables: vec![album, tbl("Artist", &["ArtistId", "Name"])],
            ..Default::default()
        };
        let _ = dialect;
        Catalog::build(&[("chinook", &schema)], Some("chinook"))
    }

    #[test]
    fn a_quoted_table_is_in_scope_for_join_completion() {
        // `join_targets` and `expand_star` resolved scope under a hardcoded MySQL
        // dialect, where `"Orders"` is a *string literal*. On the incomplete
        // statement that triggers completion the lexer fallback runs, swallows the
        // name and promotes the alias — so the scope came back as `o` and the FK
        // lookup found nothing.
        let cat = chinook(SqlDialect::Postgres);
        let sql = r#"SELECT * FROM "Album" a JOIN "#;
        let got = join_targets(sql, 0, sql.len(), sql.len(), &cat, SqlDialect::Postgres);
        let t = got
            .iter()
            .find(|t| t.table == "Artist")
            .unwrap_or_else(|| panic!("the FK neighbour must be offered: {got:?}"));
        // Bare for the popup to display and prefix-match, quoted for insertion.
        assert_eq!(t.table_sql, r#""Artist""#);
        assert_eq!(t.predicate, r#"a."ArtistId" = "Artist"."ArtistId""#);
    }

    #[test]
    fn postgres_generated_sql_quotes_every_identifier() {
        let cat = chinook(SqlDialect::Postgres);
        let pg = SqlDialect::Postgres;

        let sql = r#"SELECT * FROM "Album" JOIN "Artist" ON "#;
        assert_eq!(
            join_condition(sql, 0, sql.len(), sql.len(), &cat, pg).as_deref(),
            Some(r#""Album"."ArtistId" = "Artist"."ArtistId""#)
        );

        // The aliases are plain lower-case, so they stay bare — that is valid
        // PostgreSQL, and quoting them would only add noise. The *columns* are
        // what has to be quoted, and they are.
        let sql = r#"SELECT * FROM "Album" a JOIN "Artist" b ON "#;
        assert_eq!(
            join_condition(sql, 0, sql.len(), sql.len(), &cat, pg).as_deref(),
            Some(r#"a."ArtistId" = b."ArtistId""#)
        );

        let sql = r#"SELECT * FROM "Album""#;
        let star = expand_star(sql, 0, sql.len(), 8, &cat, pg).expect("a star expands");
        assert_eq!(star.replacement, r#""AlbumId", "Title", "ArtistId""#);

        let sql = r#"SELECT * FROM "Album" a, "Artist" b"#;
        let star = expand_star(sql, 0, sql.len(), 8, &cat, pg).expect("a star expands");
        assert_eq!(
            star.replacement,
            r#"a."AlbumId", a."Title", a."ArtistId", b."ArtistId", b."Name""#
        );
    }

    #[test]
    fn an_ordinary_lower_case_name_is_left_bare() {
        // The narrowing: quoting is what a *bare* name would get wrong, and a
        // plain lower-case identifier gets nothing wrong on either engine. This
        // keeps MySQL's generated SQL reading the way a person would write it,
        // and matches what `filter::table_query` already chose to do.
        let mut orders = tbl("orders", &["id", "customer_id"]);
        orders.foreign_keys = vec![ForeignKeyInfo {
            name: "fk".into(),
            columns: vec!["customer_id".into()],
            ref_table: "customers".into(),
            ref_columns: vec!["id".into()],
            ..Default::default()
        }];
        let schema = DbSchema {
            tables: vec![orders, tbl("customers", &["id", "name"])],
            ..Default::default()
        };
        let cat = Catalog::build(&[("shop", &schema)], Some("shop"));
        for d in [SqlDialect::MySql, SqlDialect::Postgres] {
            let sql = "SELECT * FROM orders JOIN customers ON ";
            assert_eq!(
                join_condition(sql, 0, sql.len(), sql.len(), &cat, d).as_deref(),
                Some("orders.customer_id = customers.id"),
                "{d:?}"
            );
            let sql = "SELECT * FROM orders";
            let star = expand_star(sql, 0, sql.len(), 8, &cat, d).expect("a star expands");
            assert_eq!(star.replacement, "id, customer_id", "{d:?}");
        }
    }

    #[test]
    fn a_quoted_qualifier_offers_that_tables_columns() {
        // Schemaic writes the quoted form itself — a mixed-case PostgreSQL table
        // or a reserved-word MySQL one — so `"Orders".` is a shape the user
        // reaches by opening a table from the tree, not an exotic one.
        for (sql, dialect, want) in [
            (r#"SELECT "Orders"."#, SqlDialect::Postgres, "Orders"),
            ("SELECT `orders`.", SqlDialect::MySql, "orders"),
            (
                r#"SELECT "sales"."Orders"."#,
                SqlDialect::Postgres,
                "Orders",
            ),
            // Bare must not move.
            ("SELECT orders.", SqlDialect::MySql, "orders"),
            ("SELECT orders.", SqlDialect::Postgres, "orders"),
        ] {
            let wl = word_start(sql, sql.len());
            assert_eq!(
                clause_context(sql, 0, wl, dialect),
                ClauseCtx::Qualified(want.to_string()),
                "{sql:?} ({dialect:?})"
            );
        }
    }

    #[test]
    fn an_insert_target_is_in_scope_unquoted() {
        // Three of the four statement kinds route through `object_name_parts`;
        // the INSERT arm unquoted by hand with `trim_matches('`')`, which knows
        // MySQL's quote character and no other — so a PostgreSQL target stayed
        // `"Orders"`, quotes included, and never matched the catalog.
        let cases = [
            (
                r#"INSERT INTO "Orders" (id) VALUES (1)"#,
                SqlDialect::Postgres,
                "Orders",
                None,
            ),
            (
                r#"INSERT INTO "sales"."Orders" (id) VALUES (1)"#,
                SqlDialect::Postgres,
                "Orders",
                Some("sales"),
            ),
            (
                "INSERT INTO `orders` (id) VALUES (1)",
                SqlDialect::MySql,
                "orders",
                None,
            ),
            (
                "INSERT INTO orders (id) VALUES (1)",
                SqlDialect::MySql,
                "orders",
                None,
            ),
            (
                "INSERT INTO `shop`.`orders` (id) VALUES (1)",
                SqlDialect::MySql,
                "orders",
                Some("shop"),
            ),
        ];
        for (sql, dialect, table, db) in cases {
            let scope = statement_scope(sql, 0, sql.len(), sql.len(), dialect);
            let t = scope
                .tables
                .first()
                .unwrap_or_else(|| panic!("{sql:?}: nothing in scope"));
            assert_eq!(t.name, table, "{sql:?}");
            assert_eq!(t.db.as_deref(), db, "{sql:?}");
        }
    }

    #[test]
    fn a_dot_inside_a_quoted_name_is_not_a_qualifier_separator() {
        // The latent half: splitting the qualified name on a raw `.` splits
        // `"my.table"` in the wrong place.
        let sql = r#"INSERT INTO "my.table" (id) VALUES (1)"#;
        let scope = statement_scope(sql, 0, sql.len(), sql.len(), SqlDialect::Postgres);
        let t = scope.tables.first().expect("in scope");
        assert_eq!(t.name, "my.table");
        assert_eq!(t.db, None);
    }

    /// The quoting decision itself, at the one place that makes it.
    #[test]
    fn ident_if_needed_quotes_exactly_what_a_bare_name_would_get_wrong() {
        use crate::export::ident_if_needed as q;
        for d in [SqlDialect::MySql, SqlDialect::Postgres] {
            // Plain lower-case words are safe bare on both engines.
            assert_eq!(q("orders", d), "orders", "{d:?}");
            assert_eq!(q("customer_id", d), "customer_id", "{d:?}");
            assert_eq!(q("t2", d), "t2", "{d:?}");
            // Anything PostgreSQL would fold, or that isn't a plain word.
            assert_ne!(q("ArtistId", d), "ArtistId", "{d:?}");
            assert_ne!(q("Order Details", d), "Order Details", "{d:?}");
            assert_ne!(q("naïve", d), "naïve", "{d:?}");
            assert_ne!(q("2fast", d), "2fast", "{d:?}");
            assert_ne!(q("", d), "", "{d:?}");
            // Reserved on both — a bare one is a syntax error however spelled.
            assert_ne!(q("select", d), "select", "{d:?}");
            assert_ne!(q("order", d), "order", "{d:?}");
        }
        // Each engine's own quote character, with the embedded one doubled.
        assert_eq!(q("ArtistId", SqlDialect::MySql), "`ArtistId`");
        assert_eq!(q("ArtistId", SqlDialect::Postgres), "\"ArtistId\"");
        assert_eq!(q("we`ird", SqlDialect::MySql), "`we``ird`");
        assert_eq!(q("we\"ird", SqlDialect::Postgres), "\"we\"\"ird\"");
    }

    // ── CatalogCache ─────────────────────────────────────────────────────────

    use std::sync::Arc;

    fn loaded(pairs: &[(&str, &Arc<DbSchema>)]) -> Vec<(String, Arc<DbSchema>)> {
        pairs
            .iter()
            .map(|(d, s)| ((*d).to_string(), Arc::clone(s)))
            .collect()
    }

    /// Does the catalog know an unqualified table? Enough to tell one build's
    /// content from another's.
    fn knows(cat: &Catalog, table: &str) -> bool {
        cat.columns_of(&TableRef {
            name: table.to_string(),
            alias: None,
            db: None,
        })
        .is_some()
    }

    #[test]
    fn catalog_cache_reuses_one_build_across_calls() {
        let schema = Arc::new(DbSchema {
            tables: vec![tbl("employees", &["id", "name"])],
            ..Default::default()
        });
        let l = loaded(&[("company", &schema)]);
        let mut cache = CatalogCache::default();
        let a = cache.get(&l, Some("company"));
        let b = cache.get(&l, Some("company"));
        assert!(
            Arc::ptr_eq(&a, &b),
            "an unchanged schema set must not rebuild the catalog"
        );
    }

    #[test]
    fn catalog_cache_rebuilds_when_a_schema_is_re_introspected() {
        let before = Arc::new(DbSchema {
            tables: vec![tbl("employees", &["id"])],
            ..Default::default()
        });
        let mut cache = CatalogCache::default();
        let a = cache.get(&loaded(&[("company", &before)]), Some("company"));
        assert!(!knows(&a, "departments"));

        // Re-introspection replaces the `Arc`, so the pointer differs even though
        // the map key (the database name) is the same.
        let after = Arc::new(DbSchema {
            tables: vec![tbl("employees", &["id"]), tbl("departments", &["id"])],
            ..Default::default()
        });
        let b = cache.get(&loaded(&[("company", &after)]), Some("company"));
        assert!(!Arc::ptr_eq(&a, &b));
        assert!(
            knows(&b, "departments"),
            "a re-introspected schema must be visible immediately"
        );
    }

    #[test]
    fn catalog_cache_rebuilds_when_the_same_arc_moves_database() {
        let schema = Arc::new(DbSchema {
            tables: vec![tbl("employees", &["id"])],
            ..Default::default()
        });
        let mut cache = CatalogCache::default();
        let a = cache.get(&loaded(&[("company", &schema)]), Some("company"));
        // Same `Arc`, different database name: the key is the pair, not the pointer.
        let b = cache.get(&loaded(&[("staging", &schema)]), Some("company"));
        assert!(!Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn catalog_cache_rebuilds_when_the_active_database_changes() {
        let a_schema = Arc::new(DbSchema {
            tables: vec![tbl("employees", &["id"])],
            ..Default::default()
        });
        let b_schema = Arc::new(DbSchema {
            tables: vec![tbl("orders", &["id"])],
            ..Default::default()
        });
        let l = loaded(&[("company", &a_schema), ("shop", &b_schema)]);
        let mut cache = CatalogCache::default();
        let a = cache.get(&l, Some("company"));
        let b = cache.get(&l, Some("shop"));
        assert!(!Arc::ptr_eq(&a, &b));
        // The unqualified pool is active-db scoped, so the switch is observable.
        assert!(knows(&a, "employees") && !knows(&a, "orders"));
        assert!(knows(&b, "orders") && !knows(&b, "employees"));
    }

    #[test]
    fn catalog_cache_rebuilds_when_a_database_is_loaded_or_dropped() {
        let a_schema = Arc::new(DbSchema {
            tables: vec![tbl("employees", &["id"])],
            ..Default::default()
        });
        let b_schema = Arc::new(DbSchema {
            tables: vec![tbl("orders", &["id"])],
            ..Default::default()
        });
        let mut cache = CatalogCache::default();
        let one = cache.get(&loaded(&[("company", &a_schema)]), None);
        let two = cache.get(
            &loaded(&[("company", &a_schema), ("shop", &b_schema)]),
            None,
        );
        assert!(!Arc::ptr_eq(&one, &two));
        assert!(knows(&two, "orders"));
        // …and back down again.
        let three = cache.get(&loaded(&[("company", &a_schema)]), None);
        assert!(!Arc::ptr_eq(&two, &three));
        assert!(!knows(&three, "orders"));
    }

    #[test]
    fn catalog_cache_starts_empty_and_serves_no_loaded_schemas() {
        let mut cache = CatalogCache::default();
        let a = cache.get(&[], None);
        let b = cache.get(&[], None);
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!knows(&a, "employees"));
    }
}
